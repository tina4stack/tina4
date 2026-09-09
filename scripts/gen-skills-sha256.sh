#!/usr/bin/env bash
# Generate skills.sha256 -- the checksum manifest install-skills.sh verifies every
# downloaded skill file against. Run this at release time with the framework repos
# checked out at the skills tag, then commit skills.sha256 alongside install-skills.sh.
#
#   scripts/gen-skills-sha256.sh [IDEAROOT] > skills.sha256
#
# IDEAROOT is the directory holding the framework checkouts (default: the parent of
# this repo, i.e. ~/IdeaProjects). Paths in the manifest are relative to the install
# stage (<skill>/SKILL.md, <skill>/references/<file>), so they match exactly what
# install-skills.sh stages before it verifies.
set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
ideacloneroot="${1:-$(cd "$script_dir/../.." && pwd)}"

# The skill + reference list is shared with gen-skills-bundle.sh so the manifest
# and the tina4.com bundle can never disagree. install-skills.sh keeps its own
# copy (it ships standalone); the installer verifies against THIS manifest, so a
# drifted installer fails loud at `sha256 -c` rather than silently.
. "$script_dir/skills-list.sh"

if command -v sha256sum >/dev/null 2>&1; then
  hash_of() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  hash_of() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  echo "error: need sha256sum or shasum on PATH" >&2
  exit 1
fi

emit() {   # emit <source-file> <stage-relative-path>
  [ -f "$1" ] || { echo "error: missing skill file $1" >&2; exit 1; }
  printf '%s  %s\n' "$(hash_of "$1")" "$2"
}

{
  skills_entries | while IFS='|' read -r repo skill refs; do
    [ -n "$skill" ] || continue
    root="$ideacloneroot/$repo/.claude/skills/$skill"
    emit "$root/SKILL.md" "$skill/SKILL.md"
    for reference in $refs; do
      emit "$root/references/$reference" "$skill/references/$reference"
    done
  done
} | LC_ALL=C sort -k2
