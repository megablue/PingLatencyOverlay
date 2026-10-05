use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use tiny_skia::{FillRule, IntSize, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::border::{self, BorderVisual};
use crate::config::{
    OverlayConfig, MAX_LINE_STROKE_PX, MAX_SAMPLE_CURSOR_SIZE_PX, MIN_LINE_STROKE_PX,
    MIN_SAMPLE_CURSOR_SIZE_PX,
};

#[derive(Clone, Copy, Debug)]
pub struct SamplePoint {
    pub value: Option<u32>,
    pub timestamp: Instant,
    /// True for cosmetic history that should use the prefill line color.
    pub is_prefill: bool,
}

/// One target's line: its samples and the colours that identify it.
///
/// Colours travel with the samples rather than being looked up from the
/// overlay, because they are per target and the overlay no longer has a single
/// set of them. A group of one builds exactly one of these.
pub struct Series<'a> {
    pub line_color: &'a str,
    pub timeout_color: &'a str,
    pub samples: &'a [SamplePoint],
    /// An interval longer than this between consecutive samples is a period
    /// nothing was measuring this target; see `sample_gap_threshold`.
    pub max_sample_gap: Duration,
    /// The live easing state for this series' sample cursor, when a window
    /// keeps one. `None` — the test path — draws the target position directly.
    pub cursor: Option<&'a mut CursorAnimation>,
}

/// The eased position of one series' sample cursor.
///
/// The cursor follows the drawn line at its own apex, which steps when a
/// sample arrives and creeps while the graph scrolls. Easing those steps is
/// what keeps the cursor looking attached to the line instead of teleporting
/// from point to point.
#[derive(Default)]
pub struct CursorAnimation {
    /// Last drawn y, in graph coordinates.
    y: Option<f32>,
    /// When `y` was last advanced; the next step's time constant uses it.
    at: Option<Instant>,
    /// Whether the cursor is still moving toward a newer target.
    active: bool,
}

impl CursorAnimation {
    /// Advance toward `target` and return the y to draw this frame.
    pub fn advance(&mut self, now: Instant, target: f32) -> f32 {
        let next = match (self.y, self.at) {
            (Some(y), Some(at)) => {
                let dt = now.saturating_duration_since(at).as_secs_f32();
                let k = 1.0 - (-dt / CURSOR_EASE_SECS).exp();
                y + (target - y) * k
            }
            // Nothing to ease from yet: the first frame snaps into place, and
            // so does a resumed one that has been away for a long interval,
            // because the exponential step is then all but complete.
            _ => target,
        };
        self.at = Some(now);
        if (target - next).abs() <= CURSOR_EASE_EPSILON {
            self.y = Some(target);
            self.active = false;
            target
        } else {
            self.y = Some(next);
            self.active = true;
            next
        }
    }

