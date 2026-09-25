# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. The native implementation uses **Rust + egui/eframe** for the single
configuration window and native Win32 layered windows for overlays. Probes run
on Tokio tasks and the system tray uses `tray-icon`. See `docs/SPEC.md` for
product behavior.

## Layout
- `src-tauri/` — the Cargo project; run Cargo commands here.
  - `src/lib.rs` — module wiring and the application entry point.
  - `src/config.rs` — config schema, defaults, normalization, and JSON persistence.
  - `src/probe.rs` — one-shot ICMP or TCP latency measurement.
  - `src/probes.rs` — long-lived Tokio probe tasks and bounded sample buffers.
  - `src/overlay.rs` — native layered HWND creation, DPI/work-area layout, and
    per-window alpha compositing with `UpdateLayeredWindow`.
  - `src/render.rs` — software graph rendering into premultiplied RGBA.
  - `src/border.rs` — runtime border-effect state and software RGB border drawing.
  - `src/ui.rs` — tray-mode egui configuration editor.
  - `src/tray.rs` — tray icon, menu, and bundled artwork.
- `scripts/gen-icons.mjs` — generates native artwork with no dependencies.
- `packaging/nsis/` and `scripts/build-nsis.ps1` — native installer packaging.
- The pre-egui Tauri/React implementation remains available on `main`.

## Commands
Native app (run from `src-tauri/`):
- `cargo run` — development build
- `cargo run -- --show-config` — development launch with Config visible
- `cargo build --release` — optimized standalone executable
- `cargo fmt`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

Installer (run from the repository root):
- `npm run icons`
- `npm run bundle` — build the x64 NSIS installer from the existing release exe
- `.\\scripts\\build-nsis.ps1 -Arch arm64` — build the ARM64 installer after
  building the ARM64 release exe

## Gotchas
- Windows-only: MSVC toolchain (`x86_64-pc-windows-msvc` or
  `aarch64-pc-windows-msvc`) + MSVC Build Tools. No WebView2 runtime is needed.
- ICMP uses the `ping-rs` crate (Win32 `IcmpSendEcho2`) and does not require
  Administrator. `src/probe.rs` resolves ICMP targets to IPv4; TCP supports
  hostname resolution through Tokio.
- The root egui viewport starts hidden. Tray **left-click** shows Config; the
  context menu is right-click. Config-window close hides the root viewport; only
  tray Exit closes the app.
- Overlays are not egui child viewports. Each is a native `WS_EX_LAYERED` popup
  rendered with `UpdateLayeredWindow`, so it has true per-pixel alpha, no DWM
  frame, no taskbar button, no focus, and mouse passthrough.
- Do not replace `UpdateLayeredWindow` with egui/GPU child viewports. The old
  multi-viewport renderer was the source of the white-background and excessive
  memory problems.
- `render.rs` writes premultiplied RGBA; `overlay.rs` swaps R/B to premultiplied
  BGRA before copying it into a 32-bit DIB. `bgOpacity=0` leaves the alpha byte
  at zero; positive values are composited by Windows.
- Graph orientation rotates the whole graph **anticlockwise** (90 means time runs
  bottom-to-top); values above `maxYMs` clamp to the top. Timeout samples draw a
  full-height timeout-colored line and break the latency line. See `docs/SPEC.md`.
- `ProbeManager::apply_config` must not restart all tasks for a style-only Save.
  Existing tasks read shared settings each tick; only deleted/disabled overlays
  are stopped. This keeps Save from pausing the graph.
- Sample buffers are bounded and overlay HWNDs are reused by stable ID. Do not
  allocate one renderer or surface per overlay.
- The graph uses actual physical window dimensions. Do not assume
  `windowSeconds * scale` is the drawable size under Windows DPI/text scaling.
- Right-panel edits and Add overlay are staged until Save. Sidebar enable/delete
  and Pause/Resume apply immediately. Delete uses an inline confirmation
  because native script dialogs are not used.
- `src-tauri/Cargo.toml` uses eframe with the `glow` renderer for the one config
  window and `tiny-skia` only for software overlay pixels. Do not add WebView2 or
  Tauri back into the native branch.
