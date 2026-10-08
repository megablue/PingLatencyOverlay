# History compression — bands inside the canvas

Status: shipped (unreleased) · Read when: changing `compression.rs`, `map_x`, the `visible` crop, the marker loop, the cosmetic prefill's span, the background grid's cell rule, or the editor's History Compression section.

## What it does

- With `historyCompression` on, an overlay's long axis holds far more than `windowSeconds` of history **inside the canvas the config already names** (`windowSeconds × scale` logical px, times DPI). The canvas never grows: the bands divide it.
- The newest band is the **no-compression band**, drawn at the raw density — `scale` pixels per second, one pixel per sample at 1× — so the newest samples are pixel for pixel what the same overlay draws with the feature off. It is as wide as `noZoneShare` or `noZoneMinPx` says, whichever is larger, and it therefore holds `NC_px / scale` seconds.
- Behind it is a **ladder** of equally wide bands whose ratios step evenly from 2× up to `historyCompressionRatio`, then the **reserve** at that maximum ratio. The ladder holds as many bands as the canvas has room for at ten logical pixels each — `n = min(ratio − 1, floor(middle_px / 10))` — so the count follows the canvas width and the ratio, and a step is never finer than 1×.
- Off by default, and off for every profile written before it existed: with it off the frame is drawn exactly as it always was, pixel for pixel.
- Below `historyCompressionMinAxisPx` — logical pixels, `windowSeconds × scale`, before DPI — the geometry resolves to `None` and the graph is drawn uncompressed, because the bands do not fit on a short axis.
- With the background grid on ([rendering.md](rendering.md)), the grid draws each band's own density: its columns are placed through the same mapping, so cells are 30 px wide in the no-compression band, `30 / ratio_k` in each compressed one, and the joins are where the cells visibly change.

## Map

- `crates/core/src/compression.rs` — `bands(config)` (the one statement of the widths, the ladder and the gate, read by `geometry`), `geometry(config, axis_px) -> Option<Compression>`; `Compression::x_of`, `span`, `no_zone`, `no_zone_seconds`, `max_zone`, `base`, `ratio`, `zones`, `zone_count` and `Zone::{ratio, width, from, x}`; `MIN_COMPRESSION_ZONE_PX`, `MAX_COMPRESSION_ZONES`. Pure geometry: no Win32, no I/O, no GUI crate.
- `overlay.rs::layout_in_rect` — the logical long side is `windowSeconds × scale` with no compression term at all: the box is the same size with the feature on and off.
- `render.rs::render_graph_into_internal` — `let compression = compression::geometry(config, axis_long);`, then `span_duration`, `visible_slots`, `map_x`, `visible`, the marker loop's `last_marker_x`, and `prefill_span_seconds`.
- `render.rs::draw_background_grid` — the grid, its columns placed at `d = k × 30 / base` through `x_of`; the bands come out of the mapping, so the grid code needs no band of its own.
- `config.rs` — the seven keys, their constants and the `normalize` clamps.
- `ui.rs` — the `section(ui, "History Compression", …)` block, `history_span_label`, and the `Zones` row that prints the resolved band count and the deepest ratio.
- Tests: `compression::tests::*` (thirteen), `overlay::tests::the_canvas_keeps_the_configured_size`, the `compression_*`/`compressed_history_*`/`the_cursor_still_rests_*`/`a_short_series_stays_*`/`a_run_of_timeouts_across_*`/`the_newest_instant_keeps_*` render tests, `the_newest_band_draws_the_newest_samples_at_the_plain_density`, `the_grid_cells_step_at_the_band_joins`, `history_compression_defaults_off_and_round_trips`, `normalize_clamps_the_history_compression_values`, `the_history_compression_section_reads_the_geometry_back`.

## How & why

Everything is a share of the **drawn** axis except the floors and the ladder count, which are read against the **logical** axis (`windowSeconds × scale`) so that a px floor means the same thing at every DPI. With `logical_axis` and the drawn `axis`:

```
logical_axis = windowSeconds × scale              // pre-DPI; what the gate, the floors and the ladder read
no           = max(noZoneShare/100,  noZoneMinPx  / logical_axis)
max          = max(maxZoneShare/100, maxZoneMinPx / logical_axis)
if no + max > 1: scale both by 1 / (no + max)     // a canvas too narrow for its own floors
middle       = 1 − no − max                       // may be 0: the reserve then takes it
n            = min(ratio − 1, floor(middle × logical_axis / 10))   // ladder bands, 0 when under 10 px
ratio_k      = 2 + (k − 1) × (ratio − 2) / (n − 1)                 // even from 2x to the maximum

no_zone  = no  × axis
max_zone = max × axis
base     = axis / logical_axis × scale            // the raw density: scale px/s, times DPI
W        = (axis − no_zone − max_zone) / n        // every ladder band the same width
held_k   = W × ratio_k / base                     // the distance band k holds
span     = no_zone / base + Σ held_k + max_zone × ratio / base
```

