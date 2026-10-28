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
    $lines = for ($i = 0; $i -lt 23; $i++) {
        $at = $now - 172800000 - $i * 60000
        @{ task = "task $i"; signature = 'topic:deck'; result = 'success'; reasons = @('verify_ok', 'accepted'); finished_at = $at; at = $at } | ConvertTo-Json -Compress
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
        @(1001, 'Rift Circuit', 5160, 1), @(1006, 'Hollow Peak', 1800, 3), @(1011, 'Starfall Ops', 900, 6),
        @(1015, 'Iron Tide', 600, 9), @(1020, 'Glass Orbit', 300, 14), @(1025, 'Dustline', 120, 20),
        @(1030, 'Night Vault', 0, 0), @(1035, 'Echo Front', 0, 0), @(1036, 'Emberfall', 45, 40), @(1041, 'Null Sector', 0, 0)
    )
    $apps = ''
    foreach ($g in $games) {
        $last = if ($g[3] -gt 0) { $secs - $g[3] * 86400 } else { 0 }
        Write-Utf8 (Join-Path $steam "steamapps/appmanifest_$($g[0]).acf") "`"AppState`"`n{`n`"appid`" `"$($g[0])`"`n`"name`" `"$($g[1])`"`n`"LastPlayed`" `"$last`"`n}`n"
        $apps += "`"$($g[0])`"`n{`n`"LastPlayed`" `"$last`"`n`"Playtime`" `"$($g[2])`"`n}`n"
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

# Downloads tidy: an installer for an app with a Start Menu shortcut, a big
# old ISO (a sized empty file), an archive already extracted, and two copies
# of one PDF, all dated back so they count as old.
function Seed-Tidy([string]$root) {
    $dl = Join-Path $root 'Downloads'
    $menu = Join-Path $root 'AppData\Microsoft\Windows\Start Menu\Programs'
    New-Item -ItemType Directory -Force -Path $dl, $menu, (Join-Path $dl 'photos') | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $menu 'Discord.lnk'), [byte[]]@())
    $pdf = New-Object byte[] (3MB)
    (New-Object Random 7).NextBytes($pdf)
    $files = @(
        @('DiscordSetup.exe', 90MB, 20),
        @('ubuntu-24.04-desktop-amd64.iso', 600MB, 45),
        @('photos.zip', 40MB, 2),
        @('report.pdf', 0, 10),
        @('report (1).pdf', 0, 9),
        @('notes.txt', 1KB, 1)
    )
    foreach ($f in $files) {
        $path = Join-Path $dl $f[0]
        if ($f[0] -like 'report*') {
            [IO.File]::WriteAllBytes($path, $pdf)
        } else {
            $fs = [IO.File]::Create($path)
            $fs.SetLength($f[1])
            $fs.Close()
        }
        (Get-Item $path).LastWriteTime = (Get-Date).AddDays(-$f[2])
    }
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
# selected, for slide 19 shots. $mood pins the companion's pose
# (GROKHUB_DECK_MOOD), since no turn runs here.
# $nav opens that rail page at launch (GROKHUB_DECK_NAV) instead of the
# Pulse rail click, and closes the seeded Activity list over the page.
function Shot([int]$w, [int]$h, [string]$name, [string]$theme, [string]$deckSlide, [switch]$upgrade, [string]$keys = '', [switch]$dock, [int]$wheel = 0, [switch]$pulse, [int[]]$clickAt = @(), [string]$mood = '', [switch]$library, [switch]$chatFirst, [string]$personality = '', [switch]$pinPersonality, [int]$tabs = 0, [switch]$overflow, [string]$lastChat = '', [string]$tidy = '', [string]$nav = '') {
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
    } elseif ($dock) { Seed-Dock $cfg $app 'Diamond, 3 wins left' (-not $chatFirst) }
    if ($pulse) { Seed-Pulse $cfg }
    if ($tidy) {
        Seed-Tidy $root
        $app.downloadsTidy = $true
        $env:GROKHUB_DECK_TIDY = $tidy
    } else { Remove-Item Env:GROKHUB_DECK_TIDY -ErrorAction SilentlyContinue }
    if (-not $app.personality) { $app.personality = 'viper' }
    # Viper is adults-only; without this the titlebar falls back to Nyx.
    if ($app.personality -eq 'viper') { $app.explicitPersonalities = $true }
    Write-Utf8 (Join-Path $cfg 'app.json') ($app | ConvertTo-Json -Depth 4)
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
    if ($dock -and -not $mood) { $mood = 'hanging-out' }
    $env:GROKHUB_DECK_MOOD = $mood
    if ($pinPersonality) { $env:GROKHUB_DECK_SETTINGS_SEC = 'personality' } else { Remove-Item Env:GROKHUB_DECK_SETTINGS_SEC -ErrorAction SilentlyContinue }
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
    if ($keys) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [System.Windows.Forms.SendKeys]::SendWait($keys)
        Start-Sleep -Seconds 3
    }
    $r = New-Object DeckWin+RECT
    [void][DeckWin]::GetWindowRect($hwnd, [ref]$r)
    if ($pulse -and -not $nav) {
        # Pulse is the fourth icon on the 40 px rail, at every window size.
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [void][DeckWin]::SetCursorPos($r.L + 20, $r.T + 181)
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

Shot 1600 900 '1600x900' 'ops-dark' $Slide
Shot 1100 720 '1100x720' 'ops-dark' $Slide
Shot 1600 900 '1600x900-slide20' 'ops-dark' '20'
Shot 1100 720 '1100x720-slide20' 'ops-dark' '20'
Shot 1600 900 'upgrade-1600x900' 'dark' $UpgradeSlide -upgrade
Shot 1600 900 'settings-1600x900' 'ops-dark' '07' -keys '^,'
Shot 1100 720 'settings-1100x720' 'ops-dark' '07' -clickAt 20, 695
Shot 1600 900 'dock-1600x900' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720-scrolled' 'ops-dark' '02' -dock -wheel 10
# Chat tabs HARD DONE: 3 tabs (readable) and 18 tabs with overflow menu open.
Shot 1600 900 'chattabs-3-1600x900' 'ops-dark' '02' -dock -tabs 3
Shot 1100 720 'chattabs-3-1100x720' 'ops-dark' '02' -dock -tabs 3
Shot 1600 900 'chattabs-overflow-1600x900' 'ops-dark' '02' -dock -tabs 18 -overflow
Shot 1100 720 'chattabs-overflow-1100x720' 'ops-dark' '02' -dock -tabs 18 -overflow
# Chat rail last-used: Home (empty) then Chat restores Hangout.
Shot 1600 900 'lastchat-home-1600x900' 'ops-dark' '02' -dock -tabs 3 -lastChat 'home'
Shot 1100 720 'lastchat-home-1100x720' 'ops-dark' '02' -dock -tabs 3 -lastChat 'home'
Shot 1600 900 'lastchat-chat-1600x900' 'ops-dark' '02' -dock -tabs 3 -lastChat 'chat'
Shot 1100 720 'lastchat-chat-1100x720' 'ops-dark' '02' -dock -tabs 3 -lastChat 'chat'
Shot 1600 900 'pulse-1600x900' 'ops-dark' '14' -pulse
Shot 1100 720 'pulse-1100x720' 'ops-dark' '14' -pulse
# Deck-match 6 proof: same Pulse page beside slides 13 (loadout style) and 15
# (card close-up), at the HARD DONE sizes.
Shot 1600 900 'pulse-1600x900-slide13' 'ops-dark' '13' -pulse
Shot 1100 720 'pulse-1100x720-slide13' 'ops-dark' '13' -pulse
Shot 1600 900 'pulse-1600x900-slide15' 'ops-dark' '15' -pulse
Shot 1100 720 'pulse-1100x720-slide15' 'ops-dark' '15' -pulse
Shot 2560 1440 'pulse-2560x1440' 'ops-dark' '14' -pulse
Shot 1440 900 'pulse-1440x900' 'ops-dark' '14' -pulse
Shot 1024 720 'pulse-1024x720' 'ops-dark' '14' -pulse
Shot 1024 720 'pulse-1024x720-scrolled' 'ops-dark' '14' -pulse -wheel 10
Shot 720 720 'pulse-720x720' 'ops-dark' '14' -pulse
Shot 720 720 'pulse-720x720-ideas' 'ops-dark' '14' -pulse -clickAt 505, 81
# Library is the fifth icon on the 40 px rail.
Shot 1600 900 'library-1600x900' 'ops-dark' '11' -library -clickAt 20, 221
Shot 1100 720 'library-1100x720' 'ops-dark' '11' -library -clickAt 20, 221
# Downloads tidy: the suggestion card on Pulse, then the review with its approval.
Shot 1600 900 'tidy-1600x900' 'ops-dark' '14' -tidy 'card' -nav 'pulse'
Shot 1100 720 'tidy-1100x720' 'ops-dark' '14' -tidy 'card' -nav 'pulse'
Shot 1600 900 'tidy-approve-1600x900' 'ops-dark' '14' -tidy 'approve' -nav 'pulse'
Shot 1100 720 'tidy-approve-1100x720' 'ops-dark' '14' -tidy 'approve' -nav 'pulse'
# Layout B: the chat with objectives and recent chats left, snapshot right.
Shot 1600 900 'chat-objectives-1600x900' 'ops-dark' '10' -dock -chatFirst
Shot 1100 720 'chat-objectives-1100x720' 'ops-dark' '10' -dock -chatFirst
# Objectives (mission control) is the third icon on the rail.
Shot 1600 900 'objectives-1600x900' 'ops-dark' '09' -library -clickAt 20, 141
Shot 1100 720 'objectives-1100x720' 'ops-dark' '09' -library -clickAt 20, 141
# Privacy & data is the second entry in the Settings list; the dialog is centred.
# The gear at the foot of the rail opens Settings; Ctrl+, from SendKeys
# missed the 1100x720 window.
Shot 1600 900 'privacy-1600x900' 'ops-dark' '12' -clickAt 20, 875, 410, 246
Shot 1100 720 'privacy-1100x720' 'ops-dark' '12' -clickAt 20, 695, 160, 156
# Nyx is the default personality: Settings -> Personality on a profile with no
# pick, and on one still saved on the retired 'default' preset.
Shot 1600 900 'personality-fresh-1600x900' 'ops-dark' '07' -clickAt 20, 875, 410, 334
Shot 1100 720 'personality-fresh-1100x720' 'ops-dark' '07' -clickAt 20, 695, 160, 244
Shot 1600 900 'personality-upgrade-1600x900' 'ops-dark' '07' -personality 'default' -clickAt 20, 875, 410, 334
Shot 1100 720 'personality-upgrade-1100x720' 'ops-dark' '07' -personality 'default' -clickAt 20, 695, 160, 244
# Personality picker beside slide 19; section pinned via GROKHUB_DECK_SETTINGS_SEC.
Shot 1600 900 'personality-1600x900' 'ops-dark' '19' -pinPersonality -clickAt 20, 875
Shot 1100 720 'personality-1100x720' 'ops-dark' '19' -pinPersonality -clickAt 20, 695
foreach ($m in 'hello', 'helping', 'hanging-out') {
    Shot 1600 900 "mood-$m-1600x900" 'ops-dark' '17' -dock -mood $m
    Shot 1100 720 "mood-$m-1100x720" 'ops-dark' '17' -dock -mood $m
}
Get-ChildItem $Out | Format-Table Name, Length
