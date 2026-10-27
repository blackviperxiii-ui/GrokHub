# Screenshot the Windows cabin (grokhub.exe) at the deck-match sizes and put
# each shot beside a deck slide from docs/design/loadout-baseline-v1/.
# Runs in CI after the release build. Uses a throwaway config and home, never
# the runner user's real profile. Output: target/deck-shots/*.png, plus each
# app shot as base64 JPEG between markers in the log so agents without
# artifact access can read it.
param(
    [string]$Exe = 'target/release/grokhub.exe',
    [string]$Slide = '02',
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
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[void][DeckWin]::SetProcessDPIAware()
try { Set-DisplayResolution -Width 1920 -Height 1080 -Force } catch { Write-Host "display: $_" }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$repo = (Resolve-Path .).Path
$exe = (Resolve-Path $Exe).Path
$brand = Get-Content (Join-Path $repo 'brand/brand.json') -Raw | ConvertFrom-Json
$slidePath = Join-Path $repo "docs/design/loadout-baseline-v1/slide-$Slide.png"

function Save-Jpeg($bmp, $path) {
    $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
    $params = New-Object System.Drawing.Imaging.EncoderParameters 1
    $params.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 85L
    $bmp.Save($path, $codec, $params)
}

function Shot([int]$w, [int]$h) {
    $root = Join-Path $env:RUNNER_TEMP "deck-$w"
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    $cfg = Join-Path $root 'cfg'
    New-Item -ItemType Directory -Force -Path $cfg | Out-Null
    $app = @{ getStartedDone = $true; setupDone = $true; theme = 'ops-dark'; deviceName = 'my-pc';
              window = @{ x = 0; y = 0; w = $w; h = $h; maximized = $false } }
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
    Start-Sleep -Seconds 12
    $r = New-Object DeckWin+RECT
    [void][DeckWin]::GetWindowRect($hwnd, [ref]$r)
    $cw = $r.R - $r.L; $ch = $r.B - $r.T
    Write-Host "window ${w}x${h}: rect $($r.L),$($r.T) ${cw}x${ch}"
    $bmp = New-Object System.Drawing.Bitmap $cw, $ch
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    $g.Dispose()
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $png = Join-Path $Out "app-${w}x${h}.png"
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    $jpg = Join-Path $Out "app-${w}x${h}.jpg"
    Save-Jpeg $bmp $jpg
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($jpg))
    Write-Host "DECK-SHOT-BEGIN app-${w}x${h}.jpg"
    for ($k = 0; $k -lt $b64.Length; $k += 4000) {
        Write-Host $b64.Substring($k, [Math]::Min(4000, $b64.Length - $k))
    }
    Write-Host "DECK-SHOT-END app-${w}x${h}.jpg"
    if (Test-Path $slidePath) {
        $slideImg = [System.Drawing.Image]::FromFile($slidePath)
        $sw = [int]($slideImg.Width * $ch / $slideImg.Height)
        $pair = New-Object System.Drawing.Bitmap ($cw + 16 + $sw), $ch
        $pg = [System.Drawing.Graphics]::FromImage($pair)
        $pg.Clear([System.Drawing.Color]::FromArgb(11, 11, 13))
        $pg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $pg.DrawImage($bmp, 0, 0, $cw, $ch)
        $pg.DrawImage($slideImg, $cw + 16, 0, $sw, $ch)
        $pg.Dispose()
        $pair.Save((Join-Path $Out "pair-${w}x${h}-slide$Slide.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $slideImg.Dispose(); $pair.Dispose()
    }
    $bmp.Dispose()
}

Shot 1600 900
Shot 1100 720
Get-ChildItem $Out | Format-Table Name, Length
