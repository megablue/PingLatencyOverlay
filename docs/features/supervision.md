# Supervision — the tray's watch over the renderer

Status: shipped (0.2.x) · Read when: touching restart or storm-guard logic,
`App::starting`, or a renderer that will not come back.

## What it does
- The tray supervises the renderer through the pipe; the window is a client,  that starts one at launch if nothing else has.
- A lost pipe is the signal, not a child handle: a hung renderer still holds
  its mutex while answering nothing.
- Only a process this one spawned is ever stopped, and only the tray owns the
  restart.
- Five restarts in a minute is the limit; past it the tray gives up and says
  so in the log.

## Map
- `crates/tray/src/main.rs` — the supervision loop: pipe client, storm guard
  (`should_restart(now)`, `forget_expired`), start/stop, `renderer_process`.
- `src/ui.rs` — the window's half: `App::starting` / `finish_starting`,
  `reconnect_renderer`, `toggle_running`.
- Tests: `a_crash_loop_is_stopped_and_a_rare_failure_is_not` (the tray's
  supervision test).

## How & why
### Who owns the restart
- **The tray supervises the pipe, not the child handle.** A lost pipe is a
  better signal than a child, because a hung renderer still holds its mutex
  while answering nothing.
- **Only stop a process you spawned.** `renderer_process` is set only when
  this process started the renderer, which also governs the `Shutdown` on
  `Drop` and the tray's `Exit`. A blanket kill takes an overlay set the user
  still wants off their screen.
- **A client is not a supervisor, and the restart half must not travel with
  the start half.** The window starts the renderer once at launch if nothing
  else has; the tray owns it from then on. When the window also gained a
  restart path they fought and neither could win. The window's
  `reconnect_renderer` reconnects and never spawns, and carries no storm guard
  — a storm guard stops a crash loop, and this is not the process that would
  be looping.
### The grace period and the guard
- **Bringing a renderer up and noticing a crash are two different states, and
  only the second costs an attempt.** `App::starting` is an `Option<Instant>`;
  while set, `finish_starting` retries the pipe and the config push and
  nothing counts against the guard. `START_TIMEOUT` (10s) ends the grace
  period.
- **Never discard a send error.** Both fire-and-forget pushes used
  `let _ = self.send(..)`, so a command that never left the process looked
  exactly like one that was obeyed. `finish_starting` retries the config push
  until it lands; `toggle_running` logs both halves.
- **The storm guard's clock is a parameter.** `should_restart(now)` takes an
  `Instant` so a test can drive a whole crash history in microseconds, and
  `forget_expired` is split out so forgetting is a statement about the list.
  The tray owns the guard; a renderer with no tray is not restarted.
- **Say what actually happened.** Writing the profile and telling the renderer
  can fail separately, so `persist_current` reports "Saved, but the overlays
  were not updated" rather than a confident "Saved."
## Config keys / UI
No UI and no config key: the guard's constants are compiled in.

| Constant | Value | Meaning |
| --- | --- | --- |
| `START_TIMEOUT` | 10 s | how long a started renderer may take to answer its pipe before the attempt is abandoned |
| `RESTART_LIMIT` | 5 | restarts allowed inside `RESTART_WINDOW` |
| `RESTART_WINDOW` | 60 s | how long a restart is remembered; past the limit the tray gives up and logs it |

## Tests that pin it
- `a_crash_loop_is_stopped_and_a_rare_failure_is_not` — the storm guard.
- `the_window_engine_holds_a_switch_until_the_drafts_are_resolved` — the
  window's half of a restart (ui.rs).

## Related
- runtime.md — the three processes and who launches whom.
- pipe.md — what a lost pipe is measured on.
- live-edits.md — the config push the grace period retries.
- tray.md — the resident process that owns this loop.
