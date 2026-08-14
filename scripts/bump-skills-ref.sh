#!/usr/bin/env bash
# Bump the AI-skills install pin (the TINA4_SKILLS_REF default) to a release
# version across the canonical installers in one command:
#
#   - tina4/install-skills.sh   + install-skills.ps1   (canonical, raw-hosted)
#   - tina4.com serves bootstrap wrappers that fetch these canonical files from
#     main, so those wrappers intentionally contain no framework-version pin.
#
# The pin tracks the FRAMEWORK release version (3.13.x), NOT this CLI's version
# (3.8.x). It exists so `curl ... | sh` installs the skills from a tested tag.
#
# RUN IT AS A STEP WHEN CUTTING A FRAMEWORK RELEASE, *AFTER* THE TAG IS LIVE.
# A pin pointing at a tag that does not exist yet makes every fresh install 404.
# The tina4.com wrappers pick up the new canonical pin as soon as this repo is
# pushed; they only need a docs deploy when their retry/bootstrap logic changes.
#
# Usage:
#   scripts/bump-skills-ref.sh 3.13.65
#   scripts/bump-skills-ref.sh --dry-run 3.13.65
set -eu

dry_run=0
version=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    -h|--help) grep '^#' "$0" | sed 's/^#\{1,\} \{0,1\}//'; exit 0 ;;
    -*) echo "unknown flag: $arg" >&2; exit 2 ;;
    *) version="$arg" ;;
  esac
done

if [ -z "$version" ]; then
  echo "usage: $0 [--dry-run] <version>   e.g. $0 3.13.65" >&2
  exit 2
fi
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "error: '$version' is not a MAJOR.MINOR.PATCH version" >&2
  exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tina4_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)

files="
$tina4_dir/install-skills.sh
$tina4_dir/install-skills.ps1
"

# Pull the currently-pinned version out of a file (sh vs ps1 shapes differ).
current_ref() {
  case "$1" in
    *.sh)  sed -nE 's/.*TINA4_SKILLS_REF:-([0-9]+\.[0-9]+\.[0-9]+).*/\1/p' "$1" | head -1 ;;
    *.ps1) sed -nE 's/.*else \{ "([0-9]+\.[0-9]+\.[0-9]+)" \}.*/\1/p' "$1" | head -1 ;;
  esac
}

changed=0
missing=0
for f in $files; do
  if [ ! -f "$f" ]; then
    echo "MISSING  $f"
    missing=1
    continue
  fi
  cur=$(current_ref "$f")
  if [ -z "$cur" ]; then
    echo "WARN     $f (no TINA4_SKILLS_REF default found — not touched)"
    missing=1
    continue
  fi
  if [ "$cur" = "$version" ]; then
    echo "ok       $f (already $version)"
    continue
  fi
  if [ "$dry_run" -eq 1 ]; then
    echo "would    $f  $cur -> $version"
    changed=1
    continue
  fi
  case "$f" in
    *.sh)  sed -i.bak -E "s/(TINA4_SKILLS_REF:-)[0-9]+\.[0-9]+\.[0-9]+/\1$version/g" "$f" ;;
    *.ps1) sed -i.bak -E "s/(else \{ \")[0-9]+\.[0-9]+\.[0-9]+(\")/\1$version\2/g" "$f" ;;
  esac
  rm -f "$f.bak"
  echo "bumped   $f  $cur -> $version"
  changed=1
done

if [ "$missing" -eq 1 ]; then
  echo "error: one or more install-skills files were missing or unparseable — pin only partially applied" >&2
  exit 1
fi
if [ "$dry_run" -eq 1 ]; then
  [ "$changed" -eq 1 ] && echo "(dry-run — nothing written)"
  exit 0
fi
if [ "$changed" -eq 0 ]; then
  echo "all copies already at $version — nothing to do"
  exit 0
fi

echo
echo "Pin bumped to $version. Next:"
echo "  1. commit tina4/install-skills.{sh,ps1}"
echo "  2. confirm the $version tag exists on tina4stack/tina4-python before this ships"
