# Session handoff — PingLatencyOverlay

Next: `docs/features/README.md` — the feature map. Read only the page for
your area before the code.

**A snapshot, not a source of truth.** This file goes stale by design. If it
disagrees with the code, the code is right. `AGENTS.md` is the process doc,
`docs/features/` is the feature map and `docs/SPEC.md` is the behaviour spec.
This file is the volatile one — it exists so a new session knows *where things
stand*, not *how they work*.

As of `615f704` — 2026-10-06 (two uncommitted glow-room changes in the
working tree, awaiting the visual check).

## State

- The tip is pushed and the tree holds two uncommitted glow-room changes:
  the glow's reach scales with its intensity and the reserve follows it, and
  the room below the zero line is now shared with the cursor instead of
  summed (any cast reserves `ceil(reach + 0.5)`; the bottom keeps
  `max(reserve, cursor_room)`). Built and captured; awaiting the user's
  visual check before commit. Read `git log -1` for the tip and
  `git rev-list --count HEAD` for the commit count the version derives from —
  do not trust a number written here.
- Tests: 293 across the workspace. Last bundle:
  `PingLatencyOverlay_0.2.39_x64-setup.exe` (commit `7468388`); the next
  release build takes its version from the commit count.
- The features-map migration is complete: 13 pages under `docs/features/` with
  the README index, and `AGENTS.md` is process plus hard rules only.

## Open threads

- **Glow reach and shared room, uncommitted.** `glow_reach_px` = `radius ·
  √(intensity/100)`, `line_glow_reserve_px` = `ceil(reach + 0.5)` (0 when no
  band survives the 3/255 floor), and both the renderer and the reserve read
  `glow_bands`; below the zero line the window keeps `max(reserve,
  cursor_room)` rather than their sum. At the live settings R25/I20 reserves
  12px where it used to reserve 26, and the maxed Default overlay is 312x135
  where it was 312x144; full strength is still radius + 1. Release build is in
  `src-tauri/target/release`; the capture is in
  `%LOCALAPPDATA%\Temp\opencode\plo-capture-glow\`. Commit after the user
  confirms, then bundle.
- The smooth-rendering investigation (the newest segment appearing to
  skip instead of scrolling with the rest of the line) is closed: the line
  path had no regression, and the reveal hold plus the shifted time mapping
  replaced the pop with a tip that walks its segment.

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
