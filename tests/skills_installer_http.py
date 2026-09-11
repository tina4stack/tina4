#!/usr/bin/env python3
"""Exercise the real skill installers against a real failure-capable HTTP server."""

from __future__ import annotations

import argparse
import hashlib
import http.server
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
from urllib.parse import urlparse


DEV_REFS = (
    "auth-and-services.md",
    "data-and-orm.md",
    "deployment.md",
    "routes-and-api.md",
    "templates-and-frontend.md",
    "realtime.md",
    "ai-coder-rule-path.svg",
)
# Every skill + reference install-skills.sh stages, keyed by stage-relative skill
# dir. Mirrors the installer's install_skill calls AND scripts/gen-skills-sha256.sh
# exactly -- a file here the installer does not fetch (or the reverse) breaks the
# checksum step, which is precisely what this test guards.
INSTALLS = {
    "tina4-developer-python": DEV_REFS,
    "tina4-developer-php": DEV_REFS,
    "tina4-developer-ruby": DEV_REFS,
    "tina4-developer-nodejs": DEV_REFS,
    "tina4-js": (
        "html-and-components.md",
        "signals-and-reactivity.md",
        "persistence.md",
        "rtc.md",
    ),
    "tina4-maintainer": (
        "cli-and-deployment.md",
        "frond-and-frontend.md",
        "routing-and-orm.md",
        "subsystems.md",
    ),
    "tina4-architect": (),
    "tina4-design": (),
}

SKILLS_MARKER = "/.claude/skills/"

# Kept low so the outage case stays quick; the contract it proves does not depend on
# the value, only on the count being a function of the retry walk and NOT of the
# number of files.
RETRY_COUNT = 1

# One file the first tier will be missing in "gap" mode.
GAP_FILE = "tina4-developer-php/references/realtime.md"


def stage_relpaths() -> list[str]:
    """Every stage-relative file path the installer stages, in manifest order."""
    paths: list[str] = []
    for skill, references in INSTALLS.items():
        paths.append(f"{skill}/SKILL.md")
        paths.extend(f"{skill}/references/{reference}" for reference in references)
    return paths


def fixture_bytes(relpath: str) -> bytes:
    """Deterministic content for a staged file, keyed ONLY on its stage-relative
    path -- so the primary and the mirror serve identical bytes for the same file
    and a single checksum manifest verifies whichever source answered."""
    return (f"tina4 skills fixture: {relpath}\n").encode()


def manifest_bytes() -> bytes:
    """A skills.sha256 manifest matching the fixtures above, in the same format
    scripts/gen-skills-sha256.sh emits (`<hash>  <stage-relative-path>`)."""
    lines = [
        f"{hashlib.sha256(fixture_bytes(rel)).hexdigest()}  {rel}"
        for rel in stage_relpaths()
    ]
    lines.sort(key=lambda line: line.split("  ", 1)[1])
    return ("\n".join(lines) + "\n").encode()


# One prefix per tier the installer fetches from, in the installer's own order.
# These MUST stay in step with the TINA4_SKILLS_*_ROOT names install-skills.sh and
# install-skills.ps1 read: when the installer went three-tier its variables were
# renamed, this file was not, and the whole suite quietly started testing the real
# internet instead of this server -- green, and asserting nothing.
TIERS = ("tina4", "jsdelivr", "raw")


