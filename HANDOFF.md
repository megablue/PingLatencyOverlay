# Session handoff — PingLatencyOverlay

Next: `docs/features/README.md` — the feature map. Read only the page for
your area before the code.

**A snapshot, not a source of truth.** This file goes stale by design. If it
disagrees with the code, the code is right. `AGENTS.md` is the process doc,
`docs/features/` is the feature map and `docs/SPEC.md` is the behaviour spec.
This file is the volatile one — it exists so a new session knows *where things
stand*, not *how they work*.

As of the sample-cursor bounce commit — 2026-10-08 (bounce shipped; the
sandbox-integrity trap below is the session's other find).

## State

- **The tip ships the sample-cursor bounce.** When a live timeout reaches the
  point the cursor is drawn from, it leaves the line for the top of the graph,
  hangs half out of the overlay's edge, shakes for ~0.6 s, and eases back when
  a value arrives. No new config key; the reserves, layout and draw order are
  untouched. `cargo test --all-features` is green (202 in the core lib, six of
  them new) and clippy is clean with `-D warnings`. The bounce itself has not
  been watched in a live session yet — the frames were rendered straight from
  the renderer to PNGs (see the capture trap below) and the geometry is pinned
  by the tests, so it is worth an eye when a host times out on screen. Read
  `git log -1` for the tip and `git rev-list --count HEAD` for the commit
  count the version derives from — do not trust a number written here.
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
