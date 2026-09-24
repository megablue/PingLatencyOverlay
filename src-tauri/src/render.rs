use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::config::OverlayConfig;

/// Render a graph into premultiplied RGBA bytes for a Win32 layered window.
///
/// The native window path deliberately does not use egui or a GPU surface. This
/// keeps alpha under our control and means each overlay consumes only a small
/// software buffer instead of creating another renderer/context.
pub fn render_graph(
    width: u32,
    height: u32,
    config: &OverlayConfig,
    samples: &[Option<u32>],
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }

    let mut pixmap = Pixmap::new(width, height)?;
    let full = Rect::from_ltrb(0.0, 0.0, width as f32, height as f32)?;

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
    let start = samples.len().saturating_sub(visible_samples);
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
    let map_x = |index: usize| (index as f32 + 0.5) * step;

    let mut timeout_paint = Paint::default();
    let [r, g, b] = parse_color(&config.timeout_color, [239, 68, 68]);
    timeout_paint.set_color_rgba8(r, g, b, 255);
    let mut timeout_builder = PathBuilder::new();
    for (index, sample) in samples.iter().enumerate() {
        if sample.is_none() {
            let x = map_x(index);
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
        let x = map_x(index);
        let Some(latency) = sample else {
            if in_segment {
                if let Some(path) = segment.finish() {
                    pixmap.stroke_path(&path, &line_paint, &stroke, Transform::identity(), None);
                }
                segment = PathBuilder::new();
            }
            in_segment = false;
            continue;
        };

        let y = map_y(*latency);
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

    Some(pixmap.take())
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

    #[test]
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn transparent_background_stays_transparent() {
        let config = OverlayConfig::new();
        let pixels = render_graph(120, 60, &config, &[Some(10), None, Some(20)]).expect("pixmap");
        let transparent = pixels.chunks_exact(4).filter(|pixel| pixel[3] == 0).count();
        assert!(transparent > 100);
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
            let pixels = render_graph(width, height, &config, &[None]).expect("pixmap");
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
