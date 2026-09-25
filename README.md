# PingLatencyOverlay

A lightweight Windows tray application that shows live ICMP/TCP latency graphs as
frameless, transparent, click-through overlays.

Repository: https://github.com/megablue/PingLatencyOverlay

The native Rust implementation currently lives on the `egui-rewrite` branch and
uses egui/eframe; it does not use WebView2, React, or the Tauri runtime. The
previous Tauri/React implementation remains available on the `main` branch.

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
%USERPROFILE%\.PingLatencyOverlay\config.json
```

The config format is shared with the Tauri implementation, so existing overlay
settings are migrated automatically. Each overlay can optionally enable **Smooth
rendering** and set its target redraw rate from 1–1000 FPS; it is enabled by
default at 60 FPS for new overlays and legacy configurations without an explicit
preference. The graph continues to use the actual probe timestamps and never
bridges timeout gaps. **Startup Behaviors** can show a cosmetic fake graph before
real samples arrive, with a configurable prefill color and reveal duration. The
prefill remains as cosmetic history while real samples append to the same
rendered timeline.
