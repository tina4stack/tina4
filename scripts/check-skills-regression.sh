#!/usr/bin/env bash
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Release guard: a new skills bundle must never silently ship LESS than the one
# tina4.com already serves.
#
#   scripts/check-skills-regression.sh <previous-bundle-dir> <new-bundle-dir>
#
# Fails (exit 1) when, compared with the previous bundle:
#   - a skill that was published is missing from the new bundle, or
#   - a SKILL.md shrank by more than TINA4_SKILLS_MAX_SHRINK percent (default 15).
# New skills and growth are fine. Called by gen-skills-bundle.sh against the bundle
# that docs/public/skills/latest/version points at; usable on its own or in CI.
#
# Why: 3.13.151 was staged from source that never received two merged PRs, so it
# dropped tina4-seo and tina4-a11y entirely and cut tina4-design by 27% (its approval
# gates gone) -- and nothing noticed until a user's `tina4 update` regressed them.
set -eu

if [ $# -ne 2 ]; then
  echo "usage: $0 <previous-bundle-dir> <new-bundle-dir>" >&2
  exit 2
fi
prev="$1"
new="$2"
max_shrink="${TINA4_SKILLS_MAX_SHRINK:-15}"

[ -d "$prev" ] || { echo "error: previous bundle not found: $prev" >&2; exit 2; }
[ -d "$new" ]  || { echo "error: new bundle not found: $new" >&2; exit 2; }

problems=0
for dir in "$prev"/*/; do
  skill="$(basename "$dir")"
  [ -f "$dir/SKILL.md" ] || continue
  if [ ! -f "$new/$skill/SKILL.md" ]; then
    echo "  - $skill: published before, missing now" >&2
    problems=$((problems + 1))
    continue
  fi
  old_size=$(wc -c < "$dir/SKILL.md")
  new_size=$(wc -c < "$new/$skill/SKILL.md")
  if [ $((new_size * 100)) -lt $((old_size * (100 - max_shrink))) ]; then
    echo "  - $skill/SKILL.md: shrank $(( (old_size - new_size) * 100 / old_size ))% ($old_size -> $new_size bytes)" >&2
    problems=$((problems + 1))
  fi
done

if [ "$problems" -gt 0 ]; then
  echo "error: $problems regression(s) against $(basename "$prev") -- restore the content in its source repo (see scripts/skills-list.sh for where each skill lives), or pass --allow-regress to gen-skills-bundle.sh if the removal is intentional" >&2
  exit 1
fi
echo "  No regressions against $(basename "$prev")"
