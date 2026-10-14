# Session handoff — PingLatencyOverlay

Next: `docs/features/README.md` — the feature map. Read only the page for
your area before the code.

**A snapshot, not a source of truth.** This file goes stale by design. If it
disagrees with the code, the code is right. `AGENTS.md` is the process doc,
`docs/features/` is the feature map and `docs/SPEC.md` is the behaviour spec.
This file is the volatile one — it exists so a new session knows *where things
stand*, not *how they work*.

As of the update checker — 2026-10-13. The app now asks GitHub once a day
whether a newer release exists and says so in the tray menu, names a found
release's page, and carries the same check on the Config window's About page.
`ui.checkForUpdates` is on by default and stops only the tray's daily ask: a
click on the menu item or the About page's button is a user asking, and that
is never gated. The last answer is kept in `updates.json` beside the config,
written by the tray alone, so a restart does not buy another check. **The first
check on a live machine stalled with `ERROR_WINHTTP_TIMEOUT` (12002):** the
session opened with `WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`, whose WPAD
auto-discovery (DHCP option 252, then a `wpad` DNS lookup) cannot complete on a
machine with virtual adapters, a tunnel and no DNS suffix, and the timeouts
bite before a byte is sent. Measured repeatedly: access type 4 died, types 0
and 1 answered. It now opens with `DEFAULT_PROXY`. The endpoint also moved from
the `api.github.com` JSON to `github.com/{slug}/releases/latest`, whose
redirect WinHTTP follows and whose final URL `WinHttpQueryOption`
(`WINHTTP_OPTION_URL`) reads back — so the tag and the page to open are one
string, nothing is parsed, and the shared WARP egress address that had spent
the API's sixty-an-hour is not a factor. A failure is now `describe()`'s
sentence (`the request timed out`, `the secure connection failed`, `GitHub is
rate-limiting this network; try again later`) rather than
`OS Error 12002 (FormatMessageW() returned error 317)`. The gate is
green (`cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test --all-features` — 100 shell tests, up from 97, 269 in core, 6 + 2
+ 3 + 1 elsewhere) and `cargo build --release` succeeded (1m56s), so the three
release exes carry all three fixes; nothing is committed yet. The sample
cursor's bounce reaching the cursor shipped as `8ac13ea` and was pushed, along
with the per-host adapter binding as `1bc1705` and the compression bands and
the background grid as `55dfdb2` — all on `origin/split/renderer-process`. The
ramp-and-growth design was rejected and replaced by discrete bands inside the
canvas the config names.

## State

- **The update checker is new and uncommitted.** `crates/core/src/updater.rs` is
  the one statement of it: `parse_version` / `Version` compare by number (a tag
  reads the same with or without the `v`, and only three numbers are a
  version), `decide` offers only a release *behind* the running one so a local
  build never reads "downgrade available", and `check` is one blocking request
  of `github.com/{slug}/releases/latest` through a module-local `extern "system"`
  block — no new dependency, in the project's own-FFI style. WinHTTP follows
  the redirect and `WinHttpQueryOption` (`WINHTTP_OPTION_URL`) reads back the
  final URL, so the tag and the page to open are one string: `release_from_url`
  takes the tag after `/releases/tag/`, and a page that is not a release says
  so instead of reading "up to date". `describe()` turns a WinHTTP failure into
  a sentence (`the request timed out`, the address could not be resolved, the
  connection could not be made, the secure connection failed), because Rust
  cannot read winhttp.dll's message table and would otherwise show
  `OS Error 12002 (FormatMessageW() returned error 317)`. The session opens
  with `DEFAULT_PROXY` — **never `AUTOMATIC_PROXY` again**: its WPAD discovery
  stalls past the 5 s timeouts on this machine and the check dies before a byte
  is sent. The body is never read.
  The tray ticks it on its one-second loop but runs the check on its own thread
  and answers over an `mpsc` channel, so a bad connection cannot freeze the
  pump, the supervision or the menu; the 24 h cadence is `updater::due` reading
  `checkedAt` from `updates.json`, so a restart does not buy another check.
  `crates/tray/build.rs` sets `APP_BUILD_VERSION` (the tray's own manifest says
  `0.2.0`), and the tray is the only writer of `updates.json`. The Config
  window checks for itself when the About page is opened: `sync_update_check`
  must run *before* `sync_profiles`, because that pass is what advances
  `last_page` and the About arrival is read off that transition. `ui.checkFor-
  Updates` (default on) stops the tray's daily ask and the window's arrival
  check only — a click on the menu item or the About page's
  **Check for updates** is a user asking and is never gated, and the window
  never writes the state file. Read `docs/features/updates.md`.
