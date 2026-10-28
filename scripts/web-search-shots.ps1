# Windows proof shots for free cabin web_search (PR acceptance).
# Live-fetches DuckDuckGo Lite (real titles + snippets), seeds OpenRouter +
# local chats that cite the real result URLs (answer + Sources only — never
# the model-only UNTRUSTED fence), plus offline and Settings BYOK shots.
# Launches grokhub.exe at HARD DONE sizes and captures.
# Output: target/web-search-shots/*.png plus a cites.json manifest.
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

function Html-Decode([string]$s) {
    if ([string]::IsNullOrEmpty($s)) { return '' }
    return [System.Net.WebUtility]::HtmlDecode($s)
}

function Strip-Tags([string]$s) {
    if ([string]::IsNullOrEmpty($s)) { return '' }
    $s = [regex]::Replace($s, '<[^>]+>', '')
    return ($s -replace '\s+', ' ').Trim()
}

function Hit-Host([string]$url) {
    try {
        $u = [Uri]$url
        $h = $u.Host.ToLowerInvariant()
        if ($h.StartsWith('www.')) { $h = $h.Substring(4) }
        return $h
    } catch { return '' }
}

function Get-LiveHits([string]$q, [int]$max = 4) {
    $encoded = [Uri]::EscapeDataString($q)
    $url = "https://lite.duckduckgo.com/lite/?q=$encoded"
    Write-Host "DDG Lite GET $url"
    $resp = Invoke-WebRequest -Uri $url -UseBasicParsing -UserAgent 'GrokHub/3.4 (+web-search-shots)' -TimeoutSec 30
    $html = $resp.Content
    $hits = New-Object System.Collections.Generic.List[object]
    $seenHosts = New-Object 'System.Collections.Generic.HashSet[string]'
    # Match result-link anchors; href may precede or follow class=.
    $linkRx = [regex]::new(
        '<a\b(?=[^>]*\bclass=[''"]result-link[''"])[^>]*\bhref=[''"]([^''"]+)[''"][^>]*>(.*?)</a>',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase -bor
        [System.Text.RegularExpressions.RegexOptions]::Singleline
    )
    foreach ($m in $linkRx.Matches($html)) {
        if ($hits.Count -ge $max) { break }
        $href = Html-Decode $m.Groups[1].Value
        $uddg = [regex]::Match($href, 'uddg=([^&]+)')
        if (-not $uddg.Success) { continue }
        $raw = [Uri]::UnescapeDataString($uddg.Groups[1].Value)
        $raw = Html-Decode $raw
        if (-not ($raw.StartsWith('http://') -or $raw.StartsWith('https://'))) { continue }
        $hostName = Hit-Host $raw
        if ([string]::IsNullOrEmpty($hostName)) { continue }
        if (-not $seenHosts.Add($hostName)) { continue }
        $title = Strip-Tags (Html-Decode $m.Groups[2].Value)
        if ([string]::IsNullOrEmpty($title)) { $title = $hostName }
        # Snippet: next result-snippet cell before the following result-link.
        $after = $html.Substring($m.Index + $m.Length)
        if ($after.Length -gt 4000) { $after = $after.Substring(0, 4000) }
        $nextLink = $after.IndexOf('result-link')
        if ($nextLink -ge 0) { $after = $after.Substring(0, $nextLink) }
        $snip = ''
        $sm = [regex]::Match(
            $after,
            'class=[''"]result-snippet[''"][^>]*>(.*?)</td>',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase -bor
            [System.Text.RegularExpressions.RegexOptions]::Singleline
        )
        if ($sm.Success) {
            $snip = Strip-Tags (Html-Decode $sm.Groups[1].Value)
            if ($snip.Length -gt 220) { $snip = $snip.Substring(0, 220).Trim() + '…' }
        }
        $hits.Add([pscustomobject]@{ title = $title; url = $raw; snippet = $snip })
    }
    if ($hits.Count -lt 2) {
        throw "DDG Lite returned fewer than 2 public hosts for '$q' (got $($hits.Count))"
    }
    return $hits
}

