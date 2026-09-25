use std::time::{Duration, Instant};

use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::config::{BorderEffect, OverlayConfig};

const BORDER_WIDTH: f32 = 3.0;
const RGB_CYCLE_SECONDS: f32 = 3.0;

/// Effect used for temporary selection activation. Startup effects are
/// configurable per overlay; keeping this separate makes a future global
/// activation effect reuse the same animator without changing startup logic.
pub const SELECTED_BORDER_EFFECT: BorderEffect = BorderEffect::RgbLoop;

/// Border animation is intentionally independent of the graph's Smooth FPS setting.
pub fn border_frame_interval() -> Duration {
    Duration::from_nanos(16_666_667)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderVisual {
    pub effect: BorderEffect,
    pub phase: f32,
    pub animation_time: Duration,
    pub opacity: f32,
}

#[derive(Clone, Copy)]
enum Phase {
    Inactive,
    Active {
        effect: BorderEffect,
        started_at: Instant,
        selected: bool,
        startup: bool,
    },
    Fading {
        effect: BorderEffect,
        started_at: Instant,
        fade_started_at: Instant,
        fade_duration: Duration,
    },
}

pub struct BorderAnimator {
    phase: Phase,
    startup_consumed: bool,
}

impl Default for BorderAnimator {
    fn default() -> Self {
        Self {
            phase: Phase::Inactive,
            startup_consumed: false,
        }
    }
}

impl BorderAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, config: &OverlayConfig, selected: bool, now: Instant) {
        self.update_with_selected_effect(config, selected, SELECTED_BORDER_EFFECT, now);
    }

    /// Update the animator with an explicit effect for temporary selection.
    /// The default wrapper keeps selected tabs on RGB Loop today, while a
    /// future global option can pass its configured effect through this path.
    pub fn update_with_selected_effect(
        &mut self,
        config: &OverlayConfig,
        selected: bool,
        selected_effect: BorderEffect,
        now: Instant,
    ) {
        if selected {
            self.startup_consumed = true;
            let restart = match self.phase {
                Phase::Inactive | Phase::Fading { .. } => true,
                Phase::Active {
                    effect,
                    selected: active_selected,
                    ..
                } => !active_selected || effect != selected_effect,
            };
            if restart {
                self.activate(selected_effect, now, true, false);
            }
            return;
        }

        let mut start_fade = None;
        match self.phase {
            Phase::Inactive => {
                if !self.startup_consumed {
                    self.startup_consumed = true;
                    if config.startup_border_effect != BorderEffect::Disabled {
                        self.activate(config.startup_border_effect, now, false, true);
                    }
                }
            }
            Phase::Active {
                effect,
                started_at,
                selected: active_selected,
                startup,
            } => {
                if active_selected
                    || (startup
                        && now.saturating_duration_since(started_at)
                            >= Duration::from_secs(config.border_animation_sec as u64))
                {
                    start_fade = Some((effect, started_at));
                }
            }
            Phase::Fading {
                effect: _,
                started_at: _,
                fade_started_at,
                fade_duration,
            } => {
                if fade_duration.is_zero()
                    || now.saturating_duration_since(fade_started_at) >= fade_duration
                {
                    self.phase = Phase::Inactive;
                }
            }
        }

        if let Some((effect, started_at)) = start_fade {
            self.start_fade(effect, started_at, config, now);
        }
    }

    pub fn needs_animation(&self) -> bool {
        !matches!(self.phase, Phase::Inactive)
    }

    pub fn visual(&self, now: Instant) -> Option<BorderVisual> {
        match self.phase {
            Phase::Inactive => None,
            Phase::Active {
                effect, started_at, ..
            } => Some(BorderVisual {
                effect,
                phase: elapsed_phase(started_at, now),
                animation_time: now.saturating_duration_since(started_at),
                opacity: 1.0,
            }),
            Phase::Fading {
                effect,
                started_at,
                fade_started_at,
                fade_duration,
            } => {
                let fade_elapsed = now.saturating_duration_since(fade_started_at);
                if fade_duration.is_zero() {
                    return None;
                }
                let opacity = 1.0 - (fade_elapsed.as_secs_f32() / fade_duration.as_secs_f32());
                let opacity = opacity.clamp(0.0, 1.0);
                if opacity <= 0.0 {
                    None
                } else {
                    Some(BorderVisual {
                        effect,
                        phase: elapsed_phase(started_at, now),
                        animation_time: now.saturating_duration_since(started_at),
                        opacity,
                    })
                }
            }
        }
    }

    fn activate(&mut self, effect: BorderEffect, now: Instant, selected: bool, startup: bool) {
        if effect == BorderEffect::Disabled {
            self.phase = Phase::Inactive;
        } else {
            self.phase = Phase::Active {
                effect,
                started_at: now,
                selected,
                startup,
            };
        }
    }

    fn start_fade(
        &mut self,
        effect: BorderEffect,
        started_at: Instant,
        config: &OverlayConfig,
        now: Instant,
    ) {
        if config.border_fade_sec == 0 {
            self.phase = Phase::Inactive;
        } else {
            self.phase = Phase::Fading {
                effect,
                started_at,
                fade_started_at: now,
                fade_duration: Duration::from_secs(config.border_fade_sec as u64),
            };
        }
    }
}

