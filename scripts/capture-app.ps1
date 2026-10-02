# Captures the Config window and every overlay as PNGs, so someone who cannot
# look at the screen (a person in a terminal, or an agent) can still see what the
# app actually drew.
#
# It runs a full session itself: stops whatever is running, starts plo-config
# (which brings up the tray and the renderer), waits for the overlays to have
# something on them, and photographs each window.
#
# Config writes during a capture land in a sandbox copy of the live config
# directory unless -Live is passed, so a session can never edit the real one.
# The sandbox is chosen through PLO_CONFIG_DIR, which every one of the three
# processes honours (and which also moves the log, so a session's log does not
# mix with the real one).
#
# Usage:
#   scripts/capture-app.ps1                          # sandbox a copy of the live config
#   scripts/capture-app.ps1 -SettleSeconds 15        # let the graphs fill in more
#   scripts/capture-app.ps1 -KeepRunning             # leave the session up
#   scripts/capture-app.ps1 -Live                    # use the real config directory
#   scripts/capture-app.ps1 -DebugBuild              # use target\debug instead of release
#   scripts/capture-app.ps1 -OutDir C:\somewhere     # where the PNGs go
#
# Output: <outdir>\config.png, one overlay-<nn>-<w>x<h>.png per visible overlay,
# a full-screen shot with -FullScreen, and manifest.txt naming every window.

