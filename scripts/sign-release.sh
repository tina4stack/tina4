#!/bin/sh
# FALLBACK signer for macOS / Linux. The Windows signtool path
# (scripts/sign-release.ps1) is primary and best-trodden for Authenticode;
# use this only when you cannot sign on Windows.
#
# Signs the Windows tina4-windows-amd64.exe with the Code Infinity EV cert via
# osslsigncode against the SimplySign / Certum cloud key (PKCS#11). You enter
# the SimplySign 2FA yourself (cloud card mounted) - no OTP seed is stored.
# It then re-uploads the signed .exe, regenerates SHA256SUMS over the signed
# bytes, and publishes the draft release.
#
# PREREQUISITES:
#   - SimplySign Desktop installed and LOGGED IN (your 2FA; cloud card mounted)
#   - osslsigncode installed (brew install osslsigncode)
#   - gh (GitHub CLI) installed and authenticated
#   - These env vars (paths are specific to your SimplySign install):
#       TINA4_PKCS11_MODULE  path to the SimplySign/Certum PKCS#11 library
#                            (.dylib on macOS, .so on Linux)
#       TINA4_SIGN_CERT      path to the Code Infinity certificate (PEM)
#       TINA4_KEY_ID         the PKCS#11 key id/label for the cert
#   Optional: TINA4_TS_URL (default http://time.certum.pl/)
#
# USAGE:  sh scripts/sign-release.sh v3.8.53
set -eu

TAG="${1:-}"
[ -z "$TAG" ] && { echo "Usage: sh scripts/sign-release.sh <tag>   (e.g. v3.8.53)" >&2; exit 1; }
BINARY="tina4-windows-amd64.exe"
TS_URL="${TINA4_TS_URL:-http://time.certum.pl/}"

# --- prerequisite checks (fail loud, fail helpful) ---
for tool in gh osslsigncode; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Error: '$tool' is required (see the header of this script)" >&2; exit 1; }
done
# Auto-detect the standard SimplySign macOS PKCS#11 module if not overridden.
if [ -z "${TINA4_PKCS11_MODULE:-}" ]; then
  for cand in \
    /usr/local/lib/libSimplySignPKCS.dylib \
    /Applications/proCertumSmartSign.app/Contents/MacOS/libSimplySignPKCS.dylib; do
    [ -f "$cand" ] && TINA4_PKCS11_MODULE="$cand" && break
  done
fi
: "${TINA4_PKCS11_MODULE:?Set TINA4_PKCS11_MODULE to the SimplySign PKCS#11 library path}"
: "${TINA4_SIGN_CERT:?Set TINA4_SIGN_CERT to the Code Infinity certificate PEM path}"
: "${TINA4_KEY_ID:?Set TINA4_KEY_ID to the PKCS#11 key id/label}"
[ -f "$TINA4_PKCS11_MODULE" ] || { echo "Error: PKCS#11 module not found: $TINA4_PKCS11_MODULE" >&2; exit 1; }
[ -f "$TINA4_SIGN_CERT" ]     || { echo "Error: cert PEM not found: $TINA4_SIGN_CERT" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "Downloading draft release assets for $TAG ..."
gh release download "$TAG" --dir . --clobber
[ -f "$BINARY" ] || { echo "Error: $BINARY not found in release $TAG" >&2; exit 1; }

echo "Signing $BINARY (SimplySign must be logged in) ..."
osslsigncode sign \
  -pkcs11module "$TINA4_PKCS11_MODULE" \
  -certs "$TINA4_SIGN_CERT" \
  -key "$TINA4_KEY_ID" \
  -h sha256 \
  -ts "$TS_URL" \
  -in "$BINARY" -out "${BINARY}.signed"
mv -f "${BINARY}.signed" "$BINARY"

echo "Verifying signature ..."
osslsigncode verify "$BINARY"

echo "Uploading signed $BINARY ..."
gh release upload "$TAG" "$BINARY" --clobber

echo "Regenerating SHA256SUMS over the signed assets ..."
rm -f SHA256SUMS
if command -v sha256sum >/dev/null 2>&1; then
  # exclude SHA256SUMS itself; stable order
  for f in $(ls -1 | grep -v '^SHA256SUMS$' | sort); do sha256sum "$f"; done > SHA256SUMS
else
  for f in $(ls -1 | grep -v '^SHA256SUMS$' | sort); do
    printf '%s  %s\n' "$(shasum -a 256 "$f" | awk '{print $1}')" "$f"
  done > SHA256SUMS
fi
cat SHA256SUMS
gh release upload "$TAG" SHA256SUMS --clobber

echo "Publishing release $TAG ..."
gh release edit "$TAG" --draft=false

echo ""
echo "Done: $TAG signed (EV), checksummed over signed bytes, and published."
