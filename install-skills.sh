#!/usr/bin/env bash
# Tina4 AI skills installer for macOS / Linux.
#
# Choose a target explicitly:
#   curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET=claude sh
#   curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET=codex sh
#   curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET=cursor sh
# Use TINA4_SKILLS_TARGET=all only when every supported tool should receive the skills.
#
# POSIX sh ONLY -- no bashisms. Every example above pipes into `sh`, and on
# Debian/Ubuntu that is dash. This script used `set -euo pipefail` and bash
# arrays, so the DOCUMENTED command died on line 9 with
# "set: Illegal option -o pipefail" and installed nothing. It worked on macOS,
# where /bin/sh is bash in POSIX mode and accepts pipefail, which is exactly why
# it survived: the break was invisible to anyone testing on a Mac.
#
# pipefail is not replaced with anything. Every download below uses `curl -f`,
# so a failed fetch is a non-zero exit that `set -e` already catches.
set -eu

# Pin skills to a released tag, not a moving branch, so an install is reproducible.
# Bump this when the skills change in a new release. Override with TINA4_SKILLS_REF.
ref="${TINA4_SKILLS_REF:-3.13.121}"
target="${TINA4_SKILLS_TARGET:-}"
skill_home="${TINA4_SKILLS_HOME:-$HOME}"
primary_root="${TINA4_SKILLS_PRIMARY_ROOT:-https://raw.githubusercontent.com/tina4stack}"
mirror_root="${TINA4_SKILLS_MIRROR_ROOT:-https://cdn.jsdelivr.net/gh/tina4stack}"
retry_count="${TINA4_SKILLS_RETRY_COUNT:-3}"
retry_delay="${TINA4_SKILLS_RETRY_DELAY:-2}"

# Space separated, not an array: dash has no arrays. Neither path can contain a
# space, because both are literals under $HOME.
case "$target" in
  claude) destinations="$skill_home/.claude/skills" ;;
  codex)  destinations="$skill_home/.agents/skills" ;;
  cursor) destinations="$skill_home/.cursor/skills" ;;
  all)    destinations="$skill_home/.claude/skills $skill_home/.agents/skills $skill_home/.cursor/skills" ;;
  *)
    echo "error: set TINA4_SKILLS_TARGET to claude, codex, cursor, or all" >&2
    exit 2
    ;;
esac

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

# download_file <destination> <primary-url> <fallback-url>
download_file() {
  destination="$1"; shift
  for url in "$@"; do
    if curl -fsSL --retry "$retry_count" --retry-delay "$retry_delay" "$url" -o "$destination"; then
      return 0
    fi
    rm -f "$destination"
    echo "  ! download failed, trying next source: $url" >&2
  done
  echo "error: every download source failed for $destination" >&2
  return 1
}

# install_skill <repo> <skill> <reference.md ...>
install_skill() {
  repo="$1"; skill="$2"; shift 2
  base="${primary_root}/${repo}/${ref}/.claude/skills"
  mirror="${mirror_root}/${repo}@${ref}/.claude/skills"
  mkdir -p "$stage/$skill/references"
  download_file "$stage/$skill/SKILL.md" \
    "$base/$skill/SKILL.md" "$mirror/$skill/SKILL.md"
  for reference in "$@"; do
    download_file "$stage/$skill/references/$reference" \
      "$base/$skill/references/$reference" "$mirror/$skill/references/$reference"
  done
  echo "  + $skill  ($repo)"
}

publish_skills() {
  for destination in $destinations; do
    mkdir -p "$destination"
    for legacy_skill in $LEGACY_SKILLS; do
      if [ -e "$destination/$legacy_skill" ]; then
        rm -rf "$destination/$legacy_skill"
        echo "  - removed legacy $legacy_skill"
      fi
    done
    for source in "$stage"/*; do
      skill="$(basename "$source")"
      replacement="$destination/.${skill}.tina4-new"
      rm -rf "$replacement"
      cp -R "$source" "$replacement"
      rm -rf "$destination/$skill"
      mv "$replacement" "$destination/$skill"
    done
    printf '%s\n' "$ref" > "$destination/.tina4-skills-ref"
    echo "  installed for $destination"
  done
}

# Every file under references/, not most of them. ai-coder-rule-path.svg was
# missing, so a SUCCESSFUL install still produced an incomplete skill -- the
# quiet half of this bug, which no error would ever have reported.
DEV_REFS="auth-and-services.md data-and-orm.md deployment.md routes-and-api.md templates-and-frontend.md realtime.md ai-coder-rule-path.svg"
LEGACY_SKILLS="tina4-developer"

echo ""
echo "  Tina4 Skills Installer"
echo "  Target: $target  (ref: $ref)"
echo ""

# Per-language developer skills (each from its own framework repo).
install_skill tina4-python  tina4-developer-python  $DEV_REFS
install_skill tina4-php     tina4-developer-php     $DEV_REFS
install_skill tina4-ruby    tina4-developer-ruby    $DEV_REFS
install_skill tina4-nodejs  tina4-developer-nodejs  $DEV_REFS
# Shared skills (canonical copy served from tina4-python).
install_skill tina4-python  tina4-js          html-and-components.md signals-and-reactivity.md persistence.md rtc.md
install_skill tina4-python  tina4-maintainer  cli-and-deployment.md frond-and-frontend.md routing-and-orm.md subsystems.md
install_skill tina4-python  tina4-architect

publish_skills

echo ""
echo "  Done - seven skills installed for $target (ref $ref). Restart your coding tool to pick them up."