`x_of(d)`, for a distance `d` from the newest drawn instant (seconds in smooth mode, slots in index mode), walks the bands newest to oldest:

```
d ≤ no_zone/base:  axis − d × base
inside band k:     band.x − (d − band.from) × base / ratio_k
past the reserve:  the reserve's own density, extrapolated
```

### Why bands

- **The canvas is the config's size, and that is the point.** Turning compression on does not widen the overlay — the newest samples keep the raw density and everything older is packed into what is left. `the_canvas_keeps_the_configured_size` pins the box, and `the_newest_band_draws_the_newest_samples_at_the_plain_density` pins the pixels: inside the no-compression band the two frames agree pixel for pixel on the same canvas.
- **The density is constant inside a band and steps at each join.** `x_of` is piecewise linear, so a join is a corner in x, not a kink in a curve: `the_density_is_constant_inside_a_band` samples each band's middle and the step across the first join.
- **How many bands, and how fine.** The count is what the canvas affords at `MIN_COMPRESSION_ZONE_PX` (10 logical px — below that a band reads as a stripe); the ratio ladder is even from 2× to the configured maximum, and never holds more than one band per whole ratio, because a step below 1× is a band nobody can see. A wide canvas therefore gets *wider* bands, not an endless ladder. `the_band_count_follows_the_canvas_and_the_ratio` pins both bounds and `the_ladder_steps_evenly_from_two_to_the_ratio` pins the 2, 4, 6, 8, 10, 12 of a 120 px canvas at 12×.
- **No ladder is a legitimate answer.** When the middle is under 10 px the reserve takes it and there is one step, raw straight to the maximum — honest, rather than a squeezed pretend-band. `a_narrow_canvas_falls_back_to_the_deepest_band` and `the_floors_give_way_when_the_canvas_cannot_hold_them` are the two degenerate shapes.
- **The ratio is a density, not a length.** In the reserve one pixel holds `ratio` times the time one pixel holds at the raw density, and each ladder band holds its own multiple of its width.
- **The grid on the background is the mapping made visible.** Its columns step by one cell's worth of distance — `d = k × 30 / base` — so they are 30 px apart in the no-compression band, `30 / ratio_k` apart in band k, and the joins are where they change. The drawing side (one path, one hairline stroke, the rows, the 1 px guard) is in [rendering.md](rendering.md).
### Feeding one mapping

- **Every px value in the config is logical.** A floor becomes a share via `min_px / logical_axis`, and `base` is recovered from the axis the caller hands in (`axis / logical_axis × scale`), so neither caller has to pass a DPI factor. `the_span_is_independent_of_the_dpi_scale` pins the seconds a canvas covers.
- **Both modes feed one distance.** Smooth: `d = age − SMOOTH_REVEAL_DELAY`. Index: `d = max(visible_len, windowSeconds) − 0.5 − i`, where `i` is the index inside the cropped slice and `visible_len` is that slice's length — `max` is what keeps a series shorter than the axis at the left of the no-compression band, where index mode has always drawn it, instead of stretching it across the whole axis. With compression off, both branches reduce to the old formulas exactly.
- **`visible` crops to the span.** Smooth: `age > span + SMOOTH_REVEAL_DELAY` is the cut. Index: the slice is the newest `floor(span + 0.5)` slots. With compression off the span *is* the window, so nothing moves. The span is always at least `windowSeconds` when the bands resolve, because the middle is at least 2× dense and the reserve `ratio×`: the index-mode anchor keeps at least its own window of slots.
### The rest of the frame

- **The reserve's markers.** A Stick is a full-height line, and the deepest bands can put a whole run of failures inside one pixel column. When compression is on, a marker within 1 px of the last one this series drew is skipped: overdraw is bounded, the picture is not changed. Off, the guard is not consulted at all.
- **The cosmetic prefill spreads over the span**, with `count = min(span, PREFILL_MAX_SAMPLES)`, so the startup graph fills the whole canvas instead of leaving part of it empty. The seed stream is untouched, so the curve itself is the same.
- **`None` means "draw it the way it has always been drawn".** The setting off, an axis under the gate and a degenerate geometry all resolve to `None`, and every crop and mapping falls back to the pre-compression branch. The whole pre-existing render suite is the equivalence test for that.

