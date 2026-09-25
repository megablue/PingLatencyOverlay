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
$script = Join-Path $root "packaging\nsis\installer.nsi"

if (-not (Test-Path $exe)) {
    throw "Release executable not found: $exe (run cargo build --release first)"
}

$cargoToml = Get-Content (Join-Path $project "Cargo.toml") -Raw
$versionMatch = [regex]::Match(
    $cargoToml,
    '(?ms)^\[package\]\s*.*?^version\s*=\s*"([^"]+)"'
)
if (-not $versionMatch.Success) {
    throw "Could not read the package version from src-tauri/Cargo.toml"
}
$baseVersion = $versionMatch.Groups[1].Value
$version = $baseVersion

$commitCount = $null
try {
    $countOutput = & git -C $root rev-list --count HEAD 2>$null
    if ($LASTEXITCODE -eq 0 -and $null -ne $countOutput) {
        $countText = ($countOutput | Select-Object -First 1).ToString().Trim()
        $parsedCount = 0
        if ([uint32]::TryParse($countText, [ref]$parsedCount) -and $parsedCount -gt 0) {
            $commitCount = $parsedCount
        }
    }
} catch {
    $commitCount = $null
}

if ($null -ne $commitCount) {
    $baseParts = $baseVersion.Split(".")
    if ($baseParts.Count -lt 2) {
        throw "Package version must contain major and minor components: $baseVersion"
    }
    $version = "$($baseParts[0]).$($baseParts[1]).$commitCount"
}

$versionNumbers = @(
    $version.Split(".") | ForEach-Object { [int]$_ }
)
while ($versionNumbers.Count -lt 4) {
    $versionNumbers += 0
}
$versionWithBuild = ($versionNumbers[0..3] -join ".")

$outFile = Join-Path $outDir "PingLatencyOverlay_${version}_${Arch}-setup.exe"
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
    "/DAPP_VERSION=$version",
    "/DAPP_VERSIONWITHBUILD=$versionWithBuild",
    $script
)
& $makensisPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "NSIS failed with exit code $LASTEXITCODE"
}
Write-Output "Created $outFile"