[CmdletBinding()]
param(
    [string]$OutDir,
    [string]$ConfigDir,
    [switch]$Live,
    [switch]$KeepRunning,
    [double]$SettleSeconds = 8,
    [switch]$DebugBuild,
    [switch]$FullScreen
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$ExeNames = @('plo-tray', 'plo-config', 'plo-renderer')
$RepoRoot = Split-Path -Parent $PSScriptRoot
$TargetDir = Join-Path $RepoRoot 'src-tauri\target'
$ExeDir = Join-Path $TargetDir $(if ($DebugBuild) { 'debug' } else { 'release' })
$Exes = @{}
foreach ($name in $ExeNames) {
    $exe = Join-Path $ExeDir "$name.exe"
    if (-not (Test-Path $exe)) {
        throw "Missing $exe. Build it first: cargo build $(if ($DebugBuild) { '' } else { '--release' }) (from src-tauri)."
    }
    $Exes[$name] = $exe
}

if (-not $OutDir) {
    $OutDir = Join-Path $env:TEMP ('plo-capture-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

function Stop-PloProcesses {
    foreach ($name in $ExeNames) {
        Get-Process -Name $name -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
    }
}

# --- pick the config directory this session runs against -------------------
$RealConfigDir = Join-Path $env:USERPROFILE '.config\.PingLatencyOverlay'
if ($Live) {
    $SessionConfigDir = $RealConfigDir
} elseif ($ConfigDir) {
    $SessionConfigDir = $ConfigDir
} else {
    $SessionConfigDir = Join-Path $env:TEMP ('plo-sandbox-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
if (-not $Live) {
    New-Item -ItemType Directory -Force -Path $SessionConfigDir | Out-Null
    if (Test-Path $RealConfigDir) {
        Copy-Item -Path (Join-Path $RealConfigDir '*') -Destination $SessionConfigDir -Recurse -Force
    }
}
$previousOverride = $env:PLO_CONFIG_DIR
$env:PLO_CONFIG_DIR = $SessionConfigDir

# The app is single-instance per role, so a session of our own needs the floor
# clear; the sandbox also must not have a stray process answering on the real one.
Stop-PloProcesses
Start-Sleep -Milliseconds 400

# --- P/Invoke ---------------------------------------------------------------
if (-not ('PloCaptureNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class PloCaptureNative
{
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr param);

    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr param);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, int rop);

    public const int SRCCOPY_CAPTUREBLT = unchecked((int)0x40CC0020);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
}

# Per-monitor aware v2, so every rect and screen copy below is in physical
# pixels even on a scaled display. Failure is tolerated: the session still runs,
# the rects are just in the caller's DPI virtualisation.
[PloCaptureNative]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null

function Get-WindowTitle([IntPtr]$Hwnd) {
    $buffer = New-Object System.Text.StringBuilder 256
    [PloCaptureNative]::GetWindowTextW($Hwnd, $buffer, $buffer.Capacity) | Out-Null
    return $buffer.ToString()
}

function Get-VisibleWindows([int[]]$Pids) {
    $found = New-Object System.Collections.Generic.List[object]
    $callback = [PloCaptureNative+EnumWindowsProc] {
        param([IntPtr]$hwnd, [IntPtr]$param)
        [uint32]$windowPid = 0
        [PloCaptureNative]::GetWindowThreadProcessId($hwnd, [ref]$windowPid) | Out-Null
        if (($Pids -contains [int]$windowPid) -and [PloCaptureNative]::IsWindowVisible($hwnd)) {
            $rect = New-Object PloCaptureNative+RECT
            [PloCaptureNative]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
            if (($rect.Right -gt $rect.Left) -and ($rect.Bottom -gt $rect.Top)) {
                $found.Add([pscustomobject]@{
                        Hwnd   = $hwnd
                        Pid    = [int]$windowPid
                        Title  = (Get-WindowTitle $hwnd)
                        Left   = $rect.Left
                        Top    = $rect.Top
                        Right  = $rect.Right
                        Bottom = $rect.Bottom
                        Area   = ($rect.Right - $rect.Left) * ($rect.Bottom - $rect.Top)
                    })
            }
        }
        return $true
    }
    [PloCaptureNative]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return $found
}

# A winit window has helpers beside the real one (one of them is an 18x18 that
# counts as visible), so the real window is the largest whose title matches —
# and, failing a match, simply the largest.
function Select-MainWindow($Windows) {
    if (-not $Windows) { return $null }
    $titled = @($Windows | Where-Object { $_.Title -like '*PingLatencyOverlay*' })
    if ($titled) { return ($titled | Sort-Object Area -Descending | Select-Object -First 1) }
    return ($Windows | Sort-Object Area -Descending | Select-Object -First 1)
}

function Test-BitmapBlank([System.Drawing.Bitmap]$Bitmap) {
    # Sampled on a grid: a window that rendered nothing reads as one flat colour,
    # which is the "PrintWindow returned a black square" failure this exists to
    # catch without comparing every pixel.
    $first = $Bitmap.GetPixel(0, 0)
    for ($x = 0; $x -lt $Bitmap.Width; $x += [Math]::Max(1, [int]($Bitmap.Width / 20))) {
        for ($y = 0; $y -lt $Bitmap.Height; $y += [Math]::Max(1, [int]($Bitmap.Height / 20))) {
            $pixel = $Bitmap.GetPixel($x, $y)
            if ($pixel.ToArgb() -ne $first.ToArgb()) { return $false }
        }
    }
    return $true
}

function Save-ScreenRect([int]$Left, [int]$Top, [int]$Right, [int]$Bottom, [string]$Path) {
    # Layered windows (the overlays) present nothing to PrintWindow, so they are
    # copied off the composited desktop with CAPTUREBLT. Note: NOT
    # Graphics.CopyFromScreen, which returns a black image in this environment.
    $width = $Right - $Left
    $height = $Bottom - $Top
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $destination = $graphics.GetHdc()
    $source = [PloCaptureNative]::GetDC([IntPtr]::Zero)
    [PloCaptureNative]::BitBlt($destination, 0, 0, $width, $height, $source, $Left, $Top,
        [PloCaptureNative]::SRCCOPY_CAPTUREBLT) | Out-Null
    [PloCaptureNative]::ReleaseDC([IntPtr]::Zero, $source) | Out-Null
    $graphics.ReleaseHdc($destination)
    $graphics.Dispose()
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

# PrintWindow renders the (GL) window itself, so occlusion by another window
# cannot end up in the picture. If it comes back blank, the screen copy is the
# fallback, because a black photograph is worse than an occluded one.
function Save-WindowCapture([IntPtr]$Hwnd, [string]$Path) {
    $rect = New-Object PloCaptureNative+RECT
    [PloCaptureNative]::GetWindowRect($Hwnd, [ref]$rect) | Out-Null
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $hdc = $graphics.GetHdc()
    $printed = [PloCaptureNative]::PrintWindow($Hwnd, $hdc, 2)   # PW_RENDERFULLCONTENT
    $graphics.ReleaseHdc($hdc)
    $graphics.Dispose()
    if ($printed -and -not (Test-BitmapBlank $bitmap)) {
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        return 'printwindow'
    }
    $bitmap.Dispose()
    Save-ScreenRect $rect.Left $rect.Top $rect.Right $rect.Bottom $Path
    return 'screen-fallback'
}

# --- run the session --------------------------------------------------------
# plo-config brings up the tray and the renderer; everything it starts inherits
# the sandbox override from this process's environment.
$config = Start-Process -FilePath $Exes['plo-config'] -PassThru

$deadline = (Get-Date).AddSeconds(20)
$configWindow = $null
while (-not $configWindow -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 250
    $configWindow = Select-MainWindow (Get-VisibleWindows @($config.Id))
}
if (-not $configWindow) {
    Stop-PloProcesses
    throw "The Config window of pid $($config.Id) never appeared."
}

# The overlays need a few samples (and their startup animation) before a
# photograph says anything worth reading.
if ($SettleSeconds -gt 0) { Start-Sleep -Seconds $SettleSeconds }
$configWindow = Select-MainWindow (Get-VisibleWindows @($config.Id))

$manifest = New-Object System.Collections.Generic.List[string]
$manifest.Add("captured  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')")
$head = & git -C $RepoRoot rev-parse --short HEAD 2>$null
if ($head) { $manifest.Add("head      $head") }
$manifest.Add("exedir    $ExeDir")
$manifest.Add("configdir $SessionConfigDir$(if ($Live) { ' (live)' } else { ' (sandbox)' })")
$manifest.Add("settle    ${SettleSeconds}s")
$manifest.Add("")

$configPath = Join-Path $OutDir 'config.png'
$method = Save-WindowCapture $configWindow.Hwnd $configPath
$manifest.Add(("config    hwnd=0x{0:X} pid={1} title='{2}' rect=({3},{4},{5},{6}) file=config.png method={7}" -f
        [int64]$configWindow.Hwnd, $configWindow.Pid, $configWindow.Title, $configWindow.Left,
        $configWindow.Top, $configWindow.Right, $configWindow.Bottom, $method))

# The overlays: one per visible window of the renderer, in a stable order.
$renderer = Get-Process -Name 'plo-renderer' -ErrorAction SilentlyContinue
if ($renderer) {
    $overlays = @(Get-VisibleWindows @($renderer.Id) | Sort-Object Top, Left)
    $index = 0
    foreach ($overlay in $overlays) {
        $index++
        $width = $overlay.Right - $overlay.Left
        $height = $overlay.Bottom - $overlay.Top
        $name = 'overlay-{0:00}-{1}x{2}.png' -f $index, $width, $height
        Save-ScreenRect $overlay.Left $overlay.Top $overlay.Right $overlay.Bottom (Join-Path $OutDir $name)
        $manifest.Add(("overlay   hwnd=0x{0:X} pid={1} rect=({2},{3},{4},{5}) file={6} method=screen" -f
                [int64]$overlay.Hwnd, $overlay.Pid, $overlay.Left, $overlay.Top,
                $overlay.Right, $overlay.Bottom, $name))
    }
    if ($index -eq 0) { $manifest.Add("overlay   (none visible)") }
} else {
    $manifest.Add("overlay   (no renderer process)")
}

if ($FullScreen) {
    Add-Type -AssemblyName System.Windows.Forms
    $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $name = 'screen.png'
    Save-ScreenRect $screen.Left $screen.Top $screen.Right $screen.Bottom (Join-Path $OutDir $name)
    $manifest.Add(("screen    rect=({0},{1},{2},{3}) file=screen.png method=screen" -f
            $screen.Left, $screen.Top, $screen.Right, $screen.Bottom))
}

$manifest | Set-Content -Path (Join-Path $OutDir 'manifest.txt') -Encoding UTF8

# --- clean up ---------------------------------------------------------------
if ($KeepRunning) {
    $shown = foreach ($name in $ExeNames) {
        $process = Get-Process -Name $name -ErrorAction SilentlyContinue
        if ($process) { "$name=$($process.Id -join ',')" }
    }
    Write-Host "Session left running: $($shown -join ' ')"
} else {
    Stop-PloProcesses
}
if ($null -eq $previousOverride) {
    Remove-Item Env:\PLO_CONFIG_DIR -ErrorAction SilentlyContinue
} else {
    $env:PLO_CONFIG_DIR = $previousOverride
}

Write-Host "Captured into $OutDir"
Get-ChildItem $OutDir | ForEach-Object { Write-Host "  $($_.Name)" }
