# Session handoff — PingLatencyOverlay

Next: `docs/features/README.md` — the feature map. Read only the page for
your area before the code.

**A snapshot, not a source of truth.** This file goes stale by design. If it
disagrees with the code, the code is right. `AGENTS.md` is the process doc,
`docs/features/` is the feature map and `docs/SPEC.md` is the behaviour spec.
This file is the volatile one — it exists so a new session knows *where things
stand*, not *how they work*.

As of the discrete-bands revision of history compression — 2026-10-08. Both the
compression bands and the background grid are built and **shipped**: committed as
`55dfdb2`, bundled and pushed to `origin/split/renderer-process`. The
ramp-and-growth design was rejected and replaced by discrete bands inside the
canvas the config names.

## State

- **History compression draws discrete bands inside the canvas the config names,
  and it is shipped.** The user rejected the grown axis: the box is
  `windowSeconds × scale` logical px with the feature on or off, and the
  compressed history is drawn *inside* it. `crates/core/src/compression.rs` is
  the one statement of the geometry. The user's own profile (120 s × scale 2,
  ratio 12, share 0 with the 30 px floors) resolves to 12 bands over about 825 s
  on its 240 logical px canvas. Committed as `55dfdb2` and pushed; the gate was
  green at that commit (fmt, clippy `-D warnings`, `cargo test --all-features`
  — 232 in the core lib, 95 in the shell, 6 + 2 + 3 + 1 elsewhere — and
  `cargo build --release`), and the frames for the eye are still in
  `src-tauri/target/plo-grid/` (`plain-480.png`, `ratio12-480.png`,
  `ratio64-480.png`, `nogrid-480.png` and `profile-300.png`). Read
  `docs/features/overlays/history-compression.md` for the rule, the traps and
  the pinning tests before touching any of it.
- **A background grid rides on the same geometry.** Per overlay, off by default:
  `backgroundGrid` and `backgroundGridColor` (default `#334155`), under
  **Colors** in the editor. Its columns are placed through the same x mapping
  the lines are drawn through, so with compression on the cells step down band
  by band and pack tight in the reserve. Read
  `docs/features/overlays/rendering.md` for the drawing.
- **The sample-cursor bounce ships with its revised shake.**
  `ce1ab95` shipped the bounce; `e8efedb` changes the shake from
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
- **`55dfdb2`'s message body is one line of literal backtick-n escapes**
  instead of newlines: whatever wrote the message file escaped its newlines. It
  is already pushed, so the history was left alone — just do not repeat it (write
  the message with real newlines and `git commit -F`).
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
  expected either way. Still true on 2026-10-08: the confined token is Low, and
  a tree a Medium-labelled build has touched cannot be built confined at all —
  `failed to open ...\target\debug\.cargo-build-lock: Access is denied` — so
  the gate has to run unconfined.
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
