# Sign the checked-in PowerShell installer(s) in place with the Code Infinity EV cert.
#
# WHY THIS EXISTS
#   install-skills.ps1 is a REPO TREE file, not a release asset: it is served to
#   users straight from the tag (raw.githubusercontent / the tina4.com shim), and
#   the CI gate (.github/workflows/ci.yml) and the runtime shim both require it to
#   carry a VALID Authenticode signature. Editing its body (e.g. bumping the
#   default skills ref) invalidates that signature, so it MUST be re-signed and
#   re-committed in the same change. This script is that one-command step.
#
# PREREQUISITES (same as scripts/sign-release.ps1)
#   1. SimplySign Desktop open and LOGGED IN (the Code Infinity EV cloud card is
#      then mounted in the CurrentUser\My store). EV signing is interactive, so
#      this only runs locally, never in GitHub CI.
#   2. signtool.exe on the box (Windows SDK).
#   3. CERT_THUMBPRINT set to the SHA1 thumbprint of the Code Infinity cert, or
#      let this script auto-detect it by subject.
#
# USAGE
#   pwsh ./scripts/sign-installers.ps1            # signs install-skills.ps1, then commit
#   pwsh ./scripts/sign-installers.ps1 -Thumbprint <sha1>
#
# AFTER RUNNING: git add install-skills.ps1 && git commit && (re)tag the release.

param(
    [string]$Thumbprint = $env:CERT_THUMBPRINT,
    [string[]]$Files = @("install-skills.ps1"),
    [string]$TimestampUrl = "http://time.certum.pl/"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# --- Resolve the Code Infinity cert thumbprint (auto-detect if not given) ---
if (-not $Thumbprint) {
    $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
        Where-Object { $_.Subject -like "*Code Infinity*" } |
        Sort-Object NotAfter -Descending | Select-Object -First 1
    if (-not $cert) {
        Write-Error "No Code Infinity code-signing cert in CurrentUser\My. Open SimplySign and log in, or pass -Thumbprint."
        exit 1
    }
    $Thumbprint = $cert.Thumbprint
    Write-Host "Auto-detected Code Infinity cert: $Thumbprint"
}

# --- Locate signtool.exe (newest x64 SDK build) ---
$signtool = $null
foreach ($p in @(
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\signtool.exe"
)) { if (Test-Path $p) { $signtool = $p; break } }
if (-not $signtool) {
    $found = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like "*\x64\*" } | Sort-Object FullName -Descending | Select-Object -First 1
    if ($found) { $signtool = $found.FullName }
}
if (-not $signtool) { Write-Error "signtool.exe not found - install the Windows SDK"; exit 1 }
Write-Host "Using signtool: $signtool"

$repoRoot = Split-Path -Parent $PSScriptRoot
foreach ($rel in $Files) {
    $file = Join-Path $repoRoot $rel
    if (-not (Test-Path $file)) { Write-Error "$rel not found at $file"; exit 1 }

    Write-Host "`nSigning $rel (SimplySign must be open and logged in) ..."
    & $signtool sign /sha1 $Thumbprint /tr $TimestampUrl /td sha256 /fd sha256 /v $file
    if ($LASTEXITCODE -ne 0) { Write-Error "signing $rel failed (is SimplySign logged in?)"; exit 1 }

    # Guard: assert it is OUR EV cert, so a wrong-cert or stale-unsigned file can
    # never be committed (mirrors scripts/sign-release.ps1's signer guard).
    $sig = Get-AuthenticodeSignature $file
    if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike '*Code Infinity*') {
        Write-Error ("Refusing: {0} is not validly signed by Code Infinity (status={1}, signer={2})" -f `
            $rel, $sig.Status, $sig.SignerCertificate.Subject)
        exit 1
    }
    Write-Host "Signed + verified: $rel  ($($sig.SignerCertificate.Subject))"
}

Write-Host "`nDone. Next: git add $($Files -join ' ') ; git commit ; then (re)tag the release."
