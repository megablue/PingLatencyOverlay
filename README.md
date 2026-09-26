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

Settings are stored in profiles:

```text
%USERPROFILE%\.config\.PingLatencyOverlay\
├── globalconfig.json
└── profiles\
    ├── profile_default.json
    └── profile_work.json
```

Each profile holds one complete set of overlays. The switcher at the top of the
sidebar opens the profile menu, which lists them and can create, rename, delete
and switch between them. **Save** and **Discard** sit in the status bar along the
bottom, so they work from any page, and **Save** writes to the profile that is
currently active; that choice is remembered in `globalconfig.json` as
`activeProfile` together with the file name in `activeProfileFile`. Those two
keys are the only record of the active profile: the app loads that file on
startup, and if it has been deleted or renamed away it falls back to the
`default` profile instead of picking another one on its own.

The window itself is three panes and a status bar. The leftmost **rail** switches
between the **Overlays**, **Profiles** and **Global** pages and collapses to
icons only; the middle pane lists what the current page is about, headed by the
profile switcher on the Overlays page; the right pane shows the detail. The
**Profiles** and **Global** pages arrive in the next releases.

Each profile has two names. The **display name** is what you type and what the
window title, the switcher and the profile menu show
(`PingLatencyOverlay - Current Profile: Work VPN`); it is stored in the profile
file as `profileName` and does not have to be unique. The **id** is the file
name `profile_<id>.json`, where the id is lowercased and reduced to letters,
digits, `-` and `_`. Because only the id has to be unique, creating or renaming
a profile never overwrites another one: a taken id gets a postfix, so two
profiles named *Work VPN* live in `profile_work.json` and
`profile_work_2.json`. Profiles saved before names existed are given one
derived from their id on the next launch.

**Save** and **Discard** are the two ways to resolve pending edits: Save writes
them to the active profile, and Discard reloads it from disk. Switching to
another profile is refused until one of them is used.

An existing `%USERPROFILE%\.config\.PingLatencyOverlay\config.json` is
validated and moved to `profiles\profile_default.json` on the first launch, and
is only deleted once the profile copy is in place. A file that cannot be read
is kept where it is and the reason is shown in the status bar. When `config.json`
is missing, a legacy `%USERPROFILE%\.PingLatencyOverlay\config.json` is
migrated first; that legacy directory is removed only when `config.json` was its
only entry, so any other user files are preserved.

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