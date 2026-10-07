# The pipe — the protocol the three processes meet over

Status: shipped (0.2.x) · Read when: touching `transport.rs`, the wire
format, the server loop, a message kind, or the single-instance mutexes.

## What it does
- The processes find each other as siblings of their own executable and meet
  over one named pipe; there is no registry, lock file or heartbeat.
- The pipe carries `set_config`, `set_paused`, `set_border_preview`, `ping`
  and `shutdown`; both ends compile the protocol from the same module.
- The server answers several clients at once and keeps a spare instance
  waiting, so a second caller is never a defect.

## Map
- `crates/core/src/transport.rs` — `Message` (`#[serde(tag = "kind")]`),
  `SingleInstance::acquire(Role)`, `sibling_exe`, `LEGACY_EXE_NAMES`, the
  client and the `serve` loop.
- `crates/core/src/bin/renderer.rs` — the serving side's answer to a config,
  a pause or a shutdown.
- Tests: `each_role_gets_its_own_versioned_mutex`,
  `connecting_with_nothing_listening_fails_quickly`.

## How & why
### The wire format
- `transport.rs` owns the wire format and **both ends are compiled from the
  same crate**, so the two processes cannot disagree without a compile error.
  Messages are `set_config`, `set_paused`, `set_border_preview`, `ping` and
  `shutdown`, tagged `#[serde(tag = "kind")]` so an unknown kind is
  **rejected rather than silently ignored**. `ping` is a no-op and nothing may
  act on it; borrowing `set_paused` for a probe would resume the probes
  whenever the sender's mirror was stale.
- **Framing is one JSON object per line, and the newline is not optional.** A
  pipe is a byte stream, a length prefix is the usual source of off-by-one
  bugs, and JSON escapes its own newlines.
### The server
- **The server serves several clients at once** — `serve` spawns two
  `instance_loop`s, each holding one instance and making the next when it
  finishes with a client, so a spare is always waiting. A rendezvous designed
  for one caller and handed two is a defect, not a configuration.
- **A fresh instance per connection.** Windows returns a stale
  `ERROR_PIPE_CONNECTED` (a success to tokio) rather than blocking on an
  instance a client has disconnected from, so reusing one spins at 8% CPU
  with no sleep on any leg. Only the first instance may claim
  `first_pipe_instance`; the renderer mutex, not that flag, is what guarantees
  one renderer. **No test can catch this** — only the OS shows you a spin.
### Finding each other
- **The pipe is the rendezvous**, with no registry, lock file or heartbeat,
  which is what makes "run without the tray, bring one later" free.
  `SingleInstance::acquire(Role)` uses a per-**role** mutex so the tray can
  exit while the window stays open and a crashed renderer is replaceable;
  three roles means three pairs, and `each_role_gets_its_own_versioned_mutex`
  checks all of them, because comparing only two lets the third share one.
  `Ok(None)` means somebody else holds it — the ordinary "ran it twice"
  answer, not an error.
- Each process finds the others as **siblings of its own executable**
  (`sibling_exe`), so all three must be installed into one directory and
  `build-nsis.ps1` checks all three exist before packaging.
- A client handle is opened `GENERIC_READ | GENERIC_WRITE`, and a liveness
  check must not be able to block. `PeekNamedPipe` queries rather than
  transfers, but it **needs read access**: a write-only handle answers every
  query with `ERROR_ACCESS_DENIED`, which reads as "the renderer is gone".
  Before adding a query here, check the handle can answer it.
- `ERROR_FILE_NOT_FOUND` maps to `io::ErrorKind::NotFound`, distinct from
  every other failure, because it is the answer that says *start one*.
  Lumping the ordinary answer in with a fault is what makes a working system
  look broken.
## Config keys / UI
No key and no control: the protocol is compiled into both ends, so the
message kinds are the only schema and `transport.rs` is the only copy.

## Tests that pin it
- `each_role_gets_its_own_versioned_mutex` — all three role mutexes distinct.
- `connecting_with_nothing_listening_fails_quickly` — the NotFound answer.

## Related
- runtime.md — the three processes and who launches whom.
- supervision.md — the watch kept on the connection.
- live-edits.md — the `set_config` path in detail.
- Tray — the other client, and the process that supervises.
