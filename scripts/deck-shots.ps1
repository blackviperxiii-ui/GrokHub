# Screenshot the Windows cabin (grokhub.exe) at the deck-match sizes and put
# each shot beside a deck slide from docs/design/loadout-baseline-v1/.
# Runs in CI after the release build. Uses a throwaway config and home, never
# the runner user's real profile. Output: target/deck-shots/*.png, plus each
# app shot as base64 JPEG between markers in the log so agents without
# artifact access can read it.
param(
    [string]$Exe = 'target/release/grokhub.exe',
    [string]$Slide = '02',
    [string]$UpgradeSlide = '08',
    [string]$Out = 'target/deck-shots'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DeckWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[void][DeckWin]::SetProcessDPIAware()
# The 2560 Pulse shot wants a wide screen; fall back to 1920x1080.
try { Set-DisplayResolution -Width 2560 -Height 1440 -Force } catch {
    Write-Host "display 2560: $_"
    try { Set-DisplayResolution -Width 1920 -Height 1080 -Force } catch { Write-Host "display: $_" }
}
$screenW = [DeckWin]::GetSystemMetrics(0); $screenH = [DeckWin]::GetSystemMetrics(1)
Write-Host "screen ${screenW}x${screenH}"

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$repo = (Resolve-Path .).Path
$exe = (Resolve-Path $Exe).Path
$brand = Get-Content (Join-Path $repo 'brand/brand.json') -Raw | ConvertFrom-Json

function Save-Jpeg($bmp, $path) {
    $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
    $params = New-Object System.Drawing.Imaging.EncoderParameters 1
    $params.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 85L
    $bmp.Save($path, $codec, $params)
}

function Write-Utf8($path, [string]$text) {
    New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
    [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding $false))
}

