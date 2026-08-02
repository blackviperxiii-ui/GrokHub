#Requires -Version 5.1
<#
.SYNOPSIS
  Install GrokHub for the current Windows user (source / developer install).

.DESCRIPTION
  Builds the desktop UI if needed, copies app files into
  %LOCALAPPDATA%\GrokHub, installs a Start Menu shortcut, and adds
  a `grokhub` launcher on the user PATH.

  Usage (from repo root, PowerShell):
    powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
    powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -SkipBuild
    powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Uninstall
#>
[CmdletBinding()]
param(
  [switch]$SkipBuild,
  [switch]$Uninstall,
  [string]$Prefix = $(Join-Path $env:LOCALAPPDATA "GrokHub")
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$InstallDir = $Prefix
$BinDir = Join-Path $InstallDir "bin"
$IconsDir = Join-Path $InstallDir "icons"

function Write-Info($msg) { Write-Host "  $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "  $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  $msg" -ForegroundColor Yellow }

if ($Uninstall) {
  Write-Host "Uninstalling GrokHub from $InstallDir …"
  $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\GrokHub.lnk"
  $startup   = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Startup\GrokHub.lnk"
  $desktop   = Join-Path ([Environment]::GetFolderPath("Desktop")) "GrokHub.lnk"
  foreach ($p in @($startMenu, $startup, $desktop)) {
    if (Test-Path $p) { Remove-Item $p -Force; Write-Info "Removed $p" }
  }
  if (Test-Path $InstallDir) {
    Remove-Item $InstallDir -Recurse -Force
    Write-Ok "Removed $InstallDir"
  }
  Write-Ok "GrokHub uninstalled (user data in %APPDATA%\GrokHub left intact)."
  exit 0
}

Write-Host ""
Write-Host " GrokHub Windows installer" -ForegroundColor Magenta
Write-Host " ────────────────────────" -ForegroundColor DarkGray
Write-Host " Source:  $Root"
Write-Host " Target:  $InstallDir"
Write-Host ""

# Node / npm
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js not found. Install from https://nodejs.org/ (LTS) and re-run."
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "npm not found. Reinstall Node.js LTS."
}

# Dependencies
Push-Location $Root
try {
  if (-not (Test-Path (Join-Path $Root "node_modules\electron"))) {
    Write-Info "Installing npm dependencies (includes Electron)…"
    npm install --ignore-scripts
    # electron postinstall downloads binary
    npx electron --version | Out-Null
  }

  $serverEntry = Join-Path $Root ".output\server\index.mjs"
  if (-not $SkipBuild -or -not (Test-Path $serverEntry)) {
    Write-Info "Building desktop UI (GROKHUB_DESKTOP=1)…"
    $env:GROKHUB_DESKTOP = "1"
    $env:NODE_ENV = "production"
    npm run desktop:build
  }

  if (-not (Test-Path $serverEntry)) {
    throw "Build failed — missing $serverEntry"
  }

  Write-Info "Copying app files → $InstallDir"
  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
  New-Item -ItemType Directory -Force -Path $IconsDir | Out-Null

  # Clean previous runtime copies (keep user runtime logs outside)
  foreach ($name in @(".output", "desktop", "package.json", "APP_VERSION", "VERSION", "LICENSE")) {
    $dest = Join-Path $InstallDir $name
    if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
  }

  Copy-Item -Recurse -Force (Join-Path $Root ".output") (Join-Path $InstallDir ".output")
  Copy-Item -Recurse -Force (Join-Path $Root "desktop") (Join-Path $InstallDir "desktop")
  Copy-Item -Force (Join-Path $Root "package.json") (Join-Path $InstallDir "package.json")
  if (Test-Path (Join-Path $Root "APP_VERSION")) {
    Copy-Item -Force (Join-Path $Root "APP_VERSION") (Join-Path $InstallDir "APP_VERSION")
  }
  if (Test-Path (Join-Path $Root "VERSION")) {
    Copy-Item -Force (Join-Path $Root "VERSION") (Join-Path $InstallDir "VERSION")
  }
  if (Test-Path (Join-Path $Root "LICENSE")) {
    Copy-Item -Force (Join-Path $Root "LICENSE") (Join-Path $InstallDir "LICENSE")
  }

  # Icons
  $iconSrc = Join-Path $Root "desktop\icons"
  if (Test-Path $iconSrc) {
    Copy-Item -Force (Join-Path $iconSrc "*") $IconsDir -ErrorAction SilentlyContinue
  }
  $ico = Join-Path $Root "packaging\windows\icon.ico"
  if (Test-Path $ico) {
    Copy-Item -Force $ico (Join-Path $IconsDir "icon.ico")
    Copy-Item -Force $ico (Join-Path $InstallDir "desktop\icons\icon.ico")
  }

  # Resolve electron binary
  $electronCmd = Join-Path $Root "node_modules\electron\dist\electron.exe"
  if (-not (Test-Path $electronCmd)) {
    # After npm install electron, binary is under electron/dist
    $electronCmd = (Get-Command electron.cmd -ErrorAction SilentlyContinue)?.Source
    if ($electronCmd) {
      # Prefer real exe next to cli
      $cand = Join-Path $Root "node_modules\electron\dist\electron.exe"
      if (Test-Path $cand) { $electronCmd = $cand }
    }
  }
  if (-not (Test-Path $electronCmd)) {
    Write-Warn "electron.exe not found under node_modules — launcher will use 'electron' on PATH"
    $electronCmd = "electron"
  } else {
    # Keep a copy reference path for the launcher
    $electronLink = Join-Path $InstallDir "electron.exe"
    # Don't copy the whole electron dist (large); point at repo or use npx
    # For stable install, copy electron dist into install dir
    $electronDist = Join-Path $Root "node_modules\electron\dist"
    $electronInstall = Join-Path $InstallDir "electron-runtime"
    if (Test-Path $electronDist) {
      Write-Info "Bundling Electron runtime…"
      if (Test-Path $electronInstall) { Remove-Item $electronInstall -Recurse -Force }
      Copy-Item -Recurse -Force $electronDist $electronInstall
      $electronCmd = Join-Path $electronInstall "electron.exe"
    }
  }

  # Launcher .cmd
  $cmdPath = Join-Path $InstallDir "grokhub.cmd"
  $mainJs = Join-Path $InstallDir "desktop\main.mjs"
  @"
@echo off
setlocal
set GROKHUB_HOME=$InstallDir
set GROKHUB_EXEC=$InstallDir\grokhub.cmd
set ELECTRON_NO_ATTACH_CONSOLE=1
"$electronCmd" "%GROKHUB_HOME%\desktop\main.mjs" %*
"@ | Set-Content -Encoding ASCII -Path $cmdPath

  # Also bin\grokhub.cmd for PATH
  $binCmd = Join-Path $BinDir "grokhub.cmd"
  @"
@echo off
call "$InstallDir\grokhub.cmd" %*
"@ | Set-Content -Encoding ASCII -Path $binCmd

  # PowerShell launcher
  $ps1Path = Join-Path $InstallDir "grokhub.ps1"
  @"
`$env:GROKHUB_HOME = '$InstallDir'
`$env:GROKHUB_EXEC = '$InstallDir\grokhub.cmd'
& '$electronCmd' (Join-Path `$env:GROKHUB_HOME 'desktop\main.mjs') @args
"@ | Set-Content -Encoding UTF8 -Path $ps1Path

  # Start Menu shortcut
  Write-Info "Creating Start Menu shortcut…"
  $startMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
  New-Item -ItemType Directory -Force -Path $startMenuDir | Out-Null
  $lnk = Join-Path $startMenuDir "GrokHub.lnk"
  $Wsh = New-Object -ComObject WScript.Shell
  $sc = $Wsh.CreateShortcut($lnk)
  $sc.TargetPath = $electronCmd
  $sc.Arguments = "`"$mainJs`""
  $sc.WorkingDirectory = $InstallDir
  $sc.WindowStyle = 1
  $sc.Description = "GrokHub — Grok agent desktop"
  $iconIco = Join-Path $IconsDir "icon.ico"
  if (Test-Path $iconIco) { $sc.IconLocation = "$iconIco,0" }
  $sc.Save()

  # Desktop shortcut
  $desk = Join-Path ([Environment]::GetFolderPath("Desktop")) "GrokHub.lnk"
  Copy-Item -Force $lnk $desk

  # Add bin to user PATH if missing
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if ($userPath -notlike "*$BinDir*") {
    Write-Info "Adding $BinDir to user PATH…"
    [Environment]::SetEnvironmentVariable("Path", ($userPath.TrimEnd(';') + ";" + $BinDir), "User")
    $env:Path = $env:Path + ";" + $BinDir
  }

  Write-Host ""
  Write-Ok "GrokHub installed."
  Write-Host ""
  Write-Host "  Start Menu : GrokHub"
  Write-Host "  Desktop    : GrokHub.lnk"
  Write-Host "  Launcher   : $cmdPath"
  Write-Host "  CLI        : grokhub   (new terminals pick up PATH)"
  Write-Host "  User data  : $env:APPDATA\GrokHub"
  Write-Host ""
  Write-Host "  First run: open Settings → connect Grok (API key / OAuth / website link)."
  Write-Host ""

  # Optional: launch now
  $launch = Read-Host "Launch GrokHub now? [Y/n]"
  if ($launch -eq "" -or $launch -match '^[Yy]') {
    Start-Process -FilePath $electronCmd -ArgumentList "`"$mainJs`"" -WorkingDirectory $InstallDir
  }
}
finally {
  Pop-Location
}
