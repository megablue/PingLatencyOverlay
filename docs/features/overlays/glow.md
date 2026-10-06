# Glow — the underglow cast under each line

Status: shipped (v0.2.33, swept in v0.2.36) · Read when: touching the glow code
in `render.rs`, its reserve in `overlay.rs`, or the Colors pane rows.

## What it does

- Per overlay, every host line casts a soft glow in its **own** `lineColor`, and
  the startup prefill casts in `prefillLineColor`.
- The cast falls toward the zero line in the graph's **own** frame: it rotates
  and mirrors with the overlay instead of staying screen-down.
- Intensity 0–100 (default 10) and reach 2–50 px (default 30), both per
  overlay. A new overlay starts with the glow on; a profile written before the
  setting existed has no `lineGlow` key and reads as off.
- The cast never appears on the far side of a line — however sharp a spike, no
  glow wraps over its peak — and no host's glow can tint another host's line.

## Map

- `render.rs`: `LineGlow { radius, intensity, dir }`; `LinePass { Glow, Core }`;
  `stroke_run`; `glow_layer_count`; `GLOW_BAND_WIDTH`; `GLOW_BAND_STEP`;
  `glow_direction` and `rotation`; `line_glow_reserve_px`; the `glow` and
  `pass` arguments of `draw_series`, and the `draw_lines(pass)` closure in
  `render_graph_into_internal`.
- `config.rs`: `line_glow`, `line_glow_intensity`, `line_glow_radius_px`, the
  `DEFAULT_/MIN_/MAX_LINE_GLOW_*` consts, and the clamps in `normalize`.
- `overlay.rs`: `layout_in_rect` grows the window by the reserve.
- `ui.rs`: the Colors pane's Line glow checkbox, Glow intensity and Glow
  radius sliders (the sliders are enabled only while the checkbox is on).

## How & why

- **The cast is a sweep, never a widening stroke.** The path is copied at a
  band of depths — `GLOW_BAND_STEP` (0.75 px) apart, each copy stroked thin at
  `GLOW_BAND_WIDTH` (1.5 px) with `LineJoin::Bevel` — and slid along the cast
  direction. A widening stroke inflates perpendicular to the path, spreads
  sideways around steep flanks, and its miter joins spike outward at corners;
  that is what used to wrap glow over spike tops. A translated thin copy can
  only ever sit behind its own line, so no mask is needed and no host's cast
  can clip or tint another's.
- **Layers**: `glow_layer_count(radius)` = `ceil(radius / GLOW_BAND_STEP)`
  clamped to 3..=64. Layer depth runs from half a band out to the full radius;
  alpha is `intensity · 255 · t²` with `t = 1 − depth / radius`, scaled by the
  spacing overlap so the near-line strength is stable.
- **Direction**: `glow_direction(orientation, mirrored)` is the image of the
  graph's +Y basis (toward Y0) under the same rotation and mirror
  `transform_point` uses — they share `rotation`. 0° casts down, 90° right,
  180° up, 270° left, each flipped by the mirror.
- **Reserved room**: `line_glow_reserve_px` = `radius + 1` physical px when on,
  else 0. It is read by **both** `overlay::layout_in_rect` (adding it to the
  window's short dimension) and `render_graph_into_internal` (insetting
  `bottom`), so the box and the drawing cannot disagree; the axis keeps the
  height `graphHeightPx` names. The deepest band reaches exactly the radius
  (±~0.5 px of AA), which leaves the reserve with a little slack. The
  background fill and the border cover the reserved band.
- **Draw order**: every series' cast, then every timeout marker, then every
  series' core. Markers span the canvas edge rather than the zero line precisely
  because the reserve moved the zero line inward.
- The prefill is the same walk with a different paint; the glow follows the
  segment's kind, so a prefill segment glows in the prefill colour without a
  special case.
- Known cosmetic edge: at the minimum radius or a near-vertical segment, the
  outer bands can show a faint sub-pixel halo at the sides; it is 5–20 % alpha.

## Config keys / UI

| Key | Default | Range / clamp | Control |
|---|---|---|---|
| `lineGlow` | new: on; absent key: off | on/off | Colors → Line glow |
| `lineGlowIntensity` | 10 | 0–100 | Glow intensity slider (%) |
| `lineGlowRadiusPx` | 30 | 2–50 | Glow radius slider (px) |

## Tests that pin it

`the_underglow_casts_toward_the_zero_line_in_every_orientation`,
`the_underglow_uses_each_hosts_own_line_colour`,
`the_underglow_fades_with_depth_below_the_line`,
`another_hosts_glow_does_not_tint_an_earlier_hosts_line`,
`the_underglow_does_not_reach_above_the_line_at_a_spike`,
`a_timeout_marker_reaches_through_the_underglow_reserve`,
`the_underglow_reserves_room_on_the_short_side`,
`line_glow_defaults_off_and_clamps_to_its_limits`.

## Related

- [rendering.md](rendering.md) — draw order, reserves and the layered window.
- [sample-cursor.md](sample-cursor.md) — the other per-overlay decoration.
- [smooth-rendering.md](smooth-rendering.md) — the reveal cut the glow follows.
