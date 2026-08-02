# GrokHub Windows launcher (PowerShell)
$ErrorActionPreference = "Stop"

function Resolve-AppRoot {
  if ($env:GROKHUB_HOME -and (Test-Path (Join-Path $env:GROKHUB_HOME "desktop\main.mjs"))) {
    return (Resolve-Path $env:GROKHUB_HOME).Path
  }
  $here = Split-Path -Parent $MyInvocation.MyCommand.Path
  foreach ($c in @(
      $here,
      (Join-Path $here ".."),
      (Join-Path $env:LOCALAPPDATA "GrokHub")
    )) {
    if ($c -and (Test-Path (Join-Path $c "desktop\main.mjs"))) {
      return (Resolve-Path $c).Path
    }
  }
  throw "GrokHub install not found. Run packaging\windows\install.ps1 -Build"
}

$AppRoot = Resolve-AppRoot
$env:GROKHUB_HOME = $AppRoot
if (-not $env:GROKHUB_PORT) { $env:GROKHUB_PORT = "18765" }
if (-not $env:GROKHUB_URL) { $env:GROKHUB_URL = "http://127.0.0.1:$($env:GROKHUB_PORT)" }
if (-not $env:GROKHUB_TRAY) { $env:GROKHUB_TRAY = "1" }

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "node not found — install Node.js 20+ from https://nodejs.org/"
}

$Electron = $null
$candidates = @(
  (Join-Path $AppRoot "node_modules\electron\dist\electron.exe"),
  (Join-Path $AppRoot "node_modules\.bin\electron.cmd")
)
foreach ($c in $candidates) {
  if (Test-Path $c) { $Electron = $c; break }
}
if (-not $Electron -and (Get-Command electron -ErrorAction SilentlyContinue)) {
  $Electron = (Get-Command electron).Source
}
if (-not $Electron) {
  throw "electron not found under $AppRoot\node_modules\electron — re-run install.ps1 -Build"
}

$main = Join-Path $AppRoot "desktop\main.mjs"
if (-not (Test-Path $main)) { throw "missing $main" }

# Electron main auto-starts Nitro via desktop/ui-server.cjs
Set-Location $AppRoot
& $Electron $main @args
exit $LASTEXITCODE
