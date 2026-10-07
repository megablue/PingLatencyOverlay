# Runtime — the three processes and who launches whom

Status: shipped (0.2.x) · Read when: touching process launch, the launch
matrix, the window message pump, or which process owns what.

## What it does
- `plo-tray.exe` is what a user launches and the only process that stays
  resident; `plo-config.exe` starts on demand and **exits when closed**;
  `plo-renderer.exe` draws the overlays. Two of the three carry no eframe at
  all — the GL context exists only while the window is open, so close means
  exit, never hide.
- The tray supervises the renderer through the pipe; the window is a client
  that starts one at launch if nothing else has (supervision.md).
- The processes meet over one named pipe, and appearance edits travel it
  without a Save (pipe.md, live-edits.md).

## Map
- `src/main.rs`, `src/ui.rs` — the window's client side: launch-once,
  `App::starting` / `finish_starting`, `reconnect_renderer`,
  `toggle_running`.
- `crates/tray/src/main.rs` — the resident process and its start/stop of the
  renderer; the supervision loop itself is supervision.md.
- `crates/core/src/bin/renderer.rs` — the window message pump and the redraw
  loop; the serving side of the pipe is pipe.md.
- `crates/core/tests/no_gui_dependencies.rs` — the GUI deny list, run over
  `cargo tree`.
- `crates/core/src/diagnostics.rs` — the log file and the startup
  `MessageBox` (see **Diagnostics** in the index).
- Tests: the `should_start` launch matrix over all nine cells.

## How & why
### The three processes
- **No package may reach a GPU context or an event loop.** That is the
  property the memory win depends on, and
  `crates/core/tests/no_gui_dependencies.rs` checks it with `cargo tree`. The
  deny lists are two-tier because the tray *is* `tray-icon`; the rule is named
  for what it forbids, not for the crates it happens to list. `tray-icon` is a
  GUI crate and belongs to `crates/tray` alone — the application shell once
  depended on it and nothing in `src/` used it, which is how a dead dependency
  hides in a manifest for a release.
- The renderer needs no package of its own — it uses nothing the library does
  not already depend on, and `cargo tree` reports a package's deps across
  every target, so the test guards the binary for free. The tray does need
  one: a `[[bin]]` beside the window would link eframe into the resident
  process.
- The binaries are named tersely on purpose: they sit side by side in the
  install folder and Task Manager, and a name long enough to abbreviate is
  hard to tell from its neighbour. Crates keep descriptive names; the product
  name, installer, Start Menu folder and window title are all still
  PingLatencyOverlay. `LEGACY_EXE_NAMES` in `transport.rs` holds the
  pre-rename names and the installer kills them, because a file cannot be
  deleted while its process holds it open.
- `default-run` is set on the root package; `cargo run` with two binaries is
  ambiguous exactly where a developer starts the app. The window is the
  default because it starts whatever it needs.
### Launching
- **Every process is `windows_subsystem = "windows"`, so `eprintln!` goes
  nowhere.** `diagnostics.rs` is the answer: `log_line` appends to
  `pinglatencyoverlay.log` beside the config, and `fatal` puts up a
  `MessageBox` for the tray's startup failures, the tray being the one process
  that never draws a window. **Any `return` on a tray error path needs one of
  the two** — a bare `return` is invisible forever. The tray also logs a
  heartbeat, because a wedged loop and a feature that was never written look
  identical from outside. **A release build writes nothing unless `PLO_LOG` is
  set** (`log_enabled`; a debug build logs as before), so the "one of the two"
  rule has a release reading: a log line is silent for an ordinary install,
  and anything the user must see has to be a `fatal`.
- The launch matrix is `should_start(me, missing)`, tested over all nine
  cells: a process never starts itself, the renderer starts nothing, the tray
  does not open the window on its own, and everything else missing gets
  started. It is a function because three bugs came from the rule living in
  three places and none agreeing.
### Windows
- **Any thread in ANY of our processes that creates a window must pump that
  thread's messages.** A tray icon is a window too. Windows delivers a window
  message only to a thread that pumps its queue, so a thread that sleeps
  receives *nothing* and the symptom is a window that looks perfect and is
  completely dead. `tray-icon` documents this ("an event loop must be running
  on the thread") and spawns no pump on Windows, so the caller must;
  `pump_messages()` is ours. **This shipped twice, once per process** — so
  grep for `windows_subsystem = "windows"` before assuming a new process is
  exempt. A rule that names one caller is a rule the next caller will not
  find.
- **The repaint cap and the liveness cap are different decisions.**
  `MESSAGE_POLL_INTERVAL` (16ms) is independent of `REPAINT_INTERVAL` (100ms);
  tying them together would mean a slow redraw is also an unresponsive
  process. The wait is `min(deadline, cap)` via `wait_before`, and a pipe
  command resets the deadline. Only a user can confirm Windows stops calling
  the process hung.
- **Letting the default handler run can be worse than ignoring the message.**
  `DefWindowProcW` on `WM_CLOSE` *destroys* the window and the loop rebuilds
  it, so Task Manager's "End task" concludes its graceful close worked and
  never escalates. `overlay_wnd_proc` handles `WM_CLOSE | WM_QUERYENDSESSION`
  by setting a flag and returning `0` without calling the default, and the
  loop checks `quit_requested()` right after `pump_messages`. The flag is a
  static because a window procedure cannot stop the loop that called it. Any
  `WM_*` we do not handle must be checked against this: the default is not a
  safe place to land.
## Config keys / UI
No UI. Environment variables read by the processes:

| Variable | Effect |
| --- | --- |
| `PLO_CONFIG_DIR` | overrides the config root the process reads and writes |
| `PLO_LOG` | enables the release build's log output |
Both are owned elsewhere: `PLO_LOG` by diagnostics.md, `PLO_CONFIG_DIR` by
config/storage.md.

## Tests that pin it
- The `should_start` matrix (all nine cells) — who starts whom.

## Related
- pipe.md — the protocol the three processes meet over.
- supervision.md — who watches whom, and when a restart is charged.
- live-edits.md — how an edit reaches the renderer without a Save.
- Tray — the resident process, its menu and the rules engine.
- Diagnostics — the log file, `PLO_LOG` and the fatal box.
- Overlays — rendering — what the renderer draws and how the box is sized.
- See `README.md` for the full index.
