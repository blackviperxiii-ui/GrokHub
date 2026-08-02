#Requires -Version 5.1
<#
.SYNOPSIS
  Install GrokHub for the current Windows user.

.DESCRIPTION
  Builds the desktop UI (if needed), installs Electron, copies files to
  %LOCALAPPDATA%\GrokHub, and creates a Start Menu shortcut.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1 -Build
#>
param(
  [switch]$Build,
  [switch]$NoStartMenu,
  [string]$Prefix = ""
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Write-Step($msg) { Write-Host "" ; Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg) { Write-Host "    $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "    $msg" -ForegroundColor Yellow }

# Resolve repo root (this script lives in packaging/windows)
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = (Resolve-Path (Join-Path $ScriptDir "..\..")).Path
$Dest = if ($Prefix) { $Prefix } else { Join-Path $env:LOCALAPPDATA "GrokHub" }

Write-Host "GrokHub Windows installer" -ForegroundColor White
Write-Host "  Source : $Root"
Write-Host "  Dest   : $Dest"

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js is required. Install the LTS build from https://nodejs.org/ and re-open PowerShell."
}
$nodeVer = & node -v
Write-Ok "Node $nodeVer"

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "npm not found (should ship with Node.js)."
}

Push-Location $Root
try {
  $serverEntry = Join-Path $Root ".output\server\index.mjs"
  $needBuild = $Build -or -not (Test-Path $serverEntry)

  if ($needBuild) {
    Write-Step "Installing npm dependencies"
    if (-not (Test-Path (Join-Path $Root "node_modules"))) {
      # Prefer npm install (more resilient than ci without lock fidelity)
      npm install --no-fund --no-audit
      if ($LASTEXITCODE -ne 0) { throw "npm install failed (exit $LASTEXITCODE)" }
    } else {
      Write-Ok "node_modules already present"
    }

    Write-Step "Ensuring Electron is installed"
    if (-not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron.exe")) -and
        -not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron"))) {
      npm install electron@35.1.2 --save-dev --no-fund --no-audit
      if ($LASTEXITCODE -ne 0) { throw "electron npm install failed" }
      # Force binary download if postinstall skipped
      if (-not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron.exe"))) {
        Write-Warn "Electron binary missing — running electron install"
        npx --yes electron@35.1.2 install
      }
    }
    if (-not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron.exe"))) {
      throw "Electron binary still missing at node_modules\electron\dist\electron.exe"
    }
    Write-Ok "Electron binary OK"

    Write-Step "Building production UI (desktop:build)"
    $env:GROKHUB_DESKTOP = "1"
    $env:NODE_ENV = "production"
    npm run desktop:build
    if ($LASTEXITCODE -ne 0) {
      Write-Warn "desktop:build failed — trying npm run build"
      npm run build
      if ($LASTEXITCODE -ne 0) { throw "UI build failed (exit $LASTEXITCODE)" }
    }
    if (-not (Test-Path $serverEntry)) {
      throw "Build finished but $serverEntry is missing"
    }
    Write-Ok "UI build present"
  } else {
    Write-Step "Using existing .output build"
    Write-Ok $serverEntry
    # Still ensure electron exists for copy
    if (-not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron.exe"))) {
      Write-Step "Installing Electron runtime"
      npm install electron@35.1.2 --save-dev --no-fund --no-audit
      if (-not (Test-Path (Join-Path $Root "node_modules\electron\dist\electron.exe"))) {
        npx --yes electron@35.1.2 install
      }
    }
  }

  Write-Step "Installing into $Dest"
  New-Item -ItemType Directory -Force -Path $Dest | Out-Null

  # Remove previous payload (not userData under %APPDATA%\GrokHub)
  foreach ($name in @(".output", "desktop", "node_modules", "package.json", "APP_VERSION", "VERSION", "grokhub.cmd", "grokhub.ps1", "LICENSE")) {
    $p = Join-Path $Dest $name
    if (Test-Path $p) {
      Remove-Item -Recurse -Force $p -ErrorAction SilentlyContinue
    }
  }

  Copy-Item -Recurse -Force (Join-Path $Root ".output") (Join-Path $Dest ".output")
  Copy-Item -Recurse -Force (Join-Path $Root "desktop") (Join-Path $Dest "desktop")
  Copy-Item -Force (Join-Path $Root "package.json") (Join-Path $Dest "package.json")
  if (Test-Path (Join-Path $Root "APP_VERSION")) {
    Copy-Item -Force (Join-Path $Root "APP_VERSION") (Join-Path $Dest "APP_VERSION")
  }
  if (Test-Path (Join-Path $Root "LICENSE")) {
    Copy-Item -Force (Join-Path $Root "LICENSE") (Join-Path $Dest "LICENSE")
  }

  # Launchers live at install root
  Copy-Item -Force (Join-Path $Root "packaging\windows\grokhub.cmd") (Join-Path $Dest "grokhub.cmd")
  Copy-Item -Force (Join-Path $Root "packaging\windows\grokhub.ps1") (Join-Path $Dest "grokhub.ps1")

  # Electron runtime (required to run without global electron)
  $electronSrc = Join-Path $Root "node_modules\electron"
  if (-not (Test-Path $electronSrc)) {
    throw "electron package missing after install"
  }
  Write-Step "Copying Electron runtime (this may take a minute)"
  $nm = Join-Path $Dest "node_modules"
  New-Item -ItemType Directory -Force -Path $nm | Out-Null
  # robocopy is far more reliable than Copy-Item for large trees on Windows
  $robo = Join-Path $env:SystemRoot "System32\robocopy.exe"
  if (Test-Path $robo) {
    & $robo $electronSrc (Join-Path $nm "electron") /E /NFL /NDL /NJH /NJS /nc /ns /np | Out-Null
    # robocopy exit codes 0-7 are success
    if ($LASTEXITCODE -ge 8) { throw "robocopy electron failed (code $LASTEXITCODE)" }
  } else {
    Copy-Item -Recurse -Force $electronSrc (Join-Path $nm "electron")
  }

  $electronExe = Join-Path $Dest "node_modules\electron\dist\electron.exe"
  if (-not (Test-Path $electronExe)) {
    throw "Install incomplete: missing $electronExe"
  }
  if (-not (Test-Path (Join-Path $Dest ".output\server\index.mjs"))) {
    throw "Install incomplete: missing .output\server\index.mjs"
  }
  if (-not (Test-Path (Join-Path $Dest "desktop\main.mjs"))) {
    throw "Install incomplete: missing desktop\main.mjs"
  }
  Write-Ok "Files verified"

  # User PATH
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not $userPath) { $userPath = "" }
  if ($userPath -notlike "*$Dest*") {
    $newPath = if ($userPath.Trim().Length -eq 0) { $Dest } else { "$userPath;$Dest" }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Ok "Added to user PATH: $Dest"
  }

  if (-not $NoStartMenu) {
    Write-Step "Creating Start Menu shortcut"
    $Programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    New-Item -ItemType Directory -Force -Path $Programs | Out-Null
    $Lnk = Join-Path $Programs "GrokHub.lnk"
    $Wsh = New-Object -ComObject WScript.Shell
    $Sc = $Wsh.CreateShortcut($Lnk)
    # Target cmd.exe so a console flash is avoided via /c start? Better: target electron with args
    # Using grokhub.cmd is correct (starts UI + electron)
    $Sc.TargetPath = Join-Path $Dest "grokhub.cmd"
    $Sc.WorkingDirectory = $Dest
    $Sc.WindowStyle = 7  # minimized console for cmd launcher
    $Sc.Description = "GrokHub — Grok agent desktop"
    $icon = Join-Path $Dest "desktop\icons\icon.png"
    if (Test-Path $icon) {
      # PNG as IconLocation often fails on Windows — prefer ico if present
      $ico = Join-Path $Dest "desktop\icons\icon.ico"
      if (Test-Path $ico) { $Sc.IconLocation = "$ico,0" }
    }
    $Sc.Save()
    Write-Ok $Lnk

    # Also create a direct Electron shortcut (more reliable pin / no console)
    $Lnk2 = Join-Path $Programs "GrokHub (direct).lnk"
    $Sc2 = $Wsh.CreateShortcut($Lnk2)
    $Sc2.TargetPath = $electronExe
    $Sc2.Arguments = "`"$(Join-Path $Dest 'desktop\main.mjs')`""
    $Sc2.WorkingDirectory = $Dest
    $Sc2.WindowStyle = 1
    $Sc2.Description = "GrokHub (Electron direct — auto-starts UI)"
    $Sc2.Save()
    Write-Ok $Lnk2
  }

  # Environment for GROKHUB_HOME
  [Environment]::SetEnvironmentVariable("GROKHUB_HOME", $Dest, "User")
  Write-Ok "GROKHUB_HOME=$Dest"

  Write-Host ""
  Write-Host "Install complete." -ForegroundColor Green
  Write-Host "  Launch:  $Dest\grokhub.cmd"
  Write-Host "  Or:      Start Menu → GrokHub"
  Write-Host "  Data:    $env:APPDATA\GrokHub"
  Write-Host ""
  Write-Host "Open a NEW terminal if 'grokhub' is not on PATH yet." -ForegroundColor Yellow

  # Smoke: start once detached
  Write-Step "Smoke-launching GrokHub"
  $env:GROKHUB_HOME = $Dest
  Start-Process -FilePath (Join-Path $Dest "grokhub.cmd") -WorkingDirectory $Dest
  Write-Ok "Started (check taskbar / window)"
}
catch {
  Write-Host ""
  Write-Host "INSTALL FAILED: $($_.Exception.Message)" -ForegroundColor Red
  Write-Host $_.ScriptStackTrace -ForegroundColor DarkRed
  exit 1
}
finally {
  Pop-Location
}
