# scripts/make-windows-release.tests.ps1
$src = Get-Content -Raw "$PSScriptRoot/make-windows-release.ps1"
if ($src -notmatch 'missing grokhub.exe') { throw 'pack script must fail closed without grokhub.exe' }
if ($src -notmatch 'ProgramFiles\(x86\)') { throw 'pack script must probe x86 Inno Setup' }
if ($src -notmatch 'Get-Command ISCC') { throw 'pack script must probe PATH ISCC' }
if ($src -notmatch 'missing GrokHub-Setup-\$Ver.exe') { throw 'pack script must fail if Setup.exe is missing after ISCC' }
if ($src -match 'grok-windows-artifact|grok\.exe|SkipGrok') { throw 'pack script must not bundle the Grok Build CLI' }
if (Test-Path "$PSScriptRoot/grok-windows-artifact.ps1") { throw 'the Grok Build CLI download helper must stay deleted' }
$iss = Get-Content -Raw "$PSScriptRoot/../packaging/windows/grokhub.iss"
if ($iss -notmatch 'stage\\grokhub\.exe"; DestDir: "\{app\}"') { throw 'Inno must install grokhub.exe into the app dir' }
if ($iss -match 'grok\.exe|agent\.exe|install-grok-alpha|Grok Build CLI|\.grok\\bin') { throw 'Setup must not install the Grok Build CLI or touch ~/.grok' }
if ($iss -notmatch 'SetupIconFile=grokhub.ico') { throw 'Inno must use the cabin icon for Setup.exe' }
if (Test-Path "$PSScriptRoot/../packaging/windows/install-grok-alpha.ps1") { throw 'Setup must not ship a Grok Build CLI installer' }
Write-Output 'pack script locks ok'
