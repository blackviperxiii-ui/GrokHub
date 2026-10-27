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
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[void][DeckWin]::SetProcessDPIAware()
try { Set-DisplayResolution -Width 1920 -Height 1080 -Force } catch { Write-Host "display: $_" }

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
function Seed-Dock([string]$cfg, [hashtable]$app) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $id = 'deck-hangout'
    $steps = @('Diamond, 3 wins left', 'Aim routine 12/20', 'Warm up in the range', 'Review last match', 'Pick a duo for tonight')
    $todos = @{ todos = @(for ($i = 0; $i -lt $steps.Count; $i++) {
        @{ id = "$($i + 1)"; content = $steps[$i]; status = $(if ($i -eq 0) { 'in_progress' } else { 'pending' }) }
    }) } | ConvertTo-Json -Depth 4 -Compress
    $session = @(
        (@{ id = $id; title = 'Hangout'; created = $now; cwd = '.'; model = 'grok'; engine = 'native'; renamed = $false } | ConvertTo-Json -Compress),
        (@{ type = 'function_call'; call_id = 'c1'; name = 'todo_write'; arguments = $todos } | ConvertTo-Json -Compress)
    ) -join "`n"
    Write-Utf8 (Join-Path $cfg "sessions/$id.jsonl") "$session`n"
    $thread = @{ id = $id; title = 'Hangout'; native = $true; grokSession = $id; accessedMs = $now; lastActiveMs = $now; messages = @() }
    Write-Utf8 (Join-Path $cfg 'threads.json') (ConvertTo-Json -InputObject @($thread) -Depth 4)
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
}

# One launch at $w x $h. $theme is what app.json holds before launch; with
# $upgrade the file has no themeMigrated, as a pre-3.4 install leaves it.
# With $keys, those keystrokes go to the window once it has drawn (for
# example '^,' opens Settings) and the shot waits for the new screen. With
# $dock the config is seeded first (Seed-Dock) and the shot waits for the
# hardware read behind PC loadout; $wheel scrolls the dock down that many
# notches first, for the cards below the fold of a short window.
function Shot([int]$w, [int]$h, [string]$name, [string]$theme, [string]$deckSlide, [switch]$upgrade, [string]$keys = '', [switch]$dock, [int]$wheel = 0) {
    $root = Join-Path $env:RUNNER_TEMP "deck-$name"
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    $cfg = Join-Path $root 'cfg'
    New-Item -ItemType Directory -Force -Path $cfg | Out-Null
    $app = @{ getStartedDone = $true; setupDone = $true; theme = $theme; deviceName = 'my-pc';
              window = @{ x = 0; y = 0; w = $w; h = $h; maximized = $false } }
    if (-not $upgrade) { $app.themeMigrated = $true }
    if ($dock) { Seed-Dock $cfg $app }
    $app | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $cfg 'app.json')
    $env:GROKHUB_CONFIG = $cfg
    $env:HOME = $root
    $env:USERPROFILE = $root
    $env:APPDATA = Join-Path $root 'AppData'
    $env:LOCALAPPDATA = Join-Path $root 'LocalAppData'
    $env:GROKHUB_RENDERER = 'wgpu'
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
    Start-Sleep -Seconds $(if ($dock) { 40 } else { 12 })
    if ($keys) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][DeckWin]::SetForegroundWindow($hwnd)
        [System.Windows.Forms.SendKeys]::SendWait($keys)
        Start-Sleep -Seconds 3
    }
    $r = New-Object DeckWin+RECT
    [void][DeckWin]::GetWindowRect($hwnd, [ref]$r)
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
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
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
Shot 1600 900 'upgrade-1600x900' 'dark' $UpgradeSlide -upgrade
Shot 1600 900 'settings-1600x900' 'ops-dark' '07' -keys '^,'
Shot 1100 720 'settings-1100x720' 'ops-dark' '07' -keys '^,'
Shot 1600 900 'dock-1600x900' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720' 'ops-dark' '02' -dock
Shot 1100 720 'dock-1100x720-scrolled' 'ops-dark' '02' -dock -wheel 10
Get-ChildItem $Out | Format-Table Name, Length
