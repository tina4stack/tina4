#!/usr/bin/env bash
# Tina4 AI skills installer for macOS / Linux.
#
# Choose a target explicitly:
#   curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET=claude sh
#   curl -fsSL https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.sh | TINA4_SKILLS_TARGET=codex sh
# Use TINA4_SKILLS_TARGET=all only when both tools should receive the skills.
set -euo pipefail

# Pin skills to a released tag, not a moving branch, so an install is reproducible.
# Bump this when the skills change in a new release. Override with TINA4_SKILLS_REF.
ref="${TINA4_SKILLS_REF:-3.13.77}"
target="${TINA4_SKILLS_TARGET:-}"

case "$target" in
  claude) destinations=("$HOME/.claude/skills") ;;
  codex)  destinations=("$HOME/.agents/skills") ;;
  all)    destinations=("$HOME/.claude/skills" "$HOME/.agents/skills") ;;
  *)
    echo "error: set TINA4_SKILLS_TARGET to claude, codex, or all" >&2
    exit 2
    ;;
esac

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

# install_skill <repo> <skill> <reference.md ...>
install_skill() {
  repo="$1"; skill="$2"; shift 2
  base="https://raw.githubusercontent.com/tina4stack/${repo}/${ref}/.claude/skills"
  mkdir -p "$stage/$skill/references"
  curl -fsSL "$base/$skill/SKILL.md" -o "$stage/$skill/SKILL.md"
  for reference in "$@"; do
    curl -fsSL "$base/$skill/references/$reference" -o "$stage/$skill/references/$reference"
  done
  echo "  + $skill  ($repo)"
}

publish_skills() {
  for destination in "${destinations[@]}"; do
    mkdir -p "$destination"
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

DEV_REFS="auth-and-services.md data-and-orm.md deployment.md routes-and-api.md templates-and-frontend.md realtime.md"

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

publish_skills

echo ""
echo "  Done - six skills installed for $target (ref $ref). Restart your coding tool to pick them up."
