# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. Stack: **Rust + Tauri v2** backend, **React + TypeScript + Vite**
frontend. See `docs/SPEC.md` for product behavior.

## Layout
- `src-tauri/` — Rust backend (the Cargo project; run Cargo commands here).
  - `src/lib.rs` — Tauri builder, tray/window/probe setup, `#[tauri::command]`s.
    Saving the config emits `config://updated`; overlay windows restyle live from
    it (colors, orientation, height). `overlay::reconcile` handles size/position.
  - `src/config.rs` — config model, `~/.PingLatencyOverlay/config.json` load/save,
    and `normalize()` clamping (window >= 30 s, scale 1–10 default 2, timeout
    default 1000, graph height >= 10 px, max Y default 1000 ms, bg opacity <= 100).
  - `src/probe.rs` — one-shot latency measurement (ICMP or TCP).
  - `src/probes.rs` — `ProbeManager`: one async task per enabled overlay, emits
    `latency://<id>` events; fixed 1 s cadence (1 tick == 1 s).
  - `src/overlay.rs` — creates/reconciles overlay windows and anchors them.
  - `src/tray.rs`, `src/state.rs` — tray menu and shared `AppState`.
- Frontend at the repo root. One React app (`src/main.tsx`) branches on the
  window label: `config` renders the editor, `overlay-<id>` renders the graph.
- `scripts/gen-icons.mjs` — generates `src-tauri/icons/*` with no dependencies.

## Commands
Frontend / Tauri (run from the repo root):
- `npm install`
- `npm run tauri dev` — dev app (starts Vite on port 14200; see the port note in
  `vite.config.ts`)
- `npm run tauri build` — release bundle
- `npm run icons` — regenerate app icons after changing the artwork script

Rust (run from `src-tauri/`):
- `cargo fmt`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

## Gotchas
- Windows-only: MSVC toolchain (`x86_64-pc-windows-msvc`) + MSVC Build Tools;
  the app and installer need the WebView2 runtime.
- ICMP uses the `ping-rs` crate (wraps Win32 `IcmpSendEcho2`), so it works
  **without** Administrator. Raw ICMP sockets would need elevation; TCP-connect
  latency never does. `src/probe.rs` resolves hosts to IPv4 (ICMP is v4-only).
- Installers: NSIS only (`bundle.targets = "nsis"`); Tauri's WiX/MSI bundler does
  not support ARM64. Build one `-setup.exe` per arch — there is no universal
  installer:
  `cargo tauri build --target x86_64-pc-windows-msvc` and
  `cargo tauri build --target aarch64-pc-windows-msvc --bundles nsis`.
- ARM64 builds need the MSVC v143 C++ ARM64 build tools (VS Installer) plus
  `rustup target add aarch64-pc-windows-msvc`.
- Overlay windows must be frameless, transparent, always-on-top and click-through
  (`set_ignore_cursor_events(true)`); missing any of these breaks the overlay.
- Tray app lifecycle: `lib.rs` prevents exit on window close (`ExitRequested` +
  `AppState::quitting`) and hides the config window on close. Only the tray
  "Exit" item (which sets `quitting`) actually terminates the app — don't remove
  this or the app will die when the last window closes. A single **left-click**
  on the tray opens the config window (`tray.rs` `on_tray_icon_event`); right
  click shows the menu (`show_menu_on_left_click(false)`).
- `tauri.conf.json` and `capabilities/` are read at startup and are not
  hot-reloaded — restart `tauri dev` after editing them.
- Graph rendering rules live in `src/graph.ts` and are easy to get wrong:
  orientation rotates **anticlockwise** (canvas `rotate()` is clockwise, so the
  angle is negated), and pings above `maxYMs` clamp to the top. See
  `docs/SPEC.md` for the timeout-gap and Y-axis rules.
- `drawGraph` fills the canvas's **actual** CSS size, not the requested
  `windowSeconds * scale`. WebView2's `devicePixelRatio` is the display scale
  times the Windows **text-scale** setting (observed 1.25 x 1.29 = 1.6125), so
  the CSS viewport is smaller than the logical window size and assuming the
  requested size clips the graph. Windows are still sized correctly in logical
  pixels (e.g. 60 logical -> 75 physical at 125%) — don't "correct" the size.
- Creating windows from a `#[tauri::command]` must use an **`async`** command on
  Windows (sync commands deadlock/race WebView2) — `save_config` is async for
  this reason; don't change it back.
- Don't use `window.confirm`/`window.alert` in the UI — native JS dialogs don't
  work reliably in this WebView2 setup (the delete button silently did nothing).
  Use an inline confirmation instead (see the overlay-row delete flow in
  `ConfigWindow.tsx`).
- Dev-mode staleness: Vite/HMR does **not** reliably refresh already-open
  webviews (overlay windows can keep running stale modules and stop drawing; the
  config window can serve a stale module and fail to render). If a window looks
  stale or blank in `tauri dev`, restart `tauri dev` — a fresh launch re-reads
  the files. This is a dev-only quirk; production builds embed the frontend.
- `src-tauri/Cargo.toml` uses `crate-type = ["rlib"]` on purpose (not the Tauri
  template's `staticlib`/`cdylib`, which are for mobile and add ~1 GB to the
  debug build). Don't re-add them for a desktop-only app.
- Overlay anchoring uses the monitor's **work area** (`Monitor::work_area()`),
  not its full size, so bottom/right overlays sit above a docked taskbar. Because
  the taskbar is topmost too (and wins the z-order when it auto-shows), a 1s task
  in `lib.rs` re-asserts topmost via Win32 `SetWindowPos(HWND_TOPMOST)` in
  `overlay::reassert_topmost`. Overlays are click-through, so this doesn't block
  taskbar interaction. If an overlay is ever hidden behind the taskbar, check
  both of these.
