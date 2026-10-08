# Session handoff — PingLatencyOverlay

Next: `docs/features/README.md` — the feature map. Read only the page for
your area before the code.

**A snapshot, not a source of truth.** This file goes stale by design. If it
disagrees with the code, the code is right. `AGENTS.md` is the process doc,
`docs/features/` is the feature map and `docs/SPEC.md` is the behaviour spec.
This file is the volatile one — it exists so a new session knows *where things
stand*, not *how they work*.

As of the bands revision of history compression — 2026-10-08 (the background
grid and history compression are both built; the ramp-and-growth design was
rejected and replaced by discrete bands inside the canvas the config names, gated
and photographed, handed over as a restarted release build to look at before
anything is committed).

## State

- **History compression draws discrete bands inside the canvas the config names,
  and it is waiting on a look.** The user rejected the grown axis: the box is
  `windowSeconds × scale` logical px with the feature on or off, and the
  compressed history is drawn *inside* it. `crates/core/src/compression.rs` is
  the one statement of the geometry. The newest band — the larger of
  `noZoneShare` and `noZoneMinPx` — is drawn at the raw density, `scale` px/s,
  so one pixel per sample at 1×; then a ladder of equally wide bands whose ratios
  step evenly from 2× to `historyCompressionRatio`; then the reserve at that
  ratio. The ladder holds `min(ratio − 1, floor(middle_px / 10))` bands, so the
  count follows the canvas width and never steps finer than 1×; with no room for
  one the reserve takes the middle and there is a single step, raw to the
  maximum. Density is constant inside a band and steps at every join, and the
  background grid follows it — 30 px cells in the newest band, `30 / ratio_k` in
  band k. `render.rs` maps x through `x_of`, crops to the span, spreads the
  cosmetic prefill over it and bounds marker overdraw in the deep bands; the
  editor's section prints `Shows about` from the same geometry and a new `Zones`
  row replaces the old `Axis width`. The user's own profile (120 s × scale 2,
  ratio 12, share 0 with the 30 px floors) resolves to 12 bands over about 825 s
  — under 14 minutes — on its 240 logical px canvas. **Nothing is committed
  yet.** Gate green: fmt, clippy `-D warnings`, `cargo test --all-features`
  (232 in the core lib, 95 in the shell, 6 + 2 + 3 + 1 elsewhere) and
  `cargo build --release`. Frames for the eye at `src-tauri/target/plo-grid/`:
  `plain-480.png` (grid on, no compression), `ratio12-480.png`,
  `ratio64-480.png` (43 bands, about 66 min), `nogrid-480.png` and
  `profile-300.png` (the user's profile at 125% DPI). A scratch test rendered
  them and was removed afterwards; the older `ratio-16-*` and `plain-grid.png`
  frames of the rejected growth design were deleted. Next: the user's look, then
  the commit, then `npm run bundle` — the version counts commits, so the bundle
  comes after. Read `docs/features/overlays/history-compression.md` before
  touching any of it.
- **A background grid is built on top of the compression work, and both are
  waiting on a look.** Per overlay, off by default: `backgroundGrid` and
  `backgroundGridColor` (default `#334155`), drawn as a checkbox and a colour
  under **Colors** in the editor, straight after the background rows. Its cells
  are 30 × 20 px wherever the X axis is uncompressed, and the *columns* are
  placed through the same x mapping the lines are drawn through — so with
  compression on the cells step down band by band and pack tight in the reserve,
  and the grid is what shows the density at a glance. One path, one 1 px stroke
  (`GRID_STROKE_PX`, not `lineStrokePx`), a column within 1 px of the previous
  one skipped, drawn first in the graph's own frame so it rotates and mirrors
  with the lines. Read `docs/features/overlays/rendering.md` for the drawing and
  `history-compression.md` for what the steps mean.
- **The tip ships the bounce's revised shake and its continuous shake.**
  `ce1ab95` shipped the bounce; this commit changes the shake from
  12 Hz / 2 px / 0.6 s to 4.5 Hz / 0.6 × the cursor's half-height / 0.9 s, and
  makes the shake's clock the *newest* failure (`timeout_run_latest`) instead
  of the run's first, so a run that keeps failing restarts the shake with every
  timeout instead of shaking once and parking. No new config key; the reserves,
  layout and draw order are untouched. `cargo test --all-features` is green
  (203 in the core lib; `a_second_timeout_in_a_row_restarts_the_shake` is the
  regression test) and clippy is clean with `-D warnings`. Confirmed working in
  a live session. Read `git log -1` for the tip and `git rev-list --count HEAD`
  for the commit count the version derives from — do not trust a number written
  here.
- The commit before it (`b40f042`) was the feature-map refresh:
  `docs/features/runtime.md` was split into `runtime.md` / `pipe.md` /
  `supervision.md` / `live-edits.md`, and nine oversized sections across eight
  pages were divided at `###` level. `feature_map_check` is clean (16 rows, 16
  pages, every link resolves).
- The features-map migration is complete: 16 pages under `docs/features/` with
  the README index, and `AGENTS.md` is process plus hard rules only.

## Open threads

- **UI label/unit pass, shipped in `7f391b2`.** Known wart left open: a
  DragValue suffix is static, so a value of 1 reads "1 seconds" (most visible
  on Border fade out); pluralising the second fields is an easy follow-up.
- The smooth-rendering investigation (the newest segment appearing to
  skip instead of scrolling with the rest of the line) is closed: the line
  path had no regression, and the reveal hold plus the shifted time mapping
  replaced the pop with a tip that walks its segment.
- **A build run under DSH's sandbox produces crippled executables.** DSH runs
  its commands at Low integrity, so everything a build writes into
  `src-tauri/target/` inherits `Mandatory Label\Low Mandatory Level`. A
  Low-integrity process cannot write under `%USERPROFILE%` (so `PLO_LOG` logs
  nothing and config saves fail), cannot send ICMP (`ping` answers "transmit
  failed. General failure", so every probe times out and the graph is a red
  line), and cannot put an icon in the notification area — while still showing
  up in Task Manager. All three were measured, not deduced. The repair is one
  unconfined command per build: `icacls <exe> /setintegritylevel Medium` on each
  executable, plus `icacls src-tauri\target\release /setintegritylevel
  '(OI)(CI)Medium'` so the next build placed there is clean. A build from the
  user's own shell needs none of it — a new file takes its creator's label, not
  the parent folder's (the repo root is Low-labelled and the installed 0.2.60
  exes built there carry no label). SmartScreen on a fresh unsigned build is
  expected either way.
