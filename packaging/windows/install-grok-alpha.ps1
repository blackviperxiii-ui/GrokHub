# Official Grok Build CLI alpha. Setup and first cabin launch must not assume grok is on PATH.
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$env:GROK_CHANNEL = 'alpha'
try {
  Invoke-Expression ((Invoke-WebRequest -Uri 'https://x.ai/cli/install.ps1' -UseBasicParsing).Content)
} catch {
  $ver = $null
  foreach ($u in @(
      'https://x.ai/cli/alpha',
      'https://storage.googleapis.com/grok-build-public-artifacts/cli/alpha'
    )) {
    try {
      $cand = (Invoke-WebRequest -Uri $u -UseBasicParsing).Content.Trim()
      if ($cand -match '^\d+\.\d+\.\d+') {
        $ver = $cand
        break
      }
    } catch {}
  }
  if (-not $ver) { throw "Grok Build CLI alpha install failed: $_" }
  $dir = Join-Path $env:USERPROFILE '.grok\bin'
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $out = Join-Path $dir 'grok.exe'
  $ok = $false
  foreach ($base in @(
      "https://x.ai/cli/grok-$ver-windows-x86_64.exe",
      "https://storage.googleapis.com/grok-build-public-artifacts/cli/grok-$ver-windows-x86_64.exe"
    )) {
    try {
      Invoke-WebRequest -Uri $base -OutFile $out -UseBasicParsing
      $ok = $true
      break
    } catch {}
  }
  if (-not $ok) { throw "Grok Build CLI alpha download failed: $_" }
  Copy-Item $out (Join-Path $dir 'agent.exe') -Force
}
$bin = Join-Path $env:USERPROFILE '.grok\bin\grok.exe'
if (-not (Test-Path $bin)) {
  throw 'Grok Build CLI alpha install finished but grok.exe was not found'
}
