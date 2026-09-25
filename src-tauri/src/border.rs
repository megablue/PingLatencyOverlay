use std::time::{Duration, Instant};

use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::config::{BorderEffect, OverlayConfig};

const BORDER_WIDTH: f32 = 3.0;
const RGB_CYCLE_SECONDS: f32 = 3.0;

/// Border animation is intentionally independent of the graph's Smooth FPS setting.
pub fn border_frame_interval() -> Duration {
    Duration::from_nanos(16_666_667)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderVisual {
    pub effect: BorderEffect,
    pub phase: f32,
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
        if selected {
            self.startup_consumed = true;
            let restart = match self.phase {
                Phase::Inactive | Phase::Fading { .. } => true,
                Phase::Active { selected, .. } => !selected,
            };
            if restart {
                self.activate(BorderEffect::RgbLoop, now, true, false);
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
        BorderEffect::Disabled => return,
        BorderEffect::RgbLoop => {}
    }
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
    fn border_is_drawn_three_pixels_inside_the_surface() {
        let mut pixmap = Pixmap::new(40, 30).expect("pixmap");
        draw_border(
            &mut pixmap,
            40,
            30,
            &BorderVisual {
                effect: BorderEffect::RgbLoop,
                phase: 0.0,
                opacity: 1.0,
            },
        );
        assert!(pixmap.pixel(1, 15).expect("edge pixel").alpha() > 0);
        assert_eq!(pixmap.pixel(4, 15).expect("inner pixel").alpha(), 0);
    }
}
