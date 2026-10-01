use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use tiny_skia::{IntSize, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::border::{self, BorderVisual};
use crate::config::OverlayConfig;

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
}

/// One graph tick: the rate the sampler writes samples at.
///
/// It lives here, next to the renderer that reads a gap between samples as a
/// break, rather than in `probes.rs`; the probe loop reads it from here so the
/// cadence samples are written at and the cadence a gap is measured against
/// cannot drift apart.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

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
    /// Scheduling slack, and room for the ICMP path's name resolution, which
    /// runs outside the `IcmpSendEcho2` timeout.
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
    series: &[Series<'_>],
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
    let series = [Series {
        line_color: &target.line_color,
        timeout_color: &target.timeout_color,
        samples,
        max_sample_gap: sample_gap_threshold(target.timeout_ms),
    }];
    render_series_into_with_border(width, height, config, &series, now, smooth, border, pixels)
}

#[allow(clippy::too_many_arguments)]
pub fn render_series_into_with_border(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    series: &[Series<'_>],
    now: Instant,
    smooth: bool,
    border: Option<&BorderVisual>,
    pixels: &mut Vec<u8>,
) -> bool {
    render_graph_into_internal(width, height, config, series, now, smooth, border, pixels)
}

#[allow(clippy::too_many_arguments)]
fn render_graph_into_internal(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    series: &[Series<'_>],
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
    let visible_samples = config.window_seconds.max(1) as usize;
    let window_duration = Duration::from_secs(visible_samples as u64);
    let step = long_px / visible_samples as f32;

    let y_max = config.max_y_ms.max(1) as f32;
    let pad = 2.0;
    let top = pad;
    let bottom = short_px - pad;
    let map_y = |value: u32| {
        let t = (value as f32).min(y_max) / y_max;
        bottom - t * (bottom - top)
    };
    let map_x = |index: usize, sample: &SamplePoint| -> Option<f32> {
        if smooth {
            // Move every point by its real age so the whole graph scrolls as
            // one surface; index-based positions would jump when a sample
            // arrives at the right edge.
            let age = now.saturating_duration_since(sample.timestamp);
            if age > window_duration {
                return None;
            }
            Some(long_px * (1.0 - age.as_secs_f32() / window_duration.as_secs_f32()))
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
    fn visible(
        samples: &[SamplePoint],
        smooth: bool,
        now: Instant,
        window_duration: Duration,
        visible_samples: usize,
    ) -> &[SamplePoint] {
        let start = if smooth {
            samples.partition_point(|sample| {
                now.saturating_duration_since(sample.timestamp) > window_duration
            })
        } else {
            samples.len().saturating_sub(visible_samples)
        };
        &samples[start..]
    }

    let stroke = Stroke {
        width: 1.5,
        ..Stroke::default()
    };

    // Timeout markers first, all series, so a line drawn afterwards sits on top
    // of them rather than being cut by one.
    for entry in series {
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
        let samples = visible(entry.samples, smooth, now, window_duration, visible_samples);
        for (index, sample) in samples.iter().enumerate() {
            let Some(x) = map_x(index, sample) else {
                continue;
            };
            if sample.value.is_none() {
                let start = transform_point(
                    (x, top),
                    long_px,
                    short_px,
                    config.orientation,
                    config.mirrored,
                );
                let end = transform_point(
                    (x, bottom),
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

    // The prefill colour is shared by every target: it is the overlay's
    // cosmetic line colour, and one muted colour for the whole reveal reads as
    // one event rather than as N unrelated fake graphs.
    let mut prefill_line_paint = Paint::default();
    let [r, g, b] = parse_hex_color(&config.prefill_line_color, [100, 116, 139]);
    prefill_line_paint.set_color_rgba8(r, g, b, 255);

    for entry in series {
        let samples = visible(entry.samples, smooth, now, window_duration, visible_samples);
        if samples.is_empty() {
            continue;
        }
        let mut real_line_paint = Paint::default();
        let [r, g, b] = parse_hex_color(entry.line_color, [74, 222, 128]);
        real_line_paint.set_color_rgba8(r, g, b, 255);
        draw_series(
            &mut pixmap,
            samples,
            entry.max_sample_gap,
            &real_line_paint,
            &prefill_line_paint,
            &stroke,
            &map_x,
            &map_y,
            long_px,
            short_px,
            config.orientation,
            config.mirrored,
        );
    }

    if let Some(border) = border {
        border::draw_border(&mut pixmap, width, height, border);
    }
    *pixels = pixmap.take();
    true
}

/// Stroke one target's line, breaking it at timeouts, data gaps and prefill
/// boundaries.
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
    prefill_line_paint: &Paint,
    stroke: &Stroke,
    map_x: &dyn Fn(usize, &SamplePoint) -> Option<f32>,
    map_y: &dyn Fn(u32) -> f32,
    long_px: f32,
    short_px: f32,
    orientation: u16,
    mirrored: bool,
) {
    let paint_for = |prefill: Option<bool>| {
        if prefill == Some(true) {
            prefill_line_paint
        } else {
            real_line_paint
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
                    pixmap.stroke_path(
                        &path,
                        paint_for(segment_prefill),
                        stroke,
                        Transform::identity(),
                        None,
                    );
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
                    pixmap.stroke_path(
                        &path,
                        paint_for(segment_prefill),
                        stroke,
                        Transform::identity(),
                        None,
                    );
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
                    pixmap.stroke_path(
                        &path,
                        paint_for(previous_segment_prefill),
                        stroke,
                        Transform::identity(),
                        None,
                    );
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
                    pixmap.stroke_path(
                        &path,
                        paint_for(previous_segment_prefill),
                        stroke,
                        Transform::identity(),
                        None,
                    );
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
            pixmap.stroke_path(
                &path,
                paint_for(segment_prefill),
                stroke,
                Transform::identity(),
                None,
            );
        }
    }
}

const PREFILL_MAX_SAMPLES: usize = 512;

/// Build a deterministic, plausible-looking cosmetic latency curve.
///
/// Seeded from `seed` rather than from the overlay alone. With one target the
/// overlay id is enough; with several, two targets sharing a curve would look
/// like one host with a fat line, which is the thing a group exists to
/// disambiguate.
pub fn cosmetic_prefill_values(config: &OverlayConfig, seed: &str) -> Vec<u32> {
    let count = config.window_seconds.max(1).min(PREFILL_MAX_SAMPLES as u32) as usize;
    let mut hasher = DefaultHasher::new();
    config.id.hash(&mut hasher);
    seed.hash(&mut hasher);
    let phase = (hasher.finish() % 360) as f32 * (std::f32::consts::PI / 180.0);
    let max_y = config.max_y_ms.max(1) as f32;

    (0..count)
        .map(|index| {
            let t = if count <= 1 {
                0.0
            } else {
                index as f32 / (count - 1) as f32
            };
            let wave = (t * std::f32::consts::TAU * 1.5 + phase).sin();
            let detail = (t * std::f32::consts::TAU * 4.0 + phase * 0.37).sin();
            let normalized = (0.38 + wave * 0.14 + detail * 0.05).clamp(0.05, 0.85);
            (normalized * max_y).round().max(1.0) as u32
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
    let series = [Series {
        line_color,
        timeout_color: &config.first_target().timeout_color,
        samples: points,
        max_sample_gap: sample_gap_threshold(config.first_target().timeout_ms),
    }];
    render_series_into_with_border(width, height, config, &series, now, true, border, pixels)
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

    let (sin, cos) = match orientation {
        90 => (1.0, 0.0),
        180 => (0.0, -1.0),
        270 => (-1.0, 0.0),
        _ => (0.0, 1.0),
    };
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
    use crate::config::OverlayConfig;

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
        let series: Vec<Series<'_>> = entries
            .iter()
            .map(|(line, timeout, points)| Series {
                line_color: line,
                timeout_color: timeout,
                samples: points,
                max_sample_gap: sample_gap_threshold(config.first_target().timeout_ms),
            })
            .collect();
        let mut pixels = Vec::new();
        assert!(render_series_into(
            300,
            100,
            config,
            &series,
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
        // Time runs right to left, so the newest sample is at the right edge and
        // a sample `oldest_age` old is that fraction of the width in from it.
        let expected_left = 300 - (300.0 * oldest_age as f32 / 30.0) as usize;
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
        config.window_seconds = 30;
        config.max_y_ms = 1_000;
        let now = Instant::now();
        // Three samples at 100ms, an eight-second hole, three at 300ms. In a
        // 300px / 30s window the sample before the hole is at column 200 and
        // the one after it at column 280.
        let samples = aged_samples(
            now,
            &[
                (12, 100),
                (11, 100),
                (10, 100),
                (2, 300),
                (1, 300),
                (0, 300),
            ],
        );

        let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], now, true);
        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        assert!(!green.is_empty(), "the host drew nothing");

        let drawn_in_gap = green
            .iter()
            .filter(|(_, column)| (203..=277).contains(column))
            .count();
        assert_eq!(
            drawn_in_gap, 0,
            "a line was drawn across the eight-second hole (columns 200 to 280)"
        );

        // And it resumes at the last known value rather than jumping: the stub
        // at the first sample after the hole spans the old and the new level.
        let stub: Vec<usize> = green
            .iter()
            .filter(|(_, column)| (278..=282).contains(column))
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
        let now = Instant::now();
        let entries: Vec<(u64, u32)> = (0..10u32)
            .map(|index| (u64::from(9 - index), 100 + index * 20))
            .collect();
        let samples = aged_samples(now, &entries);

        let pixels = render_hosts(&config, &[("#00ff00", "#ff0000", &samples)], now, true);
        let green = positions_of_color(&pixels, 300, [0, 255, 0]);
        // Ages 9s down to 0s sit at columns 210 to 300; every column between
        // them has to carry a pixel.
        let missing: Vec<usize> = (212..=298)
            .filter(|column| !green.iter().any(|(_, drawn)| drawn == column))
            .collect();
        assert!(
            missing.is_empty(),
            "the line is broken at columns {missing:?} although every sample is \
             one second from its neighbour"
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
}
