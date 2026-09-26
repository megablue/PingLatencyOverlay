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
  `profile_<id>.json`. The id is a lowercase slug and is the only unique part
  of a profile.
- Every profile file carries its own name in the `profileName` key. It is
  free-form, keeps the case it was typed in, and does not have to be unique; it
  is what the window title, the sidebar button and the profile popup show.
  Files written before names existed get one derived from their id on the next
  launch.
- Creating or renaming a profile never overwrites another profile file. When
  the id derived from the new name is taken, the file is stored with the first
  free postfix instead: `profile_home_2.json`, `profile_home_3.json`, and so
  on. A profile renamed to a name that resolves to its own id keeps its file.
- `~/.config/.PingLatencyOverlay/globalconfig.json` holds app-wide
  preferences. It is created as `{}` and stays empty until something needs to
  be stored. Preferences live under a single `ui` object, currently just
  `ui.railCollapsed`. Only that object is ever rewritten: the active profile
  pointer and any key a future version adds survive a preferences write, and a
  file that is missing, unparseable or holds unrelated keys simply yields the
  defaults.
- The active profile is recorded in that file as `activeProfile` (the id) plus
  `activeProfileFile` (the `profile_<id>.json` name), and both keys are removed
  when the active profile is `default`, so an absent key means `default`.
- On startup those two keys are the only source for what to load. The stored
  file name is used first, then the stored id, which keeps older files written
  before the file name existed working. A pointer that does not resolve, or
  points at a file that is gone or unreadable, falls back to `default` and the
  stale keys are cleared. No other profile is ever picked by scanning the
  directory, and a missing `default` on a first run is not a fallback: it is
  created empty.
- The active profile is loaded on startup to recreate overlays and their
  settings, and it is the target of **Save**.
- Profiles are created empty, and are listed, renamed, duplicated and deleted on
  the Profiles page. **Duplicate** copies the source profile's overlays into a
  new file and leaves the source untouched; the copy is not loaded, so switching
  to it stays a deliberate action. Rows that share a name also show their id, and
  the delete confirmation names the file it removes.
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
  migration failure, profile import, added profile names and profile fallback.

## Config window layout
- The Config window has three panes and a status bar.
- The **navigation rail** is the leftmost pane: one row per page (**Overlays**,
  **Profiles**, **Global**) plus a chevron that collapses it to icons only. The
  rail is 148px labelled and 44px collapsed, and its glyphs are painted, so
  they never depend on font coverage.
- The **list pane** holds the current page's list, headed by the page's own
  controls. The Overlays page heads it with the profile switcher, which shows
  the active profile's display name and opens the profile menu. Its footer holds
  **Add overlay** and **Pause all**. The Profiles page lists every profile with
  its overlay count and an accent dot on the active one, and its footer holds
  **+ New profile**.
- A profile row in the list **selects** its profile; it never loads it. Loading is
  the separate **Switch to this profile** action in the detail pane, because
  switching is refused while there are unsaved edits and a list selection should
  not have that side effect.
- The **profile menu** under the switcher only switches, and ends with
  **Manage profiles**, which opens the Profiles page. Create, rename, duplicate
  and delete live on that page instead, where they have room for a real name
  field rather than an editor crammed into a menu.
- The **detail pane** holds the current page's detail: the editor for the
  selected overlay on the Overlays page, and the selected profile's name, file,
  overlay count and **Switch to this profile**, **Rename**, **Duplicate** and
  **Delete** on the Profiles page. Rename, Duplicate and Delete open their editor
  in place of that detail. The Global page has no list, so the detail pane spans
  the width the list pane would have used, and shows the app-wide preferences:
  an **Appearance** group with a **Collapse the navigation rail** checkbox, and a
  **Storage** group listing the config folder, the profiles folder and
  `globalconfig.json` as read-only paths, truncated with the full path on hover.
  Toggling the rail applies immediately, so the user watches it collapse as they
  click, but the value is only written when the draft is saved and **Discard**
  puts the rail back.
- The **detail pane** ends in a sticky footer holding **Discard** and **Save**,
  right aligned with Save as the filled primary action, under a scrolling detail.
  Both are enabled only while there is something to write, which is also how
  unsaved edits are signalled. Only a page that stages a draft carries it: the
  Overlays and Global pages do, the Profiles page does not, because it acts on
  profile files immediately. On the Profiles page the pending-draft hint in the
  detail names the Overlays page as the place to resolve it, and the rail's
  Overlays row carries a dot and an "unsaved changes" hover while one is open.
- The **status bar** spans the full width and holds the transient message on the
  left and the version on the right.
- Both panes are a scrolling area above a fixed footer, so a pane's own actions
  sit next to the content they act on instead of a pane away in the status bar.
- A profile row reserves a gutter for its overlay count and active dot, so a long
  display name truncates rather than running underneath them. The name, the gap
  between widgets and the gutter all come out of the row's width.
- Switching pages is always allowed, because the Overlays draft stays in memory.
  Switching *profile* is refused while there are unsaved edits.
- The window is 860x660, resizable between 720x480 and 1400x8192, which keeps
  room for the rail, the list pane and a usable detail pane at the same time.

## Packaging
- Target architectures: x64 and ARM64.
- Installer: native NSIS (`-setup.exe`), one per architecture.
- The application is a standalone Rust executable and does not require WebView2.