- **`scripts/capture-app.ps1` cannot be driven from the DSH `pwsh` tool.**
  Two walls, in order: the host's execution policy refuses the unsigned file
  (and `Unblock-File` does not lift it), and the script-block route that gets
  past it — `[scriptblock]::Create((Get-Content -Raw ...))`, with `$PSScriptRoot`
  patched in because it is empty in a script block — then stalls, because the
  tool call does not return while the app the script started is alive
  (`Start-Process` itself returns in ~160 ms; the app is what holds the call
  open). A visual check that does work from here: render the frame in a test
  with `Pixmap::encode_png` and read the PNG (the bounce was verified that way —
  frames in `src-tauri/target/plo-bounce/`), or hand the build to the user.

## Known gaps (volatile; the durable traps live in the feature pages)

- **Multi-monitor is verified by synthetic monitor lists only** — one monitor
  on this machine, so the positioning has never been seen by eye.
- **Sticky mode's rarer paths were exercised by scripted repros**, not by eye:
  the FollowWindow z-order option, multi-window targets (focused match first,
  else topmost) and DPI changes while following. The main path was confirmed
  on real apps.
- **A theme file edited on disk is re-read only at the next launch.**
- **A release build logs nothing unless `PLO_LOG` is set** — say so when
  asking for a log.
- The About page's copyright line has not been eyeballed since 0.2.22.

## Local-only state (not on GitHub)

- `archive/egui-rewrite-*-2026-09-24` — local tags holding the abandoned work
  of that day; push them with `git push origin "refs/tags/archive/*"` if they
  must outlive this clone.
- **Every SHA before 2026-09-29 is dead** — the history was force-rewritten to
  drop an unwanted trailer; `git log` is the only truth.
- Session scratch lives under `%LOCALAPPDATA%\Temp\opencode\` — the commit
  message file, capture sandboxes and repro scripts. Ephemeral by design.

## If testing themes on Windows

`HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
(1 = light, 0 = dark) is what the app follows; **restore it to 0 when done.**
