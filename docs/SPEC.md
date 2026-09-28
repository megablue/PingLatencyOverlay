# PingLatencyOverlay — Spec

## Startup
- Launches into the system tray with no window shown. The tray is the program
  the user runs; the Config window and the renderer are started on demand.
- Tray menu: Config, Pause/Resume, Close tray (keep overlays running), Exit.
  The two exit items are deliberately different: closing the tray leaves the
  overlays running, and Exit stops the whole app.
- The app runs as three processes. The tray holds the tray icon and supervises.
  The Config window is started when it is asked for and **exits when it is
  closed**, so its window and graphics context exist only while you are looking
  at them. The renderer draws the overlay windows and runs the probes, with no
  graphics stack of any kind.
- What you launch brings the rest up with it, so any one of them gets you a
  working app. Launching the tray starts a missing renderer but does **not**
  open the window — the window opens when you ask for it from the tray's menu.
  Launching the window starts a missing tray and renderer. The renderer never
  starts anything, because it is the thing being started.
- Closing the Config window leaves the tray running. That is intended: the tray
  is the app, and a window should not take it down with it. To stop everything,
  use the tray's **Exit**.
- If something goes wrong, `pinglatencyoverlay.log` is written beside your
  config, and a failure that stops the tray from starting at all also puts up a
  message box, because the tray is the one part of this app with no window of
  its own.
- Starting the app starts the renderer. If one is already running — because you
  closed the tray, or launched the Config window directly — the caller attaches
  to it instead of starting a second.
- Each of the three runs at most once, enforced with its own named mutex. A
  second launch of the same one does nothing; it is not an error. Opening the
  Config window when it is already open focuses it rather than starting a rival.
- Closing the Config window asks first if there are unsaved changes, offering
  Save, Discard, or Cancel. It only closes if the answer succeeded: a save that
  could not be written leaves the window open and says why, because closing on a
  save that did not happen loses work silently.
- If the renderer exits on its own, the tray restarts it, up to five times in a
  minute. After that it stops trying and says so, because a crash loop is worse
  than a stopped app: it burns a core and buries the one message that would
  explain it. A renderer running with no tray is not restarted at all.

## Interfaces
- Config window: create and manage overlays.
- Overlay windows: one OS window per configured overlay.
- Tray, Config window and renderer talk over a named pipe. The first two send
  configuration, pause and resume, which overlay's preview border is showing,
  and shutdown; the renderer needs to say nothing back. If the renderer cannot
  be reached the sender says so rather than failing quietly, including the case
  where a profile was written to disk but the overlays were not updated.
- The renderer is found next to the caller's own executable, so all three
  programs must be installed into the same directory.

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
    Selecting an overlay in the list activates the RGB loop; losing the
    selection, leaving the Overlays page, or closing the Config window starts
    the fade and eventually disables the border. A border therefore animates
    only while you are actually looking at that overlay's settings.
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
  be stored. Preferences live under a single `ui` object, currently
  `ui.railCollapsed` and `ui.showVersionInTitle`. Only that object is ever
  rewritten: the active profile pointer and any key a future version adds
  survive a preferences write, and a file that is missing, unparseable or holds
  unrelated keys simply yields the defaults. `ui.showVersionInTitle` is off by
  default, because the window title is already long and the About page is where
  the version belongs.
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
  **Profiles**, **Global**, **About**) plus a chevron that collapses it to icons
  only. The rail is 148px labelled and 44px collapsed, and its glyphs are
  painted, so they never depend on font coverage.
- The **list pane** holds the current page's list, headed by the page's own
  controls. The Overlays page heads it with the profile switcher, which shows
  the active profile's display name and opens the profile menu. Its footer holds
  **Add overlay** and **Pause all**. The Profiles page lists every profile with
  its overlay count and an accent dot on the active one, and its footer holds
  **+ New profile**.
- The Overlays page opens with **nothing selected**, so pane 3 shows a short
  "No overlay selected" message and no border is animating until you choose an
  overlay. Two things clear the selection again: clicking the row you already
  have selected, and clicking the blank space in the list pane below the last
  row. The blank space is a hit target only, so a list short enough to fit never
  grows a scrollbar; a list long enough to scroll simply has no blank space and
  the row click carries on alone.
- Selection is a view concern only. Unsaved edits are held per overlay, so
  clearing the selection never discards anything, and **Save** and **Discard**
  keep their state.
- Overlay counts are read from each profile's file when the Profiles page is
  arrived at, and again after any action that changes one - creating, renaming,
  duplicating, deleting or switching a profile. A count that has not been read
  draws no number at all, because "not read yet" is not the same as "no
  overlays" and a zero would be a confident lie. A profile whose file cannot be
  read is counted the same way: no number, never a zero.
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
  puts the rail back. The Appearance group also holds **Show the version in the
  window title**, which appends the version in brackets to the title
  (`PingLatencyOverlay - Current Profile: Home (v0.1.68)`). Like the rail
  checkbox it previews immediately and is written only on Save. The About page
  has no list either, so its detail pane spans the same full width, and is
  read-only and centred: the app icon, then one column of lines — the app name
  large, the tagline under it, the version, then the repository, the copyright
  and the licence. Nothing on it can be edited, and it carries no Save/Discard
  footer because it stages nothing. No line on it is smaller than the rest of
  the window's body text. The repository line is the only clickable one: it
  hands its address to Windows, which routes it to the user's default handler
  for `https`, and the full address is in the hover tooltip. The copyright
  carries no year, so it cannot go stale between releases. The licence is named
  in words and not linked, because the licence text is not bundled with the app.
