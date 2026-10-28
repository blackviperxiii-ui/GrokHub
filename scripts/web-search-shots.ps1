# Windows proof shots for free cabin web_search (PR acceptance).
# Live-fetches DuckDuckGo Lite, seeds OpenRouter + local chats that cite the
# real result URLs, launches grokhub.exe at HARD DONE sizes, and captures.
# No provider keys and no live model turn — citations are from the live search
# HTML. Output: target/web-search-shots/*.png plus a cites.json manifest.
param(
    [string]$Exe = 'target/release/grokhub.exe',
    [string]$Out = 'target/web-search-shots',
    [string]$Query = 'Rust programming language release notes'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WebSearchWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[void][WebSearchWin]::SetProcessDPIAware()
try { Set-DisplayResolution -Width 1920 -Height 1080 -Force } catch { Write-Host "display: $_" }
$screenW = [WebSearchWin]::GetSystemMetrics(0)
$screenH = [WebSearchWin]::GetSystemMetrics(1)
Write-Host "screen ${screenW}x${screenH}"

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$exe = (Resolve-Path $Exe).Path
$brand = Get-Content (Join-Path (Resolve-Path .).Path 'brand/brand.json') -Raw | ConvertFrom-Json

function Write-Utf8($path, [string]$text) {
    New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
    [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding $false))
}

function Get-LiveHits([string]$q, [int]$max = 4) {
    $encoded = [Uri]::EscapeDataString($q)
    $url = "https://lite.duckduckgo.com/lite/?q=$encoded"
    Write-Host "DDG Lite GET $url"
    $resp = Invoke-WebRequest -Uri $url -UseBasicParsing -UserAgent 'GrokHub/3.4 (+web-search-shots)' -TimeoutSec 30
    $html = $resp.Content
    $hits = New-Object System.Collections.Generic.List[object]
    $rx = [regex]'uddg=([^&"]+)'
    foreach ($m in $rx.Matches($html)) {
        if ($hits.Count -ge $max) { break }
        $raw = [Uri]::UnescapeDataString($m.Groups[1].Value)
        $raw = [System.Net.WebUtility]::HtmlDecode($raw)
        if (-not ($raw.StartsWith('http://') -or $raw.StartsWith('https://'))) { continue }
        if ($hits | Where-Object { $_.url -eq $raw }) { continue }
        $titleHost = ([Uri]$raw).Host
        $hits.Add([pscustomobject]@{ title = $titleHost; url = $raw; snippet = "Live DuckDuckGo Lite hit for: $q" })
    }
    if ($hits.Count -lt 2) {
        throw "DDG Lite returned fewer than 2 public URLs for '$q' (got $($hits.Count))"
    }
    return $hits
}

function Format-Assistant([object[]]$hits, [string]$via) {
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Here is what cabin ``web_search`` (DuckDuckGo Lite, free) found via **$via**:")
    $lines.Add('')
    $n = 1
    foreach ($h in $hits) {
        $lines.Add("$n. [$($h.title)]($($h.url)) — $($h.snippet)")
        $n++
    }
    $lines.Add('')
    $lines.Add('Sources (real links from the live search host):')
    foreach ($h in $hits) {
        $lines.Add("- $($h.url)")
    }
    $fenceBody = ($hits | ForEach-Object { "1. $($_.title)`n   $($_.url)`n   $($_.snippet)" }) -join "`n"
    $nonce = -join ((1..8) | ForEach-Object { '{0:x}' -f (Get-Random -Max 16) })
    $lines.Add('')
    $lines.Add("<<<UNTRUSTED source=web_search nonce=$nonce>>>")
    $lines.Add($fenceBody)
    $lines.Add("<<<END_UNTRUSTED nonce=$nonce>>>")
    return ($lines -join "`n")
}

function Seed-Chat([string]$cfg, [hashtable]$app, [string]$id, [string]$title, [string]$userAsk, [string]$assistant, [hashtable]$extraApp) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    foreach ($k in $extraApp.Keys) { $app[$k] = $extraApp[$k] }
    $app.currentThread = $id
    $messages = @(
        @('user', $userAsk),
        @('assistant', $assistant)
    )
    $thread = @{
        id = $id; title = $title; native = $true; grokSession = $id
        accessedMs = $now; lastActiveMs = $now; messages = $messages
    }
    Write-Utf8 (Join-Path $cfg 'threads.json') (ConvertTo-Json -InputObject @($thread) -Depth 6)
    Write-Utf8 (Join-Path $cfg 'projects.json') '[]'
    $header = @{ id = $id; title = $title; created = $now; cwd = '.'; model = 'cabin'; engine = 'native'; renamed = $false } | ConvertTo-Json -Compress
    Write-Utf8 (Join-Path $cfg "sessions/$id.jsonl") "$header`n"
}

