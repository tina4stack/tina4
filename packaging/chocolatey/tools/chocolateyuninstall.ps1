# Chocolatey removes the package's tools directory and the auto-generated .exe
# shim on uninstall, so nothing extra is required here. This file exists to make
# the uninstall intent explicit and to leave room for future cleanup.
$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$exe = Join-Path $toolsDir 'tina4.exe'
if (Test-Path $exe) {
    Remove-Item $exe -Force -ErrorAction SilentlyContinue
}
