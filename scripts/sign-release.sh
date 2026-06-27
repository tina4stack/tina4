#!/bin/sh
# FALLBACK signer for macOS / Linux. The Windows signtool path
# (scripts/sign-release.ps1) is primary and best-trodden for Authenticode;
# use this only when you cannot sign on Windows.
#
# CAVEAT (macOS): signing a Windows PE through the SimplySign *cloud* PKCS#11
# .dylib has no verified public success story - the well-trodden osslsigncode +
# Certum recipes run on Linux against the *physical-card* module. Treat a macOS
# run as best-effort and keep a Windows box ready as the fallback. The EV key
# lives only in the cloud HSM (non-exportable) and a SimplySign session lasts
# ~2 hours, so a wrong flag burns the session - do discovery + signing in one go.
#
# Signs the Windows tina4-windows-amd64.exe with the Code Infinity EV cert via
# osslsigncode against the SimplySign / Certum cloud key (PKCS#11). You enter
# the SimplySign 2FA yourself (cloud card mounted) - no OTP seed is stored.
# It then re-uploads the signed .exe, regenerates SHA256SUMS over the signed
# bytes, and publishes the draft release.
#
# PREREQUISITES:
#   - SimplySign Desktop installed and LOGGED IN (your 2FA; cloud card mounted)
#   - osslsigncode + the pkcs11 engine + opensc (macOS):
#       brew install osslsigncode libp11 opensc
#     (Debian: apt-get install osslsigncode libengine-pkcs11-openssl opensc)
#   - gh (GitHub CLI) installed and authenticated
#   - These env vars (paths are specific to your SimplySign install):
#       TINA4_PKCS11_MODULE  path to the SimplySign/Certum PKCS#11 library
#                            (.dylib on macOS, .so on Linux)
#       TINA4_SIGN_CERT      path to a PEM holding the FULL CHAIN: the Code
#                            Infinity leaf cert FIRST, then the Certum
#                            intermediate(s) (omit the root - it is in the OS
#                            trust store). Leaf-only signing fails chain
#                            validation on verifiers.
#       TINA4_KEY_ID         the PKCS#11 key id (hex, with or without colons -
#                            this script wraps it into a pkcs11: URI), or a full
#                            pkcs11: URI. On SimplySign the cert shares the key's
#                            CKA_ID; read it (NO --login: the cloud card has no
#                            PIN, so --login fails) with:
#                              pkcs11-tool --module <lib> --list-objects --type cert
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
# Resolve a symlinked module to its real target. Some macOS PKCS#11 tooling
# rejects a symlinked provider under /usr/local/lib (OpenSC #1008), and the
# stock SimplySign install ships libSimplySignPKCS.dylib as a symlink to the
# versioned real file - so point osslsigncode at the resolved path.
if [ -L "$TINA4_PKCS11_MODULE" ]; then
  _link="$(readlink "$TINA4_PKCS11_MODULE")"
  case "$_link" in
    /*) TINA4_PKCS11_MODULE="$_link" ;;
    *)  TINA4_PKCS11_MODULE="$(cd "$(dirname "$TINA4_PKCS11_MODULE")" && pwd)/$_link" ;;
  esac
  echo "Resolved PKCS#11 module symlink -> $TINA4_PKCS11_MODULE"
fi
[ -f "$TINA4_SIGN_CERT" ]     || { echo "Error: cert PEM not found: $TINA4_SIGN_CERT" >&2; exit 1; }

# osslsigncode signs through OpenSSL's pkcs11 ENGINE (libp11). On OpenSSL 3 the
# engine is not built in - if OPENSSL_ENGINES is unset, locate libp11's
# pkcs11.{dylib,so} and point OpenSSL at it. Without this, osslsigncode fails
# with "Failed to find and load 'pkcs11' engine".
if [ -z "${OPENSSL_ENGINES:-}" ]; then
  for d in /opt/homebrew/lib/engines-3 /usr/local/lib/engines-3 \
           "$(brew --prefix libp11 2>/dev/null)/lib/engines-3" \
           /usr/lib/engines-3 /usr/lib/*/engines-3 /usr/lib/ssl/engines-3; do
    if [ -f "$d/pkcs11.dylib" ] || [ -f "$d/pkcs11.so" ]; then
      OPENSSL_ENGINES="$d"; export OPENSSL_ENGINES; break
    fi
  done
fi
if [ -z "${OPENSSL_ENGINES:-}" ]; then
  echo "Error: PKCS#11 engine (libp11) not found. Install it:" >&2
  echo "  macOS:  brew install libp11" >&2
  echo "  Debian: apt-get install libengine-pkcs11-openssl" >&2
  echo "  (or set OPENSSL_ENGINES to the dir holding pkcs11.dylib/.so)" >&2
  exit 1
fi
echo "Using PKCS#11 engine dir: $OPENSSL_ENGINES"

# osslsigncode's -key wants a PKCS#11 URI when signing through the engine.
# Accept a full pkcs11: URI as-is, or build one from a hex key id (with or
# without colons): 636c1f49.. / 63:6c:1f.. -> pkcs11:id=%63%6c%1f..;type=private
case "$TINA4_KEY_ID" in
  pkcs11:*) KEY_URI="$TINA4_KEY_ID" ;;
  *)
    _hex="$(printf '%s' "$TINA4_KEY_ID" | tr -d ':')"
    _enc="$(printf '%s' "$_hex" | sed 's/\(..\)/%\1/g')"
    KEY_URI="pkcs11:id=${_enc};type=private"
    ;;
esac
echo "Using key URI: $KEY_URI"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "Downloading draft release assets for $TAG ..."
gh release download "$TAG" --dir . --clobber
[ -f "$BINARY" ] || { echo "Error: $BINARY not found in release $TAG" >&2; exit 1; }

echo "Signing $BINARY (SimplySign must be logged in) ..."
# -t (legacy Authenticode timestamp) is the Certum-proven form against
# time.certum.pl - it is what the canonical osslsigncode + Certum recipes use
# (e.g. oneclick/rubyinstaller2). RFC3161 (-ts) has been reported to fail there
# (osslsigncode #109). If your Certum setup specifically needs RFC3161, switch
# -t back to -ts and test it BEFORE the real signing session.
osslsigncode sign \
  -pkcs11module "$TINA4_PKCS11_MODULE" \
  -certs "$TINA4_SIGN_CERT" \
  -key "$KEY_URI" \
  -h sha256 \
  -t "$TS_URL" \
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
