# Overlay /update on Windows: build the cabin and copy into the per-user prefix.
# Does not run Linux install.sh. Does not force GROK_CHANNEL on Linux.
param(
  [string]$Prefix = ""
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
if (-not (Test-Path 'Cargo.toml')) { throw "run from repo root" }
if (-not $Prefix) {
  $Prefix = Join-Path $env:LOCALAPPDATA 'Programs\GrokHub'
}
New-Item -ItemType Directory -Path $Prefix -Force | Out-Null
cargo build --release -p grokhub-app -p grokhub-hub
if (-not (Test-Path 'target/release/grokhub.exe')) { throw "missing grokhub.exe" }
if (-not (Test-Path 'target/release/grokhub-hub.exe')) { throw "missing grokhub-hub.exe" }
Copy-Item 'target/release/grokhub.exe' $Prefix -Force
Copy-Item 'target/release/grokhub-hub.exe' $Prefix -Force
if (Test-Path 'LICENSE') { Copy-Item 'LICENSE' $Prefix -Force }
Write-Output "overlay $Prefix"
