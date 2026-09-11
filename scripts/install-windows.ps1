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
function Overlay-Locked([string]$From, [string]$To) {
  $old = "$To.old"
  if (Test-Path $old) { Remove-Item $old -Force -ErrorAction SilentlyContinue }
  if (Test-Path $To) {
    try {
      Copy-Item $From $To -Force
      return
    } catch {
      # Running grokhub.exe is locked; rename the mapped image, then copy.
      Rename-Item $To $old -Force
    }
  }
  Copy-Item $From $To -Force
}
Overlay-Locked 'target/release/grokhub.exe' (Join-Path $Prefix 'grokhub.exe')
Overlay-Locked 'target/release/grokhub-hub.exe' (Join-Path $Prefix 'grokhub-hub.exe')
if (Test-Path 'LICENSE') { Copy-Item 'LICENSE' $Prefix -Force }
Write-Output "overlay $Prefix"