# Real dock data in the throwaway config: a chat whose saved session holds a
# five-step todo list (Objectives), 23 finished tasks in the outcome log
# (LVL and XP) and two unread home cards (From Pulse). The chat has no
# transcript, so launch keeps it open instead of starting a fresh one.
function Seed-Dock([string]$cfg, [hashtable]$app, [string]$first = 'Diamond, 3 wins left', [bool]$transcript = $true) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $id = 'deck-hangout'
    $steps = @($first, 'Aim routine 12/20', 'Warm up in the range', 'Review last match', 'Pick a duo for tonight')
    $todos = @{ todos = @(for ($i = 0; $i -lt $steps.Count; $i++) {
        @{ id = "$($i + 1)"; content = $steps[$i]; status = $(if ($i -eq 0) { 'in_progress' } else { 'pending' }) }
    }) } | ConvertTo-Json -Depth 4 -Compress
    $session = @(
        (@{ id = $id; title = 'Hangout'; created = $now; cwd = '.'; model = 'grok'; engine = 'native'; renamed = $false } | ConvertTo-Json -Compress),
        (@{ type = 'function_call'; call_id = 'c1'; name = 'todo_write'; arguments = $todos } | ConvertTo-Json -Compress)
    ) -join "`n"
    Write-Utf8 (Join-Path $cfg "sessions/$id.jsonl") "$session`n"
    # Sample Hangout transcript so deck-match 4 shots show red-rim assistant
    # bubbles, the companion face, and the chat header beside the dock.
    $messages = @(
        @('user', 'yo, what are we running tonight?'),
        @('assistant', "Diamond climb. You're 3 wins off — want a warm-up in the range, or I queue the duo finder?"),
        @('user', 'queue the duo and fix my stutter first'),
        @('assistant', "On it. I'll clear the shader cache, then find you a Diamond duo for tonight.")
    )
    # A thread with a transcript reopens as a fresh chat, so the session's
    # checklist never loads; mission shots seed it empty.
    if (-not $transcript) { $messages = @() }
    $thread = @{ id = $id; title = 'Hangout'; native = $true; grokSession = $id; accessedMs = $now; lastActiveMs = $now; messages = $messages }
    $threads = @($thread)
    if (-not $transcript) {
        # Older chats for the chat-first Recent list.
        $threads += @{ id = 'deck-fan'; title = 'Fan curve tweak'; native = $true; accessedMs = $now - 3600000; lastActiveMs = $now - 3600000; messages = @(,@('user', 'make the fans quieter')) }
        $threads += @{ id = 'deck-backlog'; title = 'Backlog sort'; native = $true; accessedMs = $now - 7200000; lastActiveMs = $now - 7200000; messages = @(,@('user', 'sort my backlog')) }
    }
    Write-Utf8 (Join-Path $cfg 'threads.json') (ConvertTo-Json -InputObject $threads -Depth 4)
    Write-Utf8 (Join-Path $cfg 'projects.json') '[]'
    $cards = @(
        @{ id = 'done-drv'; kind = 'automation_done'; title = 'I can update your GPU driver tonight'; body = 'Restore point first';
           createdAt = $now - 3600000; status = 'unread'; action = @{ type = 'open_automations' }; sourceId = 'drv' },
        @{ id = 'done-patch'; kind = 'automation_done'; title = 'Season patch: your main got a buff'; body = 'Patch notes';
           createdAt = $now - 10800000; status = 'unread'; action = @{ type = 'open_automations' }; sourceId = 'patch' }
    )
    Write-Utf8 (Join-Path $cfg 'updates.json') (ConvertTo-Json -InputObject $cards -Depth 4)
    # Five distinct calendar days in the last week so Mission Control streak
    # paints ~5 DAYS (not a single empty "0 DAYS" from one clustered timestamp).
    $lines = @()
    for ($d = 0; $d -lt 5; $d++) {
        $at = $now - ($d * 86400000) - 3600000
        $lines += @{ task = "streak-day-$d"; signature = 'topic:deck'; result = 'success'; reasons = @('verify_ok', 'accepted'); finished_at = $at; at = $at } | ConvertTo-Json -Compress
    }
    for ($i = 0; $i -lt 18; $i++) {
        $at = $now - 172800000 - $i * 60000
        $lines += @{ task = "task $i"; signature = 'topic:deck'; result = 'success'; reasons = @('verify_ok', 'accepted'); finished_at = $at; at = $at } | ConvertTo-Json -Compress
    }
    Write-Utf8 (Join-Path $cfg 'outcomes.jsonl') (($lines -join "`n") + "`n")
    $app.currentThread = $id
    if (-not $app.personality) { $app.personality = 'viper' }
    # Viper is adults-only; without this the titlebar falls back to Nyx.
    if ($app.personality -eq 'viper') { $app.explicitPersonalities = $true }
}

