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

ideacloneroot="${1:-$(cd "$(dirname "$0")/../.." && pwd)}"

# Mirror install-skills.sh's skill + reference list EXACTLY. Keep the two in sync:
# a file here that the installer does not fetch (or vice versa) fails verification.
dev_refs="auth-and-services.md data-and-orm.md deployment.md routes-and-api.md templates-and-frontend.md realtime.md web-push.md ai-coder-rule-path.svg"

# entry = "repo|skill|space separated references"
entries="
tina4-python|tina4-developer-python|$dev_refs
tina4-php|tina4-developer-php|$dev_refs
tina4-ruby|tina4-developer-ruby|$dev_refs
tina4-nodejs|tina4-developer-nodejs|$dev_refs
tina4-python|tina4-js|html-and-components.md signals-and-reactivity.md persistence.md rtc.md
tina4-python|tina4-maintainer|cli-and-deployment.md frond-and-frontend.md routing-and-orm.md subsystems.md
tina4-python|tina4-architect|
"

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
  printf '%s\n' "$entries" | while IFS='|' read -r repo skill refs; do
    [ -n "$skill" ] || continue
    root="$ideacloneroot/$repo/.claude/skills/$skill"
    emit "$root/SKILL.md" "$skill/SKILL.md"
    for reference in $refs; do
      emit "$root/references/$reference" "$skill/references/$reference"
    done
  done
} | LC_ALL=C sort -k2
