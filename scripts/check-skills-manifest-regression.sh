#!/usr/bin/env bash
# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# CI guard (tina4#110): the committed skills.sha256 must never drop a file that
# tina4.com already serves. It runs on every PR that touches the skills source or
# the manifest, so a dropped skill or reference is caught on the PR that drops it,
# not at release time (gen-skills-bundle.sh's check, which needs every source repo
# checked out) or, worse, on a user's `tina4 update`.
#
#   scripts/check-skills-manifest-regression.sh [local-manifest]
#
# Compares the PATH SET of the local manifest (default: the repo's skills.sha256)
# against the published baseline manifest. Fails (exit 1) when a path the baseline
# names is absent from the local one - a whole skill, OR any one of its reference
# files. New paths and growth are fine.
#
# This is the manifest-level guard; it needs nothing but the committed manifest
# and one fetch. check-skills-regression.sh is the complementary file-level guard
# (presence AND shrink) that gen-skills-bundle.sh runs at release with every
# source repo staged. A tina4-repo PR can only regress the manifest/list/tina4-cli
# skill, and all of that is captured here; a content shrink lives in the source
# repos, where the release-time guard still catches it.
#
# Baseline resolution (override any of these for tests / an air-gapped run):
#   TINA4_SKILLS_BASELINE_ROOT      default https://tina4.com/skills
#     -> fetch $ROOT/latest/version  (the published "latest" ref)
#     -> fetch $ROOT/<ref>/skills.sha256
#   TINA4_SKILLS_BASELINE_MANIFEST  a local manifest file to use AS the baseline,
#                                   skipping the fetch entirely.
set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
local_manifest="${1:-$repo_dir/skills.sha256}"
root="${TINA4_SKILLS_BASELINE_ROOT:-https://tina4.com/skills}"

[ -f "$local_manifest" ] || {
  echo "error: local manifest not found: $local_manifest" >&2
  exit 2
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
baseline="$tmp/baseline.sha256"

if [ -n "${TINA4_SKILLS_BASELINE_MANIFEST:-}" ]; then
  [ -f "$TINA4_SKILLS_BASELINE_MANIFEST" ] || {
    echo "error: TINA4_SKILLS_BASELINE_MANIFEST not found: $TINA4_SKILLS_BASELINE_MANIFEST" >&2
    exit 2
  }
  cp "$TINA4_SKILLS_BASELINE_MANIFEST" "$baseline"
  echo "Baseline: $TINA4_SKILLS_BASELINE_MANIFEST (local override)"
else
  ref="$(curl -fsSL --retry 3 "$root/latest/version" | tr -d ' \r\n')" || {
    echo "error: could not resolve the baseline ref from $root/latest/version" >&2
    exit 2
  }
  [ -n "$ref" ] || {
    echo "error: empty baseline ref from $root/latest/version" >&2
    exit 2
  }
  curl -fsSL --retry 3 "$root/$ref/skills.sha256" -o "$baseline" || {
    echo "error: could not fetch the baseline manifest $root/$ref/skills.sha256" >&2
    exit 2
  }
  echo "Baseline: $root/$ref/skills.sha256 (published ref $ref)"
fi

# The path is everything after the 64-hex digest and its separating whitespace.
# A skill path never contains spaces, but strip only the digest so an odd path
# survives intact; drop blank lines; sort for comm.
paths_of() {
  sed -E 's/^[0-9a-fA-F]{64}[[:space:]]+//' "$1" | sed '/^[[:space:]]*$/d' | LC_ALL=C sort
}
paths_of "$baseline" >"$tmp/base_paths"
paths_of "$local_manifest" >"$tmp/local_paths"

dropped="$(comm -23 "$tmp/base_paths" "$tmp/local_paths")"
if [ -n "$dropped" ]; then
  echo >&2
  echo "SKILLS REGRESSION: files tina4.com serves now are missing from ${local_manifest#"$repo_dir"/}:" >&2
  printf '%s\n' "$dropped" | sed 's/^/  - /' >&2
  echo >&2
  echo "A published skill or reference file was dropped from the manifest. Restore it in" >&2
  echo "scripts/skills-list.sh and regenerate (scripts/gen-skills-sha256.sh). If the removal" >&2
  echo "is deliberate, it ships only through a release, where the removal is reviewed." >&2
  exit 1
fi

new_count="$(comm -13 "$tmp/base_paths" "$tmp/local_paths" | grep -c . || true)"
echo "OK: no skills regression - every published file is still in the manifest (${new_count} new path(s) since the baseline)."