function Format-Assistant([object[]]$hits, [string]$via) {
    # User-facing reply only: numbered answer + clean Sources. Never include
    # <<<UNTRUSTED>>> fences (those are model-only tool results).
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Here is what cabin ``web_search`` (DuckDuckGo Lite, free) found via **$via**:")
    $lines.Add('')
    $n = 1
    foreach ($h in $hits) {
        if ([string]::IsNullOrEmpty($h.snippet)) {
            $lines.Add("$n. [$($h.title)]($($h.url))")
        } else {
            $lines.Add("$n. [$($h.title)]($($h.url)) — $($h.snippet)")
        }
        $n++
    }
    $lines.Add('')
    $lines.Add('Sources:')
    $n = 1
    foreach ($h in $hits) {
        $lines.Add("$n. $($h.url)")
        $n++
    }
    return ($lines -join "`n")
}

function Json-Escape([string]$s) {
    if ($null -eq $s) { return '' }
    $s = $s.Replace('\', '\\').Replace('"', '\"').Replace("`r", '\r').Replace("`n", '\n').Replace("`t", '\t')
    return $s
}

function Seed-Chat([string]$cfg, [hashtable]$app, [string]$id, [string]$title, [string]$userAsk, [string]$assistant, [hashtable]$extraApp) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    foreach ($k in $extraApp.Keys) { $app[$k] = $extraApp[$k] }
    $app.currentThread = $id
    # Hand-built JSON: Windows PowerShell ConvertTo-Json mangles nested string arrays,
    # and open_fresh_home drops a loaded transcript — the cabin re-reads this seed
    # under GROKHUB_WEB_SEARCH_SHOT=1 after home open.
    $seed = @"
{"id":"$(Json-Escape $id)","title":"$(Json-Escape $title)","messages":[["user","$(Json-Escape $userAsk)"],["assistant","$(Json-Escape $assistant)"]]}
"@
    Write-Utf8 (Join-Path $cfg 'web-search-shot-seed.json') $seed
    Write-Utf8 (Join-Path $cfg 'threads.json') '[]'
    Write-Utf8 (Join-Path $cfg 'projects.json') '[]'
    $header = @{ id = $id; title = $title; created = $now; cwd = '.'; model = 'cabin'; engine = 'native'; renamed = $false } | ConvertTo-Json -Compress
    Write-Utf8 (Join-Path $cfg "sessions/$id.jsonl") "$header`n"
}

function Capture-Window([System.Diagnostics.Process]$p, [string]$name, [string]$png, [int]$settleSec = 14, [switch]$PageDown) {
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 60 -and $hwnd -eq [IntPtr]::Zero; $i++) {
        Start-Sleep -Seconds 1
        $p.Refresh()
        if ($p.HasExited) { break }
        $hwnd = $p.MainWindowHandle
    }
    if ($hwnd -eq [IntPtr]::Zero) {
        throw "no $($brand.productName) window for $name"
    }
    [void][WebSearchWin]::SetForegroundWindow($hwnd)
    Start-Sleep -Seconds $settleSec
    if ($PageDown) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][WebSearchWin]::SetForegroundWindow($hwnd)
        # Settings → Cabin defaults: nudge the body so Web search is on screen.
        1..4 | ForEach-Object {
            [System.Windows.Forms.SendKeys]::SendWait('{PGDN}')
            Start-Sleep -Milliseconds 400
        }
        Start-Sleep -Seconds 2
    }
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
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "wrote $png ($((Get-Item $png).Length) bytes)"
    $bmp.Dispose()
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
    $env:GROKHUB_WEB_SEARCH_SHOT = '1'
    Remove-Item Env:GROKHUB_WEB_SEARCH_SETTINGS -ErrorAction SilentlyContinue
    Remove-Item Env:GROKHUB_DECK_SETTINGS_SEC -ErrorAction SilentlyContinue
    Remove-Item Env:GROKHUB_DECK_SHOT -ErrorAction SilentlyContinue
    Remove-Item Env:GROKHUB_DECK_MOOD -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru `
        -RedirectStandardError (Join-Path $root 'err.txt') -RedirectStandardOutput (Join-Path $root 'out.txt')
    try {
        Capture-Window $p $name (Join-Path $Out "$name.png")
    } catch {
        Get-Content (Join-Path $root 'err.txt') -ErrorAction SilentlyContinue | Select-Object -Last 40
        throw
    }
}

function Shot-Settings([int]$w, [int]$h, [string]$name) {
    $root = Join-Path $env:RUNNER_TEMP "websearch-$name"
    if (-not $env:RUNNER_TEMP) { $root = Join-Path $env:TEMP "websearch-$name" }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    $cfg = Join-Path $root 'cfg'
    New-Item -ItemType Directory -Force -Path $cfg | Out-Null
    # SearXNG selected so the optional own-server URL field is visible.
    $app = @{
        getStartedDone = $true; setupDone = $true; theme = 'ops-dark'; themeMigrated = $true
        deviceName = 'my-pc'; window = @{ x = 0; y = 0; w = $w; h = $h; maximized = $false }
        webSearch = @{ backend = 'searxng'; searxngUrl = 'https://searx.example' }
        providerModel = 'openrouter/auto'; localModel = $false
    }
    Write-Utf8 (Join-Path $cfg 'app.json') ($app | ConvertTo-Json -Depth 6)
    Write-Utf8 (Join-Path $cfg 'threads.json') '[]'
    Write-Utf8 (Join-Path $cfg 'projects.json') '[]'
    $env:GROKHUB_CONFIG = $cfg
    $env:HOME = $root
    $env:USERPROFILE = $root
    $env:APPDATA = Join-Path $root 'AppData'
    $env:LOCALAPPDATA = Join-Path $root 'LocalAppData'
    $env:GROKHUB_RENDERER = 'wgpu'
    $env:GROKHUB_WEB_SEARCH_SHOT = '1'
    $env:GROKHUB_WEB_SEARCH_SETTINGS = '1'
    $env:GROKHUB_DECK_SETTINGS_SEC = 'defaults'
    Remove-Item Env:GROKHUB_DECK_SHOT -ErrorAction SilentlyContinue
    Remove-Item Env:GROKHUB_DECK_MOOD -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru `
        -RedirectStandardError (Join-Path $root 'err.txt') -RedirectStandardOutput (Join-Path $root 'out.txt')
    try {
        Capture-Window $p $name (Join-Path $Out "$name.png") -settleSec 16 -PageDown
    } catch {
        Get-Content (Join-Path $root 'err.txt') -ErrorAction SilentlyContinue | Select-Object -Last 40
        throw
    }
}

