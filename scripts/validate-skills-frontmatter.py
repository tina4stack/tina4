#!/usr/bin/env python3
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Validate every SKILL.md front matter as STRICT YAML, within every target's limits.

    scripts/validate-skills-frontmatter.py <dir> [<dir> ...]

Walks each directory for `*/SKILL.md`, extracts the `---` ... `---` front matter,
and parses it with a strict YAML loader. Fails (exit 1) when a file:
  - has no front matter block (missing or unterminated `---` fence),
  - whose front matter is not valid YAML (a tab for indentation, a stray colon,
    an unquoted value YAML reads as the wrong type), or
  - is missing a non-empty `name` or `description` scalar, or
  - breaks a limit an install target enforces (below).

`name` and `description` are load-bearing: `description` is the text that decides
when the skill triggers, and a lenient parser that silently accepted a malformed
block let a real defect ship (tina4#112). This gate runs at the bundle cut, where
every repo's skills converge, so one check covers them all.

Target limits. `tina4 skills hermes` installs into ~/.hermes/skills, and Hermes
Agent (Nous Research) refuses a skill its validator rejects
(tools/skill_manager_tool.py `_validate_frontmatter`). The strictest target sets
the bar, so these are errors here, not warnings:
  - the file starts with `---` and the block closes with a line that is `---`
    plus optional trailing whitespace (Hermes matches `\\n---\\s*\\n`),
  - `name` is at most 64 characters and matches `^[a-z0-9][a-z0-9._-]*$`,
  - `description` is at most 1024 characters,
  - the body after the front matter is not empty,
  - the whole SKILL.md is at most 100,000 characters.

Every violation is reported with its file; the run fails if there is any.

Needs PyYAML; a CI step installs it (`pip install pyyaml`). It is a build/CI tool,
not a framework runtime dependency.
"""
import re
import sys
import pathlib

try:
    import yaml
except ModuleNotFoundError:
    sys.exit("error: PyYAML is required (pip install pyyaml) — it is a CI tool, not a runtime dependency")

# Hermes Agent's limits (tools/skill_manager_tool.py). The strictest target wins.
MAX_NAME_LENGTH = 64
NAME_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
MAX_DESCRIPTION_LENGTH = 1024
MAX_SKILL_MD_CHARS = 100_000

# A closing fence: `---` alone on its line, trailing whitespace allowed.
CLOSING_FENCE = re.compile(r"\n---[ \t]*(?:\n|$)")


def split_frontmatter(text):
    """Return (front matter YAML, body) for a leading `---` block, or None.

    The block must open on the very first line and close with a line that is
    `---` plus optional trailing whitespace; anything else (an indented `---`,
    `----`, a `--- x` line) is not a fence, matching Hermes' own reader.
    """
    if not text.startswith("---\n"):
        return None
    match = CLOSING_FENCE.search(text, 3)
    if match is None:
        return None  # unterminated fence
    return text[4:match.start()], text[match.end():]


def validate_file(path):
    errors = []
    text = path.read_text(encoding="utf-8")
    if len(text) > MAX_SKILL_MD_CHARS:
        errors.append(
            f"{path}: SKILL.md is {len(text):,} characters; the limit is {MAX_SKILL_MD_CHARS:,} (Hermes)"
        )
    parts = split_frontmatter(text)
    if parts is None:
        return errors + [f"{path}: no terminated `---` front-matter block (must open on line 1 and close with a `---` line)"]
    fm, body = parts
    try:
        data = yaml.safe_load(fm)
    except yaml.YAMLError as exc:
        where = ""
        mark = getattr(exc, "problem_mark", None)
        if mark is not None:
            where = f" (line {mark.line + 2}, column {mark.column + 1})"  # +2: past the opening fence
        return errors + [f"{path}: front matter is not valid YAML{where}: {getattr(exc, 'problem', exc)}"]
    if not isinstance(data, dict):
        return errors + [f"{path}: front matter is not a mapping (got {type(data).__name__})"]
    for key in ("name", "description"):
        value = data.get(key)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{path}: front matter `{key}` is missing or empty")
    name = data.get("name")
    if isinstance(name, str) and name.strip():
        if len(name) > MAX_NAME_LENGTH:
            errors.append(f"{path}: `name` is {len(name)} characters; the limit is {MAX_NAME_LENGTH} (Hermes)")
        if not NAME_PATTERN.match(name):
            errors.append(
                f"{path}: `name` {name!r} must match {NAME_PATTERN.pattern} "
                "(lowercase letters, digits, '.', '_', '-'; starts with a letter or digit) (Hermes)"
            )
    description = data.get("description")
    if isinstance(description, str) and len(description) > MAX_DESCRIPTION_LENGTH:
        errors.append(
            f"{path}: `description` is {len(description)} characters; the limit is "
            f"{MAX_DESCRIPTION_LENGTH} (Hermes) - trim {len(description) - MAX_DESCRIPTION_LENGTH}"
        )
    if not body.strip():
        errors.append(f"{path}: SKILL.md has no body after the front matter (Hermes)")
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