# Pulse with real cards: four ideas (Up next) and three feed cards, so the
# columns, tiling and the Feed | Ideas switch have something to reflow.
# A Steam library under the throwaway home (HOME is the shot's root): ten
# installed games with play time and last-played dates for the Library page.
function Seed-Library([string]$root) {
    $steam = Join-Path $root '.steam/steam'
    $secs = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $games = @(
        # Real store ids, so the covers are each game's real art from Steam's
        # CDN. Rift Circuit has no store page and keeps the painted cover.
        @(1001, 'Rift Circuit', 5160, 1), @(1086940, "Baldur's Gate 3", 1800, 3), @(1245620, 'ELDEN RING', 900, 6),
        @(730, 'Counter-Strike 2', 600, 9), @(1091500, 'Cyberpunk 2077', 300, 14), @(413150, 'Stardew Valley', 120, 20),
        @(367520, 'Hollow Knight', 0, 0), @(1145360, 'Hades', 0, 0), @(292030, 'The Witcher 3: Wild Hunt', 45, 40), @(105600, 'Terraria', 0, 0)
    )
    $apps = ''
    foreach ($g in $games) {
        $last = if ($g[3] -gt 0) { $secs - $g[3] * 86400 } else { 0 }
        Write-Utf8 (Join-Path $steam "steamapps/appmanifest_$($g[0]).acf") "`"AppState`"`n{`n`"appid`" `"$($g[0])`"`n`"name`" `"$($g[1])`"`n`"LastPlayed`" `"$last`"`n}`n"
        $apps += "`"$($g[0])`"`n{`n`"LastPlayed`" `"$last`"`n`"Playtime`" `"$($g[2])`"`n}`n"
    }
    # Steam keeps each game's art in appcache/librarycache, and Library reads
    # that first. Fill it from Steam's CDN the way the client does, so the shot
    # shows real covers even when the CDN is slow for the app on the runner.
    # A cover that fails here is left to the app's own CDN fetch.
    $cache = Join-Path $steam 'appcache/librarycache'
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    foreach ($g in $games) {
        if ($g[0] -eq 1001) { continue }
        foreach ($file in @('library_600x900.jpg', 'header.jpg')) {
            $out = Join-Path $cache "$($g[0])_$file"
            $url = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/$($g[0])/$file"
            $ok = $false
            for ($try = 0; $try -lt 3 -and -not $ok; $try++) {
                try { Invoke-WebRequest -Uri $url -OutFile $out -TimeoutSec 20 -UseBasicParsing; $ok = $true } catch { Start-Sleep -Seconds 2 }
            }
            Write-Host "cover $($g[0]) ${file}: $(if ($ok) { (Get-Item $out).Length } else { 'failed' })"
        }
    }
    Write-Utf8 (Join-Path $steam 'userdata/1/config/localconfig.vdf') "`"UserLocalConfigStore`"`n{`n`"Software`"`n{`n`"Valve`"`n{`n`"Steam`"`n{`n`"apps`"`n{`n$apps}`n}`n}`n}`n}`n"
}

function Seed-Pulse([string]$cfg) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $ideas = @(
        @('gpu', 'I can update your GPU driver tonight after you log off', 'A new driver fixes the stutter you hit in two of your games.', 'try'),
        @('aim', 'I can set up a 20-minute aim routine before ranked', 'You win more first matches after a warm-up.', 'automation'),
        @('bill', 'I can remind you about the electric bill on Friday', 'It is due Friday. Nothing goes out until you say so.', 'reminder'),
        @('cache', 'I can clear 38 GB of old shader caches and installers', 'Your game drive is 91% full.', 'try')
    )
    $cards = @()
    for ($i = 0; $i -lt $ideas.Count; $i++) {
        $at = $now - ($i + 1) * 600000
        $cards += @{ id = "idea-$($ideas[$i][0])-$at"; kind = 'idea'; title = $ideas[$i][1]; body = $ideas[$i][2];
                     createdAt = $at; status = 'unread'; action = @{ type = 'open_workboard' };
                     prompt = $ideas[$i][1]; ideaKind = $ideas[$i][3]; sourceId = $ideas[$i][0] }
    }
    $feed = @(
        @('gpu-news', 'New GPU driver fixes stutter in three big shooters', 'Shader compile hitches are gone in the games you play most.', 2520000),
        @('patch', 'Season patch: your main got a buff', 'Faster reload and a smaller recoil kick.', 10800000),
        @('quiet', 'While you were in quiet hours', 'Driver check ran, backup done, prices checked.', 21600000)
    )
    foreach ($f in $feed) {
        $at = $now - $f[3]
        $cards += @{ id = "done-$($f[0])"; kind = 'automation_done'; title = $f[1]; body = $f[2];
                     createdAt = $at; status = 'unread'; action = @{ type = 'open_automations' }; sourceId = $f[0] }
    }
    Write-Utf8 (Join-Path $cfg 'updates.json') (ConvertTo-Json -InputObject $cards -Depth 4)
}

# Release radar: on, never checked, and a release-day reminder that came due
# an hour ago, so launch posts "Out today" through the real reminder path and
# the live store check runs for the seeded library.
function Seed-Radar([string]$cfg) {
    $due = [DateTimeOffset]::UtcNow.AddHours(-1).ToUnixTimeMilliseconds()
    $radar = @{ on = $true; lastCheckMs = 0; posted = @();
                reminders = @{ '1030300' = @{ name = 'Hollow Knight: Silksong'; dateMs = $due } } }
    Write-Utf8 (Join-Path $cfg 'release-radar.json') ($radar | ConvertTo-Json -Depth 4)
}

# 3D model drop: a 60 x 20 x 40 mm box as ASCII STL, saved into Downloads
# after launch, since the first scan only marks what is already there.
function Write-ModelDrop([string]$root) {
    $v = @(@(0,0,0), @(60,0,0), @(60,20,0), @(0,20,0), @(0,0,40), @(60,0,40), @(60,20,40), @(0,20,40))
    $faces = @(@(0,2,1), @(0,3,2), @(4,5,6), @(4,6,7), @(0,1,5), @(0,5,4), @(2,3,7), @(2,7,6), @(1,2,6), @(1,6,5), @(0,4,7), @(0,7,3))
    $lines = @('solid bracket')
    foreach ($f in $faces) {
        $lines += '  facet normal 0 0 0', '    outer loop'
        foreach ($i in $f) { $lines += "      vertex $($v[$i][0]) $($v[$i][1]) $($v[$i][2])" }
        $lines += '    endloop', '  endfacet'
    }
    $lines += 'endsolid bracket'
    Write-Utf8 (Join-Path $root 'Downloads\bracket.stl') ($lines -join "`n")
}

# One launch at $w x $h. $theme is what app.json holds before launch; with
# $upgrade the file has no themeMigrated, as a pre-3.4 install leaves it.
# With $keys, those keystrokes go to the window once it has drawn (for
# example '^,' opens Settings) and the shot waits for the new screen. With
# $dock the config is seeded first (Seed-Dock) and the shot waits for the
# hardware read behind PC loadout; $wheel scrolls the dock down that many
# notches first, for the cards below the fold of a short window. With $pulse
# the config holds Pulse cards and the shot clicks Pulse in the rail; each
# x, y pair in $clickAt (in the window) is one more click after that. $library seeds the dock
# and a Steam library; the shot clicks Library on the rail with $clickAt.
# $personality is the stored pick in app.json ('default' is the preset older
# builds shipped). $pinPersonality opens Settings → Personality
# (GROKHUB_DECK_SETTINGS_SEC) with adults-only on and an explicit preset
# selected, for slide 19 shots. $mood pins Nyx's corner box picture
# (GROKHUB_DECK_MOOD: working, gaming, doctor, reading, hanging-out, viper),
# since no turn or game runs here; every shot defaults to hanging-out.
# $pinBehavior opens Settings →
# Behavior for the Screenshot explain toggle. $screenshotOffer seeds a live
# Explain-this chip (path+mtime ledger only; no image bytes). $openSettings
# opens Settings on that section at launch; $libraryPick selects a Steam app.
# $nav opens that rail page at launch (GROKHUB_DECK_NAV, any rail id such as
# home, pulse or library) instead of a rail click; the cabin closes the
# seeded Activity list over every pinned page.
# $localModels turns the local model suggestions on and runs the weekly check
# on a fixed 24 GB GPU and a canned reply (GROKHUB_DECK_LOCAL_MODELS).
function Shot([int]$w, [int]$h, [string]$name, [string]$theme, [string]$deckSlide, [switch]$upgrade, [string]$keys = '', [switch]$dock, [int]$wheel = 0, [switch]$pulse, [int[]]$clickAt = @(), [string]$mood = '', [switch]$library, [switch]$chatFirst, [string]$personality = '', [switch]$pinPersonality, [switch]$pinBehavior, [switch]$screenshotOffer, [string]$openSettings = '', [int]$tabs = 0, [switch]$overflow, [string]$lastChat = '', [string]$libraryPick = '', [switch]$gameLaunch, [switch]$localModels, [switch]$radar, [switch]$modelDrop, [string]$nav = '') {
    $root = Join-Path $env:RUNNER_TEMP "deck-$name"
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    $cfg = Join-Path $root 'cfg'
    New-Item -ItemType Directory -Force -Path $cfg | Out-Null
    $app = @{ getStartedDone = $true; setupDone = $true; theme = $theme; deviceName = 'my-pc';
              window = @{ x = 0; y = 0; w = $w; h = $h; maximized = $false } }
    if (-not $upgrade) { $app.themeMigrated = $true }
    if ($chatFirst) { $app.chatObjectives = $true }
    if ($personality) { $app.personality = $personality }
    if ($pinPersonality) {
        if (-not $personality) { $app.personality = 'viper' }
        $app.explicit_personalities = $true
    }
    if ($library) {
        Seed-Dock $cfg $app 'Rift Circuit: Diamond by Sunday' $false
        Seed-Library $root
    } elseif ($dock -or $screenshotOffer) { Seed-Dock $cfg $app 'Diamond, 3 wins left' (-not $chatFirst) }
    if ($pulse) { Seed-Pulse $cfg }
    if ($screenshotOffer -or $pinBehavior) {
        $app.screenshotExplain = @{ enabled = $true }
    }
    if ($radar) {
        Seed-Radar $cfg
        # The launch source check (and so the reminder) skips quiet hours.
        $app.quietStart = '00:00'; $app.quietEnd = '00:00'
    }
    if ($gameLaunch) {
        # Game launch companion: a real Steam game last played 30 days ago, so
        # the card has its news since then; Steam's RunningAppID marks the launch.
        $app.gameCompanion = $true
        # A game already running at start is not a launch: clear it first.
        if (Test-Path 'HKCU:\Software\Valve\Steam') { Set-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name 'RunningAppID' -Value 0 -Type DWord }
        $last = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - 30 * 86400
        Write-Utf8 (Join-Path $root '.steam/steam/steamapps/appmanifest_1086940.acf') "`"AppState`"`n{`n`"appid`" `"1086940`"`n`"name`" `"Baldur's Gate 3`"`n`"LastPlayed`" `"$last`"`n}`n"
    }
    if ($modelDrop) {
        $app.modelDrop = $true
        New-Item -ItemType Directory -Force -Path (Join-Path $root 'Downloads') | Out-Null
    }
    if (-not $app.personality) { $app.personality = 'viper' }
    # Viper is adults-only; without this the titlebar falls back to Nyx.
    if ($app.personality -eq 'viper') { $app.explicitPersonalities = $true }
    Write-Utf8 (Join-Path $cfg 'app.json') ($app | ConvertTo-Json -Depth 4)
    if ($screenshotOffer) {
        $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $ledger = @{
            lastSeenModifiedMs = $now
            lastTickMs = $now
            pending = @{
                path = 'C:\Users\user\Pictures\Screenshots\ExplainMe.png'
                offeredAtMs = $now
            }
        }
        Write-Utf8 (Join-Path $cfg 'screenshot-offer.json') ($ledger | ConvertTo-Json -Depth 4 -Compress)
    }
    $env:GROKHUB_CONFIG = $cfg
    $env:HOME = $root
    $env:USERPROFILE = $root
    $env:APPDATA = Join-Path $root 'AppData'
    $env:LOCALAPPDATA = Join-Path $root 'LocalAppData'
    $env:GROKHUB_RENDERER = 'wgpu'
    $env:GROKHUB_DECK_SHOT = '1'
    if ($tabs -gt 0) { $env:GROKHUB_DECK_TABS = "$tabs" } else { Remove-Item Env:GROKHUB_DECK_TABS -ErrorAction SilentlyContinue }
    if ($overflow) { $env:GROKHUB_DECK_OVERFLOW = '1' } else { Remove-Item Env:GROKHUB_DECK_OVERFLOW -ErrorAction SilentlyContinue }
    if ($lastChat) { $env:GROKHUB_DECK_LAST_CHAT = $lastChat } else { Remove-Item Env:GROKHUB_DECK_LAST_CHAT -ErrorAction SilentlyContinue }
    if ($nav) { $env:GROKHUB_DECK_NAV = $nav } else { Remove-Item Env:GROKHUB_DECK_NAV -ErrorAction SilentlyContinue }
    if ($localModels) {
        Write-Utf8 (Join-Path $cfg 'pulse-local-models.json') '{"on":true,"lastCheckMs":0,"seeded":true}'
        $env:GROKHUB_DECK_LOCAL_MODELS = '1'
    } else { Remove-Item Env:GROKHUB_DECK_LOCAL_MODELS -ErrorAction SilentlyContinue }
    if ($libraryPick) { $env:GROKHUB_DECK_LIBRARY_PICK = $libraryPick } else { Remove-Item Env:GROKHUB_DECK_LIBRARY_PICK -ErrorAction SilentlyContinue }
    if (-not $mood) { $mood = 'hanging-out' }
    $env:GROKHUB_DECK_MOOD = $mood
    if ($pinPersonality) {
        $env:GROKHUB_DECK_SETTINGS_SEC = 'personality'
    } elseif ($pinBehavior) {
        $env:GROKHUB_DECK_SETTINGS_SEC = 'behavior'
    } elseif ($openSettings) {
        $env:GROKHUB_DECK_SETTINGS_SEC = $openSettings
    } else {
        Remove-Item Env:GROKHUB_DECK_SETTINGS_SEC -ErrorAction SilentlyContinue
    }
    # $openSettings / $pinPersonality / $pinBehavior open Settings at launch, with no click.
    $env:GROKHUB_DECK_OPEN_SETTINGS = $(if ($openSettings -or $pinPersonality -or $pinBehavior) { '1' } else { '' })
    # The Activity popover would cover the Library tiles and the Settings page.
    # Home is never seeded. Other pinned $nav pages keep the seeded list (its
    # titlebar count) and the cabin closes the popover over them.
    $env:GROKHUB_DECK_ACTIVITY = $(if ($library -or $openSettings -or $pinPersonality -or $pinBehavior -or $nav -eq 'home') { '0' } else { '' })
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
        Get-Content (Join-Path $root 'err.txt') -ErrorAction SilentlyContinue | Select-Object -Last 20
        throw "no $($brand.productName) window at ${w}x${h}"
    }
    [void][DeckWin]::SetForegroundWindow($hwnd)
    Start-Sleep -Seconds $(if ($dock -or $library) { 40 } else { 12 })
    if ($gameLaunch) {
        if (-not (Test-Path 'HKCU:\Software\Valve\Steam')) { New-Item -Path 'HKCU:\Software\Valve\Steam' -Force | Out-Null }
        Set-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name 'RunningAppID' -Value 1086940 -Type DWord
        # Sampled every 5 s; the news comes back from Steam on a worker.
        Start-Sleep -Seconds 20
    }
    if ($modelDrop) {
        # Downloads is listed every 20 s; the next scan finds the new file.
        Write-ModelDrop $root
        Start-Sleep -Seconds 28
    }
    if ($keys) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [System.Windows.Forms.SendKeys]::SendWait($keys)
        Start-Sleep -Seconds 3
    }
    $r = New-Object DeckWin+RECT
    [void][DeckWin]::GetWindowRect($hwnd, [ref]$r)
    if ($pulse -and -not $nav) {
        # Pulse is the fourth icon on the 40 px rail, at every window size. The chat
        # tab strip under the titlebar pushes the rail down 37 px.
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [void][DeckWin]::SetCursorPos($r.L + 20, $r.T + 218)
        [DeckWin]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [DeckWin]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        # Let the click land before the cursor moves: a queued click that lands
        # after the move hits the parked spot. Park on the rail's empty stretch,
        # never the titlebar, whose Quiet chip opens Settings.
        Start-Sleep -Milliseconds 400
        [void][DeckWin]::SetCursorPos($r.L + 20, $r.T + 420)
        Start-Sleep -Seconds 4
    }
    # Each x, y pair in $clickAt is one click, in order.
    for ($i = 0; $i + 1 -lt $clickAt.Count; $i += 2) {
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [void][DeckWin]::SetCursorPos($r.L + $clickAt[$i], $r.T + $clickAt[$i + 1])
        [DeckWin]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [DeckWin]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        # Let the click land before the cursor moves: a queued click that lands
        # after the move hits the parked spot. Park on the rail's empty stretch,
        # never the titlebar, whose Quiet chip opens Settings.
        Start-Sleep -Milliseconds 400
        [void][DeckWin]::SetCursorPos($r.L + 20, $r.T + 420)
        Start-Sleep -Seconds 3
    }
    if ($wheel -gt 0) {
        # The dock is the right-hand column; 0x0800 is MOUSEEVENTF_WHEEL.
        [void][DeckWin]::SetCursorPos($r.R - 120, $r.T + [int](($r.B - $r.T) / 2))
        for ($n = 0; $n -lt $wheel; $n++) {
            [DeckWin]::mouse_event(0x0800, 0, 0, -120, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 150
        }
        Start-Sleep -Seconds 2
    }
    $cw = $r.R - $r.L; $ch = $r.B - $r.T
    Write-Host "window ${w}x${h}: rect $($r.L),$($r.T) ${cw}x${ch}"
    $bmp = New-Object System.Drawing.Bitmap $cw, $ch
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    if ($r.R -le $screenW -and $r.B -le $screenH) {
        $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    } else {
        # Wider than the screen: ask the window to render itself
        # (PW_RENDERFULLCONTENT).
        $dc = $g.GetHdc()
        Write-Host "PrintWindow ${name}: $([DeckWin]::PrintWindow($hwnd, $dc, 2))"
        $g.ReleaseHdc($dc)
    }
    $g.Dispose()
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $saved = Get-Content (Join-Path $cfg 'app.json') -Raw | ConvertFrom-Json
    Write-Host "app.json after ${name}: theme=$($saved.theme) themeMigrated=$($saved.themeMigrated)"
    $png = Join-Path $Out "app-$name.png"
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    $jpg = Join-Path $Out "app-$name.jpg"
    Save-Jpeg $bmp $jpg
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($jpg))
    Write-Host "DECK-SHOT-BEGIN app-$name.jpg"
    for ($k = 0; $k -lt $b64.Length; $k += 4000) {
        Write-Host $b64.Substring($k, [Math]::Min(4000, $b64.Length - $k))
    }
    Write-Host "DECK-SHOT-END app-$name.jpg"
    # Not $slide: PowerShell names are case-insensitive, so it would be $Slide.
    $slidePath = Join-Path $repo "docs/design/loadout-baseline-v1/slide-$deckSlide.png"
    if (Test-Path $slidePath) {
        $deckImg = [System.Drawing.Image]::FromFile($slidePath)
        $sw = [int]($deckImg.Width * $ch / $deckImg.Height)
        $pair = New-Object System.Drawing.Bitmap ($cw + 16 + $sw), $ch
        $pg = [System.Drawing.Graphics]::FromImage($pair)
        $pg.Clear([System.Drawing.Color]::FromArgb(11, 11, 13))
        $pg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $pg.DrawImage($bmp, 0, 0, $cw, $ch)
        $pg.DrawImage($deckImg, $cw + 16, 0, $sw, $ch)
        $pg.Dispose()
        $pair.Save((Join-Path $Out "pair-$name-slide$deckSlide.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $deckImg.Dispose(); $pair.Dispose()
    }
    $bmp.Dispose()
}

# The shot list is its own file so changes that add shots don't conflict.
. (Join-Path $PSScriptRoot 'deck-shots.list.ps1')
Get-ChildItem $Out | Format-Table Name, Length
