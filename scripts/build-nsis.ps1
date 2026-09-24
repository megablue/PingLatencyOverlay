param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64"
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$project = Join-Path $root "src-tauri"
$exe = Join-Path $project "target\release\ping-latency-overlay.exe"
$icon = Join-Path $project "icons\icon.ico"
$outDir = Join-Path $project "target\release\bundle\nsis"
$outFile = Join-Path $outDir "PingLatencyOverlay_0.1.0_${Arch}-setup.exe"
$script = Join-Path $root "packaging\nsis\installer.nsi"

if (-not (Test-Path $exe)) {
    throw "Release executable not found: $exe (run cargo build --release first)"
}
$makensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
if (-not $makensis) {
    $tauriNsis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"
    if (Test-Path $tauriNsis) {
        $makensis = Get-Item $tauriNsis
    }
}
if (-not $makensis) {
    throw "makensis.exe was not found. Install NSIS 3.x and add its bin directory to PATH."
}
$makensisPath = if ($makensis.Source) { $makensis.Source } else { $makensis.FullName }

New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$arguments = @(
    "/DAPP_EXE=$exe",
    "/DAPP_ICON=$icon",
    "/DOUT_FILE=$outFile",
    $script
)
& $makensisPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "NSIS failed with exit code $LASTEXITCODE"
}
Write-Output "Created $outFile"
