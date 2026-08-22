#!/usr/bin/env python3
"""Exercise the real skill installers against a real failure-capable HTTP server."""

from __future__ import annotations

import argparse
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
}


class SkillHandler(http.server.BaseHTTPRequestHandler):
    attempts: dict[str, int] = {}
    primary_mode = "retry"

    def do_GET(self) -> None:  # noqa: N802 - stdlib callback name
        path = urlparse(self.path).path
        count = self.attempts.get(path, 0) + 1
        self.attempts[path] = count

        target = path.endswith("/tina4-developer-python/SKILL.md")
        if path.startswith("/primary/"):
            retry_failure = self.primary_mode == "retry" and target and count == 1
            fallback_failure = self.primary_mode == "fallback" and target
            if retry_failure or fallback_failure:
                self.send_response(503)
                self.end_headers()
                self.wfile.write(b"temporary upstream failure")
                return
        elif not path.startswith("/mirror/"):
            self.send_response(404)
            self.end_headers()
            return

        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(("fixture for " + path + "\n").encode())

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


def run_installer(kind: str, mode: str, repo: Path) -> None:
    SkillHandler.attempts = {}
    SkillHandler.primary_mode = mode
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SkillHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]

    try:
        with tempfile.TemporaryDirectory(prefix="tina4-skills-test-") as temp:
            skill_home = Path(temp)
            env = os.environ.copy()
            env.update(
                {
                    "HOME": str(skill_home),
                    "TINA4_SKILLS_HOME": str(skill_home),
                    "TINA4_SKILLS_TARGET": "codex",
                    "TINA4_SKILLS_PRIMARY_ROOT": f"http://127.0.0.1:{port}/primary",
                    "TINA4_SKILLS_MIRROR_ROOT": f"http://127.0.0.1:{port}/mirror",
                    "TINA4_SKILLS_RETRY_DELAY": "0",
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
                    "TINA4_SKILLS_PRIMARY_ROOT",
                    "TINA4_SKILLS_MIRROR_ROOT",
                    "TINA4_SKILLS_RETRY_DELAY",
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
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("shell", "powershell"))
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    for mode in ("retry", "fallback"):
        run_installer(args.kind, mode, repo)
    print(f"{args.kind}: retry and fallback contracts passed")


if __name__ == "__main__":
    main()
