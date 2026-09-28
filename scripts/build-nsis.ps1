param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64",
    # Print the version this script would name the installer and stop. The
    # test suite uses it to check that this script and `build.rs` agree, since
    # the app reporting one version while its installer is named another is
    # exactly the drift nobody would otherwise notice.
    [switch]$VersionOnly
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$project = Join-Path $root "src-tauri"
$exe = Join-Path $project "target\release\ping-latency-overlay.exe"
# The renderer is a sibling executable: the shell starts it and talks to it over a
# named pipe, and it finds it by looking next to its own binary. The installer
# must therefore place the two together, so both paths are resolved here and
# both are checked before packaging rather than after.
$renderer = Join-Path $project "target\release\ping-latency-overlay-renderer.exe"
$icon = Join-Path $project "icons\icon.ico"
$license = Join-Path $root "LICENSE"
$outDir = Join-Path $project "target\release\bundle\nsis"
$script = Join-Path $root "packaging\nsis\installer.nsi"

if (-not (Test-Path $exe)) {
    throw "Release executable not found: $exe (run cargo build --release first)"
}
if (-not (Test-Path $renderer)) {
    throw "Renderer executable not found: $renderer. The shell cannot draw anything without it, and it is found by looking next to the shell's own binary."
}
if (-not (Test-Path $license)) {
    throw "License file not found: $license"
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
    # The patch is the number of commits since `countBase`, not the raw commit
    # count, so a new minor restarts at .1. Read from the same Cargo.toml line
    # `build.rs` reads so the installer name and the version the app reports
    # come out of one fact rather than two implementations agreeing by luck.
    $baseMatch = [regex]::Match($cargoToml, '(?m)^\s*countBase\s*=\s*(\d+)')
    if (-not $baseMatch.Success) {
        throw "Could not read countBase from src-tauri/Cargo.toml"
    }
    $countBase = [uint32]$baseMatch.Groups[1].Value
    if ($commitCount -le $countBase) {
        # No Git history, a shallow clone, or the commit that moved the minor:
        # the plain base version is the honest answer, and it is what
        # build.rs falls back to as well.
        $version = $baseVersion
    } else {
        $version = "$($baseParts[0]).$($baseParts[1]).$($commitCount - $countBase)"
    }
}

if ($VersionOnly) {
    Write-Output $version
    exit 0
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
    "/DAPP_RENDERER_EXE=$renderer",
    "/DAPP_ICON=$icon",
    "/DAPP_LICENSE=$license",
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