$hits = @(Get-LiveHits $Query 4)
$urls = @($hits | ForEach-Object { $_.url })
Write-Host "live cites:"
$urls | ForEach-Object { Write-Host "  $_" }
Write-Host "snippets:"
$hits | ForEach-Object { Write-Host "  $($_.title): $($_.snippet.Substring(0, [Math]::Min(80, $_.snippet.Length)))" }

$userAsk = "Search the web for current Rust release notes and cite the real links."
$orAssistant = Format-Assistant $hits 'OpenRouter + cabin web_search'
$localAssistant = Format-Assistant $hits 'local model + cabin web_search'
$offlineAsk = "Search the web for today's headlines."
$offlineAssistant = "Web search is unavailable (no network or the search host did not answer). Continue without live sources, or try again when online."

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
Shot 1600 900 'offline-1600x900' 'ws-offline' 'Offline · web search' $offlineAsk $offlineAssistant @{
    providerModel = 'openrouter/auto'
    localModel = $false
}
Shot-Settings 1600 900 'settings-web-search-1600x900'

$manifest = @{
    query = $Query
    backend = 'duckduckgo'
    fetchedAt = [DateTimeOffset]::UtcNow.ToString('o')
    cites = $urls
    snippets = @($hits | ForEach-Object { @{ title = $_.title; url = $_.url; snippet = $_.snippet } })
    shots = @(
        'openrouter-1600x900.png', 'openrouter-1100x720.png',
        'local-1600x900.png', 'local-1100x720.png',
        'offline-1600x900.png', 'settings-web-search-1600x900.png'
    )
    note = 'Live DDG Lite titles/URLs/snippets seeded into OpenRouter and local chats (answer + Sources only, no UNTRUSTED fence); offline unavailable message; Settings → Cabin defaults with SearXNG URL field. No provider keys / no live model turn.'
} | ConvertTo-Json -Depth 6
Write-Utf8 (Join-Path $Out 'cites.json') $manifest
Get-ChildItem $Out | Format-Table Name, Length
Write-Host 'web-search-shots done'
