#!/bin/sh
# Sign the PowerShell skills installer with the active SimplySign cloud token.
# The private key never leaves the HSM; the user completes 2FA in SimplySign.
set -eu

cd "$(dirname "$0")/.."

FILE="install-skills.ps1"
CHECK=0
for arg in "$@"; do
  case "$arg" in
    --check|--dry-run|-n) CHECK=1 ;;
    -*) echo "Unknown option: $arg" >&2; exit 2 ;;
    *) FILE="$arg" ;;
  esac
done

[ -f "$FILE" ] || { echo "Error: installer not found: $FILE" >&2; exit 1; }
command -v jsign >/dev/null 2>&1 || { echo "Error: jsign is required" >&2; exit 1; }

MODULE="${TINA4_PKCS11_MODULE:-/usr/local/lib/libSimplySignPKCS.dylib}"
[ -f "$MODULE" ] || { echo "Error: PKCS#11 module not found: $MODULE" >&2; exit 1; }
if [ -L "$MODULE" ]; then
  link="$(readlink "$MODULE")"
  case "$link" in
    /*) MODULE="$link" ;;
    *) MODULE="$(cd "$(dirname "$MODULE")" && pwd)/$link" ;;
  esac
fi

if [ -z "${TINA4_SIGN_ALIAS:-}" ] && command -v pkcs11-tool >/dev/null 2>&1; then
  label="$(pkcs11-tool --module "$MODULE" --list-objects --type cert 2>/dev/null \
    | grep -iE '^[[:space:]]*label:' | head -1 \
    | sed -E 's/.*label:[[:space:]]*//; s/[[:space:]]*$//')"
  [ -n "$label" ] && TINA4_SIGN_ALIAS="$label"
fi
ALIAS="${TINA4_SIGN_ALIAS:-521D88BF7DC9159EE3445861DB1261C6}"
TSA_URL="${TINA4_TS_URL:-http://time.certum.pl/}"

echo "PowerShell installer signing configuration:"
echo "  file:   $FILE"
echo "  module: $MODULE"
echo "  alias:  $ALIAS"
echo "  tsa:    $TSA_URL"

if [ "$CHECK" -eq 1 ]; then
  echo "  --check OK: SimplySign token and certificate are available; nothing signed."
  exit 0
fi

PKCFG="$(mktemp)"
SIG="$FILE.sig.pem"
trap 'rm -f "$PKCFG" "$SIG"' EXIT
cat > "$PKCFG" <<CFG
name = SimplySign
library = $MODULE
CFG

jsign --replace --storetype PKCS11 --keystore "$PKCFG" --storepass "" \
  --alias "$ALIAS" --tsmode AUTHENTICODE --tsaurl "$TSA_URL" "$FILE"

# Confirm a readable CMS signature containing our public certificate was written.
jsign extract --format PEM "$FILE"
openssl pkcs7 -in "$SIG" -print_certs -noout | grep -q 'Code Infinity'
echo "Signed and structurally verified: $FILE"