### Traps

- **A sample alone in its segment strokes nothing.** The renderer breaks a line at a gap and, when the next sample arrives, resumes at *that* sample's x from the last known y — so a lone old sample draws no pixels and its height reappears as a vertical connector at the next sample's x. A test that wants an old sample actually drawn needs a run spaced inside `sample_gap_threshold` (timeout + `SAMPLE_INTERVAL` + 2 s).
- **A sample buffer is oldest first, and the crop takes a prefix.** `visible` drops the *oldest* samples (`age > span` from the front), and index mode counts slots from the front too, so a test that builds a vector newest first gets an arbitrary cut as soon as the span is smaller than the series. The two compressed-history tests that used to pass only because the old span was longer than their series now build oldest first and stay inside the span.
- **`MAX_RENDER_DIMENSION` (2048 physical px) can still clamp the canvas.** The layout clamps the long side before DPI, and under that clamp the drawn axis is shorter than `logical_axis × dpi`: `base` comes out below the raw density and every band shrinks with it, so the no-compression band is squeezed along with the rest. It is the only way left to get a squeeze, and the direction is graceful — the span shrinks and nothing is drawn wrong.
- **The editor's readout is a preview, not a measurement.** It resolves `geometry` against `min(windowSeconds × scale, 8192)` — the same clamp the layout applies before the DPI multiply — because there is no window to measure from the Config process. `Zones` prints the count and the deepest ratio that came out, so a narrow canvas shows `reserve only`.
- **A target's buffer is 24 h deep.** A span wider than the samples available simply draws what there is; the buffer is not resized by this feature.

## Config keys / UI

| Key | Default | Range | Control |
|---|---|---|---|
| `historyCompression` | false | — | checkbox |
| `historyCompressionRatio` | 16 | 4–64 | slider, `x` |
| `historyCompressionNoZoneShare` | 50 | 0–100 | slider, `%` |
| `historyCompressionNoZoneMinPx` | 30 | 0–500 | DragValue, `px` |
| `historyCompressionMaxZoneShare` | 15 | 0–100 | slider, `%` |
| `historyCompressionMaxZoneMinPx` | 30 | 0–500 | DragValue, `px` |
| `historyCompressionMinAxisPx` | 120 | 0–2000 | DragValue, `px` |

All seven are `#[serde(default)]`, so a profile without them reads as off. The two shares are shares of the *drawn* axis and the two floors are logical pixels; the larger of each pair sizes its band. The uncompressed band no longer sizes the overlay — the canvas is `windowSeconds × scale` whatever the share is — it decides how much of the canvas keeps the raw density. They are clamped one at a time and never against each other: whether the bands fit, and how many there are, is what `compression::geometry` resolves from the canvas.

## Tests that pin it

- `compression_off_is_no_mapping_at_all`, `the_gate_keeps_a_short_axis_uncompressed`, `the_gate_reads_the_configured_axis_not_the_drawn_one`
- `the_ladder_steps_evenly_from_two_to_the_ratio`, `the_band_count_follows_the_canvas_and_the_ratio`, `a_narrow_canvas_falls_back_to_the_deepest_band`
- `the_no_compression_band_holds_the_raw_density`, `the_density_is_constant_inside_a_band`, `the_mapping_is_monotonic`
- `the_bands_tile_the_canvas_and_hold_the_span`, `the_band_floors_hold_on_a_short_canvas`, `the_floors_give_way_when_the_canvas_cannot_hold_them`, `the_span_is_independent_of_the_dpi_scale`
- `the_canvas_keeps_the_configured_size`, `the_newest_band_draws_the_newest_samples_at_the_plain_density`, `the_grid_cells_step_at_the_band_joins`
- `compression_draws_a_sample_the_plain_window_crops`, `a_run_of_timeouts_across_the_reserve_marks_its_whole_run`, `the_newest_instant_keeps_the_leading_edge`, `compressed_history_follows_orientation_and_mirror`, `the_cursor_still_rests_on_the_line_under_compression`, `a_short_series_stays_at_the_left_in_index_mode`
- `history_compression_defaults_off_and_round_trips`, `normalize_clamps_the_history_compression_values`, `the_history_compression_section_reads_the_geometry_back`
