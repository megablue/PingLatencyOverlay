# PingLatencyOverlay

Live network latency as a small graph that sits on top of your screen. Always
visible, click-through, and out of the way — and cheap enough to leave running
all day.

<!-- Add a screenshot here: the Config window. -->
<!-- Add a screenshot here: an overlay above a game or a video call. -->

## Why you might want it

- **Always-on-top graphs** that never steal your clicks. The overlay is
  click-through, so you can keep playing or working underneath it.
- **Small enough to leave running.** With no settings window open, the tray and
  the overlay use about 35 MB of memory between them. Close the tray and leave
  the overlay running and it drops to about 17 MB. Figures vary by machine.
- **Genuinely transparent.** Set the background to fully invisible and you are
  left with just the line, or a soft tinted panel if you prefer.
- **Ping or TCP.** Measure ICMP latency, or the time it takes to complete a TCP
  handshake with a host and port — useful when ICMP is filtered or lying.
- **Any monitor, any corner.** Pick the spot with a small on-screen position
  picker, then nudge it with independent horizontal and vertical margins.
- **Tuned to your taste.** Line color, timeout color, background color and
  opacity, graph size, how much history to show, and which latency counts as the
  top of the graph.
- **Start-up flourish if you want it.** Fade the line in, or run a border effect
  when an overlay comes up.
- **No telemetry, no account, no cloud.** Everything stays on your machine.
- **No Administrator rights.** It runs as you do.

## Install

Grab the installer for your machine from the
[Releases page](https://github.com/megablue/PingLatencyOverlay/releases) — pick
**x64** for most PCs, **ARM64** for Windows on Arm. One installer, nothing else
to install.

The app starts in the system tray, so it will not get in your way while you set
it up.

## Getting started

1. **Open the Config window.** Left-click the tray icon to open it. Right-click
   for a menu with **Config**, **Pause / Resume**, **Close tray, keep overlays
   running** and **Exit**.
2. **Add an overlay.** Press **Add overlay** in the list pane. Give it a name you
   will recognise later.
3. **Choose a protocol and a target.**
   - *ICMP* — enter a hostname or IP address. This is the same measurement
     `ping` makes, and it does not need Administrator.
   - *TCP* — enter a hostname or IP **and a port**. This measures the time to
     complete a connection handshake, which is a better signal than ICMP on
     networks that deprioritise or block it.
4. **Set the timeout.** Anything slower than this is drawn in the timeout color
   rather than as a latency value, so a stalled probe looks different from a
   slow one.
5. **Press Save.** Your overlay appears on screen within a second.

Everything is sampled once a second.

### The settings that matter

| Setting | What it does |
| --- | --- |
| **Name** | Shown in the list. Purely for your benefit. |
| **Protocol** | ICMP (ping) or TCP (connect). |
| **Target host / IP** | Hostname or address to probe. |
| **Port (TCP only)** | Port to connect to. |
| **Timeout** | Above this, the sample is drawn as a timeout instead of a latency. |
| **Sampling** | How much history the graph holds. |
| **X axis scale** | How many seconds each pixel of width covers. Lower means finer detail over a shorter span. |
| **Y axis height** | Height of the overlay in pixels. |
| **Latency ceiling** | The latency that reaches the top of the graph. Values above it clamp. |
| **Orientation** | Rotates the graph anticlockwise, so 90° makes time run bottom-to-top. |
| **Mirrored** | Flips the graph. |
| **Smooth rendering** | Interpolates between samples instead of stepping. |
| **Smooth FPS** | How often the smoothed graph redraws. |
| **Line color** | The latency trace. |
| **Timeout color** | The trace for samples that timed out. |
| **Background color** / **Background opacity** | The panel behind the graph. Fully transparent gives you a bare line on a bare screen. |
| **Position** | A small picker showing the screen. Click where you want the overlay. See below. |
| **Horizontal / Vertical margin** | Nudges the overlay away from its anchor. |
| **Enabled** | Turns probing on or off for this overlay without deleting it. |

There are also start-up options — **Cosmetic startup prefill**, its animation
length, and a **Startup border effect** with its own timing — if you want the
overlay to announce itself when it appears.

## Positioning an overlay

The **Position** control shows a small map of the screen. Click the spot you
want, and the overlay anchors to whichever of the nine positions is nearest, so
it sits against that edge of the work area and stays clear of the taskbar.

`Horizontal margin` and `Vertical margin` then nudge it away from that anchor.
Both start at `0`. On an edge anchor, positive values move *inward* and negative
values move *outward*; on a centred axis, positive moves right or down. Negative
values can push an overlay off the work area entirely, which is sometimes what
you want for a second monitor.

## Profiles

A profile is a complete, named set of overlays. Use them to keep separate setups
for separate situations — home and work, or a game versus a video call — and
switch between them without rebuilding anything each time.

- **Switch** from the menu at the top of the list pane, or from the **Profiles**
  page.
- **Create, rename, duplicate and delete** on the **Profiles** page.
- **Display names do not have to be unique.** Two profiles can both be called
  *Work*; they simply get separate files behind the scenes.
- The profile you were last using is remembered and reloaded at the next launch.
- You cannot switch profiles while you have unsaved edits. Press **Save** or
  **Discard** first — the app will tell you when it is blocking you.

**Save** and **Discard** live in a fixed bar under the right-hand pane, so they
sit next to the settings they write.

Closing the window closes the window — the tray and your overlays carry on
running. If you have unsaved changes it asks first, offering **Save**,
**Discard** or **Cancel**. To stop everything, use the tray's **Exit**.

**Close tray, keep overlays running** is the one to reach for if you want the
lowest footprint: the tray goes, the overlay stays, and the app is down to
about 17 MB. Bring the tray back whenever you like by running it again from the
Start menu.

## Where your settings live

```text
%USERPROFILE%\.config\.PingLatencyOverlay\
├── globalconfig.json
└── profiles\
    ├── profile_default.json
    └── profile_work.json
```

Plain JSON, one file per profile. If you are upgrading from a version that used a
single `config.json`, it is migrated for you on first launch.

## For contributors

<details>
<summary>Building and running from source</summary>

Requires the MSVC toolchain and Build Tools. From `src-tauri/`:

```powershell
cargo run                  # development build; opens the Config window
cargo build --release      # optimised standalone executables
cargo test
cargo clippy --all-targets -- -D warnings
```

Three executables are written to `src-tauri\target\release\`: the tray, the
Config window, and the renderer. `cargo run` starts the window, which brings up
the other two if they are not already running.

To build an installer, commit first — the version number comes from the Git
commit count — then from the repository root:

```powershell
npm run icons
npm run bundle
```

ARM64 needs the ARM64 build tools and Rust target:

```powershell
cargo build --release --target aarch64-pc-windows-msvc
.\scripts\build-nsis.ps1 -Arch arm64
```

Further detail lives in [`AGENTS.md`](AGENTS.md) for contributors and
[`docs/SPEC.md`](docs/SPEC.md) for the full behaviour specification.

</details>

## License

PingLatencyOverlay is free software released under the GNU General Public
License, version 3 only (`GPL-3.0-only`). Copyright © Evert Chin (megablue).

The complete license text is available in [`LICENSE`](LICENSE). Source code for
released versions is available from the corresponding GitHub release/tag.
