# Sample cursor — a per-host triangle that points at the head of each line

Status: shipped (v0.2.37) · Read when: changing `draw_sample_cursor`,
`line_y_at_x`, `CursorAnimation`, the bounce, the cursor reserves, or the
Colors rows.

## What it does
- One white triangle with a dark rim per host line, anchored to the leading
  edge of the graph. The base sits flush at the edge; the apex points back
  along the line, one size behind the edge.
- The apex sits on the **drawn** line at that x — interpolated between
  samples (a spike's flank, the resume jog after a timeout) — not on the
  nearest sample's value.
- It eases (~150 ms) to a new value instead of jumping, and asks for repaints
  while it moves, so it animates in index mode too.
- When a live timeout reaches the point it is drawn from, it leaves the line
  for the top of the graph: half the triangle hangs out of the overlay's edge,
  it shakes for ~0.9 s, and it waits there until a value arrives. Each further
  timeout in the run restarts the shake, so a host that keeps failing keeps
  shaking rather than shaking once and parking. A *stale* timeout — a host that
  stopped probing — holds the last drawn value instead, and during the startup
  prefill it rides the prefill head.
- Default on, including a profile written before the setting existed (absent
  key = on), unlike the underglow.
- With the timeout blink on, the cursor pulses in its own colour while the
  newest sample is a failure: dim → bright → dim every 2 s, starting the
  moment the failure is probed rather than when smooth rendering reveals it,
  and back to white the moment a value arrives.

## Map
- `render.rs`: `draw_sample_cursor` (with `CursorLook`), `line_y_at_x` (with
  its `cover` helper; the caller hands it the drawn cut), `drawn_samples`,
  `CursorAnimation { y, at, active }` + `advance(now, target, shake)` /
  `is_active`, `Series.cursor: Option<&'a mut CursorAnimation>`, the cursor
  pass in `render_graph_into_internal`, `SAMPLE_CURSOR_FILL` /
  `SAMPLE_CURSOR_RIM` / `SAMPLE_CURSOR_RIM_WIDTH` / `SAMPLE_CURSOR_EDGE_MARGIN`,
  `CURSOR_EASE_SECS` / `CURSOR_EASE_EPSILON`, `sample_cursor_size` /
  `sample_cursor_half_height` / `sample_cursor_reserve_px` /
  `sample_cursor_room_px`; the bounce's `cursor_shake_offset`, `CURSOR_BOUNCE_Y`
  / `CURSOR_BOUNCE_SHAKE_SECS` / `CURSOR_BOUNCE_SHAKE_HZ` /
  `CURSOR_BOUNCE_SHAKE_FRACTION`; the run's `timeout_run_anchor` and
  `timeout_run_latest`,
  `CURSOR_TIMEOUT_BLINK_PERIOD` / `CURSOR_TIMEOUT_BLINK_FLOOR`.
- `overlay.rs`: `WindowSeries.cursor` and `.max_sample_gap`, the `cursor_due`
  and `blink_due` terms in the `apply` render gate, `cursor_repaint_interval`
  and `timeout_blink_repaint_interval` (both chained in `bin/renderer.rs`'s
  `repaint_interval`).
- `config.rs`: `sample_cursor`, `sample_cursor_size_px`,
  `cursor_timeout_blink`, `cursor_timeout_blink_color`, the size/blink consts
  and the `normalize` clamp.
- `ui.rs`: the Colors pane's "Sample cursor" checkbox, "Cursor size" slider,
  "Blink on timeout" checkbox and "Blink color" picker.

## How & why

- Graph-space geometry, all three vertices through `transform_point`, so it
  rotates and mirrors with the line: apex `(base_x - size, y)`, base corners
  `(base_x, y ± size * 0.7)`, where `base_x = long_px - SAMPLE_CURSOR_EDGE_MARGIN`.
- `line_y_at_x` walks the series with `draw_series`' own state machine —
  connectors, resume stubs, gaps — so the apex cannot drift from the drawn
  line; the last covering segment wins and the fallback is the newest drawn
  valued sample (the hold rule). It is handed the drawn cut rather than
  applying it, so the walk and the bounce read the same frame.
- The easing state lives per series in `WindowSeries` and is threaded to the
  renderer as `Series.cursor`; the pure test path passes `None` and draws the
  target directly. Exponential ease with τ = 0.15 s; the first frame and any
  long gap snap; an epsilon of 0.25 px settles it (`active = false`). A shake
  is added to the eased position, never eased itself, and keeps `active` true.
- Repaints: `apply`'s `cursor_due` and `cursor_repaint_interval` use the
  border clock (~16.7 ms) while any cursor is easing — index mode has no frame
  clock of its own and would otherwise move only when a sample arrives.
- The timeout blink reads the raw samples, not the reveal-filtered slice, so
  a failure starts pulsing the moment it is recorded. `timeout_run_anchor` —
  the detector the blink counts from — requires the newest sample it
  is shown to be a failure no older than `max_sample_gap` (a host that stopped
  probing stops blinking), walks back over the contiguous run and returns its
  first timestamp; the fill is the blink colour scaled from 35% to 100% by
  `0.5 − 0.5·cos(2π·(now − anchor) / 2 s)`, and a value sample restores white.
  `blink_due` in `apply` and `timeout_blink_repaint_interval` in the renderer's
  chain supply the frames while the run lives.

### The bounce

- The trigger is the **drawn head**, not the probe: `head = reveal.unwrap_or(now)`,
  and the run is read from `drawn_samples(samples, reveal)` — the same cut the
  line is drawn to — so the cursor leaves the line when the break reaches it.
  The blink deliberately does the opposite, reading the raw samples at the wall
  clock; [smooth-rendering.md](smooth-rendering.md) keeps both rules.
- The liveness rule is the age check: the newest drawn sample must be a failure
  no older than `max_sample_gap` (`timeout_run_latest`), and the walk back over
  contiguous failures gives the run's first timestamp (`timeout_run_anchor`,
  which the blink counts from). A host that stopped probing (profile switched
  away, Pause on) fails the check, and the cursor goes back to holding the last
  drawn value.
