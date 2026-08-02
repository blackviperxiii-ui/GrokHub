# Install GrokHub for the current Windows user.
# Usage (from repo root, PowerShell):
#   powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1
# Optional:
#   -Build   run desktop production build first
#   -StartMenu  create Start Menu shortcut (default true)

param(
  [switch]$Build,
  [bool]$StartMenu = $true,
  [string]$Prefix = ""
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Dest = if ($Prefix) { $Prefix } else { Join-Path $env:LOCALAPPDATA "GrokHub" }

Write-Host "GrokHub Windows install"
Write-Host "  Source: $Root"
Write-Host "  Dest:   $Dest"

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js is required. Install from https://nodejs.org/ (LTS 20+)."
}

Push-Location $Root
try {
  if ($Build -or -not (Test-Path (Join-Path $Root ".output\server\index.mjs"))) {
    Write-Host "Building desktop UI…"
    if (-not (Test-Path "node_modules")) {
      npm ci --ignore-scripts
      if ($LASTEXITCODE -ne 0) { npm install --ignore-scripts }
    }
    # electron for the shell
    if (-not (Test-Path "node_modules\electron")) {
      npm install --save-dev electron@^35 --ignore-scripts
      npx electron install
    }
    $env:GROKHUB_DESKTOP = "1"
    npm run build
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
  }

  New-Item -ItemType Directory -Force -Path $Dest | Out-Null
  # Clean previous app payload (keep user data elsewhere under %APPDATA%\GrokHub)
  foreach ($name in @(".output", "desktop", "packaging", "node_modules", "package.json", "VERSION", "APP_VERSION")) {
    $p = Join-Path $Dest $name
    if (Test-Path $p) { Remove-Item -Recurse -Force $p }
  }

  Copy-Item -Recurse -Force (Join-Path $Root ".output") (Join-Path $Dest ".output")
  Copy-Item -Recurse -Force (Join-Path $Root "desktop") (Join-Path $Dest "desktop")
  New-Item -ItemType Directory -Force -Path (Join-Path $Dest "packaging\windows") | Out-Null
  Copy-Item -Force (Join-Path $Root "packaging\windows\grokhub.cmd") (Join-Path $Dest "grokhub.cmd")
  Copy-Item -Force (Join-Path $Root "packaging\windows\grokhub.ps1") (Join-Path $Dest "grokhub.ps1")
  Copy-Item -Force (Join-Path $Root "package.json") (Join-Path $Dest "package.json")
  if (Test-Path (Join-Path $Root "APP_VERSION")) {
    Copy-Item -Force (Join-Path $Root "APP_VERSION") (Join-Path $Dest "APP_VERSION")
  }
  if (Test-Path (Join-Path $Root "node_modules\electron")) {
    Write-Host "Copying electron runtime…"
    New-Item -ItemType Directory -Force -Path (Join-Path $Dest "node_modules") | Out-Null
    Copy-Item -Recurse -Force (Join-Path $Root "node_modules\electron") (Join-Path $Dest "node_modules\electron")
  }

  # User PATH entry (LocalAppData\GrokHub)
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if ($userPath -notlike "*$Dest*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$Dest", "User")
    Write-Host "Added $Dest to user PATH (new terminals pick this up)."
  }

  if ($StartMenu) {
    $Wsh = New-Object -ComObject WScript.Shell
    $Programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    $Lnk = Join-Path $Programs "GrokHub.lnk"
    $Sc = $Wsh.CreateShortcut($Lnk)
    $Sc.TargetPath = Join-Path $Dest "grokhub.cmd"
    $Sc.WorkingDirectory = $Dest
    $Sc.WindowStyle = 1
    $Sc.Description = "GrokHub — Grok agent desktop"
    $icon = Join-Path $Dest "desktop\icons\icon.png"
    if (Test-Path $icon) { $Sc.IconLocation = "$icon,0" }
    $Sc.Save()
    Write-Host "Start Menu: $Lnk"
  }

  Write-Host ""
  Write-Host "Installed. Launch with:"
  Write-Host "  $Dest\grokhub.cmd"
  Write-Host "  or Start Menu → GrokHub"
  Write-Host ""
  Write-Host "User data (chats, secrets) lives under:"
  Write-Host "  $env:APPDATA\GrokHub"
} finally {
  Pop-Location
}
