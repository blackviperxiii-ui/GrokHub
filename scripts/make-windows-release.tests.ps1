# scripts/make-windows-release.tests.ps1
$src = Get-Content -Raw "$PSScriptRoot/make-windows-release.ps1"
if ($src -notmatch 'missing grokhub.exe') { throw 'pack script must fail closed without grokhub.exe' }
if ($src -notmatch 'ProgramFiles\(x86\)') { throw 'pack script must probe x86 Inno Setup' }
if ($src -notmatch 'Get-Command ISCC') { throw 'pack script must probe PATH ISCC' }
if ($src -notmatch 'missing GrokHub-Setup-\$Ver.exe') { throw 'pack script must fail if Setup.exe is missing after ISCC' }
if ($src -notmatch 'grok-windows-artifact.ps1') { throw 'pack script must call grok download helper' }
if ($src -notmatch 'SkipGrok') { throw 'pack script must allow offline -SkipGrok' }
if ($src -notmatch 'missing grok.exe in stage') { throw 'release pack must fail closed without grok.exe' }
$dl = Get-Content -Raw "$PSScriptRoot/grok-windows-artifact.ps1"
if ($dl -notmatch 'AllowSkip') { throw 'grok download helper must allow an explicit skip' }
if ($dl -notmatch 'x.ai/cli/alpha') { throw 'must resolve version from x.ai/cli/alpha' }
if ($dl -notmatch 'storage.googleapis.com/grok-build-public-artifacts') { throw 'must fall back to GCS artifacts' }
$iss = Get-Content -Raw "$PSScriptRoot/../packaging/windows/grokhub.iss"
if ($iss -notmatch 'stage\\grok\.exe"; DestDir: "\{%USERPROFILE\}\\.grok\\bin"') { throw 'Inno must vendor grok.exe into ~/.grok/bin' }
if ($iss -notmatch 'SetupIconFile=grokhub.ico') { throw 'Inno must use the cabin icon for Setup.exe' }
Write-Output 'pack script locks ok'
