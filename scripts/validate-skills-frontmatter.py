#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Validate every SKILL.md front matter as STRICT YAML.

    scripts/validate-skills-frontmatter.py <dir> [<dir> ...]

Walks each directory for `*/SKILL.md`, extracts the `---` ... `---` front matter,
and parses it with a strict YAML loader. Fails (exit 1) when a file:
  - has no front matter block (missing or unterminated `---` fence),
  - whose front matter is not valid YAML (a tab for indentation, a stray colon,
    an unquoted value YAML reads as the wrong type), or
  - is missing a non-empty `name` or `description` scalar.

`name` and `description` are load-bearing: `description` is the text that decides
when the skill triggers, and a lenient parser that silently accepted a malformed
block let a real defect ship (tina4#112). This gate runs at the bundle cut, where
every repo's skills converge, so one check covers them all.

Needs PyYAML; a CI step installs it (`pip install pyyaml`). It is a build/CI tool,
not a framework runtime dependency.
"""
import sys
import pathlib

try:
    import yaml
except ModuleNotFoundError:
    sys.exit("error: PyYAML is required (pip install pyyaml) — it is a CI tool, not a runtime dependency")


def extract_frontmatter(text):
    """Return the YAML between the leading `---` fence and its terminator, or None."""
    if not text.startswith("---\n") and text != "---\n":
        return None
    lines = text.split("\n")
    # lines[0] is the opening '---'; find the next bare '---'.
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            return "\n".join(lines[1:i])
    return None  # unterminated fence


def validate_file(path):
    errors = []
    text = path.read_text(encoding="utf-8")
    fm = extract_frontmatter(text)
    if fm is None:
        return [f"{path}: no terminated `---` front-matter block"]
    try:
        data = yaml.safe_load(fm)
    except yaml.YAMLError as exc:
        where = ""
        mark = getattr(exc, "problem_mark", None)
        if mark is not None:
            where = f" (line {mark.line + 2}, column {mark.column + 1})"  # +2: past the opening fence
        return [f"{path}: front matter is not valid YAML{where}: {getattr(exc, 'problem', exc)}"]
    if not isinstance(data, dict):
        return [f"{path}: front matter is not a mapping (got {type(data).__name__})"]
    for key in ("name", "description"):
        value = data.get(key)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{path}: front matter `{key}` is missing or empty")
    return errors


def main(argv):
    roots = [pathlib.Path(a) for a in argv[1:]]
    if not roots:
        sys.exit("usage: validate-skills-frontmatter.py <dir> [<dir> ...]")
    skill_files = sorted(
        {p for root in roots for p in root.glob("*/SKILL.md")}
    )
    if not skill_files:
        sys.exit(f"error: no */SKILL.md found under {', '.join(map(str, roots))}")
    problems = []
    for path in skill_files:
        problems.extend(validate_file(path))
    if problems:
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        sys.exit(f"error: {len(problems)} skill front-matter problem(s) across {len(skill_files)} skill(s)")
    print(f"  Front matter valid for all {len(skill_files)} skill(s)")


if __name__ == "__main__":
    main(sys.argv)
