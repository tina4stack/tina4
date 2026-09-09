#!/usr/bin/env bash
# Stage the tina4.com skills bundle -- the FIRST source the installer tries, ahead
# of jsDelivr and raw.githubusercontent. It lands under tina4-documentation's
# docs/public so the existing Jenkins-on-main deploy publishes it to
# https://tina4.com/skills/<ref>/ with no new pipeline.
#
#   scripts/gen-skills-bundle.sh <ref> [--docs DIR] [--ideacloneroot DIR] [--dry-run]
#
# Layout produced (FLAT, stage-relative -- identical to what install-skills.sh
# stages, so serving it and re-staging it is a perfect round-trip):
#
#   <docs>/docs/public/skills/<ref>/
#     skills.sha256                       (byte-identical to the committed tina4/skills.sha256)
#     install-skills.sh                   (inner installer, so the bootstrap can fetch it from tina4.com first)
#     install-skills.ps1                  (signed; served as-is)
#     <skill>/SKILL.md
#     <skill>/references/<file>
#
# Run it at release time with the framework repos checked out at the skills tag,
# then commit docs/public/skills/<ref> in tina4-documentation and push main.
set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
tina4_dir="$(cd "$script_dir/.." && pwd)"

. "$script_dir/skills-list.sh"

ref=""
docs_dir="${TINA4_DOCS_DIR:-$tina4_dir/../tina4-documentation}"
ideacloneroot="${TINA4_IDEA_ROOT:-$(cd "$tina4_dir/.." && pwd)}"
dry_run=0

while [ $# -gt 0 ]; do
  case "$1" in
    --docs) docs_dir="$2"; shift 2 ;;
    --ideacloneroot) ideacloneroot="$2"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^#\{1,\} \{0,1\}//'; exit 0 ;;
    -*) echo "unknown flag: $1" >&2; exit 2 ;;
    *) ref="$1"; shift ;;
  esac
done

if [ -z "$ref" ]; then
  echo "usage: $0 <ref> [--docs DIR] [--ideacloneroot DIR] [--dry-run]" >&2
  exit 2
fi
if ! printf '%s' "$ref" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "error: '$ref' is not a MAJOR.MINOR.PATCH ref" >&2
  exit 2
fi
if [ ! -d "$docs_dir/docs/public" ]; then
  echo "error: tina4-documentation not found at: $docs_dir (point it with --docs)" >&2
  exit 1
fi

bundle="$docs_dir/docs/public/skills/$ref"

if command -v sha256sum >/dev/null 2>&1; then
  verify_cmd="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
  verify_cmd="shasum -a 256 -c"
else
  echo "error: need sha256sum or shasum on PATH to verify the bundle" >&2
  exit 1
fi

echo "Staging tina4.com skills bundle for ref $ref"
echo "  from : $ideacloneroot/<repo>/.claude/skills"
echo "  into : $bundle"

if [ "$dry_run" -eq 1 ]; then
  skills_entries | while IFS='|' read -r repo skill refs; do
    [ -n "$skill" ] || continue
    echo "  would stage $skill  ($repo): SKILL.md${refs:+ + $(printf '%s' "$refs" | wc -w | tr -d ' ') refs}"
  done
  echo "(dry-run -- nothing written)"
  exit 0
fi

# Fresh dir every time so a removed file never lingers in the published bundle.
rm -rf "$bundle"
mkdir -p "$bundle"

copy_file() {  # copy_file <source> <bundle-relative-dest>
  [ -f "$1" ] || { echo "error: missing skill file $1" >&2; exit 1; }
  mkdir -p "$bundle/$(dirname "$2")"
  cp "$1" "$bundle/$2"
}

count=0
skills_entries | while IFS='|' read -r repo skill refs; do
  [ -n "$skill" ] || continue
  root="$ideacloneroot/$repo/.claude/skills/$skill"
  copy_file "$root/SKILL.md" "$skill/SKILL.md"
  for reference in $refs; do
    copy_file "$root/references/$reference" "$skill/references/$reference"
  done
  echo "  + $skill  ($repo)"
done

# The manifest in the bundle MUST be the same generator's output as the committed
# tina4/skills.sha256, so a tina4.com download verifies against the identical bytes.
bash "$script_dir/gen-skills-sha256.sh" "$ideacloneroot" > "$bundle/skills.sha256"

# Inner installers, so the bootstrap wrapper can fetch them from tina4.com first.
copy_file "$tina4_dir/install-skills.sh" "install-skills.sh"
copy_file "$tina4_dir/install-skills.ps1" "install-skills.ps1"

# Prove the staged bytes match the manifest exactly (no missing, no truncated).
staged_count=$(grep -c . "$bundle/skills.sha256")
if ! ( cd "$bundle" && $verify_cmd skills.sha256 ) >/dev/null 2>&1; then
  echo "error: a staged bundle file failed checksum against its own manifest -- refusing" >&2
  exit 1
fi

# The bundle manifest must equal the committed tina4/skills.sha256 (same file set,
# same bytes), so all three tiers verify against one contract.
if [ -f "$tina4_dir/skills.sha256" ] && ! diff -q "$tina4_dir/skills.sha256" "$bundle/skills.sha256" >/dev/null; then
  echo "error: bundle skills.sha256 differs from committed tina4/skills.sha256 -- regenerate the committed manifest first (scripts/gen-skills-sha256.sh > skills.sha256)" >&2
  exit 1
fi

echo ""
echo "  Bundle ready: $staged_count skill files + skills.sha256 + 2 installers"
echo "  Serve check : https://tina4.com/skills/$ref/skills.sha256"
echo "  Next: commit docs/public/skills/$ref in tina4-documentation and push main (Jenkins deploys)."
