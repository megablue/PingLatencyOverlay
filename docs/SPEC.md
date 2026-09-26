# PingLatencyOverlay — Spec

## Startup
- Launches into the system tray with no window shown.
- Tray menu: Start/Pause, Config, Exit.

## Interfaces
- Config window: create and manage overlays.
- Overlay windows: one OS window per configured overlay.

## Overlay window
- Frameless, transparent background, always on top.
- Click-through: mouse events pass to the window underneath as if it did not exist.
- Renders a live line graph of latency samples, one tick per ping.
- Plot start: the first point is the first responding latency, not y0.
- Timeouts (a ping exceeding the overlay's timeout counts as no response):
  - The line is not interpolated across a timeout.
  - At each timeout, draw a vertical line from y0 to yMax in the timeout color
    (default red).
  - On resumption, the next segment starts at the next responding sample's X,
    using the last responding Y, then continues with actual samples.
- X axis:
  - User configures a time window (minimum 30 s) and an X-axis scale multiplier.
    The slider snaps from 1× through 10×; the numeric input also accepts larger
    values for wide graphs (default 2×).
  - The overlay window's long axis is sized `window(s) x scale` (e.g. 60 s at 2x
    = 120 px).
  - The graph fills the window's actual size, so the effective pixels-per-tick is
    `viewport / windowSeconds` (see the DPI note in `AGENTS.md`).
  - Optional smooth rendering scrolls the timestamped graph between probe
    samples. It is enabled by default at 60 FPS for new overlays and legacy
    configs without an explicit preference; the per-overlay smooth FPS controls
    the intermediate redraw rate, and timeout gaps are never interpolated.
- Startup behaviors:
  - `Cosmetic Startup Prefill` can show a deterministic fake latency graph
    before the first real probe result arrives.
  - The prefill uses its own line color (default `#64748b`) and reveals from
    left to right over the configured number of seconds (default 3).
  - The prefill remains as cosmetic history after the reveal; real samples are
    appended to the same rendered timeline and naturally scroll it left. The
    fake points retain the prefill color and are never added to the real sample
    buffer.
  - Prefill is enabled by default for new overlays and legacy configurations
    without an explicit preference.
  - The startup border effect defaults to RGB Loop when unspecified. RGB Noise
    assigns a new pseudorandom RGB color to every border pixel as it animates.
    The effect runs for 5 seconds by default, then fades for 1 second by default.
    Selecting an overlay tab always activates the RGB loop; losing selection or
    closing the Config window starts the fade and eventually disables the
    border.
  - Border animation uses a dedicated 60 FPS redraw path and a 3 px inline
    border, independent of the graph Smooth Rendering setting.
- Y axis:
  - Height is configurable (`graphHeightPx`, default 60 px).
  - Ceiling is configurable (`maxYMs`, default 1000 ms); pings above it clamp to
    the top of the axis.
- Style:
  - Orientation rotates the whole graph anticlockwise: 90 => time plots
    bottom to top, 180 => right to left, 270 => top to bottom.
  - Mirror: on/off; combines with any orientation.
  - Line color: configurable.
  - Timeout color: configurable, default red.
  - Background color (`bgColor`, default `#0f172a`) and opacity (`bgOpacity`,
    0–100, default 0 = fully transparent), drawn behind the graph and not
    rotated with it.
- Position: one of 9 anchors within the monitor's **work area** (which excludes
  the taskbar / other appbars), so bottom and right overlays aren't hidden behind
  the taskbar.
- Position offsets are signed screen-axis values in logical pixels:
  `horizontalMarginPx` and `verticalMarginPx`, both defaulting to 0. Edge-facing
  axes measure inward from the work-area edge; centered axes measure from the
  center. Positive values move right/down (or inward from an edge), and negative
  values move left/up (or outward from an edge). Negative values may move an
  overlay outside the work area.
- Existing `marginPx` configurations are migrated relative to each overlay's
  current anchor so their screen position is preserved.

## Probe (per overlay)
- Protocol: ICMP echo or TCP connect.
- Target: domain name or IP address.
- Port: required for TCP, unused for ICMP.
- Timeout: configurable, default 1000 ms.

## Config storage
- Settings live in profile files under
  `~/.config/.PingLatencyOverlay/profiles`, one file per profile, named
  `profile_<name>.json`.
- `~/.config/.PingLatencyOverlay/globalconfig.json` holds app-wide
  preferences. It is created as `{}` and stays empty until something needs to
  be stored; the active profile is recorded as `activeProfile` and an absent
  key means the `default` profile.
- The active profile is loaded on startup to recreate overlays and their
  settings, and it is the target of **Save**.
- Profiles are created empty, and are listed, renamed and deleted from the
  profile popup above **Add overlay** in the sidebar.
- Switching profiles is refused while there are unsaved edits. **Discard**
  reloads the active profile from disk and drops those edits.
- Deleting the active profile falls back to `default`, or to the first
  remaining profile when `default` is gone. The last remaining profile cannot
  be deleted.
- An existing `config.json` is validated and moved to
  `profiles/profile_default.json`, and only then removed. A file that fails
  validation is left in place and the problem is reported.
- If `config.json` is missing, a legacy `~/.PingLatencyOverlay/config.json` is
  migrated first. The legacy directory is removed only when `config.json` was
  its only entry; other files and subdirectories are preserved.
- The Config status bar reports successful migration, retained legacy data,
  migration failure, profile import and profile fallback.

## Packaging
- Target architectures: x64 and ARM64.
- Installer: native NSIS (`-setup.exe`), one per architecture.
- The application is a standalone Rust executable and does not require WebView2.