    /// Whether the last advance left the cursor short of its target.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

/// One graph tick: the rate the sampler writes samples at.
///
/// It lives here, next to the renderer that reads a gap between samples as a
/// break, rather than in `probes.rs`; the probe loop reads it from here so the
/// cadence samples are written at and the cadence a gap is measured against
/// cannot drift apart.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// How far behind live the smooth line is presented.
///
/// Without this the newest sample lands on the leading edge the instant it
/// arrives, so a whole one-second segment is drawn in a single frame and the
/// line's transition from the previous sample happens as a step. Holding the
/// reveal back by three sample intervals lets the tip walk along each segment
/// as time passes. Index mode steps by sample, so it is not held back.
pub const SMOOTH_REVEAL_DELAY: Duration = Duration::from_secs(3 * SAMPLE_INTERVAL.as_secs());

/// The longest interval between consecutive samples that the probe loop itself
/// can produce.
///
/// The loop measures, writes a sample — including when the measurement failed —
/// and then sleeps a whole tick, so an interval is a measurement plus a tick,
/// and a measurement is bounded by the target's timeout. Anything longer means
/// nothing was probing this target: the profile is switched away, or Pause is
/// on. The renderer breaks the line there instead of drawing a slope over a
/// period it never measured.
///
/// Derived from the timeout rather than fixed, because a long timeout is a
/// deliberate statement that slow responses are expected; a threshold that
/// ignored it would cut a line that is genuinely continuous.
pub fn sample_gap_threshold(timeout_ms: u32) -> Duration {
    /// Scheduling slack. An ICMP lookup is deliberately absent from this
    /// budget: a probe task resolves its target once and refreshes it off the
    /// sampling path, so a slow lookup can only delay the first sample — before
    /// there is a previous one to measure a gap against. What is left is the
    /// loop's own scheduling jitter.
    const SLACK: Duration = Duration::from_secs(2);
    Duration::from_millis(u64::from(timeout_ms)) + SAMPLE_INTERVAL + SLACK
}

/// Render a graph into premultiplied RGBA bytes for a Win32 layered window.
///
/// The native window path deliberately does not use egui or a GPU surface. This
/// keeps alpha under our control and means each overlay consumes only a small
/// software buffer instead of creating another renderer/context.
#[cfg(test)]
pub fn render_graph_into(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    samples: &[SamplePoint],
    now: Instant,
    smooth: bool,
    pixels: &mut Vec<u8>,
) -> bool {
    render_graph_into_with_border(width, height, config, samples, now, smooth, None, pixels)
}

#[cfg(test)]
pub fn render_series_into(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    series: &mut [Series<'_>],
    now: Instant,
    smooth: bool,
    pixels: &mut Vec<u8>,
) -> bool {
    render_series_into_with_border(width, height, config, series, now, smooth, None, pixels)
}

/// The single-series path, kept because one target is the overwhelmingly common
/// case and every existing caller and test means exactly this.
#[allow(clippy::too_many_arguments)]
pub fn render_graph_into_with_border(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    samples: &[SamplePoint],
    now: Instant,
    smooth: bool,
    border: Option<&BorderVisual>,
    pixels: &mut Vec<u8>,
) -> bool {
    let target = config.first_target();
    let mut series = [Series {
        line_color: &target.line_color,
        timeout_color: &target.timeout_color,
        samples,
        max_sample_gap: sample_gap_threshold(target.timeout_ms),
        cursor: None,
    }];
    render_series_into_with_border(
        width,
        height,
        config,
        &mut series,
        now,
        smooth,
        border,
        pixels,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn render_series_into_with_border(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    series: &mut [Series<'_>],
    now: Instant,
    smooth: bool,
    border: Option<&BorderVisual>,
    pixels: &mut Vec<u8>,
) -> bool {
    render_graph_into_internal(width, height, config, series, now, smooth, border, pixels)
}

/// The room the underglow needs past the zero line, in physical pixels.
///
/// The cast's deepest band reaches the radius past the line's centre, and
/// antialiasing spends about half a pixel more. The axis's own pad already
/// carries the stroke's half-width plus half a pixel for that (`stroke_pad`),
/// so the reserve is the radius plus one, whatever the stroke is. It is added
/// to the window's short dimension rather than taken out of the axis, so
/// `graphHeightPx` keeps naming the visible height of the graph.
///
/// The window sizing in `overlay::layout_in_rect` and the inset inside
/// `render_graph_into_internal` both read this function, so the box and the
/// drawing cannot disagree about how much room the cast has.
///
/// Zero when the glow is off: an overlay without it is sized exactly as it
/// always was.
pub fn line_glow_reserve_px(config: &OverlayConfig) -> u32 {
    if config.line_glow {
        config.line_glow_radius_px + 1
    } else {
        0
    }
}

/// The pad the axis keeps from the window edge for the stroke.
///
/// A stroke is centred on its path, so a line clamped to the top or bottom of
/// the axis reaches half a stroke past it, plus the usual half pixel of
/// antialiasing. The default 1.5px stroke keeps the historic 2px pad; a
/// thicker one grows the pad rather than letting the pixmap edge slice the
/// line flat.
fn stroke_pad(stroke_width: f32) -> f32 {
    (stroke_width / 2.0 + 0.5).max(2.0)
}

/// The cursor triangle's rim width, in pixels.
const SAMPLE_CURSOR_RIM_WIDTH: f32 = 2.0;
/// The cursor triangle's fill; white with a dark rim reads on any line colour.
const SAMPLE_CURSOR_FILL: [u8; 3] = [255, 255, 255];
/// The cursor triangle's rim; the default background colour.
const SAMPLE_CURSOR_RIM: [u8; 3] = [15, 23, 42];
/// The gap the triangle's rim leaves at the box edge, in pixels: the outer
/// edge of the rim is flush against the leading edge of the window.
const SAMPLE_CURSOR_EDGE_MARGIN: f32 = 1.0;
/// The cursor's easing time constant, in seconds.
const CURSOR_EASE_SECS: f32 = 0.15;
/// How close the cursor must be to its target to count as settled, in pixels.
const CURSOR_EASE_EPSILON: f32 = 0.25;

fn sample_cursor_size(config: &OverlayConfig) -> u32 {
    config
        .sample_cursor_size_px
        .clamp(MIN_SAMPLE_CURSOR_SIZE_PX, MAX_SAMPLE_CURSOR_SIZE_PX)
}

/// Half the cursor triangle's height, in pixels.
fn sample_cursor_half_height(config: &OverlayConfig) -> f32 {
    sample_cursor_size(config) as f32 * 0.7
}

/// The band the sample cursor reserves on the leading (newest-sample) edge of
/// the window, in physical pixels.
///
/// The triangle's base is flush against that edge, so the reserve is just the
/// rim's half width plus a pixel of slack — constant, whatever the cursor's
/// size. The apex reaches `size - 1` pixels back into the chart and follows
/// the drawn line, which is what makes a larger cursor point at an earlier
/// point. Read by both `layout_in_rect` and the renderer, the way the
/// underglow's reserve is, so the box and the drawing cannot disagree.
pub fn sample_cursor_reserve_px(config: &OverlayConfig) -> u32 {
    if config.sample_cursor {
        (SAMPLE_CURSOR_EDGE_MARGIN + SAMPLE_CURSOR_RIM_WIDTH / 2.0).ceil() as u32
    } else {
        0
    }
}

/// The room the cursor needs at each end of the latency axis, in physical
/// pixels: half the triangle's height plus its rim.
///
/// The cursor is centred on the line, so a sample resting on the zero line or
/// clamped to the ceiling would otherwise have half a triangle cut off. Both
/// ends get the room, because either is one spike away.
pub fn sample_cursor_room_px(config: &OverlayConfig) -> u32 {
    if config.sample_cursor {
        (sample_cursor_half_height(config) + SAMPLE_CURSOR_RIM_WIDTH / 2.0 + 1.0).ceil() as u32
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn render_graph_into_internal(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    series: &mut [Series<'_>],
    now: Instant,
    smooth: bool,
    border: Option<&BorderVisual>,
    pixels: &mut Vec<u8>,
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let Some(size) = IntSize::from_wh(width, height) else {
        return false;
    };
    let byte_len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if pixels.len() != byte_len {
        pixels.resize(byte_len, 0);
    } else {
        pixels.fill(0);
    }
    let data = std::mem::take(pixels);
    let mut pixmap = match Pixmap::from_vec(data, size) {
        Some(pixmap) => pixmap,
        None => {
            pixels.resize(byte_len, 0);
            return false;
        }
    };
    let Some(full) = Rect::from_ltrb(0.0, 0.0, width as f32, height as f32) else {
        *pixels = pixmap.take();
        return false;
    };

    // The background belongs to the physical window, not the rotated graph.
    // At zero opacity the pixmap remains completely transparent.
    if config.bg_opacity > 0 {
        let [r, g, b] = parse_hex_color(&config.bg_color, [0, 0, 0]);
        let alpha = ((config.bg_opacity.min(100) as f32 / 100.0) * 255.0).round() as u8;
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, alpha);
        pixmap.fill_rect(full, &paint, Transform::identity(), None);
    }

    let rotated = matches!(config.orientation, 90 | 270);
    let long_px = if rotated { height as f32 } else { width as f32 };
    let short_px = if rotated { width as f32 } else { height as f32 };
    // The cursor's triangle sits in a band past the newest sample's x, so the
    // axis stops short of the window's leading edge by the amount the window
    // grew for it — one statement of the reserve, read here and in the layout.
    let cursor_reserve = sample_cursor_reserve_px(config) as f32;
    let axis_long = long_px - cursor_reserve;
    let visible_samples = config.window_seconds.max(1) as usize;
    let window_duration = Duration::from_secs(visible_samples as u64);
    let step = axis_long / visible_samples as f32;
    // The instant this frame presents, in smooth mode: the same three-sample
    // hold the marker loop and the cursor's walk use, so the whole frame
    // agrees on where "now" ends. Index mode shows the samples it has as soon
    // as it has them.
    let reveal = (smooth && config.smooth_rendering).then(|| now - SMOOTH_REVEAL_DELAY);

    let y_max = config.max_y_ms.max(1) as f32;
    // One stroke for the whole overlay: every host line, the startup prefill
    // and the timeout markers draw with it, so the width is read once here.
    let stroke_width = config
        .line_stroke_px
        .clamp(MIN_LINE_STROKE_PX, MAX_LINE_STROKE_PX);
    let pad = stroke_pad(stroke_width);
    // The cast falls past the zero line, so the window reserves a band for it:
    // the axis keeps its configured height and the glow gets the room below.
    // The cursor needs room at both ends of the latency axis as well, or a
    // sample resting on the zero line or clamped to the ceiling is cut in half.
    let reserve = line_glow_reserve_px(config) as f32;
    let cursor_room = sample_cursor_room_px(config) as f32;
    let top = pad + cursor_room;
    let bottom = short_px - pad - reserve - cursor_room;
    let map_y = |value: u32| {
        let t = (value as f32).min(y_max) / y_max;
        bottom - t * (bottom - top)
    };
    let map_x = |index: usize, sample: &SamplePoint| -> Option<f32> {
        if smooth {
            // Move every point by its real age so the whole graph scrolls as
            // one surface; index-based positions would jump when a sample
            // arrives at the right edge. The reveal hold shifts the window a
            // whole delay later, so the newest revealed instant sits at the
            // axis end and the drawn ages run from the delay to the window
            // after it. The crop is the only visibility gate: an age past the
            // window plus hold maps to a negative x, which the line pass wants
            // for the one older sample it keeps so the polyline leaves the
            // canvas instead of starting at x = 0.
            let age = now.saturating_duration_since(sample.timestamp);
            Some(
                axis_long
                    * (1.0
                        - (age.as_secs_f32() - SMOOTH_REVEAL_DELAY.as_secs_f32())
                            / window_duration.as_secs_f32()),
            )
        } else {
            Some((index as f32 + 0.5) * step)
        }
    };

    // The slice of one series that is still inside the visible window.
    //
    // Per series rather than once for the overlay: two targets of the same
    // overlay do not have to have answered the same number of times, and a
    // target that has just been added has none of the history the others have.
    // Cropping them as one buffer would offset every line by however much the
    // shorter series is short.
    //
    // `keep_older` adds the one sample immediately older than the cut. The
    // line pass wants it: its x is negative, so the segment into the canvas is
    // clipped by the edge instead of the polyline starting at the oldest
    // on-screen vertex. Markers and the cursor do not use it — a marker an
    // edge off-screen is invisible either way, and the cursor's walk must end
    // at the same instant the line is drawn to.
    fn visible(
        samples: &[SamplePoint],
        smooth: bool,
        now: Instant,
        window_duration: Duration,
        visible_samples: usize,
        keep_older: bool,
    ) -> &[SamplePoint] {
        let start = if smooth {
            // The axis ends at the newest revealed instant, so the oldest
            // point it shows is the window plus the reveal hold old.
            let cut = samples.partition_point(|sample| {
                now.saturating_duration_since(sample.timestamp)
                    > window_duration + SMOOTH_REVEAL_DELAY
            });
            if keep_older {
                cut.saturating_sub(1)
            } else {
                cut
            }
        } else {
            samples.len().saturating_sub(visible_samples)
        };
        &samples[start..]
    }

    let stroke = Stroke {
        width: stroke_width,
        ..Stroke::default()
    };

    // The prefill colour is shared by every target: it is the overlay's
    // cosmetic line colour, and one muted colour for the whole reveal reads as
    // one event rather than as N unrelated fake graphs.
    let mut prefill_line_paint = Paint::default();
    let [prefill_r, prefill_g, prefill_b] =
        parse_hex_color(&config.prefill_line_color, [100, 116, 139]);
    prefill_line_paint.set_color_rgba8(prefill_r, prefill_g, prefill_b, 255);

    // The cast runs from each line toward the zero line in the graph's own
    // frame, so it rotates and mirrors with the graph. Its layers use the
    // segment's own colour, which is why `draw_series` gets the colour bytes
    // beside each paint.
    let glow = config.line_glow.then(|| LineGlow {
        radius: config.line_glow_radius_px as f32,
        intensity: config.line_glow_intensity.min(100) as f32 / 100.0,
        dir: glow_direction(config.orientation, config.mirrored),
    });

    // One pass over every series. A glowing overlay calls it twice — all casts,
    // then all cores — so a later host's translucent glow cannot tint an
    // earlier host's line where the two cross.
    let draw_lines = |pixmap: &mut Pixmap, pass: LinePass, series: &mut [Series<'_>]| {
        for entry in series {
            let samples = visible(
                entry.samples,
                smooth,
                now,
                window_duration,
                visible_samples,
                true,
            );
            if samples.is_empty() {
                continue;
            }
            let mut real_line_paint = Paint::default();
            let [r, g, b] = parse_hex_color(entry.line_color, [74, 222, 128]);
            real_line_paint.set_color_rgba8(r, g, b, 255);
            draw_series(
                pixmap,
                samples,
                entry.max_sample_gap,
                &real_line_paint,
                [r, g, b],
                &prefill_line_paint,
                [prefill_r, prefill_g, prefill_b],
                &stroke,
                &map_x,
                &map_y,
                long_px,
                short_px,
                config.orientation,
                config.mirrored,
                glow.as_ref(),
                pass,
                reveal,
            );
        }
    };

    // Casts first, then the markers, then the cores. A marker spans the whole
    // canvas, through the cast's reserved band, so it is drawn over the casts —
    // a glow laid over it would tint it into the glow. The cores come last,
    // which keeps the old rule that a line sits on top of a marker it crosses.
    if glow.is_some() {
        draw_lines(&mut pixmap, LinePass::Glow, &mut *series);
    }
    for entry in &*series {
        // A target that has never responded draws nothing. An empty slice would
        // otherwise contribute a marker at every x if this were expressed as a
        // gap, which is the opposite of what "no data yet" should look like.
        if entry.samples.is_empty() {
            continue;
        }
        let mut timeout_paint = Paint::default();
        let [r, g, b] = parse_hex_color(entry.timeout_color, [239, 68, 68]);
        timeout_paint.set_color_rgba8(r, g, b, 255);
        let mut timeout_builder = PathBuilder::new();
        let samples = visible(
            entry.samples,
            smooth,
            now,
            window_duration,
            visible_samples,
            false,
        );
        for (index, sample) in samples.iter().enumerate() {
            let Some(x) = map_x(index, sample) else {
                continue;
            };
            if reveal.is_some_and(|cut| sample.timestamp > cut) {
                continue;
            }
            if sample.value.is_none() {
                let start = transform_point(
                    (x, top),
                    long_px,
                    short_px,
                    config.orientation,
                    config.mirrored,
                );
                // The far end is the canvas edge, not the zero line: the band
                // the cast reserves below y0 is part of the marker's height.
                let end = transform_point(
                    (x, short_px - pad),
                    long_px,
                    short_px,
                    config.orientation,
                    config.mirrored,
                );
                timeout_builder.move_to(start.0, start.1);
                timeout_builder.line_to(end.0, end.1);
            }
        }
        if let Some(path) = timeout_builder.finish() {
            pixmap.stroke_path(&path, &timeout_paint, &stroke, Transform::identity(), None);
        }
    }
    draw_lines(&mut pixmap, LinePass::Core, &mut *series);

    // The cursor marks the drawn line at its own apex: the base sits flush
    // against the leading edge and the apex reaches `size` pixels back along
    // the line, so a larger cursor points at an earlier point. A timeout
    // writes no sample, so the walk holds the last value the line drew; the
    // startup prefill's head counts as drawn. Drawn over the cores and under
    // the border, and independent per host, so no two series' cursors can
    // affect one another.
    if config.sample_cursor {
        let size = sample_cursor_size(config) as f32;
        let target_x = long_px - SAMPLE_CURSOR_EDGE_MARGIN - size;
        for entry in &mut *series {
            let samples = visible(
                entry.samples,
                smooth,
                now,
                window_duration,
                visible_samples,
                false,
            );
            let Some(target) = line_y_at_x(
                samples,
                entry.max_sample_gap,
                &map_x,
                &map_y,
                target_x,
                reveal,
            ) else {
                continue;
            };
            let y = match entry.cursor.as_deref_mut() {
                Some(animation) => animation.advance(now, target),
                None => target,
            };
            draw_sample_cursor(
                &mut pixmap,
                y,
                long_px,
                short_px,
                config.orientation,
                config.mirrored,
                size,
            );
        }
    }

    if let Some(border) = border {
        border::draw_border(&mut pixmap, width, height, border);
    }
    *pixels = pixmap.take();
    true
}

/// One overlay's resolved underglow.
struct LineGlow {
    /// How far the cast reaches past the line's centre, in pixels.
    radius: f32,
    /// Strength as a fraction of full opacity.
    intensity: f32,
    /// Unit direction of the cast, in window coordinates.
    dir: (f32, f32),
}

/// Which half of a line a pass draws.
///
/// A glowing overlay is drawn in two passes over every series — all casts,
/// then all cores — so a later host's translucent glow cannot tint an earlier
/// host's line where the two cross.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinePass {
    Glow,
    Core,
}

/// The width of one band of a cast.
///
/// A cast is built from thin copies of the path, one per depth; a band that
/// spread sideways with depth would put glow where the line is not.
const GLOW_BAND_WIDTH: f32 = 1.5;

/// The depth step between a cast's bands: half a band, so neighbouring bands
/// overlap and the stack reads as one gradient rather than a stripe per step.
const GLOW_BAND_STEP: f32 = 0.75;

/// The number of bands a cast is drawn with: one per half-band of reach.
///
/// The lower cap keeps a small radius from collapsing into a single hard band;
/// the upper one keeps a wide cast's stroke count bounded.
fn glow_layer_count(radius: f32) -> usize {
    ((radius / GLOW_BAND_STEP).ceil() as usize).clamp(3, 64)
}

/// Stroke one finished run of a line: its core, or its cast.
///
/// The cast is the shadow the run drops along `glow.dir`: the same path copied
/// at a band of depths, each copy stroked thin with a falling alpha. Only the
/// translation moves a copy, so no part of the cast can land on the far side
/// of the line. A stroke that grew *wider* with depth cannot promise that — it
/// also grows sideways, and its miter joins spike outward at corners, which is
/// what wrapped glow over the top of every sharp spike. The copies overlap, so
/// the stack composites to less than the sum of its alphas; each band carries
/// its share of the overlap it sits in, or the cast would read weaker than the
/// nested strokes it replaced. The core covers the first band's edge on the
/// line itself.
fn stroke_run(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    paint: &Paint,
    rgb: [u8; 3],
    stroke: &Stroke,
    glow: Option<&LineGlow>,
    pass: LinePass,
) {
    if pass == LinePass::Core {
        pixmap.stroke_path(path, paint, stroke, Transform::identity(), None);
        return;
    }
    let Some(glow) = glow else {
        return;
    };
    if glow.intensity <= 0.0 {
        return;
    }
    let layers = glow_layer_count(glow.radius);
    let span = (glow.radius - GLOW_BAND_WIDTH).max(0.0);
    let spacing = span / (layers - 1) as f32;
    let overlap = if spacing > 0.0 {
        (GLOW_BAND_WIDTH / spacing + 1.0).min(4.0)
    } else {
        1.0
    };
    for index in 0..layers {
        // The band's depth past the line's centre. Starting half a band out
        // keeps the band's near edge from crossing the line.
        let depth = GLOW_BAND_WIDTH / 2.0 + spacing * index as f32;
        let t = (1.0 - depth / glow.radius).max(0.0);
        let alpha = (glow.intensity * 255.0 * t * t * overlap)
            .round()
            .clamp(0.0, 255.0) as u8;
        if alpha == 0 {
            continue;
        }
        let mut layer_paint = Paint::default();
        layer_paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], alpha);
        let layer_stroke = Stroke {
            width: GLOW_BAND_WIDTH,
            line_join: LineJoin::Bevel,
            ..stroke.clone()
        };
        let transform = Transform::from_translate(glow.dir.0 * depth, glow.dir.1 * depth);
        pixmap.stroke_path(path, &layer_paint, &layer_stroke, transform, None);
    }
}

/// Draw the cursor triangle: base flush against the leading edge, apex on the
/// drawn line, pointing back at the data.
///
/// The vertices are built in graph coordinates and transformed like any
/// sample, so the cursor rotates and mirrors with the overlay. `y` is the apex
/// height, already resolved against the drawn line and eased; `size` is the
/// distance from the base to the apex, so a larger cursor reaches further back
/// along the line. White on a dark rim, fixed for every host, because the
/// cursor is a marker rather than data.
fn draw_sample_cursor(
    pixmap: &mut Pixmap,
    y: f32,
    long_px: f32,
    short_px: f32,
    orientation: u16,
    mirrored: bool,
    size: f32,
) {
    let half = size * 0.7;
    let base_x = long_px - SAMPLE_CURSOR_EDGE_MARGIN;
    let apex_x = base_x - size;
    let vertices = [
        transform_point((apex_x, y), long_px, short_px, orientation, mirrored),
        transform_point((base_x, y - half), long_px, short_px, orientation, mirrored),
        transform_point((base_x, y + half), long_px, short_px, orientation, mirrored),
    ];
    let mut builder = PathBuilder::new();
    builder.move_to(vertices[0].0, vertices[0].1);
    builder.line_to(vertices[1].0, vertices[1].1);
    builder.line_to(vertices[2].0, vertices[2].1);
    builder.close();
    let Some(path) = builder.finish() else {
        return;
    };

    let mut fill = Paint::default();
    fill.set_color_rgba8(
        SAMPLE_CURSOR_FILL[0],
        SAMPLE_CURSOR_FILL[1],
        SAMPLE_CURSOR_FILL[2],
        255,
    );
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);

    let mut rim = Paint::default();
    rim.set_color_rgba8(
        SAMPLE_CURSOR_RIM[0],
        SAMPLE_CURSOR_RIM[1],
        SAMPLE_CURSOR_RIM[2],
        255,
    );
    let rim_stroke = Stroke {
        width: SAMPLE_CURSOR_RIM_WIDTH,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &rim, &rim_stroke, Transform::identity(), None);
}

/// The y the drawn line passes through at `target_x`, in graph coordinates.
///
/// Mirrors `draw_series`' walk — including the connector at a prefill
/// boundary and the vertical resume stub after a timeout or gap — so the
/// cursor points at what is actually on screen rather than at the nearest
/// sample's value. When nothing covers `target_x` (the line has not reached it
/// yet, or a hole passes under the apex), the newest drawn value is the
/// answer: a timeout writes no sample, and the cursor stays where the line
/// stopped. Samples newer than `reveal` are withheld, the same cut the line is
/// drawn with, so the cursor cannot point past the drawn head. Returns `None`
/// when no sample has a value at all.
fn line_y_at_x(
    samples: &[SamplePoint],
    max_sample_gap: Duration,
    map_x: &dyn Fn(usize, &SamplePoint) -> Option<f32>,
    map_y: &dyn Fn(u32) -> f32,
    target_x: f32,
    reveal: Option<Instant>,
) -> Option<f32> {
    let mut covered: Option<f32> = None;
    let mut newest_y: Option<f32> = None;

    let mut in_segment = false;
    let mut last_y: Option<f32> = None;
    let mut last_point: Option<(f32, f32)> = None;
    let mut current: Option<(f32, f32)> = None;
    let mut last_timestamp: Option<Instant> = None;

    for (index, sample) in samples.iter().enumerate() {
        let Some(x) = map_x(index, sample) else {
            continue;
        };
        if reveal.is_some_and(|cut| sample.timestamp > cut) {
            break;
        }
        let gap = last_timestamp.is_some_and(|previous| {
            sample.timestamp.saturating_duration_since(previous) > max_sample_gap
        });
        last_timestamp = Some(sample.timestamp);
        if gap {
            in_segment = false;
            last_point = None;
            current = None;
        }
        let Some(value) = sample.value else {
            in_segment = false;
            last_point = None;
            current = None;
            continue;
        };
        let y = map_y(value);
        newest_y = Some(y);
        if !in_segment {
            match last_point {
                // A prefill boundary joins continuously: the line runs from
                // the previous point straight to this one.
                Some(previous) => {
                    cover(&mut covered, previous, (x, y), target_x);
                    current = Some((x, y));
                }
                None => {
                    if let Some(previous_y) = last_y {
                        // A resume stub: a vertical at this x from the carried
                        // value to this sample's own, and the line continues
                        // from the carried value.
                        cover(&mut covered, (x, y), (x, previous_y), target_x);
                        current = Some((x, previous_y));
                    } else {
                        current = Some((x, y));
                    }
                }
            }
            in_segment = true;
        } else if let Some(previous) = current {
            cover(&mut covered, previous, (x, y), target_x);
            current = Some((x, y));
        }
        last_y = Some(y);
        last_point = Some((x, y));
    }

    covered.or(newest_y)
}

/// Fold one drawn segment into the answer when `target_x` falls on it.
fn cover(result: &mut Option<f32>, from: (f32, f32), to: (f32, f32), target_x: f32) {
    let (low, high) = if from.0 <= to.0 {
        (from.0, to.0)
    } else {
        (to.0, from.0)
    };
    if target_x < low || target_x > high {
        return;
    }
    let y = if (to.0 - from.0).abs() < f32::EPSILON {
        // A vertical piece has no slope to read; the later end — the carried
        // value the line resumes from, or the sample the stub points at — is
        // what a reader sees at this column.
        to.1
    } else {
        let t = (target_x - from.0) / (to.0 - from.0);
        from.1 + t * (to.1 - from.1)
    };
    *result = Some(y);
}

/// Stroke one target's line, breaking it at timeouts, data gaps and prefill
/// boundaries. In smooth mode `reveal` stops the walk at the instant the frame
/// presents, drawing a partial last segment so the tip advances with time
/// rather than arriving whole with its sample.
///
/// Split out of the renderer so the per-target bookkeeping is written once
/// rather than nested inside a loop over targets. The cost of that loop is that
/// every one of these has to be reset per series; anything carried across
/// iterations here would be a line joining two different hosts.
#[allow(clippy::too_many_arguments)]
fn draw_series(
    pixmap: &mut Pixmap,
    samples: &[SamplePoint],
    max_sample_gap: Duration,
    real_line_paint: &Paint,
    real_line_rgb: [u8; 3],
    prefill_line_paint: &Paint,
    prefill_line_rgb: [u8; 3],
    stroke: &Stroke,
    map_x: &dyn Fn(usize, &SamplePoint) -> Option<f32>,
    map_y: &dyn Fn(u32) -> f32,
    long_px: f32,
    short_px: f32,
    orientation: u16,
    mirrored: bool,
    glow: Option<&LineGlow>,
    pass: LinePass,
    reveal: Option<Instant>,
) {
    let paint_for = |prefill: Option<bool>| -> (&Paint, [u8; 3]) {
        if prefill == Some(true) {
            (prefill_line_paint, prefill_line_rgb)
        } else {
            (real_line_paint, real_line_rgb)
        }
    };

    let mut segment = PathBuilder::new();
    let mut in_segment = false;
    let mut segment_prefill: Option<bool> = None;
    let mut last_y: Option<f32> = None;
    let mut last_point: Option<(f32, f32)> = None;
    let mut last_timestamp: Option<Instant> = None;
    for (index, sample) in samples.iter().enumerate() {
        let Some(x) = map_x(index, sample) else {
            continue;
        };
        if let Some(cut) = reveal.filter(|cut| sample.timestamp > *cut) {
            // Stop at the reveal instant. When this sample continues the run,
            // draw part of the way to it: the tip walks along the segment as
            // time passes instead of arriving whole with its sample.
            if in_segment && segment_prefill == Some(sample.is_prefill) {
                if let (Some((previous_x, previous_y)), Some(latency), Some(previous_timestamp)) =
                    (last_point, sample.value, last_timestamp)
                {
                    let span = sample
                        .timestamp
                        .saturating_duration_since(previous_timestamp);
                    if span <= max_sample_gap {
                        let walked = cut.saturating_duration_since(previous_timestamp);
                        let partial = if span.is_zero() {
                            1.0
                        } else {
                            (walked.as_secs_f32() / span.as_secs_f32()).min(1.0)
                        };
                        let tip = (
                            previous_x + (x - previous_x) * partial,
                            previous_y + (map_y(latency) - previous_y) * partial,
                        );
                        let point = transform_point(tip, long_px, short_px, orientation, mirrored);
                        segment.line_to(point.0, point.1);
                    }
                }
            }
            break;
        }
        // Two samples further apart than the probe loop can produce were not
        // neighbours in time, so the segment ends here and the next one resumes
        // at the last known value — exactly as it does after a timeout. Without
        // this the line is drawn straight across a period nothing measured, so
        // a profile switched away and back shows a slope over the time away.
        let gap = last_timestamp.is_some_and(|previous| {
            sample.timestamp.saturating_duration_since(previous) > max_sample_gap
        });
        last_timestamp = Some(sample.timestamp);
        if gap {
            if in_segment {
                if let Some(path) = segment.finish() {
                    let (paint, rgb) = paint_for(segment_prefill);
                    stroke_run(pixmap, &path, paint, rgb, stroke, glow, pass);
                }
                segment = PathBuilder::new();
            }
            in_segment = false;
            segment_prefill = None;
            last_point = None;
        }
        let Some(latency) = sample.value else {
            if in_segment {
                if let Some(path) = segment.finish() {
                    let (paint, rgb) = paint_for(segment_prefill);
                    stroke_run(pixmap, &path, paint, rgb, stroke, glow, pass);
                }
                segment = PathBuilder::new();
            }
            in_segment = false;
            segment_prefill = None;
            last_point = None;
            continue;
        };

        let y = map_y(latency);
        let sample_prefill = sample.is_prefill;
        if !in_segment || segment_prefill != Some(sample_prefill) {
            let previous_segment_prefill = segment_prefill;
            if in_segment {
                if let Some(path) = segment.finish() {
                    let (paint, rgb) = paint_for(previous_segment_prefill);
                    stroke_run(pixmap, &path, paint, rgb, stroke, glow, pass);
                }
                segment = PathBuilder::new();
            }
            let point = transform_point((x, y), long_px, short_px, orientation, mirrored);
            if let Some((previous_x, previous_y)) = last_point {
                let previous = transform_point(
                    (previous_x, previous_y),
                    long_px,
                    short_px,
                    orientation,
                    mirrored,
                );
                let mut connector = PathBuilder::new();
                connector.move_to(previous.0, previous.1);
                connector.line_to(point.0, point.1);
                if let Some(path) = connector.finish() {
                    let (paint, rgb) = paint_for(previous_segment_prefill);
                    stroke_run(pixmap, &path, paint, rgb, stroke, glow, pass);
                }
                segment.move_to(point.0, point.1);
            } else {
                segment.move_to(point.0, point.1);
                if let Some(previous_y) = last_y {
                    // Resuming after a timeout starts at the next responding
                    // sample's X at the last responding Y, so the line picks up
                    // where it left off rather than jumping to the new value.
                    let resume =
                        transform_point((x, previous_y), long_px, short_px, orientation, mirrored);
                    segment.line_to(resume.0, resume.1);
                }
            }
            in_segment = true;
            segment_prefill = Some(sample_prefill);
        } else {
            let point = transform_point((x, y), long_px, short_px, orientation, mirrored);
            segment.line_to(point.0, point.1);
        }
        last_y = Some(y);
        last_point = Some((x, y));
    }
    if in_segment {
        if let Some(path) = segment.finish() {
            let (paint, rgb) = paint_for(segment_prefill);
            stroke_run(pixmap, &path, paint, rgb, stroke, glow, pass);
        }
    }
}

const PREFILL_MAX_SAMPLES: usize = 512;

/// Draw the next value of a splitmix64 stream.
fn prefill_step(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A uniform draw in `0.0..1.0` with 24 bits of resolution.
fn prefill_unit(state: &mut u64) -> f32 {
    (prefill_step(state) >> 40) as f32 / (1u64 << 24) as f32
}

/// Build a deterministic, plausible-looking cosmetic latency curve.
///
/// Seeded from `seed` rather than from the overlay alone. With one target the
/// overlay id is enough; with several, two targets sharing a curve would look
/// like one host with a fat line, which is the thing a group exists to
/// disambiguate.
///
/// Real latency is not a smooth wave: it drifts around a resting level, jitters
/// from sample to sample, and now and then spikes and decays back. A pair of
/// sines reads as a pacemaker, so the shape here is a random walk with jitter
/// and sparse spike episodes, drawn from a seeded splitmix64 stream. The order
/// of the draws is fixed so the series stays deterministic.
///
/// The level is in milliseconds, not a fraction of the axis. A fraction is
/// relative to whatever scale the user chose, so the same generator read as a
/// working connection on a 200 ms axis and as a congestion problem on a 1 s
/// one; plausible latency is a property of the connection. The walk rests in
/// the tens of milliseconds and spikes above that, clamped to the axis.
pub fn cosmetic_prefill_values(config: &OverlayConfig, seed: &str) -> Vec<u32> {
    let count = config.window_seconds.max(1).min(PREFILL_MAX_SAMPLES as u32) as usize;
    let mut hasher = DefaultHasher::new();
    config.id.hash(&mut hasher);
    seed.hash(&mut hasher);
    let mut state = hasher.finish();
    let max_y = config.max_y_ms.max(1) as f32;

    let mut baseline = 12.0 + prefill_unit(&mut state) * 18.0;
    let mut spike_left = 0usize;
    let mut spike_level = 0.0f32;

    (0..count)
        .map(|_| {
            // A slow, mean-reverting drift around the resting level.
            baseline = (baseline + (prefill_unit(&mut state) - 0.5) * 2.0).clamp(5.0, 45.0);
            let jitter = (prefill_unit(&mut state) - 0.5) * 6.0;
            if spike_left == 0 && prefill_unit(&mut state) < 0.07 {
                if prefill_unit(&mut state) < 0.12 {
                    // A long episode: sustained, gentler, decaying.
                    spike_left = 8 + (prefill_unit(&mut state) * 12.0) as usize;
                    spike_level = 8.0 + prefill_unit(&mut state) * 25.0;
                } else {
                    // A short episode: one to three samples, sharper.
                    spike_left = 1 + (prefill_unit(&mut state) * 3.0) as usize;
                    spike_level = 15.0 + prefill_unit(&mut state) * 115.0;
                }
            }
            let spike = if spike_left > 0 {
                let level = spike_level;
                spike_left -= 1;
                spike_level *= 0.7;
                level
            } else {
                0.0
            };
            let value = (baseline + jitter + spike).clamp(1.0, max_y);
            value.round() as u32
        })
        .collect()
}

/// Build timestamped cosmetic samples that occupy one graph window.
pub fn cosmetic_prefill_samples(
    config: &OverlayConfig,
    seed: &str,
    now: Instant,
) -> Vec<SamplePoint> {
    let values = cosmetic_prefill_values(config, seed);
    let count = values.len();
    let window_duration = Duration::from_secs(config.window_seconds.max(1) as u64);
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let fraction = if count <= 1 {
                1.0
            } else {
                index as f32 / (count - 1) as f32
            };
            let age =
                Duration::from_secs_f64(window_duration.as_secs_f64() * (1.0 - fraction as f64));
            SamplePoint {
                value: Some(value),
                timestamp: now.checked_sub(age).unwrap_or(now),
                is_prefill: true,
            }
        })
        .collect()
}

/// Render the cosmetic prefill as a left-to-right reveal.
#[allow(clippy::too_many_arguments)]
pub fn render_prefill_into(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    samples: &[SamplePoint],
    now: Instant,
    progress: f32,
    border: Option<&BorderVisual>,
    points: &mut Vec<SamplePoint>,
    pixels: &mut Vec<u8>,
) -> bool {
    let progress = progress.clamp(0.0, 1.0);
    let count = (progress * samples.len() as f32).ceil() as usize;
    points.clear();
    points.extend(samples.iter().take(count).copied());
    let line_color = &config.first_target().line_color;
    let mut series = [Series {
        line_color,
        timeout_color: &config.first_target().timeout_color,
        samples: points,
        max_sample_gap: sample_gap_threshold(config.first_target().timeout_ms),
        cursor: None,
    }];
    render_series_into_with_border(
        width,
        height,
        config,
        &mut series,
        now,
        true,
        border,
        pixels,
    )
}

/// The rotation half of `transform_point`, shared with the underglow's cast
/// direction so the two cannot disagree about which way the graph points.
fn rotation(orientation: u16) -> (f32, f32) {
    match orientation {
        90 => (1.0, 0.0),
        180 => (0.0, -1.0),
        270 => (-1.0, 0.0),
        _ => (0.0, 1.0),
    }
}

/// The direction the underglow is cast in, in window coordinates.
///
/// The cast runs from the line toward the zero line in the graph's own frame:
/// the image of the graph's +Y basis under the same rotation and mirror
/// `transform_point` applies. Screen-down would be wrong the moment the
/// overlay is rotated — at 90 and 270 degrees it would smear the cast along
/// the time axis instead of under the line — so the direction rotates with
/// the graph, and mirroring flips it exactly as it flips the graph.
fn glow_direction(orientation: u16, mirrored: bool) -> (f32, f32) {
    let (sin, cos) = rotation(orientation);
    let sign = if mirrored { -1.0 } else { 1.0 };
    (sin * sign, cos * sign)
}

/// Map logical graph coordinates to the physical layered-window coordinates.
/// The matrix is the anticlockwise rotation required by the product spec.
fn transform_point(
    point: (f32, f32),
    long_px: f32,
    short_px: f32,
    orientation: u16,
    mirrored: bool,
) -> (f32, f32) {
    let x = point.0 - long_px / 2.0;
    let mut y = point.1 - short_px / 2.0;
    if mirrored {
        y = -y;
    }

    let (sin, cos) = rotation(orientation);
    let (center_x, center_y) = if matches!(orientation, 90 | 270) {
        (short_px / 2.0, long_px / 2.0)
    } else {
        (long_px / 2.0, short_px / 2.0)
    };
    (cos * x + sin * y + center_x, -sin * x + cos * y + center_y)
}

/// Read a `#RRGGBB` colour, or fall back.
///
/// The single colour parser for the whole app. It used to exist twice, once in
/// this module and once in the configuration window, differing only in what
/// they fell back to, and a theme file is a third caller. Three copies of a
/// parser that all have to agree is a bug waiting for the day one of them is
/// edited.
///
/// Three-digit shorthand is deliberately not accepted: the config files have
/// always documented `#RRGGBB`, and silently accepting `#fff` would make a
/// typo look like a colour rather than like the mistake it is.
pub fn parse_hex_color(value: &str, fallback: [u8; 3]) -> [u8; 3] {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return fallback;
    }
    let channel = |offset: usize| u8::from_str_radix(&value[offset..offset + 2], 16).unwrap_or(0);
    [channel(0), channel(2), channel(4)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        OverlayConfig, DEFAULT_LINE_STROKE_PX, MAX_LINE_STROKE_PX, MIN_LINE_STROKE_PX,
    };

    fn render_graph(
        width: u32,
        height: u32,
        config: &OverlayConfig,
        samples: &[SamplePoint],
        now: Instant,
        smooth: bool,
    ) -> Option<Vec<u8>> {
        let mut pixels = Vec::new();
        render_graph_into(width, height, config, samples, now, smooth, &mut pixels)
            .then_some(pixels)
    }

    fn samples(values: &[Option<u32>]) -> Vec<SamplePoint> {
        let now = Instant::now();
        values
            .iter()
            .enumerate()
            .map(|(index, value)| SamplePoint {
                value: *value,
                timestamp: now - Duration::from_secs((values.len() - index) as u64),
                is_prefill: false,
            })
            .collect()
    }

    /// Samples at explicit ages in seconds, for tests that care where the hole
    /// is rather than only about the values.
    fn aged_samples(now: Instant, entries: &[(u64, u32)]) -> Vec<SamplePoint> {
        entries
            .iter()
            .map(|(age, value)| SamplePoint {
                value: Some(*value),
                timestamp: now - Duration::from_secs(*age),
                is_prefill: false,
            })
            .collect()
    }

    /// Where a colour was drawn: every `(row, column)` it covers.
    ///
    /// Positions rather than a pixel count, because two lines can overlap and
    /// one can then hide the other, and a count cannot tell "hidden" from
    /// "absent". A set of positions can: each host's own set has to match
    /// whether it was drawn alone or beside another.
    ///
    /// The alpha is divided out before the comparison. tiny-skia antialiases a
    /// 1.5px stroke, so a partially covered pixel is premultiplied to something
    /// like half the colour, and matching on the stored bytes would find only
    /// the fully-covered pixels in the middle of a line. That in turn would let
    /// a test pass on the wrong colour entirely: an assertion for one host's
    /// timeout marker can be satisfied by another host's *line*, because the
    /// timeout pass runs first and the line pass draws over it.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn positions_of_color(pixels: &[u8], width: usize, color: [u8; 3]) -> Vec<(usize, usize)> {
        pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, pixel)| {
                let alpha = pixel[3];
                if alpha == 0 {
                    return false;
                }
                let unpremultiplied = |channel: u8| {
                    let scaled = u32::from(channel) * 255 / u32::from(alpha);
                    scaled.min(255) as i32
                };
                let difference = (unpremultiplied(pixel[0]) - i32::from(color[0])).abs()
                    + (unpremultiplied(pixel[1]) - i32::from(color[1])).abs()
                    + (unpremultiplied(pixel[2]) - i32::from(color[2])).abs();
                difference <= 24
            })
            .map(|(index, _)| (index / width, index % width))
            .collect()
    }

    /// Render a whole set of hosts into one buffer.
    #[allow(clippy::type_complexity)]
    fn render_hosts(
        config: &OverlayConfig,
        entries: &[(&str, &str, &[SamplePoint])],
        now: Instant,
        smooth: bool,
    ) -> Vec<u8> {
        let entries: Vec<(&str, &str, &[SamplePoint])> = entries.to_vec();
        let mut series: Vec<Series<'_>> = entries
            .iter()
            .map(|(line, timeout, points)| Series {
                line_color: line,
                timeout_color: timeout,
                samples: points,
                max_sample_gap: sample_gap_threshold(config.first_target().timeout_ms),
                cursor: None,
            })
            .collect();
        let mut pixels = Vec::new();
        assert!(render_series_into(
            300,
            100,
            config,
            &mut series,
            now,
            smooth,
            &mut pixels
        ));
        pixels
    }

    /// Every host in a group is drawn, in its own colour, at its own height.
    ///
    /// The colours are checked separately because counting the total strokes
    /// passes with two lines in one colour, which is the single thing a group
    /// must never show.
    #[test]
    fn every_host_in_a_group_draws_in_its_own_colour_at_its_own_height() {
        let mut config = OverlayConfig::new();
        config.max_y_ms = 1_000;
        let now = Instant::now();
        let slow = samples(&[Some(100); 40]);
        let fast = samples(&[Some(900); 40]);

        let pixels = render_hosts(
            &config,
            &[("#00ff00", "#ff0000", &slow), ("#0000ff", "#ff00ff", &fast)],
            now,
            false,
        );

        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        let blue = positions_of_color(&pixels, 300, [0, 0, 255]);
        assert!(!green.is_empty(), "the slow host's line is missing");
        assert!(!blue.is_empty(), "the fast host's line is missing");
        let mean_row = |rows: &Vec<(usize, usize)>| {
            rows.iter().map(|(row, _)| *row).sum::<usize>() / rows.len()
        };
        assert!(
            mean_row(&green) > mean_row(&blue),
            "100ms and 900ms on a shared 1000ms axis drew at the same height \
             (green row {}, blue row {}), so they are one line",
            mean_row(&green),
            mean_row(&blue)
        );
    }

    /// A host that has never answered draws nothing at all.
    ///
    /// The failure this guards is a line sitting along y0, which reads as "this
    /// host is extremely fast" rather than "this host has said nothing".
    #[test]
    fn a_host_with_no_samples_draws_nothing() {
        let config = OverlayConfig::new();
        let now = Instant::now();
        let answered = samples(&[Some(500); 40]);
        let empty: [SamplePoint; 0] = [];

        let with_empty = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &answered),
                ("#0000ff", "#ff00ff", &empty),
            ],
            now,
            false,
        );
        let alone = render_hosts(&config, &[("#00ff00", "#ff0000", &answered)], now, false);

        assert_eq!(
            with_empty, alone,
            "a host with no samples drew something the answering host did not"
        );
    }

    /// A timeout belongs to the host that had it, in that host's colour.
    ///
    /// A marker is a full-height vertical line, so on a shared plot one host's
    /// timeout is otherwise indistinguishable from another's — which is why the
    /// colour travels with the samples rather than being one "something went
    /// wrong" red for the whole window.
    #[test]
    fn a_timeout_is_marked_in_its_own_host_colour_and_only_its_own() {
        let config = OverlayConfig::new();
        let now = Instant::now();
        let steady = samples(&[Some(200); 40]);
        // The last sample is a non-response, so the line breaks and a marker is
        // drawn in the second host's timeout colour.
        let mut gapped = samples(&[Some(200); 40]);
        gapped.last_mut().expect("not empty").value = None;

        let pixels = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &steady),
                ("#0000ff", "#00ff00", &gapped),
            ],
            now,
            false,
        );

        assert!(
            !positions_of_color(&pixels, 300, [0, 255, 0]).is_empty(),
            "the host that timed out did not mark in its own colour"
        );
        assert!(
            positions_of_color(&pixels, 300, [255, 0, 0]).is_empty(),
            "the host that never timed out drew a marker anyway"
        );
    }

    /// Two hosts dropping at different seconds both mark, in their own colours.
    ///
    /// Two hosts dropping at the *same* second draw their markers over each
    /// other and only the later one is visible — the same thing that happens to
    /// two lines crossing, and unavoidable while a marker is a full-height
    /// vertical line. So this uses different seconds, which is the case where
    /// the per-host colour is what tells the two drops apart.
    #[test]
    fn two_hosts_dropping_both_mark_in_their_own_colours() {
        let config = OverlayConfig::new();
        let now = Instant::now();
        // Different columns: with index-based positioning the sample at index N
        // is always at the same x, so two hosts dropping at the same index would
        // stack their markers and only the second would be visible.
        let mut first = samples(&[Some(200); 40]);
        first[33].value = None;
        let mut second = samples(&[Some(400); 40]);
        second[38].value = None;
        second[39].value = None;

        let pixels = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &first),
                ("#0000ff", "#0000ff", &second),
            ],
            now,
            false,
        );

        let red = positions_of_color(&pixels, 300, [255, 0, 0]);
        let blue = positions_of_color(&pixels, 300, [0, 0, 255]);
        assert!(!red.is_empty(), "the first host's marker is missing");
        assert!(!blue.is_empty(), "the second host's marker is missing");
        assert_ne!(
            red[0].1, blue[0].1,
            "both markers are in the same column, so one is hidden behind the \
             other rather than the two being distinguishable"
        );
    }

    /// A host's line is where it would be if it were the only one.
    ///
    /// The bookkeeping per target — the segment in progress, the last point, the
    /// last Y — is the kind of thing that works for one line and silently draws
    /// a diagonal from one host's last sample to another's first when there are
    /// two. The two series are far apart vertically so neither can hide the
    /// other, which makes the comparison exact.
    #[test]
    fn a_host_is_drawn_where_it_would_be_drawn_alone() {
        let mut config = OverlayConfig::new();
        config.max_y_ms = 1_000;
        let now = Instant::now();
        let low = samples(&[Some(100); 30]);
        let high = samples(&[Some(900); 30]);

        let first_alone = render_hosts(&config, &[("#00ff00", "#ff0000", &low)], now, false);
        let second_alone = render_hosts(&config, &[("#0000ff", "#00ff00", &high)], now, false);
        let together = render_hosts(
            &config,
            &[("#00ff00", "#ff0000", &low), ("#0000ff", "#00ff00", &high)],
            now,
            false,
        );

        assert_eq!(
            positions_of_color(&together, 300, [0, 255, 0]),
            positions_of_color(&first_alone, 300, [0, 255, 0]),
            "the first host's line moved because a second host was drawn"
        );
        assert_eq!(
            positions_of_color(&together, 300, [0, 0, 255]),
            positions_of_color(&second_alone, 300, [0, 0, 255]),
            "the second host's line moved because a first host was drawn"
        );
    }

    /// A newly added host is drawn where its own timestamps put it.
    ///
    /// A host added to an existing group has a handful of samples and none of the
    /// history the others have. If the visible window were computed once for the
    /// overlay and then applied to every series, the new host's samples would be
    /// indexed against the *oldest* host's buffer: either dropped entirely, or
    /// drawn as if they were 55 seconds old at the left of the plot. Both are
    /// silent, and both look like the host has been dead for most of the window.
    #[test]
    fn a_newly_added_host_is_placed_by_its_own_timestamps() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        // The cursor's reserve would move the right edge this test measures
        // against; it is about placement, not the marker.
        config.sample_cursor = false;
        let now = Instant::now();
        let long = samples(&[Some(100); 60]);

        // Five samples as if the host had just been added: ages 5s down to 1s.
        let fresh = samples(&[Some(900); 5]);
        let oldest_age = fresh
            .first()
            .expect("not empty")
            .timestamp
            .elapsed()
            .as_secs();

        let pixels = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &long),
                ("#0000ff", "#00ff00", &fresh),
            ],
            now,
            true,
        );

        let blue = positions_of_color(&pixels, 300, [0, 0, 255]);
        assert!(!blue.is_empty(), "the newly added host drew nothing");
        let leftmost = blue
            .iter()
            .map(|(_, column)| *column)
            .min()
            .expect("not empty");
        // Time runs right to left and the reveal shifts the window: the newest
        // revealed instant sits at the right edge, so a sample `oldest_age`
        // old is `oldest_age - 3` seconds in from it.
        let expected_left = 300 - (300.0 * oldest_age.saturating_sub(3) as f32 / 30.0) as usize;
        assert!(
            leftmost >= expected_left.saturating_sub(6) && leftmost <= 300,
            "the new host's line starts at column {leftmost}, but samples no \
             older than {oldest_age}s in a 30s window belong at column \
             {expected_left} or further right — its history is being cropped \
             with the other host's"
        );
    }

    /// A hole with no samples at all is not interpolated in smooth mode.
    ///
    /// This is the shape an auto profile switch leaves behind: targets that are
    /// not in the active profile stop being probed, but their buffers survive,
    /// so on the way back both sides of the hole are still in memory. Joining
    /// them draws a slope over seconds that were never measured.
    #[test]
    fn a_data_gap_is_not_interpolated_in_smooth_mode() {
        let mut config = OverlayConfig::new();
        // The cast's round caps at the run ends would bleed line-coloured pixels
        // into the hole; this test is about where the line itself was drawn.
        config.line_glow = false;
        // Same for the cursor: its reserve and its triangle are not the line.
        config.sample_cursor = false;
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // Three samples at 100ms, an eight-second hole, three at 300ms. The
        // reveal holds the newest three seconds back and shifts the window, so
        // in a 300px / 30s plot age 3 sits at the right edge and each second is
        // 10px: the sample before the hole (age 16) is at column 170 and the
        // one after it (age 6) at 270.
        let samples = aged_samples(
            now,
            &[
                (16, 100),
                (15, 100),
                (14, 100),
                (6, 300),
                (5, 300),
                (4, 300),
            ],
        );

        let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], now, true);
        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        assert!(!green.is_empty(), "the host drew nothing");

        let drawn_in_gap = green
            .iter()
            .filter(|(_, column)| (193..=267).contains(column))
            .count();
        assert_eq!(
            drawn_in_gap, 0,
            "a line was drawn across the eight-second hole (columns 190 to 270)"
        );

        // And it resumes at the last known value rather than jumping: the stub
        // at the first sample after the hole spans the old and the new level.
        let stub: Vec<usize> = green
            .iter()
            .filter(|(_, column)| (268..=272).contains(column))
            .map(|(row, _)| *row)
            .collect();
        let span = match (stub.iter().min(), stub.iter().max()) {
            (Some(min), Some(max)) => max - min,
            _ => panic!("the line never came back after the hole"),
        };
        assert!(
            span >= 12,
            "the line jumped to the new value instead of resuming at the last \
             known one (the stub spans {span}px)"
        );
    }

    /// The same hole in index mode: the two sides are not joined by a diagonal.
    ///
    /// Index mode positions samples by their order rather than by their time,
    /// so the break shows as a vertical seam at the first sample after the
    /// hole. The timestamps still say the two sides are not neighbours.
    #[test]
    fn a_data_gap_is_not_interpolated_in_index_mode() {
        let mut config = OverlayConfig::new();
        // The cast's round caps at the run ends would bleed line-coloured pixels
        // into the hole; this test is about where the line itself was drawn.
        config.line_glow = false;
        // Same for the cursor: its reserve and its triangle are not the line.
        config.sample_cursor = false;
        config.window_seconds = 10;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // Ages 9, 8, 7 then 2, 1, 0: a five-second hole between the neighbours
        // at index 2 and index 3, which sit at columns 75 and 105 of a 300px
        // window with ten visible samples.
        let samples = aged_samples(
            now,
            &[(9, 100), (8, 100), (7, 100), (2, 300), (1, 300), (0, 300)],
        );

        let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], now, false);
        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        assert!(!green.is_empty(), "the host drew nothing");

        let drawn_in_gap = green
            .iter()
            .filter(|(_, column)| (78..=102).contains(column))
            .count();
        assert_eq!(
            drawn_in_gap, 0,
            "a diagonal was drawn between two samples five seconds apart"
        );

        let stub: Vec<usize> = green
            .iter()
            .filter(|(_, column)| (103..=107).contains(column))
            .map(|(row, _)| *row)
            .collect();
        let span = match (stub.iter().min(), stub.iter().max()) {
            (Some(min), Some(max)) => max - min,
            _ => panic!("the line never came back after the hole"),
        };
        assert!(
            span >= 12,
            "the line jumped to the new value instead of resuming at the last \
             known one (the stub spans {span}px)"
        );
    }

    /// A one-second cadence is not a hole, and the line stays continuous.
    ///
    /// The threshold has to be loose enough that the cadence the probe loop
    /// actually keeps — a measurement plus a tick — never breaks a healthy
    /// line, and this is the half that would fail if it were tightened.
    #[test]
    fn a_one_second_cadence_is_not_a_gap() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        // The cursor would sit at the right edge of the columns under test.
        config.sample_cursor = false;
        let now = Instant::now();
        // The reveal holds the newest three seconds back and shifts the window,
        // so age 3 sits at the right edge and a second is 10px: ages 12s down
        // to 3s sit at columns 210 to 300.
        let entries: Vec<(u64, u32)> = (0..10u32)
            .map(|index| (u64::from(12 - index), 100 + index * 20))
            .collect();
        let samples = aged_samples(now, &entries);

        let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], now, true);
        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        // Every column between them has to carry a pixel.
        let missing: Vec<usize> = (212..=298)
            .filter(|column| !green.iter().any(|(_, drawn)| drawn == column))
            .collect();
        assert!(
            missing.is_empty(),
            "the line is broken at columns {missing:?} although every sample is \
             one second from its neighbour"
        );
    }

    /// Smooth mode presents the line a fixed three-sample delay behind live.
    ///
    /// The newest samples are withheld so the tip walks along each segment as
    /// time passes instead of the whole segment arriving with its sample; the
    /// reveal shifts the window so the tip's x is pinned to the axis end while
    /// its y moves.
    #[test]
    fn smooth_mode_stops_at_a_delayed_reveal_time() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.sample_cursor = false;
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // A flat run whose oldest sample is at age 12 and newest at age 3; the
        // reveal shifts the window, so the age-3 sample sits at the axis end
        // (column 300).
        let entries: Vec<(u64, u32)> = (0..10).map(|index| (12 - index, 100)).collect();
        let revealed = aged_samples(now, &entries);

        let rows_near_end = |samples: &[SamplePoint]| -> Vec<usize> {
            let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", samples)], now, true);
            let green = positions_of_color(&pixels, 300, [0, 255, 0]);
            green
                .iter()
                .filter(|(_, column)| (294..=299).contains(column))
                .map(|(row, _)| *row)
                .collect()
        };

        let flat = rows_near_end(&revealed);
        assert!(!flat.is_empty(), "the line stopped short of the axis end");

        // The same run plus a sample inside the hold: it must not be drawn, so
        // the pixels at the axis end cannot move.
        let mut held_entries = entries.clone();
        held_entries.push((2, 900));
        let held = aged_samples(now, &held_entries);
        assert_eq!(
            flat,
            rows_near_end(&held),
            "a sample inside the reveal hold was drawn"
        );
    }

    /// The tip's x is fixed by the delay while its y walks the current segment:
    /// half a second later the same column carries a later interpolation.
    #[test]
    fn the_reveal_tip_walks_the_segment_as_time_passes() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.sample_cursor = false;
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // The segment under the tip spans 100ms to 300ms, so the cut's
        // interpolation — two thirds in at `now`, five sixths at `now + 0.5s` —
        // climbs from 233ms toward 267ms. The tip's x stays at the axis end
        // (column 300), so the rows to compare are in the last few columns.
        let samples = aged_samples(now, &[(5, 100), (2, 300)]);

        let rows_near_tip = |at: Instant| -> Vec<usize> {
            let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], at, true);
            let green = positions_of_color(&pixels, 300, [0, 255, 0]);
            green
                .iter()
                .filter(|(_, column)| (294..=299).contains(column))
                .map(|(row, _)| *row)
                .collect()
        };

        let before = rows_near_tip(now);
        let after = rows_near_tip(now + Duration::from_millis(500));
        assert!(
            !before.is_empty() && !after.is_empty(),
            "the tip drew nothing"
        );
        let mean = |rows: &[usize]| rows.iter().sum::<usize>() as f64 / rows.len() as f64;
        assert!(
            mean(&before) - mean(&after) > 1.0,
            "the tip did not walk the segment: {before:?} then {after:?}"
        );
    }

    /// The line pass keeps one sample older than the visible cut, so the
    /// departing vertex maps to a negative x and the segment into the picture
    /// is clipped by the edge. Without it the polyline would start at the
    /// oldest on-screen sample and leave the first few columns empty until
    /// that sample arrived at the edge.
    #[test]
    fn the_line_reaches_the_left_edge_through_one_older_sample() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.sample_cursor = false;
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // Window 30 plus the 3s hold puts the cut at age 33 and 10px per
        // second maps age to x = 330 - 10 * age: the oldest sample here is 34s
        // old (x = -10) and the next is 32 (x = 10). Cropped without the older
        // sample the line would start at x = 10 — the first columns empty.
        let samples = aged_samples(
            now,
            &[
                (34, 100),
                (32, 100),
                (31, 100),
                (30, 100),
                (29, 100),
                (28, 100),
            ],
        );

        let leftmost = |at: Instant| -> usize {
            let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], at, true);
            let green = positions_of_color(&pixels, 300, [0, 255, 0]);
            green
                .iter()
                .map(|(_, column)| *column)
                .min()
                .expect("the line drew nothing")
        };

        let first = leftmost(now);
        assert!(first <= 1, "the line did not reach the left edge: {first}");
        // Half a second later the cut keeps a different older sample (the
        // same ones are half a second older), and the line still leaves
        // through the edge: the segment now runs from x = -15 to x = 5.
        let later = leftmost(now + Duration::from_millis(500));
        assert!(
            later <= 1,
            "the line stopped at column {later} half a second later"
        );
    }

    #[test]
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn transparent_background_stays_transparent() {
        let config = OverlayConfig::new();
        let samples = samples(&[Some(10), None, Some(20)]);
        let pixels =
            render_graph(120, 60, &config, &samples, Instant::now(), false).expect("pixmap");
        let transparent = pixels.chunks_exact(4).filter(|pixel| pixel[3] == 0).count();
        assert!(transparent > 100);
    }

    #[test]
    fn smooth_rendering_uses_timestamp_positions() {
        let now = Instant::now();
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        let samples = vec![
            SamplePoint {
                value: Some(100),
                timestamp: now - Duration::from_secs(15),
                is_prefill: false,
            },
            SamplePoint {
                value: Some(200),
                timestamp: now - Duration::from_secs(14),
                is_prefill: false,
            },
        ];
        let first = render_graph(300, 100, &config, &samples, now, true).expect("pixmap");
        let later = render_graph(
            300,
            100,
            &config,
            &samples,
            now + Duration::from_millis(500),
            true,
        )
        .expect("pixmap");
        assert_ne!(first, later);
    }

    #[test]
    fn cosmetic_prefill_values_are_deterministic_and_bounded() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 60;
        config.max_y_ms = 1_000;
        let first = cosmetic_prefill_values(&config, "t");
        let second = cosmetic_prefill_values(&config, "t");
        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert!(first.iter().all(|value| (1..=1_000).contains(value)));
    }

    /// The prefill has to look measured, not generated.
    ///
    /// The pair of sines this replaced changed direction about a dozen times
    /// across a minute and never moved more than ~4% of the axis in one
    /// sample; both numbers are what made it read as a pacemaker. The walk
    /// with jitter and spikes should flip sign far more often than that, and
    /// a spike's attack has to clear the old generator's biggest step.
    ///
    /// The window is the full sample cap rather than a minute because spikes
    /// are sparse and seeded from the overlay's id: over sixty samples there
    /// is roughly a one-in-ten chance that no attack is big enough, which is
    /// a flake about the draw, not about the generator. Over 512 samples the
    /// same threshold is met many times over.
    #[test]
    fn the_cosmetic_prefill_is_jagged_rather_than_a_curve() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 600;
        config.max_y_ms = 1_000;
        let values = cosmetic_prefill_values(&config, "t");
        assert_eq!(values.len(), 512, "the long window should fill the cap");

        let deltas: Vec<i64> = values
            .windows(2)
            .map(|pair| pair[1] as i64 - pair[0] as i64)
            .collect();
        let flips = deltas
            .windows(2)
            .filter(|pair| (pair[0] > 0) != (pair[1] > 0))
            .count();
        assert!(
            flips * 3 >= deltas.len(),
            "the prefill changed direction {flips} times across {} deltas; a smooth curve \
             flips far less often",
            deltas.len()
        );

        let largest_step = deltas
            .iter()
            .map(|delta| delta.unsigned_abs())
            .max()
            .unwrap_or(0);
        assert!(
            largest_step >= 60,
            "the largest single-sample jump was {largest_step} of 1000, too little to read \
             as a spike"
        );
    }

    /// The resting level has to read as a healthy connection.
    ///
    /// Drawn as a fraction of the axis, the generator rested at a fifth to a
    /// third of a one-second axis — 200 to 320 ms — which looks like a problem
    /// before a single real sample arrives. The level is absolute milliseconds
    /// for exactly this reason; the fraction is only what gets drawn.
    #[test]
    fn the_cosmetic_prefill_rests_at_a_healthy_latency() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 60;
        config.max_y_ms = 1_000;
        let mut values = cosmetic_prefill_values(&config, "t");
        values.sort_unstable();
        let median = values[values.len() / 2];
        assert!(
            median <= 60,
            "the middle value was {median} ms against a 1 s axis; that reads as a bad \
             connection"
        );
    }

    #[test]
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn cosmetic_prefill_reveals_progressively() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 60;
        config.prefill_line_color = "#ff00ff".to_string();
        let now = Instant::now();
        let samples = cosmetic_prefill_samples(&config, "t", now);
        let mut early = Vec::new();
        let mut complete = Vec::new();
        let mut points = Vec::new();
        assert!(render_prefill_into(
            300,
            100,
            &config,
            &samples,
            now,
            0.25,
            None,
            &mut points,
            &mut early,
        ));
        assert!(render_prefill_into(
            300,
            100,
            &config,
            &samples,
            now,
            1.0,
            None,
            &mut points,
            &mut complete,
        ));
        let early_pixels = early.chunks_exact(4).filter(|pixel| pixel[3] > 0).count();
        let complete_pixels = complete
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(complete_pixels > early_pixels);
    }

    #[test]
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn prefill_history_and_real_samples_render_together() {
        let now = Instant::now();
        let mut config = OverlayConfig::new();
        config.window_seconds = 60;
        config.first_target_mut().line_color = "#00ff00".to_string();
        config.prefill_line_color = "#ff00ff".to_string();
        let mut samples = cosmetic_prefill_samples(&config, "t", now - Duration::from_secs(2));
        samples.push(SamplePoint {
            value: Some(500),
            timestamp: now - Duration::from_secs(1),
            is_prefill: false,
        });
        samples.push(SamplePoint {
            value: Some(450),
            timestamp: now,
            is_prefill: false,
        });
        let mut pixels = Vec::new();
        assert!(render_graph_into(
            300,
            100,
            &config,
            &samples,
            now,
            true,
            &mut pixels,
        ));
        let magenta = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 20 && pixel[0] > 80 && pixel[2] > 80 && pixel[1] < 100)
            .count();
        let green = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 20 && pixel[1] > 80 && pixel[0] < 100 && pixel[2] < 100)
            .count();
        assert!(magenta > 0);
        assert!(green > 0);
    }

    #[test]
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn prefill_to_real_transition_is_connected() {
        let now = Instant::now();
        let mut config = OverlayConfig::new();
        config.window_seconds = 3;
        config.first_target_mut().line_color = "#00ff00".to_string();
        config.prefill_line_color = "#ff00ff".to_string();
        let samples = vec![
            SamplePoint {
                value: Some(100),
                timestamp: now - Duration::from_secs(2),
                is_prefill: true,
            },
            SamplePoint {
                value: Some(100),
                timestamp: now - Duration::from_secs(1),
                is_prefill: true,
            },
            SamplePoint {
                value: Some(500),
                timestamp: now,
                is_prefill: false,
            },
        ];
        let pixels = render_graph(300, 100, &config, &samples, now, false).expect("pixmap");
        let magenta_near_transition = pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(index, pixel)| {
                let x = index % 300;
                (235..=245).contains(&x)
                    && pixel[3] > 20
                    && pixel[0] > 80
                    && pixel[2] > 80
                    && pixel[1] < 100
            })
            .count();
        let green_at_transition = pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(index, pixel)| {
                index % 300 == 250
                    && pixel[3] > 20
                    && pixel[1] > 80
                    && pixel[0] < 100
                    && pixel[2] < 100
            })
            .count();
        assert!(magenta_near_transition > 0);
        assert!(green_at_transition < 15);
    }

    #[test]
    fn smooth_timeout_marker_moves_with_timestamp() {
        let now = Instant::now();
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        config.first_target_mut().timeout_color = "#ff0000".to_string();
        let samples = vec![SamplePoint {
            value: None,
            timestamp: now - Duration::from_secs(15),
            is_prefill: false,
        }];
        let width = 300;
        let height = 100;
        let red_x_bounds = |pixels: &[u8]| {
            let mut min_x = width;
            let mut max_x = 0;
            for y in 0..height {
                for x in 0..width {
                    let offset = ((y * width + x) * 4) as usize;
                    let pixel = &pixels[offset..offset + 4];
                    if pixel[3] > 20 && pixel[0] > 100 && pixel[1] < 100 && pixel[2] < 100 {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x);
                    }
                }
            }
            (min_x, max_x)
        };
        let first = render_graph(width, height, &config, &samples, now, true).expect("pixmap");
        let later = render_graph(
            width,
            height,
            &config,
            &samples,
            now + Duration::from_millis(500),
            true,
        )
        .expect("pixmap");
        let first_bounds = red_x_bounds(&first);
        let later_bounds = red_x_bounds(&later);
        assert!(
            first_bounds.0 < width && first_bounds.1 > 0,
            "no timeout pixels"
        );
        assert!(
            later_bounds.1 < first_bounds.1,
            "timeout marker did not move left"
        );
    }

    /// A marker runs the full height of the canvas, through the band the
    /// underglow reserves below y0.
    ///
    /// The reserve exists only for the cast; without this the marker stops at
    /// the zero line and a timeout partly disappears behind a glow.
    #[test]
    fn a_timeout_marker_reaches_through_the_underglow_reserve() {
        let mut config = OverlayConfig::new();
        config.line_glow = true;
        config.line_glow_radius_px = 10;
        config.first_target_mut().line_color = "#00ff00".to_string();
        config.first_target_mut().timeout_color = "#ff0000".to_string();
        let width = 300;
        let height = 100 + line_glow_reserve_px(&config) as usize;
        let pixels = render_graph(
            width as u32,
            height as u32,
            &config,
            &samples(&[None]),
            Instant::now(),
            false,
        )
        .expect("pixmap");
        let deepest = positions_of_color(&pixels, width, [255, 0, 0])
            .into_iter()
            .map(|(row, _)| row)
            .max()
            .expect("the timeout marker was not drawn");
        assert!(
            deepest >= height - 3,
            "the marker stopped at row {deepest}; the canvas is {height} tall"
        );
    }

    /// The pad that keeps a stroke off the pixmap edge: exactly the old 2px at
    /// the default width, growing by half the extra stroke.
    #[test]
    fn the_stroke_pad_keeps_the_default_and_grows_with_the_stroke() {
        assert_eq!(stroke_pad(MIN_LINE_STROKE_PX), 2.0);
        assert_eq!(stroke_pad(DEFAULT_LINE_STROKE_PX), 2.0);
        assert_eq!(stroke_pad(MAX_LINE_STROKE_PX), 3.5);
    }

    /// A wider stroke draws a wider line: the count of rows the line covers in
    /// one column follows the setting.
    #[test]
    fn a_thicker_stroke_draws_a_thicker_line() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.first_target_mut().line_color = "#00ff00".to_string();
        let now = Instant::now();
        let samples = samples(&[Some(500); 40]);
        let rows_at = |config: &OverlayConfig| {
            let pixels = render_graph(300, 100, config, &samples, now, false).expect("pixmap");
            let rows: std::collections::BTreeSet<usize> =
                positions_of_color(&pixels, 300, [0, 255, 0])
                    .into_iter()
                    .filter(|(_, column)| *column == 150)
                    .map(|(row, _)| row)
                    .collect();
            rows.len()
        };

        let thin = rows_at(&config);
        assert!(
            (2..=3).contains(&thin),
            "a 1.5px stroke covered {thin} rows in one column"
        );
        config.line_stroke_px = 5.0;
        let thick = rows_at(&config);
        assert!(
            thick >= 5,
            "a 5px stroke only covered {thick} rows in one column"
        );
    }

    /// The timeout markers are stroked with the same width as the lines.
    #[test]
    fn timeout_markers_follow_the_stroke_width() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.first_target_mut().timeout_color = "#ff0000".to_string();
        let now = Instant::now();
        let samples = samples(&[None]);
        let columns_at_row = |config: &OverlayConfig| {
            let pixels = render_graph(300, 100, config, &samples, now, false).expect("pixmap");
            positions_of_color(&pixels, 300, [255, 0, 0])
                .into_iter()
                .filter(|(row, _)| *row == 50)
                .count()
        };

        let thin = columns_at_row(&config);
        assert!(
            (1..=3).contains(&thin),
            "a 1.5px marker covered {thin} columns in one row"
        );
        config.line_stroke_px = 5.0;
        let thick = columns_at_row(&config);
        assert!(
            thick >= 5,
            "a 5px marker only covered {thick} columns in one row"
        );
    }

    /// A thick stroke clamped to the ceiling or resting on the zero line is not
    /// sliced by the pixmap edge: the outermost row it reaches keeps its
    /// antialiased half coverage instead of a hard cut.
    #[test]
    fn a_thick_stroke_at_the_axis_edges_is_not_sliced() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.line_stroke_px = MAX_LINE_STROKE_PX;
        // The cursor's room would inset the line from both edges and make the
        // edge assertions pass for the wrong reason.
        config.sample_cursor = false;
        let now = Instant::now();
        let alpha_at = |pixels: &[u8], row: usize, column: usize| -> u8 {
            pixels[(row * 300 + column) * 4 + 3]
        };

        // Clamped at the ceiling.
        let pixels = render_graph(300, 100, &config, &samples(&[Some(1_000); 40]), now, false)
            .expect("pixmap");
        let top_alpha = alpha_at(&pixels, 0, 150);
        assert!(
            top_alpha < 200,
            "the top row is {top_alpha} opaque; the line was sliced flat"
        );

        // Resting on the zero line.
        let pixels =
            render_graph(300, 100, &config, &samples(&[Some(0); 40]), now, false).expect("pixmap");
        let bottom_alpha = alpha_at(&pixels, 99, 150);
        assert!(
            (1..200).contains(&bottom_alpha),
            "the bottom row is {bottom_alpha} opaque; the line was clipped"
        );
    }

    #[test]
    fn timeout_marker_follows_graph_orientation() {
        for (orientation, expected) in [(0, "left"), (180, "right"), (90, "bottom"), (270, "top")] {
            let mut config = OverlayConfig::new();
            config.orientation = orientation;
            config.window_seconds = 30;
            config.first_target_mut().line_color = "#00ff00".to_string();
            config.first_target_mut().timeout_color = "#ff0000".to_string();
            let (width, height) = if matches!(orientation, 90 | 270) {
                (100, 300)
            } else {
                (300, 100)
            };
            let pixels = render_graph(
                width,
                height,
                &config,
                &samples(&[None]),
                Instant::now(),
                false,
            )
            .expect("pixmap");
            let mut min_x = width;
            let mut min_y = height;
            let mut max_x = 0;
            let mut max_y = 0;
            for y in 0..height {
                for x in 0..width {
                    let offset = ((y * width + x) * 4) as usize;
                    let pixel = &pixels[offset..offset + 4];
                    if pixel[3] > 100 && pixel[0] > 120 && pixel[1] < 100 && pixel[2] < 100 {
                        min_x = min_x.min(x);
                        min_y = min_y.min(y);
                        max_x = max_x.max(x);
                        max_y = max_y.max(y);
                    }
                }
            }
            assert!(max_x >= min_x && max_y >= min_y, "no timeout pixels");
            let horizontal = max_x - min_x > 80;
            let vertical = max_y - min_y > 80;
            match expected {
                "right" => {
                    assert!(vertical && !horizontal);
                    assert!(min_x > width / 4);
                }
                "left" => {
                    assert!(vertical && !horizontal);
                    assert!(max_x < width * 3 / 4);
                }
                "bottom" => {
                    assert!(horizontal && !vertical);
                    assert!(min_y > height / 4);
                }
                "top" => {
                    assert!(horizontal && !vertical);
                    assert!(max_y < height * 3 / 4);
                }
                _ => unreachable!(),
            }
        }
    }

    /// The cast falls from the line toward the zero line in the graph's own
    /// frame — down at 0 degrees, right at 90, and so on — and never shows on
    /// the far side of the line, whether or not the overlay is mirrored.
    #[test]
    fn the_underglow_casts_toward_the_zero_line_in_every_orientation() {
        for (orientation, mirrored) in [
            (0u16, false),
            (0, true),
            (90, false),
            (90, true),
            (180, false),
            (180, true),
            (270, false),
            (270, true),
        ] {
            let mut config = OverlayConfig::new();
            config.line_glow = true;
            config.line_glow_radius_px = 4;
            config.line_glow_intensity = 100;
            config.orientation = orientation;
            config.mirrored = mirrored;
            config.first_target_mut().line_color = "#00ff00".to_string();

            let (width, height) = if matches!(orientation, 90 | 270) {
                (100u32, 300u32)
            } else {
                (300u32, 100u32)
            };
            let pixels = render_graph(
                width,
                height,
                &config,
                &samples(&[Some(500); 40]),
                Instant::now(),
                false,
            )
            .expect("pixmap");

            let long_px = if matches!(orientation, 90 | 270) {
                height as f32
            } else {
                width as f32
            };
            let short_px = if matches!(orientation, 90 | 270) {
                width as f32
            } else {
                height as f32
            };
            let pad = 2.0;
            let bottom = short_px - pad - line_glow_reserve_px(&config) as f32;
            let line_y = bottom - 0.5 * (bottom - pad);
            let center = transform_point(
                (long_px / 2.0, line_y),
                long_px,
                short_px,
                orientation,
                mirrored,
            );
            let dir = glow_direction(orientation, mirrored);

            let positions = positions_of_color(&pixels, width as usize, [0, 255, 0]);
            assert!(
                !positions.is_empty(),
                "the line itself is missing at {orientation}/{mirrored}"
            );
            let mut deepest = f32::MIN;
            let mut shallowest = f32::MAX;
            for (row, column) in positions {
                // Pixel centres, not corners: the line's own antialiased back
                // edge sits half a pixel behind the centre, and the far-side
                // assertion must not mistake it for a cast that leaked.
                let delta = (column as f32 + 0.5 - center.0) * dir.0
                    + (row as f32 + 0.5 - center.1) * dir.1;
                deepest = deepest.max(delta);
                shallowest = shallowest.min(delta);
            }
            assert!(
                deepest > 1.0,
                "no cast on the zero-line side at {orientation}/{mirrored}"
            );
            assert!(
                deepest <= config.line_glow_radius_px as f32 + 2.0,
                "the cast ran past its radius at {orientation}/{mirrored}: {deepest}"
            );
            assert!(
                shallowest > -1.5,
                "the cast leaked to the far side at {orientation}/{mirrored}: {shallowest}"
            );
        }
    }

    /// A cast is a shadow: no glow may reach past the line on the far side of
    /// the cast's direction, however sharp the spike it falls from.
    ///
    /// A stroke that grows wider with depth also grows sideways, and its miter
    /// joins spike outward at a corner — so a bright overlay used to wrap glow
    /// over the top of every peak. The sweep only ever translates the path
    /// along the cast direction; this compares the lit render against the same
    /// line drawn without any glow, and no visible pixel may appear on the far
    /// side of where the bare line reaches.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    #[test]
    fn the_underglow_does_not_reach_above_the_line_at_a_spike() {
        for (orientation, mirrored) in [
            (0u16, false),
            (0, true),
            (90, false),
            (90, true),
            (180, false),
            (180, true),
            (270, false),
            (270, true),
        ] {
            let spiky: Vec<Option<u32>> = (0..41)
                .map(|index| Some(if index == 20 { 900 } else { 100 }))
                .collect();
            let points = samples(&spiky);
            let mut config = OverlayConfig::new();
            config.line_glow = true;
            config.line_glow_radius_px = 20;
            config.line_glow_intensity = 100;
            config.orientation = orientation;
            config.mirrored = mirrored;
            config.first_target_mut().line_color = "#00ff00".to_string();
            // The cursor is in both renders, and its own corners reach on both
            // sides of the line: leaving it in would flatten the comparison.
            config.sample_cursor = false;

            let (width, height) = if matches!(orientation, 90 | 270) {
                (100u32, 300u32)
            } else {
                (300u32, 100u32)
            };
            let lit = render_graph(width, height, &config, &points, Instant::now(), false)
                .expect("pixmap");
            // Keep the reserve, drop only the strength, so both renders share
            // the same axis geometry — turning the glow off would move the
            // line up by the whole reserved band.
            config.line_glow_intensity = 0;
            let bare = render_graph(width, height, &config, &points, Instant::now(), false)
                .expect("pixmap");

            let dir = glow_direction(orientation, mirrored);
            // The furthest reach along the far side of the cast: the smallest
            // projection onto the cast direction among visible pixels.
            let reach = |pixels: &[u8]| {
                let mut furthest = f32::MAX;
                for (index, pixel) in pixels.chunks_exact(4).enumerate() {
                    if pixel[3] < 24 {
                        continue;
                    }
                    let column = (index % width as usize) as f32 + 0.5;
                    let row = (index / width as usize) as f32 + 0.5;
                    furthest = furthest.min(column * dir.0 + row * dir.1);
                }
                furthest
            };
            let line_reach = reach(&bare);
            let lit_reach = reach(&lit);
            assert!(
                lit_reach >= line_reach - 1.5,
                "the cast reached past the line at {orientation}/{mirrored}: \
                 lit {lit_reach} vs line {line_reach}"
            );
        }
    }

    /// Every host's cast is drawn in its own line's colour, the same rule the
    /// lines themselves follow.
    #[test]
    fn the_underglow_uses_each_hosts_own_line_colour() {
        let mut config = OverlayConfig::new();
        config.line_glow = true;
        config.line_glow_radius_px = 4;
        // This test computes the line's row from the plain pad and reserve; the
        // cursor's room would move it.
        config.sample_cursor = false;
        let now = Instant::now();
        let upper = samples(&[Some(300); 40]);
        let lower = samples(&[Some(700); 40]);
        let pixels = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &upper),
                ("#0000ff", "#ff0000", &lower),
            ],
            now,
            false,
        );

        let bottom = 100.0 - 2.0 - line_glow_reserve_px(&config) as f32;
        for (color, value) in [([0u8, 255u8, 0u8], 300u32), ([0u8, 0u8, 255u8], 700u32)] {
            let line_y = bottom - (value as f32 / 1_000.0) * (bottom - 2.0);
            let row = (line_y + 2.5).round() as usize;
            let cast = positions_of_color(&pixels, 300, color);
            assert!(
                cast.iter().any(|(cast_row, _)| *cast_row == row),
                "no {color:?} cast at row {row}"
            );
        }
    }

    /// The cast is brightest against the line and fades with depth; a layer
    /// stack that fell flat would read as one hard band.
    #[test]
    fn the_underglow_fades_with_depth_below_the_line() {
        let mut config = OverlayConfig::new();
        config.line_glow = true;
        config.line_glow_radius_px = 4;
        config.first_target_mut().line_color = "#00ff00".to_string();
        let pixels = render_graph(
            300,
            100,
            &config,
            &samples(&[Some(500); 40]),
            Instant::now(),
            false,
        )
        .expect("pixmap");

        let bottom = 100.0 - 2.0 - line_glow_reserve_px(&config) as f32;
        let line_y = bottom - 0.5 * (bottom - 2.0);
        let alpha_at = |row: usize| pixels[(row * 300 + 150) * 4 + 3];
        let first = (line_y + 1.5).round() as usize;
        let faded: Vec<u8> = (first..first + 4).map(alpha_at).collect();
        assert!(
            faded[0] > faded[1] && faded[1] > faded[2] && faded[2] >= faded[3],
            "the cast does not fade with depth: {faded:?}"
        );
    }

    /// A later host's cast cannot tint an earlier host's line.
    ///
    /// The second host is the upper one, so its cast falls across the first
    /// host's core. If each series were stroked glow-then-core in one pass,
    /// that cast would paint over a core that was already down, and the first
    /// host's line would come out a blend of two colours.
    #[test]
    fn another_hosts_glow_does_not_tint_an_earlier_hosts_line() {
        let mut config = OverlayConfig::new();
        config.line_glow = true;
        config.line_glow_radius_px = 4;
        let now = Instant::now();
        let lower = samples(&[Some(500); 40]);
        let upper = samples(&[Some(522); 40]);
        let pixels = render_hosts(
            &config,
            &[
                ("#00ff00", "#ff0000", &lower),
                ("#0000ff", "#ff0000", &upper),
            ],
            now,
            false,
        );
        let core_row = 47;
        let run = positions_of_color(&pixels, 300, [0, 255, 0])
            .iter()
            .filter(|(row, _)| *row == core_row)
            .count();
        assert!(
            run > 100,
            "the earlier host's core was tinted by the later host's glow: {run} pure pixels"
        );
    }

    /// The cursor is anchored to the box, not to the sample: its base lies on
    /// the leading edge and its apex reaches `size` back from it, in every
    /// orientation and mirror — the triangle is built in graph coordinates and
    /// transformed like the line, so it follows the rotation rather than
    /// staying upright.
    #[test]
    fn the_sample_cursor_sits_flush_at_the_leading_edge() {
        for (orientation, mirrored) in [
            (0u16, false),
            (0, true),
            (90, false),
            (90, true),
            (180, false),
            (180, true),
            (270, false),
            (270, true),
        ] {
            let mut config = OverlayConfig::new();
            config.orientation = orientation;
            config.mirrored = mirrored;
            config.window_seconds = 30;
            config.first_target_mut().line_color = "#00ff00".to_string();
            let now = Instant::now();
            let points = samples(&[Some(200); 20]);
            let (width, height) = if matches!(orientation, 90 | 270) {
                (100u32, 300u32)
            } else {
                (300u32, 100u32)
            };
            let pixels = render_graph(width, height, &config, &points, now, false).expect("pixmap");
            let white = positions_of_color(&pixels, width as usize, [255, 255, 255]);
            assert!(
                !white.is_empty(),
                "no cursor was drawn at {orientation}/{mirrored}"
            );

            // Where the triangle should be, from the same reserves the
            // renderer reads rather than from hardcoded numbers. The line is
            // flat, so the apex sits on the flat value's y wherever it lands.
            let (long_px, short_px) = if matches!(orientation, 90 | 270) {
                (height as f32, width as f32)
            } else {
                (width as f32, height as f32)
            };
            let pad = stroke_pad(config.line_stroke_px);
            let top = pad + sample_cursor_room_px(&config) as f32;
            let bottom = short_px
                - pad
                - line_glow_reserve_px(&config) as f32
                - sample_cursor_room_px(&config) as f32;
            let line_y = bottom - 0.2 * (bottom - top);
            let size = sample_cursor_size(&config) as f32;
            let base_x = long_px - SAMPLE_CURSOR_EDGE_MARGIN;
            let apex_x = base_x - size;

            let nearest = |target: (f32, f32)| {
                white
                    .iter()
                    .map(|(row, column)| {
                        let dx = *column as f32 + 0.5 - target.0;
                        let dy = *row as f32 + 0.5 - target.1;
                        (dx * dx + dy * dy).sqrt()
                    })
                    .fold(f32::MAX, f32::min)
            };

            let apex = transform_point((apex_x, line_y), long_px, short_px, orientation, mirrored);
            let apex_distance = nearest(apex);
            // The rim wraps the acute tip, so the nearest white pixel sits a
            // few pixels inside the vertex; the old geometry (apex on the
            // newest sample) would miss by tens.
            assert!(
                apex_distance <= 4.0,
                "the apex is {apex_distance:.1}px from the edge anchor at \
                 {orientation}/{mirrored}"
            );
            let half = size * 0.7;
            for corner in [(base_x, line_y - half), (base_x, line_y + half)] {
                let corner = transform_point(corner, long_px, short_px, orientation, mirrored);
                let corner_distance = nearest(corner);
                assert!(
                    corner_distance <= 5.0,
                    "a base corner is {corner_distance:.1}px from the edge at \
                     {orientation}/{mirrored}"
                );
            }
        }
    }

    /// A bigger cursor grows back along the line: the base stays on the edge
    /// and the apex sits further in, so the size decides how far back it
    /// points.
    #[test]
    fn the_sample_cursor_apex_moves_with_its_size() {
        let now = Instant::now();
        let points = samples(&[Some(200); 20]);
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        config.line_glow = false;
        config.first_target_mut().line_color = "#00ff00".to_string();
        let min_column = |size: u32, config: &mut OverlayConfig| {
            config.sample_cursor_size_px = size;
            let pixels = render_graph(300, 100, config, &points, now, false).expect("pixmap");
            let white = positions_of_color(&pixels, 300, [255, 255, 255]);
            assert!(!white.is_empty(), "no cursor was drawn at size {size}");
            white
                .iter()
                .map(|(_, column)| *column)
                .min()
                .expect("columns")
        };
        let small = min_column(8, &mut config);
        let large = min_column(20, &mut config);
        // The rim covers a few pixels of the triangle's tip, so the leftmost
        // white column names the apex within the rim's reach; what must hold
        // exactly is that the apex moves back by the difference in size.
        assert!(
            (small as i32 - (299 - 8)).abs() <= 4,
            "size 8 put the apex at {small}"
        );
        assert!(
            (large as i32 - (299 - 20)).abs() <= 4,
            "size 20 put the apex at {large}"
        );
        assert!(
            (small as i32 - large as i32 - 12).abs() <= 1,
            "the apex must move back by the difference in size: {small} vs {large}"
        );
    }

    /// The cursor's easing: the first frame snaps, a short step lands between
    /// the old and new targets, and a long one arrives and settles.
    #[test]
    fn the_cursor_animation_eases_toward_its_target() {
        let mut animation = CursorAnimation::default();
        let start = Instant::now();
        assert_eq!(
            animation.advance(start, 100.0),
            100.0,
            "the first frame has nothing to ease from"
        );
        assert!(!animation.is_active());

        let mid = animation.advance(start + Duration::from_millis(50), 200.0);
        assert!(mid > 100.0 && mid < 200.0, "a short step jumped: {mid}");
        assert!(animation.is_active(), "a moving cursor must keep its clock");

        let settled = animation.advance(start + Duration::from_secs(1), 200.0);
        assert_eq!(settled, 200.0, "a long step must arrive");
        assert!(
            !animation.is_active(),
            "a settled cursor must stop asking for frames"
        );
    }

    /// A trailing timeout does not move the cursor: it keeps pointing at the
    /// last value the line actually drew.
    #[test]
    fn the_sample_cursor_stays_on_the_last_drawn_value_through_a_timeout() {
        let mut config = OverlayConfig::new();
        config.window_seconds = 10;
        config.first_target_mut().line_color = "#00ff00".to_string();
        let now = Instant::now();
        let mut timed_out = vec![Some(200); 9];
        timed_out.push(None);
        let valued = samples(&[Some(200); 9]);
        let interrupted = samples(&timed_out);
        let (width, height) = (300u32, 100u32);
        let valued_pixels =
            render_graph(width, height, &config, &valued, now, false).expect("pixmap");
        let interrupted_pixels =
            render_graph(width, height, &config, &interrupted, now, false).expect("pixmap");
        let valued_cursor = positions_of_color(&valued_pixels, 300, [255, 255, 255]);
        let interrupted_cursor = positions_of_color(&interrupted_pixels, 300, [255, 255, 255]);
        assert!(
            !valued_cursor.is_empty() && !interrupted_cursor.is_empty(),
            "the cursor is missing"
        );
        assert_eq!(
            valued_cursor, interrupted_cursor,
            "the trailing timeout moved the cursor off the last drawn value"
        );
    }

    /// A cursor on the zero line or clamped at the ceiling is not sliced: the
    /// room reserved at both ends of the axis belongs to the triangle.
    #[test]
    fn a_cursor_on_the_zero_line_or_the_ceiling_stays_whole() {
        let mut config = OverlayConfig::new();
        config.line_glow = false;
        config.first_target_mut().line_color = "#00ff00".to_string();
        let now = Instant::now();
        // The rim stroke covers the thin apex, so the white interior of a whole
        // triangle spans fewer rows than the triangle's height. "Whole" is
        // therefore measured against the same cursor at mid-axis, where no
        // clipping is possible, rather than an absolute number.
        let span_of = |pixels: &[u8]| {
            let rows: Vec<usize> = positions_of_color(pixels, 300, [255, 255, 255])
                .iter()
                .map(|(row, _)| *row)
                .collect();
            assert!(!rows.is_empty(), "no cursor was drawn");
            rows.iter().max().expect("rows") - rows.iter().min().expect("rows") + 1
        };
        let mid = render_graph(300, 100, &config, &samples(&[Some(500); 20]), now, false)
            .expect("pixmap");
        let whole_span = span_of(&mid);
        for (value, edge_row) in [(0u32, 99usize), (1_000, 0usize)] {
            let pixels = render_graph(300, 100, &config, &samples(&[Some(value); 20]), now, false)
                .expect("pixmap");
            let white = positions_of_color(&pixels, 300, [255, 255, 255]);
            let span = span_of(&pixels);
            assert!(
                span + 1 >= whole_span,
                "the triangle was sliced at value {value}: it spans {span} rows \
                 against {whole_span} at mid-axis"
            );
            assert!(
                !white.iter().any(|(row, _)| *row == edge_row),
                "the cursor reached the pixmap edge at value {value}"
            );
        }
    }
}
