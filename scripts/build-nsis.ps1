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
# Three executables, all in the same directory, because each finds the others
# by looking beside its own binary. The tray is the one a user launches and the
# only one that gets a Start Menu shortcut; the window is spawned by the tray
# on demand, and the renderer by whichever of the two is asked to start first.
$tray = Join-Path $project "target\release\plo-tray.exe"
$config = Join-Path $project "target\release\plo-config.exe"
$renderer = Join-Path $project "target\release\plo-renderer.exe"
$icon = Join-Path $project "icons\icon.ico"
$license = Join-Path $root "LICENSE"
$outDir = Join-Path $project "target\release\bundle\nsis"
$script = Join-Path $root "packaging\nsis\installer.nsi"

# The `-VersionOnly` exit and the executable guards both live BELOW, after the
# version has actually been computed. An early exit would report an empty
# string, which is worse than no answer because a caller cannot tell it apart
# from a real version, and a guard before the exit would make the version
# unreadable exactly when someone is trying to find out what it is -- the test
# suite asks on a clean tree, before anything has been built.

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

# After the exit above, and for the reason given at the top: asking what this
# script would name the installer must not require that anything was built.
foreach ($required in @(
    @{ Path = $tray; What = "Tray executable. This is the app a user launches; it cannot start without it." },
    @{ Path = $config; What = "Config window executable. The tray spawns it on demand, and finds it next to itself." },
    @{ Path = $renderer; What = "Renderer executable. Nothing can draw an overlay without it, and it is found next to the others." }
)) {
    if (-not (Test-Path $required.Path)) {
        throw "$($required.What)`nNot found: $($required.Path)`n(run cargo build --release first)"
    }
}
if (-not (Test-Path $license)) {
    throw "License file not found: $license"
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

# The copyright notice in the installer's own version block. Read from the
# same manifest key `crates/build-support` reads for the exes, so the installer
# and everything it installs agree about whose work this is.
$copyrightMatch = [regex]::Match($cargoToml, '(?m)^\s*copyright\s*=\s*"([^"]+)"')
if (-not $copyrightMatch.Success) {
    throw "Could not read the copyright from src-tauri/Cargo.toml"
}
$copyright = $copyrightMatch.Groups[1].Value

$arguments = @(
    "/DAPP_TRAY_EXE=$tray",
    "/DAPP_CONFIG_EXE=$config",
    "/DAPP_RENDERER_EXE=$renderer",
    "/DAPP_ICON=$icon",
    "/DAPP_LICENSE=$license",
    "/DOUT_FILE=$outFile",
    "/DAPP_VERSION=$version",
    "/DAPP_VERSIONWITHBUILD=$versionWithBuild",
    "/DAPP_COPYRIGHT=$copyright",
    $script
)
& $makensisPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "NSIS failed with exit code $LASTEXITCODE"
}
Write-Output "Created $outFile"
