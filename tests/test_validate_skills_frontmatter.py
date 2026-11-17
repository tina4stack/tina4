#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""The skills front-matter validator is a real gate for every target's limits.

Writes real SKILL.md files into a temp skills directory and runs the real
scripts/validate-skills-frontmatter.py over it as a subprocess. A good skill
passes; each Hermes limit (name pattern + length, description length, file size,
closing fence, empty body) fails the run and is reported with its file.
"""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]
VALIDATOR = REPO / "scripts" / "validate-skills-frontmatter.py"

GOOD_BODY = "\n# Skill\n\nDoes a thing.\n"


def skill(name: str = "tina4-good", description: str = "Use when testing.", body: str = GOOD_BODY) -> str:
    return f"---\nname: {name}\ndescription: {description}\n---\n{body}"


def run(files: dict[str, str]) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory(prefix="tina4-skill-validate-") as temp:
        root = Path(temp)
        for directory, text in files.items():
            (root / directory).mkdir()
            (root / directory / "SKILL.md").write_text(text, encoding="utf-8", newline="\n")
        return subprocess.run(
            [sys.executable, str(VALIDATOR), str(root)],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )


def expect_pass(label: str, files: dict[str, str]) -> None:
    result = run(files)
    assert result.returncode == 0, f"{label}: expected a pass, got:\n{result.stderr}"
    print(f"pass  {label}")


def expect_fail(label: str, files: dict[str, str], *needles: str) -> None:
    result = run(files)
    assert result.returncode != 0, f"{label}: expected a failure, the validator passed"
    for needle in needles:
        assert needle in result.stderr, f"{label}: {needle!r} not reported:\n{result.stderr}"
    print(f"fail  {label} (as expected)")


def main() -> None:
    expect_pass("a well-formed skill", {"tina4-good": skill()})
    expect_pass("description at exactly 1024", {"a": skill(description="x" * 1024)})
    expect_pass("name at exactly 64 with . _ -", {"a": skill(name="a" * 61 + "._-")})
    expect_pass("trailing spaces after the closing fence", {"a": skill().replace("\n---\n", "\n---  \n", 1)})

    expect_fail(
        "description over 1024",
        {"tina4-long": skill(description="x" * 1025)},
        "tina4-long",
        "`description` is 1025 characters",
    )
    expect_fail("uppercase name", {"bad": skill(name="Tina4-Bad")}, "`name` 'Tina4-Bad' must match")
    expect_fail("name starting with -", {"bad": skill(name="-tina4")}, "must match")
    expect_fail("name over 64", {"bad": skill(name="a" * 65)}, "`name` is 65 characters")
    expect_fail("SKILL.md over 100,000 chars", {"big": skill(body="\n" + "x" * 100_000)}, "the limit is 100,000")
    expect_fail("no closing fence", {"open": "---\nname: a\ndescription: b\n# body\n"}, "no terminated")
    expect_fail("indented closing fence", {"indent": skill().replace("\n---\n", "\n ---\n", 1)}, "no terminated")
    expect_fail("empty body", {"empty": skill(body="\n  \n")}, "no body")
    expect_fail("missing description", {"nodesc": "---\nname: a\n---\nbody\n"}, "`description` is missing")
    expect_fail("strict YAML still enforced", {"yaml": "---\nname: a\ndescription: b: c\n---\nbody\n"}, "not valid YAML")

    # Every violation, in every file, is reported - not just the first.
    expect_fail(
        "all violations reported",
        {
            "one": skill(name="One", description="x" * 2000),
            "two": skill(name="two", body=""),
        },
        "`name` 'One' must match",
        "`description` is 2000 characters",
        "no body",
        "3 skill front-matter problem(s)",
    )
    print("validate-skills-frontmatter: every target limit is a gate")


if __name__ == "__main__":
    main()