fn elapsed_phase(started_at: Instant, now: Instant) -> f32 {
    (now.saturating_duration_since(started_at).as_secs_f32() / RGB_CYCLE_SECONDS).rem_euclid(1.0)
}

pub fn draw_border(pixmap: &mut Pixmap, width: u32, height: u32, visual: &BorderVisual) {
    match visual.effect {
        BorderEffect::Disabled => {}
        BorderEffect::RgbLoop => draw_solid_border(pixmap, width, height, visual),
        BorderEffect::RgbNoise => draw_noise_border(pixmap, width, height, visual),
    }
}

fn draw_solid_border(pixmap: &mut Pixmap, width: u32, height: u32, visual: &BorderVisual) {
    if visual.opacity <= 0.0 || width < 3 || height < 3 {
        return;
    }
    let inset = BORDER_WIDTH / 2.0;
    let Some(rect) = Rect::from_xywh(
        inset,
        inset,
        width as f32 - BORDER_WIDTH,
        height as f32 - BORDER_WIDTH,
    ) else {
        return;
    };
    let [red, green, blue] = rgb_at_phase(visual.phase);
    let alpha = (visual.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, alpha);

    let mut path = PathBuilder::new();
    path.move_to(rect.left(), rect.top());
    path.line_to(rect.right(), rect.top());
    path.line_to(rect.right(), rect.bottom());
    path.line_to(rect.left(), rect.bottom());
    path.close();
    let Some(path) = path.finish() else {
        return;
    };
    pixmap.stroke_path(
        &path,
        &paint,
        &Stroke {
            width: BORDER_WIDTH,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
}

fn draw_noise_border(pixmap: &mut Pixmap, width: u32, height: u32, visual: &BorderVisual) {
    if visual.opacity <= 0.0 || width < 3 || height < 3 {
        return;
    }

    let border_width = BORDER_WIDTH as u32;
    let alpha = (visual.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let stride = width as usize;
    let pixels = pixmap.data_mut();
    let mut draw_pixel = |x: u32, y: u32| {
        let rgb = rgb_noise_at(x, y, visual.animation_time);
        blend_pixel(pixels, stride, x, y, rgb, alpha);
    };

    // Draw the top and bottom rows first, then the two side columns. Keeping
    // the border exactly three pixels wide makes every border pixel receive
    // its own color while avoiding corner pixels being painted twice.
    for x in 0..width {
        for inset in 0..border_width {
            draw_pixel(x, inset);
            draw_pixel(x, height - 1 - inset);
        }
    }
    for y in border_width..height.saturating_sub(border_width) {
        for x in 0..border_width {
            draw_pixel(x, y);
        }
        for x in width - border_width..width {
            draw_pixel(x, y);
        }
    }
}

fn blend_pixel(pixels: &mut [u8], stride: usize, x: u32, y: u32, rgb: [u8; 3], alpha: u8) {
    if alpha == 0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if x >= stride {
        return;
    }
    let Some(start) = y.checked_mul(stride).and_then(|row| row.checked_add(x)) else {
        return;
    };
    let Some(end) = start.checked_add(4) else {
        return;
    };
    let Some(pixel) = pixels.get_mut(start..end) else {
        return;
    };

    let inverse_alpha = 255 - alpha;
    for channel in 0..3 {
        let source = ((rgb[channel] as u16 * alpha as u16 + 127) / 255) as u8;
        let destination = ((pixel[channel] as u16 * inverse_alpha as u16 + 127) / 255) as u8;
        pixel[channel] = source.saturating_add(destination);
    }
    let destination_alpha = ((pixel[3] as u16 * inverse_alpha as u16 + 127) / 255) as u8;
    pixel[3] = alpha.saturating_add(destination_alpha);
}

fn rgb_noise_at(x: u32, y: u32, animation_time: Duration) -> [u8; 3] {
    let mut seed = (x as u64) | ((y as u64) << 32);
    let elapsed_nanos = animation_time.as_nanos();
    let time_seed = (elapsed_nanos as u64) ^ ((elapsed_nanos >> 64) as u64);
    seed ^= time_seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let noise = mix_noise_seed(seed);
    [noise as u8, (noise >> 8) as u8, (noise >> 16) as u8]
}

fn mix_noise_seed(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn rgb_at_phase(phase: f32) -> [u8; 3] {
    let hue = (phase.rem_euclid(1.0)) * 6.0;
    let sector = hue.floor() as u32 % 6;
    let fraction = hue - hue.floor();
    let second = 1.0 - fraction;
    match sector {
        0 => [255, (fraction * 255.0) as u8, 0],
        1 => [(second * 255.0) as u8, 255, 0],
        2 => [0, 255, (fraction * 255.0) as u8],
        3 => [0, (second * 255.0) as u8, 255],
        4 => [(fraction * 255.0) as u8, 0, 255],
        _ => [255, 0, (second * 255.0) as u8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> OverlayConfig {
        let mut config = OverlayConfig::new();
        config.startup_border_effect = BorderEffect::RgbLoop;
        config.border_animation_sec = 2;
        config.border_fade_sec = 1;
        config
    }

    #[test]
    fn startup_effect_runs_then_fades_to_inactive() {
        let config = config();
        let start = Instant::now();
        let mut animator = BorderAnimator::new();
        animator.update(&config, false, start);
        assert!(animator.needs_animation());

        animator.update(&config, false, start + Duration::from_secs(2));
        assert!(animator.needs_animation());
        assert!(animator.visual(start + Duration::from_secs(2)).is_some());

        animator.update(&config, false, start + Duration::from_secs(3));
        assert!(!animator.needs_animation());
    }

    #[test]
    fn selected_effect_loops_until_deselected() {
        let config = config();
        let start = Instant::now();
        let mut animator = BorderAnimator::new();
        animator.update(&config, true, start);
        assert!(animator.needs_animation());
        animator.update(&config, true, start + Duration::from_secs(10));
        assert!(animator.needs_animation());

        animator.update(&config, false, start + Duration::from_secs(10));
        assert!(animator.needs_animation());
        animator.update(&config, false, start + Duration::from_secs(11));
        assert!(!animator.needs_animation());
    }

    #[test]
    fn disabled_startup_effect_waits_for_selection() {
        let mut config = config();
        config.startup_border_effect = BorderEffect::Disabled;
        let start = Instant::now();
        let mut animator = BorderAnimator::new();
        animator.update(&config, false, start);
        assert!(!animator.needs_animation());
        animator.update(&config, true, start);
        assert!(animator.needs_animation());
    }

    #[test]
    fn selected_activation_keeps_using_rgb_loop() {
        let mut config = config();
        config.startup_border_effect = BorderEffect::RgbNoise;
        let start = Instant::now();
        let mut animator = BorderAnimator::new();
        animator.update(&config, true, start);
        assert_eq!(
            animator.visual(start).expect("selected border").effect,
            BorderEffect::RgbLoop
        );
    }

    #[test]
    fn selected_effect_can_be_supplied_for_a_future_global_option() {
        let config = config();
        let start = Instant::now();
        let mut animator = BorderAnimator::new();
        animator.update_with_selected_effect(&config, true, BorderEffect::RgbNoise, start);
        assert_eq!(
            animator.visual(start).expect("selected border").effect,
            BorderEffect::RgbNoise
        );

        animator.update_with_selected_effect(
            &config,
            true,
            BorderEffect::RgbLoop,
            start + Duration::from_secs(1),
        );
        assert_eq!(
            animator
                .visual(start + Duration::from_secs(1))
                .expect("updated border")
                .effect,
            BorderEffect::RgbLoop
        );
    }

    #[test]
    fn noise_border_uses_independent_colors_and_animates() {
        let first_visual = BorderVisual {
            effect: BorderEffect::RgbNoise,
            phase: 0.0,
            animation_time: Duration::ZERO,
            opacity: 1.0,
        };
        let second_visual = BorderVisual {
            phase: 0.25,
            animation_time: Duration::from_millis(250),
            ..first_visual
        };
        let mut first = Pixmap::new(40, 30).expect("first pixmap");
        let mut second = Pixmap::new(40, 30).expect("second pixmap");
        draw_border(&mut first, 40, 30, &first_visual);
        draw_border(&mut second, 40, 30, &second_visual);

        let first_row: Vec<_> = (0..40)
            .map(|x| first.pixel(x, 0).expect("top border pixel"))
            .collect();
        assert!(first_row.iter().any(|pixel| *pixel != first_row[0]));
        assert!((0..40).any(|x| {
            first.pixel(x, 0).expect("first pixel") != second.pixel(x, 0).expect("second pixel")
        }));
        assert_eq!(first.pixel(4, 15).expect("inner pixel").alpha(), 0);
    }

    #[test]
    fn border_is_drawn_three_pixels_inside_the_surface() {
        let mut pixmap = Pixmap::new(40, 30).expect("pixmap");
        draw_border(
            &mut pixmap,
            40,
            30,
            &BorderVisual {
                effect: BorderEffect::RgbLoop,
                phase: 0.0,
                animation_time: Duration::ZERO,
                opacity: 1.0,
            },
        );
        assert!(pixmap.pixel(1, 15).expect("edge pixel").alpha() > 0);
        assert_eq!(pixmap.pixel(4, 15).expect("inner pixel").alpha(), 0);
    }
}
