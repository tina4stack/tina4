# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Chocolatey removes the package's tools directory and the auto-generated .exe
# shim on uninstall, so nothing extra is required here. This file exists to make
# the uninstall intent explicit and to leave room for future cleanup.
$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$exe = Join-Path $toolsDir 'tina4.exe'
if (Test-Path $exe) {
    Remove-Item $exe -Force -ErrorAction SilentlyContinue
}
