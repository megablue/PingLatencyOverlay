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
  the taskbar. The gap from the work-area edge is configurable (`marginPx`,
  default 20 logical px); center anchors ignore it.

## Probe (per overlay)
- Protocol: ICMP echo or TCP connect.
- Target: domain name or IP address.
- Port: required for TCP, unused for ICMP.
- Timeout: configurable, default 1000 ms.

## Config storage
- JSON at `~/.PingLatencyOverlay/config.json`.
- Loaded on startup to recreate overlays and their settings.

## Packaging
- Target architectures: x64 and ARM64.
- Installer: native NSIS (`-setup.exe`), one per architecture.
- The application is a standalone Rust executable and does not require WebView2.
