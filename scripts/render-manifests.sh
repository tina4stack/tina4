#!/usr/bin/env bash
#
# render-manifests.sh <version> <sha256sums-file>
#
# Rewrites every package-manager manifest in packaging/ (and the Homebrew
# formula) to a given release: its version, download URLs, and SHA-256 hashes.
# ONE source of truth for the per-release values, so the manifests can never
# drift from each other or from the release. Pure, deterministic text
# substitution - no network, no side effects beyond editing the tracked files.
#
#   version          the release version WITHOUT a leading v, e.g. 3.8.78
#   sha256sums-file   a local copy of the release's SHA256SUMS asset
#
# Called by .github/workflows/release-published.yml after a release is
# published, and runnable by hand to preview a bump. Verify a render with:
#   git diff -- packaging homebrew
set -euo pipefail

VER="${1:?usage: render-manifests.sh <version> <sha256sums-file>}"
SUMS="${2:?usage: render-manifests.sh <version> <sha256sums-file>}"

case "$VER" in
  v*) echo "error: pass the version WITHOUT a leading 'v' (got '$VER')" >&2; exit 2;;
esac
[ -f "$SUMS" ] || { echo "error: SHA256SUMS file not found: $SUMS" >&2; exit 2; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Pull each asset's hash out of the SHA256SUMS manifest by exact filename.
hash_for() {
  local name="$1" h
  h="$(awk -v n="$name" '$2==n{print $1}' "$SUMS")"
  [ -n "$h" ] || { echo "error: no SHA-256 for '$name' in $SUMS" >&2; exit 3; }
  printf '%s' "$h"
}
upper() { printf '%s' "$1" | tr 'a-f' 'A-F'; }

WIN="$(hash_for tina4-windows-amd64.exe)"; WINU="$(upper "$WIN")"
LAMD="$(hash_for tina4-linux-amd64)"
LARM="$(hash_for tina4-linux-arm64)"
DAMD="$(hash_for tina4-darwin-amd64)"
DARM="$(hash_for tina4-darwin-arm64)"

echo "Rendering manifests for v$VER"
echo "  windows-amd64 $WIN"

# in-place sed that works the same on GNU and BSD sed (CI is Linux; a
# maintainer may run this on macOS).
sedi() { if sed --version >/dev/null 2>&1; then sed -i "$@"; else sed -i '' "$@"; fi; }

# ERE (we use `sed -E`): a SHA-256 hex string and a semver.
H='[0-9a-fA-F]{64}'
V='[0-9]+\.[0-9]+\.[0-9]+'

# ---- Scoop ---------------------------------------------------------------
f="$ROOT/packaging/scoop/tina4.json"
sedi -E "s#(\"version\": \")$V(\")#\1$VER\2#" "$f"
# Only the real download URL carries a numeric version; the autoupdate URL uses
# the literal \$version token, so $V cannot match it. Match up to the asset name
# to avoid the '#/tina4.exe' rename fragment (which would clash with sed's #).
sedi -E "s#(releases/download/v)$V(/tina4-windows-amd64)#\1$VER\2#" "$f"
sedi -E "s#(\"hash\": \")$H(\")#\1$WIN\2#" "$f"

# ---- Chocolatey ----------------------------------------------------------
f="$ROOT/packaging/chocolatey/tina4.nuspec"
sedi -E "s#(<version>)$V(</version>)#\1$VER\2#" "$f"
sedi -E "s#(releases/tag/v)$V#\1$VER#" "$f"
f="$ROOT/packaging/chocolatey/tools/chocolateyinstall.ps1"
sedi -E "s#(\\\$version *= *')$V(')#\1$VER\2#" "$f"
sedi -E "s#(\\\$checksum64 *= *')$H(')#\1$WINU\2#" "$f"
f="$ROOT/packaging/chocolatey/tools/VERIFICATION.txt"
sedi -E "s#(releases/download/v)$V#\1$VER#g" "$f"
sedi -E "s#^  $H\$#  $WINU#" "$f"

# ---- winget --------------------------------------------------------------
f="$ROOT/packaging/winget/Tina4Stack.Tina4.installer.yaml"
sedi -E "s#(PackageVersion: )$V#\1$VER#" "$f"
sedi -E "s#(releases/download/v)$V#\1$VER#" "$f"
sedi -E "s#(InstallerSha256: )$H#\1$WINU#" "$f"
for f in "$ROOT/packaging/winget/Tina4Stack.Tina4.locale.en-US.yaml" \
         "$ROOT/packaging/winget/Tina4Stack.Tina4.yaml"; do
  sedi -E "s#(PackageVersion: )$V#\1$VER#" "$f"
done

# ---- Homebrew ------------------------------------------------------------
f="$ROOT/homebrew/tina4.rb"
sedi -E "s#(version \")$V(\")#\1$VER\2#" "$f"
sedi -E "s#(download/v)$V(/tina4-darwin-arm64\")#\1$VER\2#" "$f"
sedi -E "s#(download/v)$V(/tina4-darwin-amd64\")#\1$VER\2#" "$f"
sedi -E "s#(download/v)$V(/tina4-linux-arm64\")#\1$VER\2#" "$f"
sedi -E "s#(download/v)$V(/tina4-linux-amd64\")#\1$VER\2#" "$f"
# The four sha256 lines sit under their url lines in on_macos/on_linux order:
#   darwin arm64, darwin amd64, linux arm64, linux amd64.
awk -v darm="$DARM" -v damd="$DAMD" -v larm="$LARM" -v lamd="$LAMD" '
  /sha256 "/ { n++
    if (n==1) sub(/"[0-9a-fA-F]{64}"/, "\"" darm "\"")
    else if (n==2) sub(/"[0-9a-fA-F]{64}"/, "\"" damd "\"")
    else if (n==3) sub(/"[0-9a-fA-F]{64}"/, "\"" larm "\"")
    else if (n==4) sub(/"[0-9a-fA-F]{64}"/, "\"" lamd "\"")
  }
  { print }
' "$f" > "$f.tmp" && mv "$f.tmp" "$f"

echo "Done. Review with: git diff -- packaging homebrew"