- The shake's clock is `head − latest`, where `latest` is the newest failure the
  drawn head has reached (`timeout_run_latest`), not the run's first: zero on
  the frame a failure arrives, and restarted by each further failure so a run
  that keeps failing keeps shaking. It is the same number in index and smooth
  mode. The blink keeps `timeout_run_anchor`'s oldest timestamp, so its pulse
  does not reset mid-run. `cursor_shake_offset` is a decaying
  sine — `0.6 · half-height · (1 − t / 0.9 s) · sin(2π · 4.5 Hz · t)` — zero at
  both ends, so starting or stopping it cannot step the cursor. A run already in
  progress when the cursor appears (a hidden overlay shown mid-run, the cursor
  switched on) shakes only for what is left of the newest failure's window;
  once that has passed it is drawn parked.
- **The shake's numbers are unexciting on purpose.** Shipped first at 12 Hz and
  2 px, it was invisible in the running app: a 30 fps overlay samples a 12 Hz
  sine on unrelated phases, so it reads as a twitch, and 2 px against a 12 px
  triangle is a wobble nobody sees. The amplitude now follows the cursor's
  half-height — a bigger triangle shakes further — at 0.6 of it, which keeps a
  sliver of white on the canvas at the far end of the swing; at the full
  half-height only the dark rim is left up there, which against the background
  reads as the cursor blinking out rather than bouncing.
- The rest position is `CURSOR_BOUNCE_Y = 0`, the canvas edge in graph
  coordinates, so half the triangle is clipped by the window. It is
  deliberately outside the room `sample_cursor_room_px` keeps at the ceiling:
  that room is for a *value* clamped at the ceiling, and the bounce is not one.
- The shake rides on top of the ease and sets `active`, which is why
  `cursor_due` and `cursor_repaint_interval` needed no new term: the ease
  settles in ~150 ms while the shake runs for 600 ms, and both gates already
  read that flag.

### The reserves, and the draw order

- Two reserves, read by both `overlay::layout_in_rect` and the renderer:
  `sample_cursor_reserve_px` is a constant 2 px gutter at the leading edge
  (edge margin + half the rim), and `sample_cursor_room_px` (`ceil(size * 0.7
  + rim / 2 + 1)`) pads the ceiling end of the latency axis. The zero-line
  end shares its band with the glow's reserve, so `layout_in_rect` keeps
  `max(glow reserve, cursor room)` there rather than adding both; a cursor on
  0 ms or clamped at the ceiling stays whole either way.
- Draw order: after the cores, before the border, per host. At the minimum
  size 4 the triangle is mostly rim — cosmetic, not a bug.

## Config keys / UI
| Key | Default | Range / clamp | Control |
| --- | --- | --- | --- |
| `sampleCursor` | on (new and absent) | on/off | Colors → Sample cursor |
| `sampleCursorSizePx` | 10 | 4–20 | Colors → Cursor size |
| `cursorTimeoutBlink` | off (new and absent) | on/off | Colors → Blink on timeout |
| `cursorTimeoutBlinkColor` | `#ef4444` | hex | Colors → Blink color |

The bounce has no key of its own: it is part of the cursor and follows
`sampleCursor`.

## Tests that pin it
`the_sample_cursor_sits_flush_at_the_leading_edge`,
`the_sample_cursor_apex_moves_with_its_size`,
`the_cursor_bounces_off_the_top_when_the_timeout_reaches_it`,
`the_bounced_cursor_shakes_then_settles_at_the_top`,
`a_second_timeout_in_a_row_restarts_the_shake`,
`a_value_sample_brings_the_cursor_back_onto_the_line`,
`the_bounce_waits_for_the_reveal_hold_in_smooth_mode`,
`a_stale_timeout_does_not_pin_the_cursor_to_the_top`,
`the_shake_starts_and_ends_at_rest`,
`a_cursor_on_the_zero_line_or_the_ceiling_stays_whole`,
`the_cursor_animation_eases_toward_its_target`,
`the_cursor_animation_keeps_its_clock_while_the_shake_lasts`,
`the_sample_cursor_reserves_room_on_both_axes`,
`the_timeout_blink_pulses_dim_then_bright`,
`the_timeout_blink_starts_before_smooth_rendering_reveals_the_failure`,
`a_value_sample_ends_the_timeout_blink`,
`the_timeout_run_anchor_is_the_start_of_the_failed_run`,
`cursor_timeout_blink_defaults_off_and_round_trips`.

## Related
[rendering.md](rendering.md) · [glow.md](glow.md) ·
[smooth-rendering.md](smooth-rendering.md)
