#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Generator-parity gate for the tina4 CLI.

Drives a REAL project end-to-end for one language — `tina4 init`, then
`generate model`, `generate crud … --fields …` (and `generate page|component`
for tina4js), then `tina4 routes` — and asserts the produced names and paths
against the committed naming contract (tests/fixtures/generator_contract.json).
No mocks: it runs the built binary against the actual framework toolchain.

It guards three shipped bugs at once:

  * bug 2 — `tina4 routes` must LOAD the generated route files, not fail every
    one with "No module named 'src'" and list only the built-ins. The gate
    fails if fewer than `routes_min_registered` routes come back, or if the
    output carries a load error.
  * bug 3 — in a tina4js project `generate page|component` must delegate to
    `tina4js`, not `vite`. The gate fails if the expected files are missing or
    the output mentions a `vite generate` failure.
  * bug 1 — crud route/template names must be the SINGLE plural of the singular
    base (Order → orders), never a double-pluralised reserved-word table
    (orderss). This lives in the framework generator, which the CLI forwards to
    verbatim, so it is marked `known_framework_bug` in the fixture: the gate
    reports it XFAIL (expected until the framework PR lands) rather than failing
    the CLI build, and flips to a loud XPASS the day the framework is fixed so
    the marker gets removed.

Exit status is non-zero on any hard FAIL. XFAIL/XPASS never fail the build on
their own; pass --strict-xfail to also fail on an XPASS (contract drifted).

Usage:
  generator_parity_gate.py --language python [--bin tina4] [--fixture PATH]
                           [--keep] [--strict-xfail]
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_FIXTURE = ROOT / "tests" / "fixtures" / "generator_contract.json"

# init name → the language token `tina4 init` expects.
INIT_TOKEN = {"python": "python", "php": "php", "ruby": "ruby", "nodejs": "nodejs", "tina4js": "js"}


class Report:
    """Collects PASS / FAIL / XFAIL / XPASS lines and prints a compact summary."""

    def __init__(self) -> None:
        self.rows: list[tuple[str, str]] = []

    def ok(self, msg: str) -> None:
        self.rows.append(("PASS", msg))
        print(f"  ✓ {msg}")

    def fail(self, msg: str) -> None:
        self.rows.append(("FAIL", msg))
        print(f"  ✗ FAIL: {msg}")

    def xfail(self, msg: str) -> None:
        self.rows.append(("XFAIL", msg))
        print(f"  ~ XFAIL (known framework bug): {msg}")

    def xpass(self, msg: str) -> None:
        self.rows.append(("XPASS", msg))
        print(f"  ! XPASS (framework fixed — update the fixture): {msg}")

    def count(self, kind: str) -> int:
        return sum(1 for k, _ in self.rows if k == kind)


