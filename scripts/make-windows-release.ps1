param([switch]$SkipGrok)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
if (-not (Test-Path 'Cargo.toml')) { throw "run from repo root" }
$Ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $Ver) { throw "version missing" }
cargo build --release --locked -p grokhub-app -p grokhub-hub
if (-not (Test-Path 'target/release/grokhub.exe')) { throw "missing grokhub.exe" }
if (-not (Test-Path 'target/release/grokhub-hub.exe')) { throw "missing grokhub-hub.exe" }
$Stage = Join-Path $Root 'packaging/windows/stage'
Remove-Item $Stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $Stage | Out-Null
Copy-Item 'target/release/grokhub.exe' $Stage
Copy-Item 'target/release/grokhub-hub.exe' $Stage
Copy-Item 'LICENSE' $Stage
$art = Join-Path $Root 'scripts/grok-windows-artifact.ps1'
if ($SkipGrok) {
  & $art -DestDir $Stage -AllowSkip
} else {
  & $art -DestDir $Stage
}
if (-not $SkipGrok -and -not (Test-Path (Join-Path $Stage 'grok.exe'))) {
  throw "missing grok.exe in stage"
}
New-Item -ItemType Directory -Path (Join-Path $Root 'dist-release') -Force | Out-Null
$candidates = @(
  "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
  "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
)
$Iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Iscc) {
  $cmd = Get-Command ISCC -ErrorAction SilentlyContinue
  if ($cmd) { $Iscc = $cmd.Source }
}
if (-not $Iscc) { throw "Inno Setup 6 ISCC.exe not found" }
& $Iscc "/DMyAppVersion=$Ver" (Join-Path $Root 'packaging/windows/grokhub.iss')
$Setup = Join-Path $Root "dist-release/GrokHub-Setup-$Ver.exe"
if (-not (Test-Path $Setup)) { throw "missing GrokHub-Setup-$Ver.exe" }
$Zip = Join-Path $Root "dist-release/grokhub-windows-v$Ver.zip"
Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $Zip -Force
Write-Output $Setup
Write-Output $Zip