class SkillHandler(http.server.BaseHTTPRequestHandler):
    attempts: dict[str, int] = {}
    hits: dict[str, int] = {tier: 0 for tier in TIERS}
    primary_mode = "retry"
    tier = "tina4"

    def do_GET(self) -> None:  # noqa: N802 - stdlib callback name
        path = urlparse(self.path).path
        count = self.attempts.get(path, 0) + 1
        self.attempts[path] = count

        tier = self.tier
        self.hits[tier] = self.hits.get(tier, 0) + 1

        target = path.endswith("/tina4-developer-python/SKILL.md")
        if tier == "tina4":
            retry_failure = self.primary_mode == "retry" and target and count == 1
            fallback_failure = self.primary_mode == "fallback" and target
            # "outage": the first tier is down for everything, for the whole run --
            # the shape of a real CDN incident, and the one that shows whether the
            # installer remembers a dead host or re-proves it once per file.
            outage_failure = self.primary_mode == "outage"
            if retry_failure or fallback_failure or outage_failure:
                self.send_response(503)
                self.end_headers()
                self.wfile.write(b"temporary upstream failure")
                return
            # "gap": this tier is healthy but does not have one file -- the shape of a
            # mirror that has not finished catching up with a freshly published ref.
            # A missing file says nothing about the next one, so a tier must NOT be
            # written off for it. Without that distinction one 404 costs the tier for
            # the rest of the run, which is the fallback disabling itself.
            if self.primary_mode == "gap" and path.endswith(GAP_FILE):
                self.send_response(404)
                self.end_headers()
                return
            # "revival": this tier fails its first walk -- long enough to be written
            # off -- and is healthy afterwards, while BOTH other tiers are missing one
            # file. Only the written-off tier can serve it. Skipping a source must
            # never lose it, so the run has to come back and ask anyway.
            if self.primary_mode == "revival" and self.hits["tina4"] <= RETRY_COUNT + 1:
                self.send_response(503)
                self.end_headers()
                self.wfile.write(b"temporary upstream failure")
                return
        elif self.primary_mode == "revival" and path.endswith(GAP_FILE):
            self.send_response(404)
            self.end_headers()
            return

        if path.endswith("/skills.sha256"):
            body = manifest_bytes()
        else:
            # Each tier has its own path shape: jsDelivr and raw carry the skills
            # directory in the URL, tina4.com serves the stage-relative path flat
            # under the ref. Resolving both is what makes this server stand in for
            # all three.
            marker = path.find(SKILLS_MARKER)
            if marker != -1:
                relpath = path[marker + len(SKILLS_MARKER):]
            else:
                relpath = path.lstrip("/").split("/", 1)[-1]
            body = fixture_bytes(relpath)

        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, message: str, *args: object) -> None:
        print("http:", message % args, flush=True)


def installer_default_ref(repo: Path) -> str:
    """The expected ref is the installer's OWN default, so this test tracks the
    installer instead of drifting against a hardcoded version."""
    text = (repo / "install-skills.sh").read_text()
    match = re.search(r"TINA4_SKILLS_REF:-([0-9][0-9.]*)", text)
    assert match, "could not read the default ref from install-skills.sh"
    return match.group(1)


def verify_install(skill_home: Path, expected_ref: str) -> None:
    destination = skill_home / ".agents" / "skills"
    assert (destination / ".tina4-skills-ref").read_text().strip() == expected_ref
    for skill, references in INSTALLS.items():
        assert (destination / skill / "SKILL.md").is_file(), skill
        for reference in references:
            assert (destination / skill / "references" / reference).is_file(), (
                skill,
                reference,
            )


def assert_dead_tier_written_off(mode: str) -> None:
    """An unreachable tier costs one retry walk per RUN, not one per file.

    The installer fetches 40-odd files from three tiers. Nothing in the old walk
    remembered that a tier had already failed, so a tier that was down was re-proven
    for every single file: (RETRY_COUNT + 1) doomed requests and RETRY_COUNT x
    retry_delay seconds each time. The bound below is what separates "asked once" from
    "asked once per file" -- against the unfixed installer this count is in the
    hundreds, and one file's worth of slack keeps it from being brittle.
    """
    if mode == "revival":
        # The only proof that matters is that the install completed at all, which
        # verify_install has already asserted; this pins where the file came from.
        # It is asked once more, for the one file only it has -- not once per file,
        # which would be the defect this whole change is about coming back.
        walk = RETRY_COUNT + 1
        hits = SkillHandler.hits["tina4"]
        assert hits > walk, (
            "the written-off tier was never asked again, so the file that only it had "
            "could not have been installed"
        )
        assert hits <= walk + 2, (
            f"the written-off tier was asked {hits} times; coming back for one file "
            f"costs at most {walk + 2}"
        )
        return
    if mode == "gap":
        # One 404 must cost exactly one fallback, not the whole tier.
        assert SkillHandler.hits["jsdelivr"] == 1, (
            f"a single missing file sent {SkillHandler.hits['jsdelivr']} requests to "
            "the fallback tier; the first tier was written off for a 404"
        )
        assert SkillHandler.hits["tina4"] > 10, (
            "the first tier stopped being used after one missing file"
        )
        return
    if mode != "outage":
        return
    walk = RETRY_COUNT + 1
    budget = walk * 2
    hits = SkillHandler.hits["tina4"]
    assert hits <= budget, (
        f"the down tier was asked {hits} times; a run that remembers it asks at most "
        f"{budget} ({walk} per retry walk). It is being re-proven per file."
    )
    assert SkillHandler.hits["jsdelivr"] > 0, "the fallback tier was never reached"


