#!/bin/sh
# macOS Authenticode signer for the tina4 Windows CLI binary, via JSIGN against
# the Certum SimplySign CLOUD key (PKCS#11). This is the PROVEN macOS path.
#
# WHY jsign, not osslsigncode: osslsigncode + the OpenSSL libp11 engine FAILS on
# the SimplySign *cloud* module - the pkcs11 provider won't load, and the legacy
# engine path dies with "PKCS#11 module: Attribute type invalid" (the cloud HSM
# rejects the RSA sign the engine issues). jsign talks PKCS#11 directly (no
# OpenSSL engine) and signs cleanly. tina4 v3.8.55 was signed on macOS this way.
# scripts/sign-release.sh (osslsigncode) is kept only for a physical-card / Linux
# setup; on the SimplySign cloud module use THIS script.
#
# It stores NO secret: the EV private key never leaves the SimplySign cloud HSM.
# You log into SimplySign Desktop yourself (your 2FA); this points jsign at that
# live session, signs the release's Windows .exe, verifies, regenerates
# SHA256SUMS over the signed bytes, and publishes the draft release.
#
#   usage:  sh sign-mac.sh v3.8.56            (sign + publish)
#           sh sign-mac.sh v3.8.56 --check    (preview resolved config; no signing)
#
# PREREQUISITES:
#   - SimplySign Desktop installed and LOGGED IN (cloud card mounted; your 2FA)
#   - brew install jsign          (jsign 7.x; pulls a JDK)
#   - brew install osslsigncode   (verify step only) + gh (authenticated)
#
# OVERRIDES (env): TINA4_PKCS11_MODULE, TINA4_SIGN_ALIAS, TINA4_TS_URL, TINA4_REPO
set -eu

cd "$(dirname "$0")"                        # tina4 repo root
# Args: <tag> plus optional --check/--dry-run (in any position). --check resolves
# and prints the config WITHOUT signing or publishing, so you can confirm before
# spending your SimplySign session.
TAG=""
CHECK=0
for arg in "$@"; do
  case "$arg" in
    --check|--dry-run|-n) CHECK=1 ;;
    -*) echo "Unknown option: $arg" >&2; exit 1 ;;
    *)  TAG="$arg" ;;
  esac
done
[ -z "$TAG" ] && { echo "Usage: sh sign-mac.sh <tag> [--check]   (e.g. v3.8.56)" >&2; exit 1; }
BINARY="tina4-windows-amd64.exe"
TSA_URL="${TINA4_TS_URL:-http://time.certum.pl/}"

command -v jsign >/dev/null 2>&1 || { echo "Error: 'jsign' required -> brew install jsign" >&2; exit 1; }
command -v gh    >/dev/null 2>&1 || { echo "Error: 'gh' required (authenticated)" >&2; exit 1; }

# SimplySign cloud PKCS#11 module. Resolve the symlink to the concrete versioned
# .dylib - SunPKCS11 (jsign's provider) wants the real file, not the symlink.
MODULE="${TINA4_PKCS11_MODULE:-/usr/local/lib/libSimplySignPKCS.dylib}"
[ -f "$MODULE" ] || { echo "Error: PKCS#11 module not found: $MODULE" >&2; exit 1; }
if [ -L "$MODULE" ]; then
  _l="$(readlink "$MODULE")"
  case "$_l" in
    /*) MODULE="$_l" ;;
    *)  MODULE="$(cd "$(dirname "$MODULE")" && pwd)/$_l" ;;
  esac
  echo "Resolved PKCS#11 symlink -> $MODULE"
fi

# Cert alias = the token's CKA_LABEL for the Code Infinity EV cert. If opensc's
# pkcs11-tool is present, read it straight off the token so a cert REISSUE (new
# label) does not silently break signing; otherwise fall back to the known label.
# jsign pulls the signing certificate (and any chain the token holds) from the
# token itself, so no cert PEM needs to live on disk.
if [ -z "${TINA4_SIGN_ALIAS:-}" ] && command -v pkcs11-tool >/dev/null 2>&1; then
  _lbl="$(pkcs11-tool --module "$MODULE" --list-objects --type cert 2>/dev/null \
          | grep -iE '^[[:space:]]*label:' | head -1 | sed -E 's/.*label:[[:space:]]*//; s/[[:space:]]*$//')"
  [ -n "$_lbl" ] && { TINA4_SIGN_ALIAS="$_lbl"; echo "Auto-discovered cert alias from token: $_lbl"; }
fi
ALIAS="${TINA4_SIGN_ALIAS:-521D88BF7DC9159EE3445861DB1261C6}"

echo "Signing $TAG with:"
echo "  module: $MODULE"
echo "  alias:  $ALIAS"
echo "  tsa:    $TSA_URL   (Authenticode mode)"
echo "  (SimplySign Desktop must be logged in - your 2FA)"
echo ""

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cat > "$WORK/pkcs11.cfg" <<CFG
name = SimplySign
library = $MODULE
CFG

REPO="${TINA4_REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)}"
[ -n "$REPO" ] || REPO="tina4stack/tina4"
echo "Target repo: $REPO"

if [ "$CHECK" -eq 1 ]; then
  echo ""
  echo "  --check OK: config resolved (module, alias, repo, timestamp); jsign + gh present."
  echo "  Nothing signed or published. Re-run without --check to sign + publish $TAG."
  exit 0
fi

cd "$WORK"
echo "Downloading draft release assets for $TAG ..."
gh release download "$TAG" --repo "$REPO" --dir . --clobber
[ -f "$BINARY" ] || { echo "Error: $BINARY not found in release $TAG" >&2; exit 1; }

echo "Signing $BINARY via jsign ..."
# --storepass "" : the cloud session has no PIN (the 2FA gate is SimplySign Desktop).
# --tsmode AUTHENTICODE : Certum's proven timestamp mode (their RFC3161 endpoint
#   is flaky; AUTHENTICODE is what worked for v3.8.55). jsign signs in place.
jsign --storetype PKCS11 --keystore "$WORK/pkcs11.cfg" --storepass "" \
      --alias "$ALIAS" --tsmode AUTHENTICODE --tsaurl "$TSA_URL" "$BINARY"

echo "Verifying signature ..."
if command -v osslsigncode >/dev/null 2>&1; then
  osslsigncode verify "$BINARY"
else
  echo "  (osslsigncode not installed - skipping local verify; brew install osslsigncode to verify)"
fi

echo "Uploading signed $BINARY ..."
gh release upload "$TAG" "$BINARY" --repo "$REPO" --clobber

echo "Regenerating SHA256SUMS over the signed assets ..."
rm -f SHA256SUMS
if command -v sha256sum >/dev/null 2>&1; then
  for f in $(ls -1 | grep -v '^SHA256SUMS$' | sort); do sha256sum "$f"; done > SHA256SUMS
else
  for f in $(ls -1 | grep -v '^SHA256SUMS$' | sort); do
    printf '%s  %s\n' "$(shasum -a 256 "$f" | awk '{print $1}')" "$f"
  done > SHA256SUMS
fi
cat SHA256SUMS
gh release upload "$TAG" SHA256SUMS --repo "$REPO" --clobber

echo "Publishing release $TAG ..."
gh release edit "$TAG" --repo "$REPO" --draft=false

echo ""
echo "Done: $TAG signed (EV, jsign), checksummed over signed bytes, and published."
