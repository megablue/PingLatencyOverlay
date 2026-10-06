# Sample cursor — a per-host triangle that points at the head of each line

Status: shipped (v0.2.37) · Read when: changing `draw_sample_cursor`,
`line_y_at_x`, `CursorAnimation`, the cursor reserves, or the Colors rows.

## What it does
- One white triangle with a dark rim per host line, anchored to the leading
  edge of the graph. The base sits flush at the edge; the apex points back
  along the line, one size behind the edge.
- The apex sits on the **drawn** line at that x — interpolated between
  samples (a spike's flank, the resume jog after a timeout) — not on the
  nearest sample's value.
- It eases (~150 ms) to a new value instead of jumping, and asks for repaints
  while it moves, so it animates in index mode too.
- Through a timeout it holds the last drawn value; during the startup prefill
  it rides the prefill head.
- Default on, including a profile written before the setting existed (absent
  key = on), unlike the underglow.

## Map
- `render.rs`: `draw_sample_cursor`, `line_y_at_x` (with its `cover` helper),
  `CursorAnimation { y, at, active }` + `advance(now, target)` / `is_active`,
  `Series.cursor: Option<&'a mut CursorAnimation>`, the cursor pass in
  `render_graph_into_internal`, `SAMPLE_CURSOR_FILL` / `SAMPLE_CURSOR_RIM` /
  `SAMPLE_CURSOR_RIM_WIDTH` / `SAMPLE_CURSOR_EDGE_MARGIN`, `CURSOR_EASE_SECS`
  / `CURSOR_EASE_EPSILON`, `sample_cursor_size` / `sample_cursor_half_height`
  / `sample_cursor_reserve_px` / `sample_cursor_room_px`.
- `overlay.rs`: `WindowSeries.cursor`, the `cursor_due` term in the `apply`
  render gate, `cursor_repaint_interval` (chained in `bin/renderer.rs`'s
  `repaint_interval`).
- `config.rs`: `sample_cursor`, `sample_cursor_size_px`, the size consts and
  the `normalize` clamp.
- `ui.rs`: the Colors pane's "Sample cursor" checkbox and "Cursor size" slider.

## How & why
- Graph-space geometry, all three vertices through `transform_point`, so it
  rotates and mirrors with the line: apex `(base_x - size, y)`, base corners
  `(base_x, y ± size * 0.7)`, where `base_x = long_px - SAMPLE_CURSOR_EDGE_MARGIN`.
- `line_y_at_x` walks the series with `draw_series`' own state machine —
  connectors, resume stubs, gaps — so the apex cannot drift from the drawn
  line; the last covering segment wins and the fallback is the newest revealed
  valued sample (the timeout rule).
- The easing state lives per series in `WindowSeries` and is threaded to the
  renderer as `Series.cursor`; the pure test path passes `None` and draws the
  target directly. Exponential ease with τ = 0.15 s; the first frame and any
  long gap snap; an epsilon of 0.25 px settles it (`active = false`).
- Repaints: `apply`'s `cursor_due` and `cursor_repaint_interval` use the
  border clock (~16.7 ms) while any cursor is easing — index mode has no frame
  clock of its own and would otherwise move only when a sample arrives.
- Two reserves, read by both `overlay::layout_in_rect` and the renderer:
  `sample_cursor_reserve_px` is a constant 2 px gutter at the leading edge
  (edge margin + half the rim), and `sample_cursor_room_px` (twice the
  `ceil(size * 0.7 + rim / 2 + 1)`) pads both ends of the latency axis so a
  cursor on 0 ms or clamped at the ceiling stays whole.
- Draw order: after the cores, before the border, per host. At the minimum
  size 4 the triangle is mostly rim — cosmetic, not a bug.

## Config keys / UI
| Key | Default | Range / clamp | Control |
| --- | --- | --- | --- |
| `sampleCursor` | on (new and absent) | on/off | Colors → Sample cursor |
| `sampleCursorSizePx` | 10 | 4–20 | Colors → Cursor size (px) |

## Tests that pin it
`the_sample_cursor_sits_flush_at_the_leading_edge`,
`the_sample_cursor_apex_moves_with_its_size`,
`the_sample_cursor_stays_on_the_last_drawn_value_through_a_timeout`,
`a_cursor_on_the_zero_line_or_the_ceiling_stays_whole`,
`the_cursor_animation_eases_toward_its_target`,
`the_sample_cursor_reserves_room_on_both_axes`.

## Related
[rendering.md](rendering.md) · [glow.md](glow.md) ·
[smooth-rendering.md](smooth-rendering.md)