def run_installer(kind: str, mode: str, repo: Path) -> None:
    SkillHandler.attempts = {}
    SkillHandler.hits = {tier: 0 for tier in TIERS}
    SkillHandler.primary_mode = mode
    # One server per tier, on its own port. In production the three tiers are three
    # different origins, and a fix that writes off a host it has just watched fail can
    # only be measured when they are actually distinct -- share one port between them
    # and the test proves nothing about which of them was skipped.
    servers = {}
    threads = []
    for tier in TIERS:
        handler = type(f"{tier}Handler", (SkillHandler,), {"tier": tier})
        servers[tier] = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=servers[tier].serve_forever, daemon=True)
        thread.start()
        threads.append(thread)
    roots = {
        tier: f"http://127.0.0.1:{servers[tier].server_address[1]}" for tier in TIERS
    }

    try:
        with tempfile.TemporaryDirectory(prefix="tina4-skills-test-") as temp:
            skill_home = Path(temp)
            env = os.environ.copy()
            env.update(
                {
                    "HOME": str(skill_home),
                    "TINA4_SKILLS_HOME": str(skill_home),
                    "TINA4_SKILLS_TARGET": "codex",
                    "TINA4_SKILLS_TINA4_ROOT": roots["tina4"],
                    "TINA4_SKILLS_JSDELIVR_ROOT": roots["jsdelivr"],
                    "TINA4_SKILLS_RAW_ROOT": roots["raw"],
                    "TINA4_SKILLS_RETRY_DELAY": "0",
                    "TINA4_SKILLS_RETRY_COUNT": str(RETRY_COUNT),
                }
            )
            if kind == "shell":
                command = ["sh", str(repo / "install-skills.sh")]
            elif os.environ.get("TINA4_TEST_POWERSHELL_CONTAINER"):
                command = [
                    "docker",
                    "run",
                    "--rm",
                    "--network",
                    "host",
                    "-v",
                    f"{repo}:{repo}:ro",
                    "-v",
                    "/tmp:/tmp",
                ]
                for name in (
                    "HOME",
                    "TINA4_SKILLS_HOME",
                    "TINA4_SKILLS_TARGET",
                    "TINA4_SKILLS_TINA4_ROOT",
                    "TINA4_SKILLS_JSDELIVR_ROOT",
                    "TINA4_SKILLS_RAW_ROOT",
                    "TINA4_SKILLS_RETRY_DELAY",
                    "TINA4_SKILLS_RETRY_COUNT",
                ):
                    command.extend(("-e", name))
                command.extend(
                    (
                        "mcr.microsoft.com/powershell:latest",
                        "pwsh",
                        "-NoProfile",
                        "-File",
                        str(repo / "install-skills.ps1"),
                    )
                )
            else:
                command = [
                    os.environ.get("TINA4_TEST_POWERSHELL", "powershell.exe"),
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(repo / "install-skills.ps1"),
                ]
            print(f"running {kind} installer in {mode} mode", flush=True)
            subprocess.run(command, cwd=repo, env=env, check=True)
            verify_install(skill_home, installer_default_ref(repo))
            assert_dead_tier_written_off(mode)
    finally:
        for tier in TIERS:
            servers[tier].shutdown()
            servers[tier].server_close()
        for thread in threads:
            thread.join(timeout=5)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("shell", "powershell"))
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    for mode in ("retry", "fallback", "outage", "gap", "revival"):
        run_installer(args.kind, mode, repo)
    print(f"{args.kind}: retry, fallback, outage, gap and revival contracts passed")


if __name__ == "__main__":
    main()