function Shot([int]$w, [int]$h, [string]$name, [string]$id, [string]$title, [string]$userAsk, [string]$assistant, [hashtable]$extraApp) {
    $root = Join-Path $env:RUNNER_TEMP "websearch-$name"
    if (-not $env:RUNNER_TEMP) { $root = Join-Path $env:TEMP "websearch-$name" }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    $cfg = Join-Path $root 'cfg'
    New-Item -ItemType Directory -Force -Path $cfg | Out-Null
    $app = @{
        getStartedDone = $true; setupDone = $true; theme = 'ops-dark'; themeMigrated = $true
        deviceName = 'my-pc'; window = @{ x = 0; y = 0; w = $w; h = $h; maximized = $false }
        webSearch = @{ backend = 'duckduckgo'; searxngUrl = '' }
    }
    Seed-Chat $cfg $app $id $title $userAsk $assistant $extraApp
    Write-Utf8 (Join-Path $cfg 'app.json') ($app | ConvertTo-Json -Depth 6)
    $env:GROKHUB_CONFIG = $cfg
    $env:HOME = $root
    $env:USERPROFILE = $root
    $env:APPDATA = Join-Path $root 'AppData'
    $env:LOCALAPPDATA = Join-Path $root 'LocalAppData'
    $env:GROKHUB_RENDERER = 'wgpu'
    # Do not set GROKHUB_DECK_SHOT — that seed overwrites the citation transcript.
    Remove-Item Env:GROKHUB_DECK_SHOT -ErrorAction SilentlyContinue
    Remove-Item Env:GROKHUB_DECK_MOOD -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru `
        -RedirectStandardError (Join-Path $root 'err.txt') -RedirectStandardOutput (Join-Path $root 'out.txt')
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 60 -and $hwnd -eq [IntPtr]::Zero; $i++) {
        Start-Sleep -Seconds 1
        $p.Refresh()
        if ($p.HasExited) { break }
        $hwnd = $p.MainWindowHandle
    }
    if ($hwnd -eq [IntPtr]::Zero) {
        Get-Content (Join-Path $root 'err.txt') -ErrorAction SilentlyContinue | Select-Object -Last 40
        throw "no $($brand.productName) window for $name at ${w}x${h}"
    }
    [void][WebSearchWin]::SetForegroundWindow($hwnd)
    Start-Sleep -Seconds 14
    $r = New-Object WebSearchWin+RECT
    [void][WebSearchWin]::GetWindowRect($hwnd, [ref]$r)
    $cw = $r.R - $r.L; $ch = $r.B - $r.T
    Write-Host "window ${name}: rect $($r.L),$($r.T) ${cw}x${ch}"
    $bmp = New-Object System.Drawing.Bitmap $cw, $ch
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    if ($r.R -le $screenW -and $r.B -le $screenH) {
        $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    } else {
        $dc = $g.GetHdc()
        Write-Host "PrintWindow ${name}: $([WebSearchWin]::PrintWindow($hwnd, $dc, 2))"
        $g.ReleaseHdc($dc)
    }
    $g.Dispose()
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $png = Join-Path $Out "$name.png"
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "wrote $png ($((Get-Item $png).Length) bytes)"
    $bmp.Dispose()
}

$hits = @(Get-LiveHits $Query 4)
$urls = @($hits | ForEach-Object { $_.url })
Write-Host "live cites:"
$urls | ForEach-Object { Write-Host "  $_" }

$userAsk = "Search the web for current Rust release notes and cite the real links."
$orAssistant = Format-Assistant $hits 'OpenRouter + cabin web_search'
$localAssistant = Format-Assistant $hits 'local model + cabin web_search'

Shot 1600 900 'openrouter-1600x900' 'ws-openrouter' 'OpenRouter · web search' $userAsk $orAssistant @{
    providerModel = 'openrouter/auto'
    localModel = $false
}
Shot 1100 720 'openrouter-1100x720' 'ws-openrouter' 'OpenRouter · web search' $userAsk $orAssistant @{
    providerModel = 'openrouter/auto'
    localModel = $false
}
Shot 1600 900 'local-1600x900' 'ws-local' 'Local · web search' $userAsk $localAssistant @{
    localModel = $true
    providerModel = ''
}
Shot 1100 720 'local-1100x720' 'ws-local' 'Local · web search' $userAsk $localAssistant @{
    localModel = $true
    providerModel = ''
}

$manifest = @{
    query = $Query
    backend = 'duckduckgo'
    fetchedAt = [DateTimeOffset]::UtcNow.ToString('o')
    cites = $urls
    shots = @(
        'openrouter-1600x900.png', 'openrouter-1100x720.png',
        'local-1600x900.png', 'local-1100x720.png'
    )
    note = 'Live DDG Lite URLs seeded into OpenRouter and local chat transcripts on windows-latest; no provider keys / no live model turn.'
} | ConvertTo-Json -Depth 4
Write-Utf8 (Join-Path $Out 'cites.json') $manifest
Get-ChildItem $Out | Format-Table Name, Length
Write-Host 'web-search-shots done'
