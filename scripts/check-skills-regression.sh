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
#   - a published file is missing from the new bundle (a whole skill, OR any one
#     of its reference files), or
#   - any published file shrank by more than TINA4_SKILLS_MAX_SHRINK percent
#     (default 15).
# New skills/files and growth are fine. Called by gen-skills-bundle.sh against the
# bundle that docs/public/skills/latest/version points at; usable on its own or in CI.
#
# Why: 3.13.151 was staged from source that never received two merged PRs, so it
# dropped tina4-seo and tina4-a11y entirely and cut tina4-design by 27% (its approval
# gates gone) -- and nothing noticed until a user's `tina4 update` regressed them.
# A skill is SKILL.md PLUS its references/ (and checklists/) tree, which carry most
# of the real content, so the guard covers every published file, not just SKILL.md
# (#109) -- a gutted reference file is the same silent regression as a gutted SKILL.md.
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
  # Only real skill directories (SKILL.md present) -- skips latest/ and any
  # non-skill top-level dir.
  [ -f "$dir/SKILL.md" ] || continue
  # Every published file in this skill -- SKILL.md and every reference/checklist
  # file under it -- must still be present and not shrink past the threshold.
  while IFS= read -r f; do
    rel="${f#"$dir"}"
    newf="$new/$skill/$rel"
    if [ ! -f "$newf" ]; then
      echo "  - $skill/$rel: published before, missing now" >&2
      problems=$((problems + 1))
      continue
    fi
    old_size=$(wc -c < "$f")
    new_size=$(wc -c < "$newf")
    # Guard the division; a genuinely empty published file can't shrink.
    if [ "$old_size" -gt 0 ] && [ $((new_size * 100)) -lt $((old_size * (100 - max_shrink))) ]; then
      echo "  - $skill/$rel: shrank $(( (old_size - new_size) * 100 / old_size ))% ($old_size -> $new_size bytes)" >&2
      problems=$((problems + 1))
    fi
  done <<EOF
$(find "$dir" -type f | LC_ALL=C sort)
EOF
done

if [ "$problems" -gt 0 ]; then
  echo "error: $problems regression(s) against $(basename "$prev") -- restore the content in its source repo (see scripts/skills-list.sh for where each skill lives), or pass --allow-regress to gen-skills-bundle.sh if the removal is intentional" >&2
  exit 1
fi
echo "  No regressions against $(basename "$prev")"
