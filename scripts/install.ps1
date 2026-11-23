# Install GrokHub Shell on Windows from the prebuilt GitHub release, with
# nothing to compile.
#
# It downloads SHA256SUMS and SHA256SUMS.minisig, checks the signature
# against the release key (the same key as packaging/minisign.pub), checks the
# installer against SHA256SUMS, and only then runs it. The signature check
# uses minisign: the one on PATH, or minisign 0.12 from its GitHub release,
# which is itself checked against a pinned SHA-256 before it runs.
#
#   irm https://raw.githubusercontent.com/blackviperxiii-ui/GrokHub/main/scripts/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/blackviperxiii-ui/GrokHub/main/scripts/install.ps1))) -Channel beta
#   .\scripts\install.ps1 -Version v3.5.0
#   .\scripts\install.ps1 -FromDir C:\Downloads\grokhub   # files already downloaded, no network
#
# The channel is remembered in the config folder's `channel` receipt, the
# same one the app's Settings -> Updates switch reads and writes.
param(
    [ValidateSet('', 'beta', 'stable')]
    [string]$Channel = '',
    [string]$Version = '',
    [string]$FromDir = '',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo = 'blackviperxiii-ui/GrokHub'
$ReleasePubkey = 'RWR4FhfLw4OqMXxiX+l3k/KmAVfsRzTpUD28zoz3HacmWPiwbTnyXgeD'
$MinisignZip = 'https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip'
# SHA-256 of minisign-0.12-win64.zip, whose own minisig verifies with the
# minisign author's key (RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3).
$MinisignZipSha256 = '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479'

function Fail([string]$Why) {
    Write-Host "error: $Why" -ForegroundColor Red
    throw $Why
}

$ConfigDir = if ($env:GROKHUB_CONFIG) { $env:GROKHUB_CONFIG } else { Join-Path $env:APPDATA 'GrokHub' }
$Receipt = Join-Path $ConfigDir 'channel'
$Switch = [bool]$Channel
if (-not $Channel) {
    $Channel = 'stable'
    if (Test-Path $Receipt) {
        $saved = (Get-Content -Raw $Receipt).Trim().ToLowerInvariant()
        if ($saved -eq 'beta' -or $saved -eq 'stable') { $Channel = $saved }
    }
}
if ($Version) {
    if ($Version -notmatch '^v?\d+\.\d+\.\d+(-beta\.\d+)?$') {
        Fail "-Version takes a release tag like v3.5.0, not '$Version'"
    }
    $Version = 'v' + $Version.TrimStart('v')
}

if ($DryRun) {
    Write-Output "channel=$Channel switch=$([int]$Switch) receipt=$Receipt version=$Version fromdir=$FromDir"
    return
}

function Get-LatestTag {
    $headers = @{ 'User-Agent' = 'grokhub-install' }
    if ($Channel -eq 'stable') {
        $tag = (Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases/latest").tag_name
    } else {
        $list = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases?per_page=20"
        $tag = ($list | Where-Object { $_.tag_name -match '-beta\.\d+$' } | Select-Object -First 1).tag_name
    }
    if ($tag -notmatch '^v\d+\.\d+\.\d+(-beta\.\d+)?$') { Fail "couldn't find a $Channel release" }
    return $tag
}

function Get-ReleaseFile([string]$Name) {
    $to = Join-Path $Work $Name
    if ($FromDir) {
        $from = Join-Path $FromDir $Name
        if (-not (Test-Path -LiteralPath $from)) { Fail "$FromDir has no $Name" }
        Copy-Item -LiteralPath $from -Destination $to
    } else {
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$Repo/releases/download/$Tag/$Name" -OutFile $to
        } catch {
            Fail "couldn't download $Name from $Tag"
        }
    }
    return $to
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

# The minisign to check with: the one on PATH, or the pinned release.
function Get-Minisign {
    $onPath = Get-Command minisign -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $zip = Join-Path $Work 'minisign.zip'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $MinisignZip -OutFile $zip
    } catch {
        Fail "couldn't download minisign to check the release; install minisign and run this again"
    }
    if ((Get-Sha256 $zip) -ne $MinisignZipSha256) { Fail "the minisign download doesn't match its pinned SHA-256; nothing was installed" }
    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $Work 'minisign')
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    return (Join-Path $Work "minisign\minisign-win64\$arch\minisign.exe")
}

$Work = Join-Path ([IO.Path]::GetTempPath()) ("grokhub-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Work | Out-Null
try {
    if ($FromDir) {
        $Tag = if ($Version) { $Version } else { 'local files' }
        Write-Host "install: checking the release files in $FromDir"
    } else {
        $Tag = if ($Version) { $Version } else { Get-LatestTag }
        Write-Host "install: $Channel release $Tag"
    }
    $sums = Get-ReleaseFile 'SHA256SUMS'
    $sig = Get-ReleaseFile 'SHA256SUMS.minisig'
    $minisign = Get-Minisign
    & $minisign -V -q -P $ReleasePubkey -m $sums -x $sig | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "SHA256SUMS isn't signed by the release key; nothing was installed" }

    $line = Get-Content -LiteralPath $sums |
        Where-Object { $_ -match '^[0-9a-f]{64}  GrokHub-Shell-Setup-[^ ]+\.exe$' } |
        Select-Object -First 1
    if (-not $line) { Fail "that release has no Windows installer" }
    $want, $asset = $line -split '  ', 2
    $exe = Get-ReleaseFile $asset
    if ((Get-Sha256 $exe) -ne $want) { Fail "$asset doesn't match SHA256SUMS; nothing was installed" }
    Write-Host "install: verified $asset"

    # The installer installs for this user; /S runs it without its pages.
    $run = Start-Process -FilePath $exe -ArgumentList '/S' -Wait -PassThru
    if ($run.ExitCode -ne 0) { Fail "the installer stopped with exit code $($run.ExitCode)" }

    New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
    [IO.File]::WriteAllText($Receipt, "$Channel`n")
    if ($Switch) { Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $ConfigDir 'channel.beta-since') }
    Write-Host "installed GrokHub Shell (channel $Channel, $Tag)"
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue -LiteralPath $Work
}
