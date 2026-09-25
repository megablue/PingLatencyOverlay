use std::time::{Duration, Instant};

use tiny_skia::{IntSize, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::config::OverlayConfig;

#[derive(Clone, Copy, Debug)]
pub struct SamplePoint {
    pub value: Option<u32>,
    pub timestamp: Instant,
}

/// Render a graph into premultiplied RGBA bytes for a Win32 layered window.
///
/// The native window path deliberately does not use egui or a GPU surface. This
/// keeps alpha under our control and means each overlay consumes only a small
/// software buffer instead of creating another renderer/context.
pub fn render_graph_into(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    samples: &[SamplePoint],
    now: Instant,
    smooth: bool,
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
        let [r, g, b] = parse_color(&config.bg_color, [0, 0, 0]);
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
    let start = if smooth {
        samples.partition_point(|sample| {
            now.saturating_duration_since(sample.timestamp) > window_duration
        })
    } else {
        samples.len().saturating_sub(visible_samples)
    };
    let samples = &samples[start..];
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

    let mut timeout_paint = Paint::default();
    let [r, g, b] = parse_color(&config.timeout_color, [239, 68, 68]);
    timeout_paint.set_color_rgba8(r, g, b, 255);
    let mut timeout_builder = PathBuilder::new();
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
        pixmap.stroke_path(
            &path,
            &timeout_paint,
            &Stroke {
                width: 1.5,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }

    let mut line_paint = Paint::default();
    let [r, g, b] = parse_color(&config.line_color, [74, 222, 128]);
    line_paint.set_color_rgba8(r, g, b, 255);
    let stroke = Stroke {
        width: 1.5,
        ..Stroke::default()
    };

    let mut segment = PathBuilder::new();
    let mut in_segment = false;
    let mut last_y: Option<f32> = None;
    for (index, sample) in samples.iter().enumerate() {
        let Some(x) = map_x(index, sample) else {
            continue;
        };
        let Some(latency) = sample.value else {
            if in_segment {
                if let Some(path) = segment.finish() {
                    pixmap.stroke_path(&path, &line_paint, &stroke, Transform::identity(), None);
                }
                segment = PathBuilder::new();
            }
            in_segment = false;
            continue;
        };

        let y = map_y(latency);
        if !in_segment {
            let start = transform_point(
                (x, y),
                long_px,
                short_px,
                config.orientation,
                config.mirrored,
            );
            segment.move_to(start.0, start.1);
            if let Some(previous_y) = last_y {
                let resume = transform_point(
                    (x, previous_y),
                    long_px,
                    short_px,
                    config.orientation,
                    config.mirrored,
                );
                segment.line_to(resume.0, resume.1);
            }
            in_segment = true;
        } else {
            let point = transform_point(
                (x, y),
                long_px,
                short_px,
                config.orientation,
                config.mirrored,
            );
            segment.line_to(point.0, point.1);
        }
        last_y = Some(y);
    }
    if in_segment {
        if let Some(path) = segment.finish() {
            pixmap.stroke_path(&path, &line_paint, &stroke, Transform::identity(), None);
        }
    }

    *pixels = pixmap.take();
    true
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

fn parse_color(value: &str, fallback: [u8; 3]) -> [u8; 3] {
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
            })
            .collect()
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
            },
            SamplePoint {
                value: Some(200),
                timestamp: now - Duration::from_secs(14),
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
    fn smooth_timeout_marker_moves_with_timestamp() {
        let now = Instant::now();
        let mut config = OverlayConfig::new();
        config.window_seconds = 30;
        config.timeout_color = "#ff0000".to_string();
        let samples = vec![SamplePoint {
            value: None,
            timestamp: now - Duration::from_secs(15),
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
            config.line_color = "#00ff00".to_string();
            config.timeout_color = "#ff0000".to_string();
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
