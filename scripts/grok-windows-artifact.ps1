# scripts/grok-windows-artifact.ps1
param(
  [Parameter(Mandatory = $true)][string]$DestDir,
  [switch]$AllowSkip
)
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Path $DestDir -Force | Out-Null

function Get-GrokVersion([string]$Url) {
  (Invoke-WebRequest -Uri $Url -UseBasicParsing).Content.Trim()
}

function Save-Grok([string]$Url, [string]$OutFile) {
  Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
}

try {
  $ver = $null
  foreach ($u in @(
      'https://x.ai/cli/alpha',
      'https://storage.googleapis.com/grok-build-public-artifacts/cli/alpha'
    )) {
    try {
      $cand = Get-GrokVersion $u
      if ($cand -match '^\d+\.\d+\.\d+') {
        $ver = $cand
        break
      }
    } catch {}
  }
  if (-not $ver) { throw 'could not resolve Grok Build version from x.ai/cli/alpha' }
  $out = Join-Path $DestDir 'grok.exe'
  $ok = $false
  foreach ($url in @(
      "https://x.ai/cli/grok-$ver-windows-x86_64.exe",
      "https://storage.googleapis.com/grok-build-public-artifacts/cli/grok-$ver-windows-x86_64.exe"
    )) {
    try {
      Save-Grok $url $out
      $ok = $true
      break
    } catch {}
  }
  if (-not $ok) { throw "grok $ver download failed" }
  Copy-Item $out (Join-Path $DestDir 'agent.exe')
  Write-Output "bundled $ver"
  exit 0
} catch {
  if ($AllowSkip) {
    Write-Warning "grok download skipped: $_"
    exit 0
  }
  throw
}
