# Certum SimplySign automated code signing script for CI/CD
# Required environment variables:
#   SIMPLYSIGN_OTP_URI   - otpauth:// URI from SimplySign QR code
#   SIMPLYSIGN_EXE_PATH  - Path to SimplySign Desktop executable
#   CERT_THUMBPRINT      - SHA1 thumbprint of the code signing certificate
#   SIGN_TARGET          - Path to the .exe file to sign

param(
    [string]$Target = $env:SIGN_TARGET,
    [string]$Thumbprint = $env:CERT_THUMBPRINT,
    [string]$OtpUri = $env:SIMPLYSIGN_OTP_URI,
    [string]$SimplySignPath = $env:SIMPLYSIGN_EXE_PATH,
    [int]$MaxRetries = 3,
    [int]$WaitSeconds = 15
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# --- TOTP generation (RFC 6238) ---
function ConvertFrom-Base32 {
    param([string]$Base32)
    $Base32 = $Base32.ToUpper() -replace '[=]+$', ''
    $alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
    $bits = ""
    foreach ($char in $Base32.ToCharArray()) {
        $val = $alphabet.IndexOf($char)
        if ($val -lt 0) { throw "Invalid Base32 character: $char" }
        $bits += [Convert]::ToString($val, 2).PadLeft(5, '0')
    }
    $bytes = New-Object byte[] ([math]::Floor($bits.Length / 8))
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $bytes[$i] = [Convert]::ToByte($bits.Substring($i * 8, 8), 2)
    }
    return $bytes
}

function Get-TOTP {
    param(
        [byte[]]$Secret,
        [int]$Digits = 6,
        [int]$Period = 30
    )
    $epoch = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $counter = [math]::Floor($epoch / $Period)
    $counterBytes = [BitConverter]::GetBytes([long]$counter)
    if ([BitConverter]::IsLittleEndian) {
        [Array]::Reverse($counterBytes)
    }
    $hmac = New-Object System.Security.Cryptography.HMACSHA1
    $hmac.Key = $Secret
    $hash = $hmac.ComputeHash($counterBytes)
    $offset = $hash[$hash.Length - 1] -band 0x0F
    $code = (($hash[$offset] -band 0x7F) -shl 24) -bor
             (($hash[$offset + 1] -band 0xFF) -shl 16) -bor
             (($hash[$offset + 2] -band 0xFF) -shl 8) -bor
              ($hash[$offset + 3] -band 0xFF)
    $otp = $code % [math]::Pow(10, $Digits)
    return $otp.ToString().PadLeft($Digits, '0')
}

function Parse-OtpUri {
    param([string]$Uri)
    $parsed = [Uri]$Uri
    $query = [System.Web.HttpUtility]::ParseQueryString($parsed.Query)
    $secret = $query["secret"]
    $digits = if ($query["digits"]) { [int]$query["digits"] } else { 6 }
    $period = if ($query["period"]) { [int]$query["period"] } else { 30 }
    return @{
        Secret = $secret
        Digits = $digits
        Period = $period
    }
}

# --- Validation ---
if (-not $Target -or -not (Test-Path $Target)) {
    Write-Error "SIGN_TARGET not set or file not found: $Target"
    exit 1
}
if (-not $Thumbprint) {
    Write-Error "CERT_THUMBPRINT not set"
    exit 1
}
if (-not $OtpUri) {
    Write-Error "SIMPLYSIGN_OTP_URI not set"
    exit 1
}
if (-not $SimplySignPath -or -not (Test-Path $SimplySignPath)) {
    Write-Error "SIMPLYSIGN_EXE_PATH not set or not found: $SimplySignPath"
    exit 1
}

# --- Load System.Web for URI parsing ---
Add-Type -AssemblyName System.Web

# --- Launch SimplySign and authenticate ---
Write-Host "Launching SimplySign Desktop..."
$process = Start-Process -FilePath $SimplySignPath -PassThru

Write-Host "Waiting for SimplySign window..."
Start-Sleep -Seconds 5

# Generate TOTP
$otpParams = Parse-OtpUri -Uri $OtpUri
$secretBytes = ConvertFrom-Base32 -Base32 $otpParams.Secret
$totp = Get-TOTP -Secret $secretBytes -Digits $otpParams.Digits -Period $otpParams.Period
Write-Host "TOTP generated successfully"

# Send OTP to SimplySign login dialog via COM automation
$wshell = New-Object -ComObject WScript.Shell
$retries = 0
$activated = $false

while (-not $activated -and $retries -lt $MaxRetries) {
    Start-Sleep -Seconds 3
    $activated = $wshell.AppActivate($process.Id)
    if (-not $activated) {
        # Try by window title
        $activated = $wshell.AppActivate("SimplySign")
    }
    $retries++
}

if (-not $activated) {
    Write-Error "Could not activate SimplySign window after $MaxRetries attempts"
    exit 1
}

Start-Sleep -Seconds 1
$wshell.SendKeys($totp)
Start-Sleep -Milliseconds 500
$wshell.SendKeys("{ENTER}")

Write-Host "OTP sent, waiting for smart card to mount..."
Start-Sleep -Seconds $WaitSeconds

# --- Sign the binary ---
Write-Host "Signing $Target ..."

# Find signtool.exe
$signtoolPaths = @(
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\signtool.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\signtool.exe"
)

$signtool = $null
foreach ($path in $signtoolPaths) {
    if (Test-Path $path) {
        $signtool = $path
        break
    }
}

# Fallback: search for signtool
if (-not $signtool) {
    $found = Get-ChildItem "C:\Program Files (x86)\Windows Kits" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
             Where-Object { $_.FullName -like "*\x64\*" } |
             Sort-Object FullName -Descending |
             Select-Object -First 1
    if ($found) {
        $signtool = $found.FullName
    }
}

if (-not $signtool) {
    Write-Error "signtool.exe not found. Ensure Windows SDK is installed."
    exit 1
}

Write-Host "Using signtool: $signtool"

$signArgs = @(
    "sign",
    "/sha1", $Thumbprint,
    "/tr", "http://time.certum.pl/",
    "/td", "sha256",
    "/fd", "sha256",
    "/v",
    $Target
)

$signResult = & $signtool @signArgs
$signExitCode = $LASTEXITCODE

Write-Host $signResult

if ($signExitCode -ne 0) {
    Write-Error "Signing failed with exit code $signExitCode"
    exit 1
}

# --- Verify the signature ---
Write-Host "Verifying signature..."
$verifyArgs = @("verify", "/pa", "/v", $Target)
$verifyResult = & $signtool @verifyArgs
Write-Host $verifyResult

if ($LASTEXITCODE -ne 0) {
    Write-Error "Signature verification failed"
    exit 1
}

Write-Host "Successfully signed and verified: $Target"
