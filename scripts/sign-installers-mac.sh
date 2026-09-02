#!/usr/bin/env bash
# macOS Authenticode signer for install-skills.ps1.
#
# WHY THIS EXISTS
#   scripts/sign-installers.ps1 uses Windows signtool.exe + the CurrentUser\My
#   cert store, so it only runs on Windows. On macOS the same EV signature is
#   produced with osslsigncode against the Code Infinity EV cert on the Certum
#   SimplySign cloud card (a PKCS#11 token). osslsigncode 2.x signs the PowerShell
#   .ps1 script SIP, and the result validates under Windows Get-AuthenticodeSignature
#   -- proven by the "Verify canonical PowerShell installer signature" job in
#   .github/workflows/ci.yml on windows-latest.
#
# PREREQUISITES
#   brew install osslsigncode opensc libp11
#   proCertumSmartSign / SimplySign Desktop OPEN and LOGGED IN (mounts the EV card
#   as a PKCS#11 token). EV signing is interactive, so this only runs locally.
#
# USAGE
#   scripts/sign-installers-mac.sh [install-skills.ps1]
#   then: git add install-skills.ps1 && git commit && (re)create the skills tag.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
file="${1:-$here/../install-skills.ps1}"
module="${TINA4_PKCS11_MODULE:-/Applications/proCertumSmartSign.app/Contents/MacOS/libSimplySignPKCS.dylib}"
engine="${TINA4_PKCS11_ENGINE:-/opt/homebrew/lib/engines-3/pkcs11.dylib}"
chain="${TINA4_CERT_CHAIN:-$here/codeinfinity-chain.pem}"
ts="${TINA4_TS_URL:-http://time.certum.pl/}"

for t in osslsigncode pkcs11-tool openssl; do
  command -v "$t" >/dev/null 2>&1 || { echo "error: missing $t (brew install osslsigncode opensc)" >&2; exit 1; }
done
[ -f "$file" ]   || { echo "error: no such file: $file" >&2; exit 1; }
[ -f "$module" ] || { echo "error: SimplySign PKCS#11 module not found: $module" >&2; exit 1; }
[ -f "$engine" ] || { echo "error: OpenSSL pkcs11 engine not found: $engine (brew install libp11)" >&2; exit 1; }
[ -f "$chain" ]  || { echo "error: cert chain not found: $chain" >&2; exit 1; }

# Auto-detect the EV private-key id on the logged-in card.
keyid="$(pkcs11-tool --module "$module" -O --type privkey 2>/dev/null | awk -F'ID:' '/ID:/{gsub(/[ :]/,"",$2); print $2; exit}')"
[ -n "$keyid" ] || { echo "error: no private key on the card -- open SimplySign and LOG IN" >&2; exit 1; }
keyuri="pkcs11:id=$(printf '%s' "$keyid" | sed 's/../%&/g');type=private"

tmp="$(mktemp).ps1"
echo "Signing $(basename "$file") with the Code Infinity EV card (key id $keyid) ..."
osslsigncode sign \
  -pkcs11engine "$engine" -pkcs11module "$module" -key "$keyuri" \
  -certs "$chain" -h sha256 -t "$ts" \
  -in "$file" -out "$tmp"

# Sanity: the content digest is present and the PowerShell signature block was emitted.
osslsigncode verify -in "$tmp" 2>&1 | grep -q 'Calculated message digest' || { echo "error: signature has no content digest" >&2; exit 1; }
grep -q '# SIG # Begin signature block' "$tmp" || { echo "error: no PowerShell signature block in output" >&2; exit 1; }

mv "$tmp" "$file"
echo "Signed in place. Windows Get-AuthenticodeSignature (ci.yml, windows-latest) is the final validator."
echo "Next: git add $(basename "$file") && git commit && (re)create the skills tag 3.13.NNN at that commit."
