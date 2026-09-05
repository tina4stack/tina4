# Chocolatey install script for the tina4 CLI.
#
# This is a DOWNLOAD package: it fetches the official EV-signed Windows binary
# from the project's GitHub Release and verifies its SHA-256 before installing.
# The $version / $url64 / $checksum64 lines below are the ONLY per-release
# values; the release-published workflow (.github/workflows/release-published.yml)
# rewrites them from the tag and SHA256SUMS. Keep them exact.
$ErrorActionPreference = 'Stop'

$version     = '3.8.82'
$url64       = "https://github.com/tina4stack/tina4/releases/download/v$version/tina4-windows-amd64.exe"
$checksum64  = '6E5A67AD435DB97C40364361B4CA347E4F7D9035678D68428F1613481797F920'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

# Download straight to tools\tina4.exe. Chocolatey auto-generates a PATH shim
# for every .exe left in the package tools directory, so no explicit Install-Bin
# call is needed - the command lands on PATH as `tina4`.
Get-ChocolateyWebFile `
    -PackageName    'tina4' `
    -FileFullPath   (Join-Path $toolsDir 'tina4.exe') `
    -Url64bit       $url64 `
    -Checksum64     $checksum64 `
    -ChecksumType64 'sha256'