- **The sample cursor's bounce waits for the break to reach it.** The gate is
  the apex, not the drawn head: the run must be live at the drawn head
  (`timeout_run_latest`) *and* the line's end — the newest drawn value — must
  have scrolled to or behind the apex's x (`axis_long - EDGE_MARGIN - size`),
  so the cursor leaves the line only when it points at the timeout indicator.
  At the live 125 s × 2 window (312 px axis) that is ~6 s into a run; a 1 s
  blip never bounces, while the blink still pulses the instant the failure is
  recorded. The shake counts from the frame the break reached the cursor — the
  anchor lives in the series' `CursorAnimation` (`bounce_anchor(now, latest)`,
  dropped by `end_bounce` when the cursor is back on the line) and `latest` is
  its restart key, so each further failure re-shakes. The rest is unchanged:
  whole against the box's top edge (topmost pixel flush), one-sided shake
  (dips into the graph), a *value* clamped at the ceiling still hangs half
  out. The base keeps its 2px gap from the graph's leading edge
  (`base_x = axis_long - SAMPLE_CURSOR_EDGE_GAP - rim / 2`); the leading
  reserve is `leading_reserve_px` (`stroke_pad` of the line's width, 2px for
  the default stroke), so the box hugs the graph — 314×136 at 125% DPI — and
  the zero-line cursor stays whole. Shipped as `8ac13ea` and pushed. Read
  `docs/features/overlays/sample-cursor.md`.
- **A host can pick the network adapter its probes leave through.** Per host:
  `TargetConfig.adapter` (an adapter GUID; absent = Automatic), with a
  "Network adapter" ComboBox in the host editor. Bound probes leave from the
  adapter's current IPv4 — `IcmpSendEcho2Ex`'s `SourceAddress` for ICMP, a
  `TcpSocket::bind` before connect for TCP — and a saved adapter that is not
  attached fails the sample rather than falling back to another interface.
  `crates/core/src/adapters.rs` is new (`GetAdaptersAddresses` through its own
  `extern "system"` block); `ping-rs` is gone, replaced by the module-local
  `IcmpSendEcho2Ex` call in `probe.rs`. Shipped as `1bc1705`; the gate was
  green (fmt, clippy `-D warnings`, `cargo test --all-features` — 251 core,
  97 shell, 6 + 2 + 3 + 1 elsewhere — and `cargo build --release`) and it was
  confirmed on a live look before the commit.
- **The support links are in.** The README's `## Support the project`, the
  About page's second clickable line (`Buy me a coffee: paypal.me/mega3dp`,
  `ABOUT_SUPPORT`) and `.github/FUNDING.yml`'s `custom` entry all point at
  `https://www.paypal.me/mega3dp`. FUNDING.yml only counts once it is on the
  default branch.
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
- **The sample-cursor bounce ships with its revised shake.** `ce1ab95` shipped
  the bounce; `e8efedb` moved the shake to 4.5 Hz / 0.6 × the cursor's
  half-height / 0.9 s. The current (uncommitted) change reworked the trigger
  and the shake's clock — see the first bullet above; no new config key.
  `a_fresh_timeout_does_not_bounce_the_cursor`,
  `a_timeout_that_reached_the_cursor_bounces`,
  `a_second_timeout_in_a_row_restarts_the_shake` and
  `the_bounced_cursor_shakes_then_settles_at_the_top` are the regression tests.
  Read `git log -1` for the tip and `git rev-list --count HEAD` for the commit
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
