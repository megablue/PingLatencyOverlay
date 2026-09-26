# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. The native implementation uses **Rust + egui/eframe** for the single
configuration window and native Win32 layered windows for overlays. Probes run
on Tokio tasks and the system tray uses `tray-icon`. See `docs/SPEC.md` for
product behavior.

## Layout
- `src-tauri/` — the Cargo project; run Cargo commands here.
  - `src/lib.rs` — module wiring and the application entry point.
  - `src/config.rs` — config schema, profiles, persistence, and directory
    migration.
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
- Config lives at `~/.config/.PingLatencyOverlay/config.json`. When that file is
  missing, `config.rs` migrates `~/.PingLatencyOverlay/config.json` and only
  removes the legacy directory when `config.json` was its sole entry.
- `horizontalMarginPx` and `verticalMarginPx` are signed screen-axis offsets.
  Edge anchors measure inward from the work-area edge; centered axes measure
  from the center. Legacy `marginPx` is mapped per anchor during normalization.
- Config lives at `~/.config/.PingLatencyOverlay/`. `profiles/` holds one
  `profile_<id>.json` per profile and is the only place overlays are saved;
  `globalconfig.json` holds app-wide preferences and is created as `{}`, with
  `activeProfile` plus `activeProfileFile` added only once a non-default
  profile is selected. When that file is missing, `config.rs` migrates
  `~/.PingLatencyOverlay/config.json`, then moves `config.json` to
  `profiles/profile_default.json` and deletes it only after the copy succeeds.
  All of this lives in `Store` in `config.rs`, which takes its root directory so
  tests can point it at a temp folder.
- `globalconfig.json` is the only source for the active profile. `Store::load`
  builds a candidate list from the stored file name, then the stored id, then
  `default`, and never scans the directory for a substitute. A missing `default`
  on a first run is the fresh-config case, not a `ProfileFallback`.
- A profile has two names. The id is the file name and the only unique part; the
  free-form `profileName` inside the file is what the title bar, sidebar and
  popup show, and it may repeat. Collisions are resolved by postfixing the id
  (`profile_work_2.json`) instead of refusing, and `create_profile` /
  `rename_profile` return the `ProfileEntry` that resulted. `with_postfix`
  keeps the id inside the sanitizer's length cap so `list_profiles` still
  accepts it. `Store::backfill_profile_names` writes a derived name into
  existing files at startup and reports it as a `ConfigNotice`; unreadable
  files are never rewritten.
- The window title is `PingLatencyOverlay - Current Profile: <name>`, pushed
  with `ViewportCommand::Title` only when it changes (`sync_window_title`).
- Profile switching is refused while `dirty`; **Discard** reloads the active
  profile from disk. Save/Discard are the only ways to resolve pending edits.
- The Config window is three panes plus a status bar (`config_ui`): a
  navigation rail (`show_rail`, `Page`/`PAGES`, `rail_width`), a list pane
  (`show_list_pane`, which is dropped on the Global page) and a detail pane.
  Pane switching is never guarded; only profile switching is.
- The bottom row of the Config window is the **status bar**
  (`show_status_bar`); **Save** and **Discard** live there, not in a pane, so
  pending edits can be resolved from any page. Transient operation messages
  appear beside them, with the version label right-aligned. The status string is
  cloned before the buttons are drawn, because clicking one needs `&mut self`.
- The profile switcher (`show_profile_switcher`) heads the Overlays list pane and
  anchors the existing profile popup. Rail rows and glyphs are painted with
  `ui.painter()`, not buttons, so the label can sit beside the icon.
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
