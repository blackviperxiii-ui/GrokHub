# GrokHub Windows launcher (PowerShell)
# Starts Nitro UI then Electron. Prefer grokhub.cmd for Start Menu shortcuts.
$ErrorActionPreference = "Stop"

function Resolve-AppRoot {
  if ($env:GROKHUB_HOME -and (Test-Path (Join-Path $env:GROKHUB_HOME "desktop\main.mjs"))) {
    return (Resolve-Path $env:GROKHUB_HOME).Path
  }
  $here = Split-Path -Parent $MyInvocation.MyCommand.Path
  foreach ($c in @($here, (Join-Path $here ".."), (Join-Path $env:LOCALAPPDATA "GrokHub"))) {
    if ($c -and (Test-Path (Join-Path $c "desktop\main.mjs"))) {
      return (Resolve-Path $c).Path
    }
  }
  throw "GrokHub install not found. Set GROKHUB_HOME or install to %LOCALAPPDATA%\GrokHub"
}

$AppRoot = Resolve-AppRoot
$env:GROKHUB_HOME = $AppRoot
$Port = if ($env:GROKHUB_PORT) { $env:GROKHUB_PORT } else { "18765" }
$Url = if ($env:GROKHUB_URL) { $env:GROKHUB_URL } else { "http://127.0.0.1:$Port" }
$Runtime = Join-Path $env:LOCALAPPDATA "GrokHub\runtime"
New-Item -ItemType Directory -Force -Path $Runtime | Out-Null
$Log = Join-Path $Runtime "ui.log"

function Test-Ui {
  try {
    $r = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2
    return $r.StatusCode -ge 200 -and $r.StatusCode -lt 500
  } catch { return $false }
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "node not found — install Node.js 20+ from https://nodejs.org/"
}

$Electron = $null
if (Get-Command electron -ErrorAction SilentlyContinue) {
  $Electron = (Get-Command electron).Source
} elseif (Test-Path (Join-Path $AppRoot "node_modules\electron\dist\electron.exe")) {
  $Electron = Join-Path $AppRoot "node_modules\electron\dist\electron.exe"
} elseif (Test-Path (Join-Path $AppRoot "node_modules\.bin\electron.cmd")) {
  $Electron = Join-Path $AppRoot "node_modules\.bin\electron.cmd"
} else {
  throw "electron not found — npm install electron --save-dev  (or install the Windows release package)"
}

if (-not (Test-Ui)) {
  $server = Join-Path $AppRoot ".output\server\index.mjs"
  if (-not (Test-Path $server)) {
    throw "Missing UI build at $server — run: npm run desktop:build"
  }
  Write-Host "Starting GrokHub UI on $Url …"
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = "node"
  $psi.Arguments = "`"$server`""
  $psi.WorkingDirectory = $AppRoot
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true
  $psi.Environment["PORT"] = $Port
  $psi.Environment["NITRO_PORT"] = $Port
  $psi.Environment["HOST"] = "127.0.0.1"
  $psi.Environment["NITRO_HOST"] = "127.0.0.1"
  $p = [System.Diagnostics.Process]::Start($psi)
  $ok = $false
  for ($i = 0; $i -lt 80; $i++) {
    Start-Sleep -Milliseconds 150
    if (Test-Ui) { $ok = $true; break }
  }
  if (-not $ok) { throw "UI failed to start — check $Log" }
}

$env:GROKHUB_URL = $Url
$env:GROKHUB_TRAY = if ($env:GROKHUB_TRAY) { $env:GROKHUB_TRAY } else { "1" }
$main = Join-Path $AppRoot "desktop\main.mjs"
& $Electron $main @args
