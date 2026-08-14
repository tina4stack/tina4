#!/usr/bin/env bash
# Bump the AI-skills install pin (the TINA4_SKILLS_REF default) to a release
# version across the canonical installers and public bootstrap wrappers:
#
#   - tina4/install-skills.sh   + install-skills.ps1   (canonical, raw-hosted)
#   - tina4-documentation/docs/public/install-skills.{sh,ps1} (tina4.com wrappers)
#
# The pin tracks the FRAMEWORK release version (3.13.x), NOT this CLI's version
# (3.8.x). It exists so `curl ... | sh` installs the skills from a tested tag.
#
# RUN IT AS A STEP WHEN CUTTING A FRAMEWORK RELEASE, *AFTER* THE TAG IS LIVE.
# A pin pointing at a tag that does not exist yet makes every fresh install 404.
# The tina4.com wrappers fetch the matching immutable bare tag from this repo.
# Create that tag before deploying the wrappers, or every public install is a 404.
#
# Usage:
#   scripts/bump-skills-ref.sh 3.13.65
#   scripts/bump-skills-ref.sh --dry-run 3.13.65
#   TINA4_DOCS_DIR=/path/to/tina4-documentation scripts/bump-skills-ref.sh 3.13.65
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
docs_dir="${TINA4_DOCS_DIR:-$tina4_dir/../tina4-documentation}"

if [ ! -d "$docs_dir/docs/public" ]; then
  echo "error: tina4-documentation not found at: $docs_dir" >&2
  echo "       point it explicitly with TINA4_DOCS_DIR" >&2
  exit 1
fi

files="
$tina4_dir/install-skills.sh
$tina4_dir/install-skills.ps1
$docs_dir/docs/public/install-skills.sh
$docs_dir/docs/public/install-skills.ps1
"

# Pull either the canonical content pin or the public bootstrap tag.
current_ref() {
  sed -nE \
    -e 's/.*TINA4_SKILLS_REF:-([0-9]+\.[0-9]+\.[0-9]+).*/\1/p' \
    -e 's/.*else \{ "([0-9]+\.[0-9]+\.[0-9]+)" \}.*/\1/p' \
    -e 's|.*tina4stack/tina4[@/]([0-9]+\.[0-9]+\.[0-9]+)/install-skills.*|\1|p' \
    "$1" | head -1
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
    echo "WARN     $f (no skills release pin found — not touched)"
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
  sed -i.bak -E \
    -e "s/(TINA4_SKILLS_REF:-)[0-9]+\.[0-9]+\.[0-9]+/\1$version/g" \
    -e "s/(else \{ \")[0-9]+\.[0-9]+\.[0-9]+(\")/\1$version\2/g" \
    -e "s|(tina4stack/tina4[@/])[0-9]+\.[0-9]+\.[0-9]+(/install-skills)|\1$version\2|g" \
    "$f"
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
echo "  1. sign and commit tina4/install-skills.ps1 after its final edit"
echo "  2. create the bare $version tag in tina4 and all framework repos"
echo "  3. commit and deploy tina4-documentation/docs/public/install-skills.{sh,ps1}"
