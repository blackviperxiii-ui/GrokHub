#Requires -Version 5.1
<#
.SYNOPSIS
  Download GrokHub from GitHub (ZIP) and install — no git required.

  Uses Windows / PowerShell TLS (not Git's ca-bundle.crt), so it works when:
    fatal: error adding trust anchors from file: .../ca-bundle.crt

.EXAMPLE
  # From any folder (e.g. Downloads or your user profile):
  irm https://raw.githubusercontent.com/blackviperxiii-ui/Grok-Hub/main/packaging/windows/download-and-install.ps1 | iex

  # Or save the file and run:
  powershell -ExecutionPolicy Bypass -File download-and-install.ps1
#>
param(
  [string]$Repo = "blackviperxiii-ui/Grok-Hub",
  [string]$Branch = "main",
  [string]$WorkDir = "",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Write-Step($m) { Write-Host "`n==> $m" -ForegroundColor Cyan }
function Write-Ok($m) { Write-Host "    $m" -ForegroundColor Green }

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js LTS is required first: https://nodejs.org/  (install, then open a NEW PowerShell)"
}
Write-Ok "Node $(& node -v)"

if (-not $WorkDir) {
  $WorkDir = Join-Path $env:USERPROFILE "Downloads\Grok-Hub-src"
}
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$zip = Join-Path $env:TEMP "Grok-Hub-$Branch.zip"
$url = "https://github.com/$Repo/archive/refs/heads/$Branch.zip"

Write-Step "Downloading $url"
# Invoke-WebRequest uses the Windows certificate store (not Git's mingw ca-bundle)
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
Write-Ok "Saved $zip"

Write-Step "Extracting to $WorkDir"
if (Test-Path $WorkDir) {
  Get-ChildItem $WorkDir -Force | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}
Expand-Archive -Path $zip -DestinationPath $WorkDir -Force
# GitHub zip extracts to Repo-branch\
$inner = Get-ChildItem $WorkDir -Directory | Select-Object -First 1
if (-not $inner) { throw "ZIP extract produced no folder" }
$Root = $inner.FullName
Write-Ok "Source: $Root"

$install = Join-Path $Root "packaging\windows\install.ps1"
if (-not (Test-Path $install)) {
  throw "install.ps1 missing in archive — unexpected layout at $Root"
}

Write-Step "Running installer"
$args = @("-ExecutionPolicy", "Bypass", "-File", $install)
if (-not $SkipBuild) { $args += "-Build" }
& powershell @args
if ($LASTEXITCODE -ne 0) { throw "install.ps1 failed (exit $LASTEXITCODE)" }

Write-Host "`nDone. Launch Start Menu → GrokHub or:" -ForegroundColor Green
Write-Host "  $env:LOCALAPPDATA\GrokHub\grokhub.cmd"