- The **detail pane** ends in a sticky footer holding **Discard** and **Save**,
  right aligned with Save as the filled primary action, under a scrolling detail.
  Both are enabled while **either** draft has something to write - the profile
  or the app-wide preferences - which is also how unsaved edits are signalled. A
  Save that only has preferences to write leaves the profile file alone. Only a
  page that stages a draft carries it: the
  Overlays and Global pages do, the Profiles and About pages do not, because
  they act on files immediately or stage nothing at all. On the Profiles page
  the pending-draft hint in the detail names the Overlays page as the place to
  resolve it, and the rail's Overlays row carries a dot and an "unsaved changes"
  hover while one is open.
- The **status bar** spans the full width and holds the transient operation
  message. The version is not here; it is on the About page, and optionally in
  the window title.
- Both panes are a scrolling area above a fixed footer, so a pane's own actions
  sit next to the content they act on instead of a pane away in the status bar.
- A row in the list pane is its contents plus the same margin on every side, so
  the controls inside it never sit flush against the fill. A profile row reserves
  a gutter for its overlay count and active dot, so a long display name
  truncates rather than running underneath them. The gutter is two columns: the
  count, right aligned, and the active profile's dot in a fixed slot after it, so
  the numbers line up down the list and the dot never moves them. The name, the
  gap between widgets and the gutter all come out of the row's width.
- The profile switcher at the head of the list pane carries the same fills the
  control it replaced had: a resting fill, a hover fill, and the selection while
  its menu is open, with the profile name in the normal text colour. It has no
  outline of its own, like the rows.
- Rows in the list pane carry no border of their own. Like the rail's rows they
  are painted: the selected row gets the selection fill, a hovered row a
  muted fill, and an untouched row nothing at all. The rail is the reference,
  so a selection looks the same wherever it appears.
- The list pane's header, its rows and its footer are all one width, and that
  width is one column centred in the pane, so both of the pane's boundaries get
  the same margin. The scroll bar's width is reserved whether or not one is
  showing, which is the slack the centring divides. Nothing in the pane can
  therefore be wider or narrower than its neighbour, and the rows do not shift
  sideways when a list outgrows the pane.
- The two pane boundaries are drawn the same way: a hairline centred in the gap
  between the panes, then the gap itself. The list pane is spaced identically on
  the rail side and the detail side, measured from the rail's rows to the column
  and from the column to the detail pane's content. (Hairline to nearest content
  is the wrong measure and reads unevenly, because the rail and the detail pane
  sit flush to their pane edges while the column is inset.)
- Each pane claims the width `config_ui` allocated for it. A pane that draws
  itself as a positioned child is invisible to the layout that allocated it, so
  without that claim the next pane is laid out on top of it.
- Switching pages is always allowed, because the Overlays draft stays in memory.
  Switching *profile* is refused while there are unsaved edits.
- The window is 860x660, resizable between 720x480 and 1400x8192, which keeps
  room for the rail, the list pane and a usable detail pane at the same time.

The renderer keeps its layered windows responsive: it services the Windows
messages those windows post (cursor changes, hover tracking, repaints) on
every pass of its loop, at least sixteen times a second, so Windows never
decides the window is hung. This is a separate limit from how often it
redraws, because a slow redraw must not also mean an unresponsive process.
The renderer waits for a client to connect whenever it has no shell attached,
and it sits idle rather than busy — closing and reopening the Config window
attaches to the same renderer instead of starting another.


- Target architectures: x64 and ARM64.
- Installer: native NSIS (`-setup.exe`), one per architecture.
- The installer runs four pages: welcome, destination folder, install, and
  finish. There is no components page.
- The finish page carries two ticked checkboxes: **Launch PingLatencyOverlay**
  and **Create a desktop shortcut**. A silent install (`/S`) never shows the
  finish page, so it creates the desktop shortcut to match the default.
- The application ships three executables and does not require WebView2:
  `plo-tray.exe` (the tray), `plo-config.exe` (the Config window) and
  `plo-renderer.exe` (the overlay
  windows and the probes). They are installed into the same directory, because
  each finds the others next to itself. Only the tray gets a Start Menu or
  desktop shortcut, since only the tray is what a user launches, and the
  installer's "Launch" checkbox launches the tray too.
- The GPLv3 text ships as `LICENSE` in the install directory beside the
  executables. It is installed unconditionally, not as an option, and the
  uninstaller removes it.
- The build version is `MAJOR.MINOR.(commits since countBase)` — the count
  since a base recorded in `src-tauri/Cargo.toml`, not the raw commit count, so
  each new minor restarts at `.1`. It is taken from Git at packaging time, and
  is the version shown in the Config window. If Git metadata is unavailable, or
  the count has not passed the base yet, the version in
  `src-tauri/Cargo.toml` is used as it stands. Run the packaging step *after*
  committing, or the installer reports the previous commit's number. The app's
  reported version and the installer's file name are computed from the same
  place, so they cannot drift apart.
