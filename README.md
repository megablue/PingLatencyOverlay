# PingLatencyOverlay

A lightweight Windows tray application that turns ICMP and TCP latency measurements into always-on-top, transparent graphs. Overlays are frameless and click-through, run locally without telemetry, and can be placed on any monitor with configurable timing, scale, orientation, colors, and startup effects.

## Installer

Build the release executable first, then from the repository root:

```powershell
npm run bundle
```

This uses NSIS and creates:

```text
src-tauri/target/release/bundle/nsis/PingLatencyOverlay_<version>_x64-setup.exe
```

The build version is derived from the current Git commit count in the form
`MAJOR.MINOR.<commit-count>` and is shown in the Config window. If Git metadata
is unavailable, the package version from `src-tauri/Cargo.toml` is used.

For ARM64, install the MSVC ARM64 build tools and Rust target, build with
`cargo build --release --target aarch64-pc-windows-msvc`, then run:

```powershell
.\scripts\build-nsis.ps1 -Arch arm64
```

A separate installer is required for each architecture.

## Configuration

Settings are stored at:

```text
%USERPROFILE%\.config\.PingLatencyOverlay\config.json
```

When the new file is missing, the app migrates a legacy
`%USERPROFILE%\.PingLatencyOverlay\config.json` file. The legacy directory is
removed only when `config.json` was its only entry; any other user files are
preserved. The Config status bar reports the migration result.

## Positioning

Each overlay uses a work-area anchor with independent signed screen-axis
offsets. `Horizontal margin` and `Vertical margin` default to `0` pixels.
On centered anchors, positive values move right/down and negative values move
left/up. On edge-facing anchors, positive values move inward from the work-area
edge and negative values move outward. For example, `CenterLeft` with
`horizontal = 0` and `vertical = 0` sits at the left edge and vertically
centered. Negative values may move an overlay outside the work area.

Margins refer to screen axes, even when the graph orientation is rotated.
Existing configurations using the old `marginPx` value are migrated relative
to each overlay's current anchor.

## Build and run

From `src-tauri/`:

```powershell
cargo run
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

The release executable is written to:

```text
src-tauri/target/release/ping-latency-overlay.exe
```

The app starts in tray mode. Left-click the tray icon to open Config; right-click
for Start/Pause, Config, and Exit. For development, `cargo run -- --show-config`
opens Config immediately.

The config editor is the only egui window. Overlays are independent native
Win32 layered windows, so their alpha is composited by Windows rather than by a
second GPU renderer. This keeps transparent/partial backgrounds reliable and
avoids allocating a renderer for every overlay.

## License

PingLatencyOverlay is free software released under the GNU General Public License,
version 3 only (`GPL-3.0-only`). Copyright (C) 2026 megablue.

The complete license text is available in [`LICENSE`](LICENSE). Source code for
released versions is available from the corresponding GitHub release/tag.