def run(binary: str, args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["TINA4_NO_BROWSER"] = "true"
    env["TINA4_INIT_NO_SERVE"] = "1"
    return subprocess.run(
        [binary, *args],
        cwd=str(cwd),
        env=env,
        capture_output=True,
        text=True,
        timeout=600,
    )


def route_paths(routes_output: str) -> list[str]:
    """Pull the Path column out of `tina4 routes` table output.

    The table is whitespace-separated `Method  Path  Auth  Handler`; the path is
    the first token that starts with a slash. Splitting on columns (rather than a
    slash regex) keeps the FULL path — an earlier `\\b(/\\S+)` grabbed `/products`
    out of the middle of `/api/products` and broke every match.
    """
    paths = []
    for line in routes_output.splitlines():
        if "---" in line or not line.strip():
            continue
        for token in line.split():
            if token.startswith("/"):
                paths.append(token)
                break
    return paths


def check_files(report: Report, proj: Path, files: list[str], label: str) -> None:
    for rel in files:
        if (proj / rel).is_file():
            report.ok(f"{label}: {rel} written")
        else:
            report.fail(f"{label}: expected file missing: {rel}")


def check_globs(report: Report, proj: Path, patterns: list[str], label: str) -> None:
    for pat in patterns:
        if glob.glob(str(proj / pat)):
            report.ok(f"{label}: {pat} matched")
        else:
            report.fail(f"{label}: no file matched {pat}")


def gate_tina4js(report: Report, binary: str, proj: Path, fixture: dict) -> None:
    spec = fixture["tina4js"]
    for kind in ("page", "component"):
        entry = spec[kind]
        res = run(binary, ["generate", kind, entry["name"]], proj)
        combined = res.stdout + res.stderr
        if "vite generate" in combined or re.search(r"Failed to run vite", combined):
            report.fail(f"generate {kind} delegated to vite, not tina4js: {combined.strip()[:200]}")
            continue
        check_files(report, proj, entry["files"], f"generate {kind}")


def gate_backend(report: Report, binary: str, proj: Path, language: str, fixture: dict) -> None:
    lang = fixture["languages"][language]

    # 1. model — cross-language parity anchor (table + migration name identical).
    model = fixture["model"]
    res = run(binary, ["generate", "model", model["class"]], proj)
    if res.returncode != 0:
        report.fail(f"generate model {model['class']} failed: {(res.stderr or res.stdout).strip()[:200]}")
    if "model_file" in lang:
        check_files(report, proj, [lang["model_file"]], "model")
    if "model_file_globs" in lang:
        check_globs(report, proj, lang["model_file_globs"], "model")
    if not glob.glob(str(proj / lang["migration_glob"])):
        report.fail(f"model migration missing (parity): expected {lang['migration_glob']}")
    else:
        report.ok(f"model migration matches contract: {lang['migration_glob']}")

    # 2. crud — the naming contract + the reserved-word bug detector.
    for entry in fixture["crud"]:
        name, fields = entry["name"], entry["fields"]
        res = run(binary, ["generate", "crud", name, "--fields", fields], proj)
        if res.returncode != 0:
            report.fail(f"generate crud {name} failed: {(res.stderr or res.stdout).strip()[:200]}")
            continue

        mig = str(proj / lang["migration_glob"]).replace("create_widget", entry["migration"])
        if glob.glob(mig):
            report.ok(f"crud {name}: migration {entry['migration']} matches contract")
        else:
            report.fail(f"crud {name}: expected migration {entry['migration']} (glob {mig})")

        files = lang.get("crud_files", {}).get(name)
        globs = lang.get("crud_files_globs", {}).get(name)
        bug = entry.get("known_framework_bug")
        if files is not None:
            _check_crud_files(report, proj, files, name, bug)
        if globs is not None:
            check_globs(report, proj, globs, f"crud {name}")

    # 3. routes — bug 2: the generated routes must actually LOAD and be listed.
    res = run(binary, ["routes"], proj)
    combined = res.stdout + res.stderr
    if "No module named" in combined or re.search(r"Failed to load .*routes", combined):
        report.fail(f"`routes` failed to load project route files (bug 2): {combined.strip()[:200]}")
    paths = route_paths(res.stdout)
    if len(paths) >= fixture["routes_min_registered"]:
        report.ok(f"`routes` loaded {len(paths)} routes (>= {fixture['routes_min_registered']})")
    else:
        report.fail(f"`routes` listed only {len(paths)} routes — generated routes did not load (bug 2)")

    # The route is asserted by its PLURAL SEGMENT, not a full path: frameworks
    # differ on the prefix (Python mounts /api/products, PHP mounts /products),
    # but every one must use the single plural `products` / `orders` and never
    # the double-pluralised `orderss`. Segment matching is prefix-agnostic.
    segments = {seg for p in paths for seg in p.strip("/").split("/")}
    for entry in fixture["crud"]:
        bug = entry.get("known_framework_bug")
        want = entry["route_plural"]
        present = want in segments
        bad = sorted({b for b in entry.get("must_not_appear", []) if any(b in p for p in paths)})
        clean = present and not bad
        if bug and clean:
            report.xpass(f"crud {entry['name']}: route segment '{want}' present, {entry.get('must_not_appear')} gone — framework fixed {bug['id']}; drop the known_framework_bug marker")
        elif clean:
            report.ok(f"crud {entry['name']}: route segment '{want}' present, no double-plural")
        elif bug:
            report.xfail(f"crud {entry['name']}: {bad or 'route segment missing'} instead of '{want}' — {bug['why']}")
        else:
            report.fail(f"crud {entry['name']}: expected route segment '{want}'; got {bad or 'missing'}")


def _check_crud_files(report: Report, proj: Path, files: list[str], name: str, bug: dict | None) -> None:
    for rel in files:
        if (proj / rel).is_file():
            report.ok(f"crud {name}: {rel} written")
        elif bug:
            # The double-pluralised sibling is what the framework writes today.
            report.xfail(f"crud {name}: {rel} missing (double-pluralised by framework) — {bug['id']}")
        else:
            report.fail(f"crud {name}: expected file missing: {rel}")


def main() -> int:
    ap = argparse.ArgumentParser(description="tina4 generator-parity gate")
    ap.add_argument("--language", required=True, choices=sorted(INIT_TOKEN))
    ap.add_argument("--bin", default=os.environ.get("TINA4_BIN", "tina4"))
    ap.add_argument("--fixture", default=str(DEFAULT_FIXTURE))
    ap.add_argument("--keep", action="store_true", help="keep the temp project")
    ap.add_argument("--strict-xfail", action="store_true", help="fail the build on an XPASS")
    args = ap.parse_args()

    fixture = json.loads(Path(args.fixture).read_text())
    report = Report()
    workdir = Path(tempfile.mkdtemp(prefix=f"tina4-gate-{args.language}-"))
    proj = workdir / "proj"

    print(f"== generator-parity gate: {args.language} ==")
    try:
        res = run(args.bin, ["init", INIT_TOKEN[args.language], "proj"], workdir)
        if res.returncode != 0 or not proj.is_dir():
            report.fail(f"init {args.language} failed: {(res.stderr or res.stdout).strip()[:300]}")
        else:
            report.ok(f"init {args.language} scaffolded a project")
            if args.language == "tina4js":
                gate_tina4js(report, args.bin, proj, fixture)
            else:
                gate_backend(report, args.bin, proj, args.language, fixture)
    finally:
        if not args.keep:
            shutil.rmtree(workdir, ignore_errors=True)

    failed, xpass = report.count("FAIL"), report.count("XPASS")
    print(
        f"\n{args.language}: {report.count('PASS')} passed, {failed} failed, "
        f"{report.count('XFAIL')} xfail, {xpass} xpass"
    )
    if failed:
        return 1
    if xpass and args.strict_xfail:
        print("XPASS with --strict-xfail: the framework was fixed; update the fixture.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
