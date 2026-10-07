# Live edits — how an edit reaches the renderer without a Save

Status: shipped (0.2.x) · Read when: touching `sync_runtime_config`,
`push_config`, `persist_current`, or an appearance edit that only shows up
on Save.

## What it does
- Appearance edits reach the renderer without a Save; Pause, Exit and the
  border preview travel as messages like everything else.
- An unchanged frame costs a field comparison and no send; a changed one is
  one pipe write and one config parse.

## Map
- `src/ui.rs` — `sync_runtime_config` (called from `logic()` every frame),
  `push_config`, `persist_current`, the `last_pushed` record,
  `sync_border_preview` / `release_border_preview`.
- `crates/core/src/transport.rs` — `set_config`, `set_paused` and
  `set_border_preview`, the kinds this path uses.
- Tests: `an_out_of_range_draft_settles_instead_of_resending`.

## How & why
### An edit must not wait for Save
- **An overlay edit reaches the renderer without a Save.** Before the renderer
  was its own process, `logic()` called `sync_overlays()` on **every frame**
  and the in-process overlay manager re-read `self.config`, so any staged
  change was on screen immediately. `a2e0404` turned that per-frame call into
  a pipe message and put the message only on the save paths — so every
  appearance edit silently became save-only, and the only edit that stayed
  live was the list pane's enable toggle, because `ac58523` had added an
  explicit call for that one action. **The regression was silent**: the
  overlay still appeared on Save, so nothing failed and the app just stopped
  feeling live. If live updating ever looks broken, check
  `sync_runtime_config` is still called from `logic()` before suspecting the
  pipe.
### Normalize before comparing

- **`sync_runtime_config` normalizes the draft IN PLACE before comparing, and
  that order is load-bearing twice over.** Compared the other way — a
  normalized `last_pushed` against an un-normalized draft — the two never
  match, so the window sends the whole configuration on **every frame** for
  as long as it stays open. That is not slow enough to look wrong: it is a
  pipe write and a full config parse ten times a second, forever, with no
  symptom. It also makes the drag story right, since a clamped value is what
  Save would write anyway. `an_out_of_range_draft_settles_instead_of_resending`
  counts sends over 20 frames, which is the only way that bug is visible —
  there is nothing to assert against except the count.
### The record of what was sent

- **The comparison is the affordability argument, so do not "simplify" it
  into an unconditional push.** `Config` derives `PartialEq`, so an unchanged
  frame costs a field comparison and *no clone*; the clone only happens when
  there is something to send.
- **`last_pushed` must be updated by every path that sends a config**, and
  `persist_current` is the exception that proves it: it sends directly rather
  than through `push_config` because it has to tell "written to disk" from
  "renderer told" apart, and it updates the record **only on success** so a
  failed send leaves `sync_runtime_config` still wanting to try.
### What travels as a message
- Neither the tray nor the window has a `ProbeManager` or `OverlayManager`, or
  a tokio runtime. Anything the renderer needs travels over the pipe —
  **including the animated border preview**, which used to be a direct call
  and is now `set_border_preview`, sent only when it changes, and cleared by
  `release_border_preview` on close.
## Config keys / UI
No key of its own: every appearance control the window already has travels
this path. `docs/SPEC.md` is the user-visible behavior and config/storage.md
owns the keys behind them.

## Tests that pin it
- `an_out_of_range_draft_settles_instead_of_resending` — the send count over
  20 frames, which is the only way the resend bug is visible.

## Related
- config/window.md — the drafts and the controls that stage them.
- config/storage.md — what a Save writes, and the keys involved.
- pipe.md — the protocol the message travels over.
- supervision.md — the retry that covers a renderer not yet listening.
