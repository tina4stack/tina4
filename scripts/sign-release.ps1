# Local, human-in-the-loop EV code-signing + release finalize for the tina4 CLI.
#
# The SimplySign 2FA is entered BY YOU at release time - no OTP seed is ever
# stored in CI, so nothing automated can sign as Code Infinity. The Release
# workflow ships a DRAFT (built, audited, attested, checksummed, UNSIGNED);
# this script signs the Windows .exe, regenerates SHA256SUMS over the signed
# bytes, and publishes the release.
#
# PREREQUISITES (run on Windows):
#   1. SimplySign Desktop installed and LOGGED IN (you complete the 2FA - the
#      Code Infinity EV cert / cloud card is then mounted in the Windows store).
#   2. signtool.exe available (Windows SDK).
#   3. gh (GitHub CLI) installed and authenticated (gh auth login).
#   4. CERT_THUMBPRINT = the SHA1 thumbprint of the Code Infinity cert
#      (read it from SimplySign: double-click the cert -> details -> Thumbprint).
#      Not a secret - you may also pass it with -Thumbprint.
#
# USAGE:
#   $env:CERT_THUMBPRINT = "<sha1>"
#   pwsh ./scripts/sign-release.ps1 -Tag v3.8.53
#
# What it does: download the draft's assets -> sign the .exe -> verify ->
# re-upload the signed .exe -> regenerate + re-upload SHA256SUMS -> publish.

param(
    [Parameter(Mandatory = $true)][string]$Tag,            # e.g. v3.8.53
    [string]$Thumbprint = $env:CERT_THUMBPRINT,
    [string]$Repo = "tina4stack/tina4",
    [string]$Binary = "tina4-windows-amd64.exe",
    [string]$TimestampUrl = "http://time.certum.pl/"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $Thumbprint) {
    Write-Error "Set CERT_THUMBPRINT (SHA1 thumbprint of the Code Infinity cert) or pass -Thumbprint"
    exit 1
}
foreach ($tool in @("gh")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Write-Error "$tool is required (install it and authenticate: gh auth login)"
        exit 1
    }
}

# --- Locate signtool.exe (newest x64 SDK build) ---
$signtool = $null
$candidates = @(
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\signtool.exe"
)
foreach ($p in $candidates) { if (Test-Path $p) { $signtool = $p; break } }
if (-not $signtool) {
    $found = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like "*\x64\*" } | Sort-Object FullName -Descending | Select-Object -First 1
    if ($found) { $signtool = $found.FullName }
}
if (-not $signtool) { Write-Error "signtool.exe not found - install the Windows SDK"; exit 1 }
Write-Host "Using signtool: $signtool"

$work = Join-Path $env:TEMP "tina4-sign-$Tag"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force -Path $work | Out-Null
Push-Location $work
try {
    Write-Host "Downloading draft release assets for $Tag ..."
    gh release download $Tag --repo $Repo --dir . --clobber
    if ($LASTEXITCODE -ne 0) { Write-Error "gh release download failed for $Tag"; exit 1 }
    if (-not (Test-Path $Binary)) { Write-Error "$Binary not found in release $Tag"; exit 1 }

    Write-Host "Signing $Binary (SimplySign must be open and logged in) ..."
    & $signtool sign /sha1 $Thumbprint /tr $TimestampUrl /td sha256 /fd sha256 /v $Binary
    if ($LASTEXITCODE -ne 0) { Write-Error "signing failed (is SimplySign logged in?)"; exit 1 }

    Write-Host "Verifying signature ..."
    & $signtool verify /pa /v $Binary
    if ($LASTEXITCODE -ne 0) { Write-Error "signature verification failed"; exit 1 }

    # Guard: 'verify /pa' accepts ANY valid chain. Assert this is OUR EV cert so a
    # wrong-cert or stale-unsigned binary can never be published. Subject is
    # 'CN=Code Infinity (Pty) Ltd, O=Code Infinity (Pty) Ltd, ...'.
    $sig = Get-AuthenticodeSignature $Binary
    if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike '*Code Infinity*') {
        Write-Error ("Refusing to publish: {0} is not validly signed by Code Infinity (status={1}, signer={2})" -f `
            $Binary, $sig.Status, $sig.SignerCertificate.Subject)
        exit 1
    }
    Write-Host "Signer verified: $($sig.SignerCertificate.Subject)"

    Write-Host "Uploading signed $Binary ..."
    gh release upload $Tag $Binary --repo $Repo --clobber
    if ($LASTEXITCODE -ne 0) { Write-Error "upload of signed binary failed"; exit 1 }

    # Regenerate SHA256SUMS over the SIGNED set (sha256sum format: "<hash>  <name>").
    Write-Host "Regenerating SHA256SUMS over the signed assets ..."
    Remove-Item "SHA256SUMS" -ErrorAction SilentlyContinue
    $lines = Get-ChildItem -File | Where-Object { $_.Name -ne "SHA256SUMS" } | Sort-Object Name | ForEach-Object {
        $h = (Get-FileHash $_.Name -Algorithm SHA256).Hash.ToLower()
        "$h  $($_.Name)"
    }
    # LF line endings + trailing newline, like sha256sum.
    [System.IO.File]::WriteAllText((Join-Path $work "SHA256SUMS"), (($lines -join "`n") + "`n"))
    Get-Content "SHA256SUMS"
    gh release upload $Tag "SHA256SUMS" --repo $Repo --clobber
    if ($LASTEXITCODE -ne 0) { Write-Error "upload of SHA256SUMS failed"; exit 1 }

    Write-Host "Publishing release $Tag ..."
    gh release edit $Tag --repo $Repo --draft=false
    if ($LASTEXITCODE -ne 0) { Write-Error "publishing (un-draft) failed"; exit 1 }

    Write-Host ""
    Write-Host "Done: $Tag signed (EV), checksummed over signed bytes, and published." -ForegroundColor Green
}
finally {
    Pop-Location
}
