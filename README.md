# PingLatencyOverlay

A lightweight Windows tray application that shows live ICMP/TCP latency graphs as
frameless, transparent, click-through overlays.

The current `egui-rewrite` branch is a native Rust rewrite using egui/eframe;
it does not use WebView2, React, or the Tauri runtime. The previous Tauri/React
implementation remains available on the `main` branch.

## Build and run

From `src-tauri/`:

```powershell
cargo run
cargo build --release
```

The release executable is written to:

```text
src-tauri/target/release/ping-latency-overlay.exe
```

The app starts in tray mode. Left-click the tray icon to open Config; right-click
for Start/Pause, Config, and Exit. For development, `cargo run -- --show-config`
opens Config immediately.

## Installer

Build the release executable first, then from the repository root:

```powershell
npm run bundle
```

This uses NSIS and creates:

```text
src-tauri/target/release/bundle/nsis/PingLatencyOverlay_0.1.0_x64-setup.exe
```

For ARM64, install the MSVC ARM64 build tools and Rust target, build with
`cargo build --release --target aarch64-pc-windows-msvc`, then run:

```powershell
.\scripts\build-nsis.ps1 -Arch arm64
```

A separate installer is required for each architecture.

## Configuration

Settings are stored at:

```text
%USERPROFILE%\.PingLatencyOverlay\config.json
```

The config format is shared with the Tauri implementation, so existing overlay
settings are migrated automatically.
