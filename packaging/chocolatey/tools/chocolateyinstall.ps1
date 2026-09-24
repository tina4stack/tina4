# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Chocolatey install script for the tina4 CLI.
#
# This is a DOWNLOAD package: it fetches the official EV-signed Windows binary
# from the project's GitHub Release and verifies its SHA-256 before installing.
# The $version / $url64 / $checksum64 lines below are the ONLY per-release
# values; the release-published workflow (.github/workflows/release-published.yml)
# rewrites them from the tag and SHA256SUMS. Keep them exact.
$ErrorActionPreference = 'Stop'

$version     = '3.8.91'
$url64       = "https://github.com/tina4stack/tina4/releases/download/v$version/tina4-windows-amd64.exe"
$checksum64  = '77D5EC9AE4ED154A3C77106881883DEA1EED96B2E32D7C460AE38A774AE3F1E7'

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
