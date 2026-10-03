use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::rules::{AutoRules, Condition};

/// Minimum graph time window, in seconds. Mirrors the spec.
pub const MIN_WINDOW_SECONDS: u32 = 30;
/// Default ping timeout, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 1000;
/// Default height of the Y axis on screen, in logical pixels.
pub const DEFAULT_GRAPH_HEIGHT_PX: u32 = 60;
/// Smallest allowed graph height, in logical pixels.
pub const MIN_GRAPH_HEIGHT_PX: u32 = 10;
/// Default latency ceiling, in milliseconds.
pub const DEFAULT_MAX_Y_MS: u32 = 1000;
/// Largest value shown by the X-axis scale slider.
pub const MAX_SCALE: u32 = 10;
/// Largest value accepted by the numeric X-axis scale input.
pub const MAX_SCALE_INPUT: u32 = 1_000;
/// Default smooth overlay redraw rate.
pub const DEFAULT_SMOOTH_FPS: u32 = 60;
/// Smallest allowed smooth redraw rate.
pub const MIN_SMOOTH_FPS: u32 = 1;
/// Largest allowed smooth redraw rate.
pub const MAX_SMOOTH_FPS: u32 = 1_000;
/// Default cosmetic startup prefill line color.
pub const DEFAULT_PREFILL_LINE_COLOR: &str = "#64748b";
/// Default cosmetic startup prefill animation duration, in seconds.
pub const DEFAULT_PREFILL_ANIMATION_SEC: u32 = 3;
/// Smallest allowed cosmetic startup prefill animation duration, in seconds.
pub const MIN_PREFILL_ANIMATION_SEC: u32 = 1;
/// Largest allowed cosmetic startup prefill animation duration, in seconds.
pub const MAX_PREFILL_ANIMATION_SEC: u32 = 60;
/// Default duration before a startup border effect begins fading, in seconds.
pub const DEFAULT_BORDER_ANIMATION_SEC: u32 = 5;
/// Smallest allowed border animation duration, in seconds.
pub const MIN_BORDER_ANIMATION_SEC: u32 = 1;
/// Largest allowed border animation duration, in seconds.
pub const MAX_BORDER_ANIMATION_SEC: u32 = 60;
/// Default border fade duration, in seconds.
pub const DEFAULT_BORDER_FADE_SEC: u32 = 1;
/// Largest allowed border fade duration, in seconds.
pub const MAX_BORDER_FADE_SEC: u32 = 60;
/// Default horizontal offset from the anchor reference, in logical pixels.
pub const DEFAULT_HORIZONTAL_MARGIN_PX: i32 = 0;
/// Default vertical offset from the anchor reference, in logical pixels.
pub const DEFAULT_VERTICAL_MARGIN_PX: i32 = 0;
/// Smallest allowed signed margin offset, in logical pixels.
pub const MIN_MARGIN_OFFSET_PX: i32 = -10_000;
/// Largest allowed signed margin offset, in logical pixels.
pub const MAX_MARGIN_OFFSET_PX: i32 = 10_000;
/// Default overlay background color.
pub const DEFAULT_BG_COLOR: &str = "#0f172a";
/// Default overlay background opacity (0 = fully transparent).
pub const DEFAULT_BG_OPACITY: u32 = 0;
/// Default strength of the underglow cast below each line, in percent.
pub const DEFAULT_LINE_GLOW_INTENSITY: u32 = 10;
/// Smallest accepted underglow strength, in percent (0 = invisible).
pub const MIN_LINE_GLOW_INTENSITY: u32 = 0;
/// Largest accepted underglow strength, in percent.
pub const MAX_LINE_GLOW_INTENSITY: u32 = 100;
/// Default distance the underglow reaches past a line, in physical pixels.
pub const DEFAULT_LINE_GLOW_RADIUS_PX: u32 = 30;
/// Smallest accepted underglow reach, in physical pixels.
pub const MIN_LINE_GLOW_RADIUS_PX: u32 = 2;
/// Largest accepted underglow reach, in physical pixels.
pub const MAX_LINE_GLOW_RADIUS_PX: u32 = 50;
/// Default width every line and timeout marker is stroked with, in pixels.
pub const DEFAULT_LINE_STROKE_PX: f32 = 1.5;
/// Smallest accepted stroke width, in pixels.
pub const MIN_LINE_STROKE_PX: f32 = 0.5;
/// Largest accepted stroke width, in pixels.
pub const MAX_LINE_STROKE_PX: f32 = 6.0;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Name shown in the window title, the sidebar and the profile menu.
    ///
    /// It is free-form and does not have to be unique: the profile id in the
    /// file name is the only unique part, so this may be absent in files
    /// written before profiles had display names.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile_name: String,
    #[serde(default)]
    pub overlays: Vec<OverlayConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Graph rotation: 0, 90, 180 or 270 degrees.
    #[serde(default)]
    pub orientation: u16,
    /// Mirror the graph (combines with any orientation).
    #[serde(default)]
    pub mirrored: bool,
    #[serde(default)]
    pub position: Anchor,

    /// Visible time window in seconds (each tick = one ping, one tick per second).
    #[serde(default = "default_window_seconds")]
    pub window_seconds: u32,
    /// Visual scale multiplier: pixels per tick.
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Continuously redraw the graph between timestamped samples.
    #[serde(default = "default_true")]
    pub smooth_rendering: bool,
    /// Target redraw rate while smooth rendering is enabled.
    #[serde(default = "default_smooth_fps")]
    pub smooth_fps: u32,
    /// Legacy millisecond setting accepted when loading older configurations.
    #[serde(rename = "smoothDelayMs", default, skip_serializing)]
    legacy_smooth_delay_ms: Option<u32>,
    /// Show a cosmetic fake graph until the first real probe result arrives.
    #[serde(default = "default_true")]
    pub cosmetic_startup_prefill: bool,
    /// Line color used by the cosmetic startup graph.
    #[serde(default = "default_prefill_line_color")]
    pub prefill_line_color: String,
    /// Duration of the cosmetic startup reveal, in seconds.
    #[serde(default = "default_prefill_animation_sec")]
    pub prefill_animation_sec: u32,
    /// Border effect to play once during startup.
    #[serde(default = "default_startup_border_effect")]
    pub startup_border_effect: BorderEffect,
    /// Time before a startup border effect begins fading, in seconds.
    #[serde(default = "default_border_animation_sec")]
    pub border_animation_sec: u32,
    /// Duration of the border fade-out, in seconds.
    #[serde(default = "default_border_fade_sec")]
    pub border_fade_sec: u32,

    /// The hosts this overlay graphs, each drawn as its own line.
    ///
    /// One target is one line on a shared plot, and an overlay with a single
    /// target is what every configuration written before grouping existed. There
    /// is no separate group type: a group is an overlay with more than one
    /// target, so there is one code path rather than a single-target one and a
    /// grouped one that could disagree.
    ///
    /// `normalize` guarantees this is never empty, so every reader can index
    /// the first element rather than treat "no targets" as a state.
    #[serde(default)]
    pub targets: Vec<TargetConfig>,

    /// The probe of a configuration written before targets existed.
    ///
    /// Consumed into a one-element `targets` list by `normalize` and never
    /// written back, which is what keeps a migrated profile readable by an
    /// older build: it keeps exactly the keys it already had.
    #[serde(rename = "probe", default, skip_serializing)]
    legacy_probe: Option<ProbeConfig>,
    /// Legacy line colour, mapped onto the first target.
    #[serde(rename = "lineColor", default, skip_serializing)]
    legacy_line_color: Option<String>,
    /// Legacy timeout colour, mapped onto the first target.
    #[serde(rename = "timeoutColor", default, skip_serializing)]
    legacy_timeout_color: Option<String>,
    /// Legacy ping timeout, mapped onto the first target.
    #[serde(rename = "timeoutMs", default, skip_serializing)]
    legacy_timeout_ms: Option<u32>,

    /// Height of the Y axis on screen, in logical pixels.
    #[serde(default = "default_graph_height_px")]
    pub graph_height_px: u32,
    /// Latency ceiling in milliseconds; higher pings clamp to the top.
    #[serde(default = "default_max_y_ms")]
    pub max_y_ms: u32,
    /// Signed horizontal offset from the anchor's horizontal reference.
    #[serde(default)]
    pub horizontal_margin_px: i32,
    /// Signed vertical offset from the anchor's vertical reference.
    #[serde(default)]
    pub vertical_margin_px: i32,
    /// Legacy single-axis margin accepted when loading older configurations.
    #[serde(rename = "marginPx", default, skip_serializing)]
    legacy_margin_px: Option<u32>,
    /// Background color drawn behind the graph.
    #[serde(default = "default_bg_color")]
    pub bg_color: String,
    /// Background opacity, 0 (transparent) to 100 (opaque).
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: u32,
    /// Width every line and timeout marker is stroked with, in pixels.
    ///
    /// One value for the whole overlay: the host lines, the startup prefill
    /// and the timeout markers all read it, so a group of hosts cannot end up
    /// with lines of different weights. The axis pads itself by half the
    /// stroke, so a thick line resting on the zero line or clamped at the
    /// ceiling is not sliced by the window edge.
    #[serde(default = "default_line_stroke_px")]
    pub line_stroke_px: f32,
    /// Draw a soft cast under every line, in that line's own colour.
    ///
    /// The cast falls toward the zero line in the graph's own frame, so it
    /// rotates and mirrors with the overlay. The window reserves room past
    /// the zero line for it — `line_glow_reserve_px` is the one statement of
    /// how much — so a glowing line on the zero line is not sliced flat by
    /// the window edge. `new()` turns this on, while an absent key reads as
    /// off, so a profile written before the setting existed does not start
    /// glowing on its own.
    #[serde(default)]
    pub line_glow: bool,
    /// Strength of the underglow, 0 to 100.
    #[serde(default = "default_line_glow_intensity")]
    pub line_glow_intensity: u32,
    /// How far the underglow reaches past a line, in physical pixels.
    #[serde(default = "default_line_glow_radius_px")]
    pub line_glow_radius_px: u32,
    /// The display this overlay belongs to, as a Win32 device name
    /// (`\\.\DISPLAY2`), or `None` to follow the primary monitor.
    ///
    /// `None` is every profile written before this field existed, and it is
    /// also what a hand-edited empty string becomes in `normalize`, because a
    /// name that matches no attached monitor hides the overlay with nothing on
    /// screen to say why.
    #[serde(default)]
    pub monitor_device: Option<String>,
    /// Which of the three placements this overlay uses.
    ///
    /// One enum rather than a flag per strategy: Global, Sticky and Wallpaper
    /// are alternatives for the same window, and two booleans could say
    /// "wallpaper and sticky", which means nothing.
    #[serde(default)]
    pub display_mode: DisplayMode,
    /// The window a Sticky Mode overlay follows.
    ///
    /// `None`, or a matcher with nothing usable in it, hides the overlay until
    /// a target is configured — a sticky overlay without a window has no
    /// position, and the editor says so where the user can see it.
    #[serde(default)]
    pub sticky_target: Option<StickyTarget>,
    /// How a Sticky Mode overlay orders itself against other windows.
    ///
    /// Only Sticky reads it: Global and Wallpaper are always on top of
    /// everything but the desktop, and a choice they cannot honour would be a
    /// setting that looks inert.
    #[serde(default)]
    pub sticky_z_order: StickyZOrder,
    /// Legacy `wallpaperMode` boolean, folded into `display_mode` by
    /// `normalize`.
    ///
    /// A field rather than a serde rename on the enum, because the old key
    /// held a boolean and the new one names a mode: mapping `true` to
    /// `Wallpaper` at load time is the only translation, and
    /// `skip_serializing` stops the old key reappearing in files a new build
    /// writes.
    #[serde(rename = "wallpaperMode", default, skip_serializing)]
    legacy_wallpaper_mode: Option<bool>,
}

/// How an overlay decides where it lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    /// Anchored inside a display's work area, chosen in the monitor picker.
    #[default]
    Global,
    /// Placed inside the client area of a matched window, and moved with it.
    Sticky,
    /// Parked directly above the shell's desktop window: above the wallpaper
    /// and below the desktop icons, covered by ordinary windows.
    Wallpaper,
}

/// The window a Sticky Mode overlay follows.
///
/// The conditions are the auto-switch rules' own [`Condition`] type, so
/// "matches" cannot come to mean two things in one app, and the editor's
/// three boxes are exactly the process, title and class parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickyTarget {
    /// Every condition must match the same window. Blank values are dropped in
    /// `normalize`, so a half-typed box cannot match everything.
    #[serde(default)]
    pub when: Vec<Condition>,
}

/// Where a Sticky Mode overlay sits in the z-order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StickyZOrder {
    /// Above every other window, like the other display modes.
    #[default]
    AboveEverything,
    /// In the followed window's z-order band: the overlay is owned by the
    /// target, so switching to another app takes it off the screen with the
    /// window it belongs to.
    FollowWindow,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum ProbeConfig {
    Icmp { host: String },
    Tcp { host: String, port: u16 },
}

impl ProbeConfig {
    pub fn host(&self) -> &str {
        match self {
            Self::Icmp { host } | Self::Tcp { host, .. } => host,
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Self::Icmp { .. } => 0,
            Self::Tcp { port, .. } => *port,
        }
    }
}

/// One host drawn as one line on an overlay's shared plot.
///
/// Everything here is per host rather than per overlay. The rest of
/// `OverlayConfig` is the appearance of the window and of the plot, which is
/// shared by every target in it: one window, one X axis, one Y ceiling, one
/// position. What cannot be shared is the probe itself, the timeout it is
/// measured against, and the colours that identify which line is which.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TargetConfig {
    /// Stable identity, unique within its overlay.
    ///
    /// This is what a probe task and a sample buffer are keyed by, so it has to
    /// survive a rename or a reorder: changing it is indistinguishable from
    /// deleting the target and adding another, and the graph starts over.
    pub id: String,
    /// Disabled targets keep their settings but are not probed and not drawn.
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub probe: ProbeConfig,
    /// Ping timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
    /// Colour of this target's line.
    #[serde(default = "default_line_color")]
    pub line_color: String,
    /// Colour of this target's timeout markers.
    #[serde(default = "default_timeout_color")]
    pub timeout_color: String,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl TargetConfig {
    /// A new target, seeded from the defaults a first-ever overlay would use.
    pub fn new() -> Self {
        Self {
            id: unique_id(),
            enabled: true,
            probe: ProbeConfig::Icmp {
                host: "1.1.1.1".to_string(),
            },
            timeout_ms: default_timeout_ms(),
            line_color: default_line_color(),
            timeout_color: default_timeout_color(),
        }
    }

    /// A new target that starts out looking like one the user already has.
    ///
    /// A group is usually built by adding a host next to an existing one, and a
    /// new line in the same colour as the line above it is indistinguishable
    /// from it. Copying the probe's shape and timeout is what makes an added
    /// host feel like a variation on the same thing; the line colour is moved
    /// one step round the palette instead, because that is the one thing copying
    /// cannot usefully do. The timeout colour is copied: it says "this host did
    /// not answer" rather than "this is this host", so a group reading alike is
    /// the point.
    ///
    /// The palette is a cycle, so past its size two hosts further apart can
    /// share a colour. Adjacent hosts never do, and that is the property worth
    /// having — the pair a reader is comparing is the pair drawn next to each
    /// other.
    pub fn like(source: &TargetConfig) -> Self {
        Self {
            id: unique_id(),
            enabled: source.enabled,
            probe: source.probe.clone(),
            timeout_ms: source.timeout_ms,
            line_color: next_line_color(source.line_color.as_str()),
            timeout_color: source.timeout_color.clone(),
        }
    }

    /// The host as it reads in a list: the name, without the port noise.
    pub fn label(&self) -> &str {
        let host = self.probe.host().trim();
        if host.is_empty() {
            "(no host)"
        } else {
            host
        }
    }
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Ids are a millisecond timestamp and a monotonic counter, because two targets
/// added in the same millisecond must not collide.
fn unique_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{sequence}")
}

/// A line colour for a target added next to an existing one.
///
/// Steps one entry round a small fixed palette, picking the entry nearest the
/// colour it was given so a hand-picked colour still lands somewhere sensible.
/// Rotating the hue rather than shifting the value keeps the family
/// recognisable: a group that started green and added three hosts is four
/// greens of visibly different hue, which reads as one set.
///
/// It is a **cycle**, not a walk. A target whose colour is already this far
/// round comes back to the plain default rather than drifting further from what
/// the user chose, which means a group larger than the palette has two hosts
/// sharing a colour — never two neighbours, because each step is relative to
/// the host added before it.
fn next_line_color(source: &str) -> String {
    let [r, g, b] = parse_rgb(source);
    // Roughly evenly spaced hues that stay legible on a transparent overlay.
    const HUES: [[u8; 3]; 6] = [
        [74, 222, 128],
        [96, 165, 250],
        [251, 146, 60],
        [244, 114, 182],
        [167, 139, 250],
        [250, 204, 21],
    ];
    let distance = |candidate: &[u8; 3]| {
        let dr = i32::from(r) - i32::from(candidate[0]);
        let dg = i32::from(g) - i32::from(candidate[1]);
        let db = i32::from(b) - i32::from(candidate[2]);
        dr * dr + dg * dg + db * db
    };
    let closest = HUES
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| distance(candidate))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let [r, g, b] = HUES[(closest + 1) % HUES.len()];
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Read `#rrggbb`, falling back to the default line colour.
///
/// Tolerant on purpose: a colour can be typed by hand into a profile file, and
/// a value this cannot read should fall back to something legible rather than
/// fail to load a whole profile over one key.
fn parse_rgb(value: &str) -> [u8; 3] {
    let hex = value.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let parsed = (|| {
        if hex.len() != 6 {
            return None;
        }
        let component = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
        Some([component(0)?, component(2)?, component(4)?])
    })();
    parsed.unwrap_or_else(|| parse_rgb(&default_line_color()))
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayConfig {
    /// A sensible starting point for a newly added overlay.
    pub fn new() -> Self {
        Self {
            id: unique_id(),
            name: "New overlay".to_string(),
            enabled: true,
            orientation: 0,
            mirrored: false,
            position: Anchor::TopRight,
            window_seconds: default_window_seconds(),
            scale: default_scale(),
            smooth_rendering: true,
            smooth_fps: default_smooth_fps(),
            legacy_smooth_delay_ms: None,
            cosmetic_startup_prefill: true,
            prefill_line_color: default_prefill_line_color(),
            prefill_animation_sec: default_prefill_animation_sec(),
            startup_border_effect: default_startup_border_effect(),
            border_animation_sec: default_border_animation_sec(),
            border_fade_sec: default_border_fade_sec(),
            targets: vec![TargetConfig::new()],
            legacy_probe: None,
            legacy_line_color: None,
            legacy_timeout_color: None,
            legacy_timeout_ms: None,
            graph_height_px: default_graph_height_px(),
            max_y_ms: default_max_y_ms(),
            horizontal_margin_px: DEFAULT_HORIZONTAL_MARGIN_PX,
            vertical_margin_px: DEFAULT_VERTICAL_MARGIN_PX,
            legacy_margin_px: None,
            bg_color: default_bg_color(),
            bg_opacity: default_bg_opacity(),
            line_stroke_px: default_line_stroke_px(),
            line_glow: true,
            line_glow_intensity: default_line_glow_intensity(),
            line_glow_radius_px: default_line_glow_radius_px(),
            monitor_device: None,
            display_mode: DisplayMode::Global,
            sticky_target: None,
            sticky_z_order: StickyZOrder::AboveEverything,
            legacy_wallpaper_mode: None,
        }
    }

    /// The target a single-target overlay is really talking about.
    ///
    /// `normalize` guarantees at least one, so this is never a guess. It is the
    /// accessor for the many places that care about one host — a list row, the
    /// prefill seed, the window title of the overlay — so those places do not
    /// each have to remember that indexing an empty list is a panic.
    pub fn first_target(&self) -> &TargetConfig {
        &self.targets[0]
    }

    pub fn first_target_mut(&mut self) -> &mut TargetConfig {
        &mut self.targets[0]
    }

    /// Append a host modelled on the one above it.
    ///
    /// **The last target, not the first.** The colour walk in
    /// [`TargetConfig::like`] moves one step from whatever it is given, so
    /// copying from a fixed donor gives every added host the same colour — five
    /// additions to a group produce five lines drawn over each other in the
    /// second palette entry. Copying from the end makes the walk continue, so
    /// each new host is unlike the one directly above it.
    pub fn add_target(&mut self) -> &mut TargetConfig {
        let target = match self.targets.last() {
            Some(existing) => TargetConfig::like(existing),
            None => TargetConfig::new(),
        };
        self.targets.push(target);
        self.targets.last_mut().expect("just pushed")
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BorderEffect {
    #[default]
    RgbLoop,
    RgbNoise,
    Disabled,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    TopLeft,
    TopCenter,
    #[default]
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

fn default_true() -> bool {
    true
}
fn default_line_color() -> String {
    "#4ade80".to_string()
}
fn default_timeout_color() -> String {
    "#ef4444".to_string()
}
fn default_window_seconds() -> u32 {
    60
}
fn default_scale() -> u32 {
    2
}
fn default_smooth_fps() -> u32 {
    DEFAULT_SMOOTH_FPS
}
fn default_prefill_line_color() -> String {
    DEFAULT_PREFILL_LINE_COLOR.to_string()
}
fn default_prefill_animation_sec() -> u32 {
    DEFAULT_PREFILL_ANIMATION_SEC
}
fn default_startup_border_effect() -> BorderEffect {
    BorderEffect::RgbLoop
}
fn default_border_animation_sec() -> u32 {
    DEFAULT_BORDER_ANIMATION_SEC
}
fn default_border_fade_sec() -> u32 {
    DEFAULT_BORDER_FADE_SEC
}

/// Convert a target frame rate into the interval used by the repaint scheduler.
pub fn smooth_frame_interval(fps: u32) -> Duration {
    let fps = fps.clamp(MIN_SMOOTH_FPS, MAX_SMOOTH_FPS);
    Duration::from_secs_f64(1.0 / fps as f64)
}

fn smooth_fps_from_legacy_delay(delay_ms: u32) -> u32 {
    let delay_ms = delay_ms.max(1);
    ((1_000 + delay_ms / 2) / delay_ms).clamp(MIN_SMOOTH_FPS, MAX_SMOOTH_FPS)
}

fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}
fn default_graph_height_px() -> u32 {
    DEFAULT_GRAPH_HEIGHT_PX
}
fn default_max_y_ms() -> u32 {
    DEFAULT_MAX_Y_MS
}
fn default_bg_color() -> String {
    DEFAULT_BG_COLOR.to_string()
}
fn default_bg_opacity() -> u32 {
    DEFAULT_BG_OPACITY
}
fn default_line_stroke_px() -> f32 {
    DEFAULT_LINE_STROKE_PX
}
fn default_line_glow_intensity() -> u32 {
    DEFAULT_LINE_GLOW_INTENSITY
}
fn default_line_glow_radius_px() -> u32 {
    DEFAULT_LINE_GLOW_RADIUS_PX
}

fn anchor_has_horizontal_edge(anchor: Anchor) -> bool {
    matches!(
        anchor,
        Anchor::TopLeft
            | Anchor::TopRight
            | Anchor::CenterLeft
            | Anchor::CenterRight
            | Anchor::BottomLeft
            | Anchor::BottomRight
    )
}

fn anchor_has_vertical_edge(anchor: Anchor) -> bool {
    matches!(
        anchor,
        Anchor::TopLeft
            | Anchor::TopCenter
            | Anchor::TopRight
            | Anchor::BottomLeft
            | Anchor::BottomCenter
            | Anchor::BottomRight
    )
}

impl TargetConfig {
    /// Clamp values that the UI might send out of range.
    ///
    /// A blank host is left alone rather than replaced. `validate` is what
    /// refuses to load a profile with an empty host, so this cannot quietly
    /// turn a host the user is mid-way through typing into a working ping.
    pub fn normalize(&mut self) {
        if self.timeout_ms == 0 {
            self.timeout_ms = DEFAULT_TIMEOUT_MS;
        }
    }
}

impl OverlayConfig {
    /// Fold a configuration written before targets existed into one.
    ///
    /// An overlay that already has targets keeps them and its legacy keys are
    /// dropped, which is what stops a profile written by a newer build from
    /// gaining a second host when an older one loads it.
    ///
    /// An overlay with *neither* targets nor a legacy probe is deliberately left
    /// with none. Inventing a host would put `1.1.1.1` on someone's screen that
    /// they never asked to ping, which is worse than refusing the file: it is a
    /// confident wrong answer with no gap in the UI to notice it by. `validate`
    /// rejects it instead, and the file is kept and reported.
    fn migrate_probe_into_targets(&mut self) {
        let legacy_probe = self.legacy_probe.take();
        let legacy_line = self.legacy_line_color.take();
        let legacy_timeout = self.legacy_timeout_color.take();
        let legacy_timeout_ms = self.legacy_timeout_ms.take();

        if self.targets.is_empty() {
            // `Option` so "this file predates targets" is distinguishable from
            // "this file has a target whose probe is missing".
            if let Some(probe) = legacy_probe {
                let mut target = TargetConfig {
                    id: unique_id(),
                    enabled: true,
                    probe,
                    timeout_ms: legacy_timeout_ms.unwrap_or_else(default_timeout_ms),
                    line_color: legacy_line.unwrap_or_else(default_line_color),
                    timeout_color: legacy_timeout.unwrap_or_else(default_timeout_color),
                };
                target.normalize();
                self.targets.push(target);
            }
            return;
        }

        for target in &mut self.targets {
            target.normalize();
        }
    }
}

impl Config {
    /// Clamp values that the UI might send out of range.
    pub fn normalize(&mut self) {
        self.profile_name = normalize_profile_name(&self.profile_name);
        for o in &mut self.overlays {
            o.window_seconds = o.window_seconds.max(MIN_WINDOW_SECONDS);
            o.scale = o.scale.clamp(1, MAX_SCALE_INPUT);
            if let Some(delay_ms) = o.legacy_smooth_delay_ms.take() {
                o.smooth_fps = smooth_fps_from_legacy_delay(delay_ms);
            }
            o.smooth_fps = o.smooth_fps.clamp(MIN_SMOOTH_FPS, MAX_SMOOTH_FPS);
            o.prefill_animation_sec = o
                .prefill_animation_sec
                .clamp(MIN_PREFILL_ANIMATION_SEC, MAX_PREFILL_ANIMATION_SEC);
            o.border_animation_sec = o
                .border_animation_sec
                .clamp(MIN_BORDER_ANIMATION_SEC, MAX_BORDER_ANIMATION_SEC);
            o.border_fade_sec = o.border_fade_sec.min(MAX_BORDER_FADE_SEC);
            if let Some(legacy_margin) = o.legacy_margin_px.take() {
                let legacy_margin = legacy_margin.min(MAX_MARGIN_OFFSET_PX as u32) as i32;
                o.horizontal_margin_px = if anchor_has_horizontal_edge(o.position) {
                    legacy_margin
                } else {
                    0
                };
                o.vertical_margin_px = if anchor_has_vertical_edge(o.position) {
                    legacy_margin
                } else {
                    0
                };
            }
            o.horizontal_margin_px = o
                .horizontal_margin_px
                .clamp(MIN_MARGIN_OFFSET_PX, MAX_MARGIN_OFFSET_PX);
            o.vertical_margin_px = o
                .vertical_margin_px
                .clamp(MIN_MARGIN_OFFSET_PX, MAX_MARGIN_OFFSET_PX);
            // Migrates a pre-targets profile and guarantees at least one target,
            // so the rest of this function and every reader downstream can
            // index the list without asking whether it is empty.
            o.migrate_probe_into_targets();
            if o.graph_height_px < MIN_GRAPH_HEIGHT_PX {
                o.graph_height_px = MIN_GRAPH_HEIGHT_PX;
            }
            if o.max_y_ms == 0 {
                o.max_y_ms = DEFAULT_MAX_Y_MS;
            }
            if !matches!(o.orientation, 0 | 90 | 180 | 270) {
                o.orientation = 0;
            }
            o.bg_opacity = o.bg_opacity.min(100);
            // A hand-edited `1e999` parses as an infinity and an infinity would
            // poison every transform it reaches; anything that is not a real
            // number settles at the default before the clamp.
            o.line_stroke_px = if o.line_stroke_px.is_finite() {
                o.line_stroke_px
                    .clamp(MIN_LINE_STROKE_PX, MAX_LINE_STROKE_PX)
            } else {
                DEFAULT_LINE_STROKE_PX
            };
            o.line_glow_intensity = o
                .line_glow_intensity
                .clamp(MIN_LINE_GLOW_INTENSITY, MAX_LINE_GLOW_INTENSITY);
            o.line_glow_radius_px = o
                .line_glow_radius_px
                .clamp(MIN_LINE_GLOW_RADIUS_PX, MAX_LINE_GLOW_RADIUS_PX);
            // A blank name is not a monitor. Left alone it would name a display
            // that does not exist and the overlay would stay hidden with no
            // way to tell from the screen that it had been asked for.
            o.monitor_device = o
                .monitor_device
                .take()
                .map(|device| device.trim().to_string())
                .filter(|device| !device.is_empty());
            // A file from before Display Mode says `"wallpaperMode": true`, and
            // the legacy key is folded in only while the mode is still the
            // default: a file that names a mode explicitly has already said
            // what it wants, and a stale key must not override it.
            if o.legacy_wallpaper_mode.take() == Some(true) && o.display_mode == DisplayMode::Global
            {
                o.display_mode = DisplayMode::Wallpaper;
            }
            // A half-typed condition must not match every window. The editor
            // drops emptied boxes as they are cleared, and this is the same
            // statement for a file that was hand-edited.
            if let Some(target) = &mut o.sticky_target {
                target
                    .when
                    .retain(|condition| !condition.value.trim().is_empty());
            }
        }
    }
}

const CONFIG_FILE: &str = "config.json";
const PROFILES_DIR_NAME: &str = "profiles";
const THEMES_DIR_NAME: &str = "themes";
const PROFILE_PREFIX: &str = "profile_";
const PROFILE_EXTENSION: &str = ".json";
const GLOBAL_CONFIG_FILE: &str = "globalconfig.json";
/// The window-based auto profile switching rules. Its own file rather than a
/// key in `globalconfig.json` because it is the one app-wide thing that is
/// also *read by the tray on a clock*: keeping it separate means a rule save
/// is one small file replace instead of a read-modify-write of the file the
/// active profile pointer lives in.
const RULES_FILE: &str = "rules.json";
const ACTIVE_PROFILE_KEY: &str = "activeProfile";
const ACTIVE_PROFILE_FILE_KEY: &str = "activeProfileFile";
/// Profile used when nothing else is stored, and the fallback after a failure.
pub const DEFAULT_PROFILE: &str = "default";
const MAX_PROFILE_NAME_LEN: usize = 48;
/// Largest accepted profile display name, in characters. Display names never
/// reach the file system, so this only keeps the UI from growing unbounded.
const MAX_PROFILE_DISPLAY_NAME_LEN: usize = 64;
/// Highest postfix tried when a profile file name is already taken.
const MAX_PROFILE_POSTFIX: u32 = 9999;
/// JSON key holding a profile's display name.
const PROFILE_NAME_KEY: &str = "profileName";
/// JSON key holding the Config window's own preferences.
const UI_PREFS_KEY: &str = "ui";

/// Result of loading the configuration, including any one-time migrations.
pub struct ConfigLoad {
    pub config: Config,
    pub active_profile: String,
    pub profiles: Vec<ProfileEntry>,
    pub notices: Vec<ConfigNotice>,
    pub prefs: GlobalPrefs,
}

/// App-wide preferences, stored beside the active profile pointer in
/// `globalconfig.json`.
///
/// Every field defaults, so a file written by an older build, or one a user
/// hand-edited, still loads. Only the `ui` object is ever rewritten, which
/// leaves `activeProfile`, `activeProfileFile` and any key a future version
/// adds untouched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPrefs {
    #[serde(default)]
    pub ui: UiPrefs,
}

/// Preferences that belong to the Config window rather than to a profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPrefs {
    /// Whether the navigation rail is collapsed to its icons.
    #[serde(default)]
    pub rail_collapsed: bool,
    /// Whether the window title carries the app version after the profile name.
    ///
    /// Off by default: the title is already long, and the About page is where
    /// the version belongs.
    #[serde(default)]
    pub show_version_in_title: bool,
    /// Whether selecting an overlay animates that overlay's border.
    ///
    /// On by default: the border is what ties a row in the Overlays list to the
    /// window on screen. Off suppresses only that preview; the per-overlay
    /// startup effect (`OverlayConfig::startup_border_effect`) is a separate
    /// setting, configured in the editor.
    #[serde(default = "default_true")]
    pub selection_border_animation: bool,
    /// Whether a target that leaves the active configuration keeps being probed.
    ///
    /// On by default: a profile you switch away from keeps its history
    /// continuous, and a host whose removal is still an unsaved edit keeps
    /// measuring until the removal is saved. Off stops every departure
    /// immediately, which is the behavior before this preference existed.
    #[serde(default = "default_true")]
    pub background_tracking: bool,
    /// Which of a theme's two files the window uses.
    ///
    /// `System` by default, so the window and the native tray menu agree without
    /// either being told: both read the same Windows setting. An explicit
    /// choice is the only way they can disagree.
    #[serde(default)]
    pub theme: ThemeMode,
}

/// Hand-written rather than derived because some fields default to on.
///
/// A derive can only produce `false` for a bool, so a derived `Default` would
/// disagree with the serde default used when a key is missing from an existing
/// `globalconfig.json`: `GlobalPrefs::default()` is what a missing or
/// unreadable file yields.
impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            rail_collapsed: false,
            show_version_in_title: false,
            selection_border_animation: true,
            background_tracking: true,
            theme: ThemeMode::System,
        }
    }
}

/// Which of a theme's two files the Config window uses.
///
/// Lives here rather than in the shell's `theme` module because `UiPrefs` holds
/// it, and `UiPrefs` is here. It carries no GUI dependency, which is the only
/// thing that would stop it: the palette and the egui mapping stay in the shell
/// because the renderer never reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    /// Follow Windows. The default, and the only value under which the Config
    /// window and the native tray menu agree for free: both read the same
    /// setting, so neither has to be told.
    #[default]
    System,
    Light,
    Dark,
}

/// A profile in the profiles directory: its id and the name shown in the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEntry {
    /// Canonical slug taken from the file name, unique per profile.
    pub id: String,
    /// Display name read from inside the file.
    pub name: String,
}

/// The active profile pointer as stored in `globalconfig.json`.
///
/// Either half can be missing: a build that predates `activeProfileFile`
/// wrote only the id, and a hand-edited file may keep just one of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StoredActive {
    id: Option<String>,
    file: Option<String>,
}

/// User-visible results of the one-time startup migrations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigNotice {
    Migrated {
        legacy_directory_retained: bool,
    },
    MigrationFailed(String),
    /// `config.json` was validated and moved into the profiles directory.
    ProfileImported,
    /// A default profile already existed, so `config.json` was left in place.
    ProfileImportSkipped,
    ProfileImportFailed(String),
    /// Profile files that had no display name were given a derived one.
    ProfileNamesBackfilled {
        count: usize,
    },
    ProfileFallback {
        profile: String,
        reason: String,
    },
    GlobalConfigFailed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MigrationOutcome {
    LegacyDirectoryRemoved,
    LegacyDirectoryRetained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileImport {
    Imported,
    DefaultAlreadyExists,
}

/// The on-disk configuration layout rooted at a single directory.
///
/// Every path is derived from `root`, so the whole store can be pointed at a
/// temporary directory in tests — and, through `PLO_CONFIG_DIR`, at a sandbox
/// for a whole session. The Config window keeps one so every file it writes
/// goes to the same root it loaded from.
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The directory every path below is derived from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join(PROFILES_DIR_NAME)
    }

    /// The theme directory this store reads from.
    pub fn themes_dir(&self) -> PathBuf {
        self.root.join(THEMES_DIR_NAME)
    }

    /// Only the migration source for the profiles directory; new
    /// configurations are stored as profile files.
    fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn global_config_path(&self) -> PathBuf {
        self.root.join(GLOBAL_CONFIG_FILE)
    }

    fn profile_path(&self, name: &str) -> io::Result<PathBuf> {
        Ok(self.profile_file_path(&canonical_profile_name(name)?))
    }

    /// Path of a profile id that is already canonical.
    ///
    /// Ids are built here instead of through `profile_path` because a postfix
    /// can make an id longer than the sanitizer's own length cap, and the
    /// sanitizer would then truncate it back to a different name.
    fn profile_file_path(&self, slug: &str) -> PathBuf {
        self.profiles_dir()
            .join(format!("{PROFILE_PREFIX}{slug}{PROFILE_EXTENSION}"))
    }

    /// The id a stored profile file name refers to, if it names one.
    ///
    /// The name has to match `profile_<slug>.json` with a canonical slug, so a
    /// hand-edited path can never be used to read a file outside the profiles
    /// directory.
    fn profile_id_from_file_name(&self, file_name: &str) -> Option<String> {
        let stem = file_name
            .strip_prefix(PROFILE_PREFIX)?
            .strip_suffix(PROFILE_EXTENSION)?;
        sanitize_profile_name(stem).ok()
    }

    /// Every readable profile in the profiles directory, sorted by name.
    fn list_profiles(&self) -> Vec<String> {
        let mut profiles = Vec::new();
        let Ok(entries) = fs::read_dir(self.profiles_dir()) else {
            return profiles;
        };
        for entry in entries.flatten() {
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(file_name) = entry.file_name().into_string() else {
                continue;
            };
            let Some(stem) = file_name.strip_suffix(PROFILE_EXTENSION) else {
                continue;
            };
            let Some(name) = stem.strip_prefix(PROFILE_PREFIX) else {
                continue;
            };
            // Ignore anything that is not already a canonical slug, so a
            // hand-made file can never be addressed as a path.
            if sanitize_profile_name(name).as_deref() == Ok(name) {
                profiles.push(name.to_string());
            }
        }
        profiles.sort();
        profiles
    }

    /// Every readable profile with the display name stored inside it.
    ///
    /// A file that cannot be read still appears in the list under a name
    /// derived from its id, so it can be selected and reported on.
    pub fn list_profiles_detailed(&self) -> Vec<ProfileEntry> {
        self.list_profiles()
            .into_iter()
            .map(|id| {
                let name = self
                    .read_profile_name(&id)
                    .unwrap_or_else(|| prettify_profile_id(&id));
                ProfileEntry { id, name }
            })
            .collect()
    }

    /// Overlay count per profile id, for every profile whose file parses.
    ///
    /// A profile whose file cannot be read is left **out of the map** rather than
    /// counted as zero: "not read yet" and "no overlays" are different facts and
    /// the UI draws the first as no number at all. A profile that really has no
    /// overlays is in the map, holding zero.
    ///
    /// This costs one file read per profile, so it belongs on a user action or on
    /// arrival at the page that shows the counts, never in a frame.
    pub fn profile_overlay_counts(&self) -> HashMap<String, usize> {
        self.list_profiles()
            .into_iter()
            .filter_map(|id| {
                let count = self.load_profile(&id).ok()?.overlays.len();
                Some((id, count))
            })
            .collect()
    }

    /// The display name stored in a profile file, if it has a usable one.
    fn read_profile_name(&self, id: &str) -> Option<String> {
        let raw = fs::read_to_string(self.profile_file_path(id)).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(strip_bom(&raw)).ok()?;
        let name = normalize_profile_name(value.get(PROFILE_NAME_KEY)?.as_str()?);
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Store a derived display name in every profile file that lacks one.
    ///
    /// Returns how many files were updated. Unreadable files are left alone so
    /// a broken profile is still reported when it is selected.
    fn backfill_profile_names(&self) -> usize {
        let mut updated = 0;
        for id in self.list_profiles() {
            if self.read_profile_name(&id).is_some() {
                continue;
            }
            let Ok(mut config) = self.load_profile(&id) else {
                continue;
            };
            config.profile_name = prettify_profile_id(&id);
            if self.save_profile(&id, &config).is_ok() {
                updated += 1;
            }
        }
        updated
    }

    /// The first free id for `base`, so a taken file name never overwrites a
    /// profile that is not being replaced.
    ///
    /// Display names may repeat, so the collision is resolved by appending
    /// `_2`, `_3` and so on to the id. `replace` is the file a rename is
    /// allowed to overwrite, which keeps "rename to the same name" in place.
    fn unique_profile_id(&self, base: &str, replace: Option<&Path>) -> io::Result<String> {
        let taken = |slug: &str| {
            let path = self.profile_file_path(slug);
            path.exists() && replace != Some(path.as_path())
        };
        if !taken(base) {
            return Ok(base.to_string());
        }
        for postfix in 2..=MAX_PROFILE_POSTFIX {
            let candidate = with_postfix(base, postfix);
            if !taken(&candidate) {
                return Ok(candidate);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("No free profile id is left for \"{base}\"."),
        ))
    }

    /// Read, validate and normalize one profile file.
    pub fn load_profile(&self, name: &str) -> io::Result<Config> {
        let path = self.profile_path(name)?;
        let raw = fs::read_to_string(&path)?;
        let mut config = parse_and_validate(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        config.normalize();
        Ok(config)
    }

    /// Write one profile file atomically.
    pub fn save_profile(&self, name: &str, config: &Config) -> io::Result<()> {
        let json = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
        write_atomic(
            &self.profile_file_path(&canonical_profile_name(name)?),
            &json,
        )
    }

    /// Create an empty profile under a display name and return its id and name.
    ///
    /// The display name is stored in the new file and does not have to be
    /// unique; only the id is, so a taken file name gets a postfix instead of
    /// an error.
    pub fn create_profile(&self, display_name: &str) -> io::Result<ProfileEntry> {
        let base = canonical_profile_name(display_name)?;
        fs::create_dir_all(self.profiles_dir())?;
        let id = self.unique_profile_id(&base, None)?;
        let name = stored_profile_name(display_name, &id);
        let mut config = Config {
            profile_name: name.clone(),
            ..Config::default()
        };
        config.normalize();
        self.save_profile(&id, &config)?;
        Ok(ProfileEntry { id, name })
    }

    /// Rename a profile, store the new display name in it, and return the id
    /// and name it ended up with.
    ///
    /// The file is moved first so the overlays are never rewritten in place,
    /// and the new name is written afterwards because it lives in the file.
    pub fn rename_profile(&self, from: &str, display_name: &str) -> io::Result<ProfileEntry> {
        let from_id = canonical_profile_name(from)?;
        let from_path = self.profile_file_path(&from_id);
        if !from_path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("There is no profile named \"{from}\"."),
            ));
        }
        let base = canonical_profile_name(display_name)?;
        fs::create_dir_all(self.profiles_dir())?;
        let id = self.unique_profile_id(&base, Some(from_path.as_path()))?;
        let name = stored_profile_name(display_name, &id);
        let mut config = self.load_profile(from)?;
        config.profile_name = name.clone();
        config.normalize();
        if id != from_id {
            fs::rename(&from_path, self.profile_file_path(&id))?;
        }
        self.save_profile(&id, &config)?;
        Ok(ProfileEntry { id, name })
    }

    /// Copy a profile's overlays into a new profile and return its id and name.
    ///
    /// The source file is only read, never moved, and the new id is postfixed
    /// when the name is taken, exactly like [`Store::create_profile`].
    pub fn duplicate_profile(&self, from: &str, display_name: &str) -> io::Result<ProfileEntry> {
        let from_id = canonical_profile_name(from)?;
        if !self.profile_file_path(&from_id).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("There is no profile named \"{from}\"."),
            ));
        }
        let base = canonical_profile_name(display_name)?;
        fs::create_dir_all(self.profiles_dir())?;
        let id = self.unique_profile_id(&base, None)?;
        let name = stored_profile_name(display_name, &id);
        let mut config = self.load_profile(&from_id)?;
        config.profile_name = name.clone();
        config.normalize();
        self.save_profile(&id, &config)?;
        Ok(ProfileEntry { id, name })
    }

    pub fn delete_profile(&self, name: &str) -> io::Result<()> {
        fs::remove_file(self.profile_path(name)?)
    }

    /// Remember the active profile for the next launch.
    ///
    /// Both the id and the file name are stored, and the file name is the only
    /// thing [`Store::load`] needs to find the profile again. Both keys are
    /// removed when the default profile is active, so a fresh install keeps
    /// `globalconfig.json` as an empty object until something is actually
    /// stored.
    pub fn set_active_profile(&self, name: &str) -> io::Result<()> {
        let slug = canonical_profile_name(name)?;
        // Preserve any other keys so future global preferences are not lost.
        let mut value = self.read_global_config();
        let Some(object) = value.as_object_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "globalconfig.json is not a JSON object.",
            ));
        };
        if slug == DEFAULT_PROFILE {
            object.remove(ACTIVE_PROFILE_KEY);
            object.remove(ACTIVE_PROFILE_FILE_KEY);
        } else {
            object.insert(ACTIVE_PROFILE_KEY.to_string(), serde_json::json!(slug));
            object.insert(
                ACTIVE_PROFILE_FILE_KEY.to_string(),
                serde_json::json!(profile_file_name(&slug)),
            );
        }
        let json = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        write_atomic(&self.global_config_path(), &json)
    }

    /// Where `globalconfig.json` says the active profile is.
    ///
    /// The file name is authoritative, because it is what the profile list and
    /// every file operation agree on. The id is only a fallback for a
    /// `globalconfig.json` written by a build that did not store it yet.
    fn stored_active_profile(&self) -> StoredActive {
        let value = self.read_global_config();
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        };
        StoredActive {
            id: string(ACTIVE_PROFILE_KEY),
            file: string(ACTIVE_PROFILE_FILE_KEY),
        }
    }

    fn ensure_global_config(&self) -> io::Result<()> {
        let path = self.global_config_path();
        if path.exists() {
            return Ok(());
        }
        write_atomic(&path, "{}")
    }

    fn read_global_config(&self) -> serde_json::Value {
        match fs::read_to_string(self.global_config_path()) {
            Ok(raw) => serde_json::from_str::<serde_json::Value>(strip_bom(&raw))
                .ok()
                .filter(|value| value.is_object())
                .unwrap_or_else(|| serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        }
    }

    /// The app-wide preferences, defaulting rather than failing when the file
    /// holds something unexpected.
    pub fn read_global_prefs(&self) -> GlobalPrefs {
        serde_json::from_value(self.read_global_config()).unwrap_or_default()
    }

    /// Store the preferences, replacing only the `ui` key.
    ///
    /// The merge is the whole point: the active profile pointer lives in the
    /// same file, so writing the preferences must not drop it.
    pub fn write_global_prefs(&self, prefs: &GlobalPrefs) -> io::Result<()> {
        let ui = serde_json::to_value(&prefs.ui).map_err(io::Error::other)?;
        let mut value = self.read_global_config();
        let Some(object) = value.as_object_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "globalconfig.json is not a JSON object.",
            ));
        };
        object.insert(UI_PREFS_KEY.to_string(), ui);
        let json = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        write_atomic(&self.global_config_path(), &json)
    }

    /// Path of `rules.json`.
    pub fn rules_path(&self) -> PathBuf {
        self.root.join(RULES_FILE)
    }

    /// Read the auto profile switching rules.
    ///
    /// A missing file is not an error: it is the state of every installation
    /// that has never used the feature, and it means "switching off" rather
    /// than "empty rules pending". A file that exists but will not parse is an
    /// error, and its caller decides what to do — the tray keeps the last good
    /// rules it had and the window refuses to overwrite the file silently.
    pub fn load_rules(&self) -> io::Result<AutoRules> {
        let path = self.rules_path();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(AutoRules::default())
            }
            Err(error) => return Err(error),
        };
        serde_json::from_str(strip_bom(&raw)).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not valid rules JSON: {error}", path.display()),
            )
        })
    }

    /// Write the rules through the same temporary-file dance profiles use.
    ///
    /// Atomic because the tray watches this file's modification time: a
    /// half-written file would both parse as garbage and look like a change.
    pub fn save_rules(&self, rules: &AutoRules) -> io::Result<()> {
        let contents = serde_json::to_string_pretty(rules).map_err(io::Error::other)?;
        write_atomic(&self.rules_path(), &format!("{contents}\n"))
    }

    /// Move a validated `config.json` into the profiles directory.
    fn import_config(&self) -> io::Result<Option<ProfileImport>> {
        let root = self.config_path();
        if !root.is_file() {
            return Ok(None);
        }
        if self.profile_path(DEFAULT_PROFILE)?.exists() {
            return Ok(Some(ProfileImport::DefaultAlreadyExists));
        }
        let raw = fs::read_to_string(&root)?;
        let mut config = parse_and_validate(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        config.normalize();
        self.save_profile(DEFAULT_PROFILE, &config)?;
        // Only drop the old file once the profile copy is safely in place.
        fs::remove_file(&root)?;
        Ok(Some(ProfileImport::Imported))
    }

    /// Load the active profile, creating the profiles layout on first run.
    pub fn load(&self, legacy_dir: &Path) -> ConfigLoad {
        let mut notices = Vec::new();

        if !self.config_path().exists() {
            match migrate_legacy_config(&self.root, legacy_dir) {
                Ok(Some(MigrationOutcome::LegacyDirectoryRemoved)) => {
                    notices.push(ConfigNotice::Migrated {
                        legacy_directory_retained: false,
                    });
                }
                Ok(Some(MigrationOutcome::LegacyDirectoryRetained)) => {
                    notices.push(ConfigNotice::Migrated {
                        legacy_directory_retained: true,
                    });
                }
                Ok(None) => {}
                Err(error) => notices.push(ConfigNotice::MigrationFailed(error.to_string())),
            }
        }

        match fs::create_dir_all(self.profiles_dir()) {
            Ok(()) => match self.import_config() {
                Ok(Some(ProfileImport::Imported)) => notices.push(ConfigNotice::ProfileImported),
                Ok(Some(ProfileImport::DefaultAlreadyExists)) => {
                    notices.push(ConfigNotice::ProfileImportSkipped);
                }
                Ok(None) => {}
                Err(error) => notices.push(ConfigNotice::ProfileImportFailed(error.to_string())),
            },
            Err(error) => notices.push(ConfigNotice::ProfileImportFailed(error.to_string())),
        }

        if let Err(error) = self.ensure_global_config() {
            notices.push(ConfigNotice::GlobalConfigFailed(error.to_string()));
        }

        // Profile files written before profiles had display names get one
        // derived from their id, so every list and title has a name to show.
        let backfilled = self.backfill_profile_names();
        if backfilled > 0 {
            notices.push(ConfigNotice::ProfileNamesBackfilled { count: backfilled });
        }

        // `globalconfig.json` is the only source for what to load: the stored
        // file name first, then the stored id for a file written by a build
        // that did not store one. Nothing else is scanned, so a profile the
        // user never selected can never come back on its own.
        let stored = self.stored_active_profile();
        let mut candidates: Vec<String> = Vec::new();
        let mut fallback: Option<(String, String)> = None;
        // The first pointer that resolves to something is the one reported when
        // it turns out to be unusable.
        let add = |id: String, candidates: &mut Vec<String>| {
            if !candidates.contains(&id) {
                candidates.push(id);
            }
        };
        if let Some(file) = stored.file.as_deref() {
            match self.profile_id_from_file_name(file) {
                Some(id) => add(id, &mut candidates),
                None => {
                    fallback = Some((
                        file.to_string(),
                        "its stored file name is not a profile file".to_string(),
                    ));
                }
            }
        }
        if let Some(id) = stored.id.as_deref() {
            match sanitize_profile_name(id) {
                Ok(_) => add(id.to_string(), &mut candidates),
                Err(_) if fallback.is_none() => {
                    fallback = Some((id.to_string(), "its stored name is not valid".to_string()));
                }
                Err(_) => {}
            }
        }
        // The default profile is always the last resort.
        add(DEFAULT_PROFILE.to_string(), &mut candidates);

        let mut loaded = None;
        for id in &candidates {
            if !self.profile_file_path(id).is_file() {
                // A missing default profile is the first-run case, which the
                // fresh config below creates, not a fallback worth reporting.
                if fallback.is_none() && id != DEFAULT_PROFILE {
                    fallback = Some((id.clone(), "it is no longer on disk".to_string()));
                }
                continue;
            }
            match self.load_profile(id) {
                Ok(config) => {
                    loaded = Some((id.clone(), config));
                    break;
                }
                Err(error) if fallback.is_none() => {
                    fallback = Some((id.clone(), error.to_string()));
                }
                Err(_) => {}
            }
        }

        let (active_profile, config) = match loaded {
            Some(resolved) => resolved,
            // Nothing readable on disk: start from a fresh default profile.
            None => {
                let mut fresh = Config {
                    profile_name: prettify_profile_id(DEFAULT_PROFILE),
                    ..Config::default()
                };
                fresh.normalize();
                if let Err(error) = self.save_profile(DEFAULT_PROFILE, &fresh) {
                    notices.push(ConfigNotice::ProfileImportFailed(error.to_string()));
                }
                (DEFAULT_PROFILE.to_string(), fresh)
            }
        };

        if let Some((profile, reason)) = fallback {
            notices.push(ConfigNotice::ProfileFallback { profile, reason });
        }

        // An absent key already means the default profile, so a fresh install
        // never has to rewrite globalconfig.json.
        let wanted_id = (active_profile != DEFAULT_PROFILE).then_some(active_profile.as_str());
        let wanted_file = wanted_id.map(profile_file_name);
        if stored.id.as_deref() != wanted_id || stored.file.as_deref() != wanted_file.as_deref() {
            if let Err(error) = self.set_active_profile(&active_profile) {
                notices.push(ConfigNotice::GlobalConfigFailed(error.to_string()));
            }
        }

        ConfigLoad {
            config,
            active_profile,
            profiles: self.list_profiles_detailed(),
            notices,
            prefs: self.read_global_prefs(),
        }
    }
}

fn store() -> Store {
    Store::new(config_dir())
}

/// Load the configuration as if the store were rooted at `root`.
///
/// The legacy migration is deliberately skipped: a rooted load is a sandbox
/// (tests, capture sessions), and the user's `~/.PingLatencyOverlay` is none of
/// its business.
pub fn load_rooted(root: &Path) -> ConfigLoad {
    Store::new(root.to_path_buf()).load(&root.join("missing-legacy"))
}

fn canonical_profile_name(name: &str) -> io::Result<String> {
    sanitize_profile_name(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// `~/.config/.PingLatencyOverlay`, or `PLO_CONFIG_DIR` when that names one.
///
/// The override moves the profiles, the themes, `globalconfig.json`,
/// `rules.json` and the log in one step, so a capture session or a portable
/// run can work against a sandbox without touching the user's own files.
pub fn config_dir() -> PathBuf {
    resolve_config_dir(std::env::var_os("PLO_CONFIG_DIR").as_deref())
}

/// The decision `config_dir` makes, split out so a test can drive it without
/// mutating the process environment.
///
/// `diagnostics::log_path` reads `config_dir` too, so a test that set the
/// variable would be racing every other thread's log line.
fn resolve_config_dir(override_value: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(value) = override_value {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".PingLatencyOverlay")
}

/// `~/.config/.PingLatencyOverlay/profiles`
pub fn profiles_dir() -> PathBuf {
    store().profiles_dir()
}

/// `~/.config/.PingLatencyOverlay/themes`
///
/// One directory per theme, each holding that theme's colours and any artwork
/// it wants to override. `default` is the theme the app ships and repairs; the
/// rest are the user's.
pub fn themes_dir() -> PathBuf {
    store().themes_dir()
}

/// `~/.config/.PingLatencyOverlay/globalconfig.json`
pub fn global_config_path() -> PathBuf {
    store().global_config_path()
}

/// Legacy `~/.PingLatencyOverlay` directory.
fn legacy_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".PingLatencyOverlay")
}

/// Turn a user-entered profile name into a stable, portable slug.
///
/// Names are lowercased and reduced to ASCII alphanumerics plus `-` and `_`, so
/// a slug can never contain a path separator and profile files always stay
/// inside the profiles directory. The `profile_` file prefix also keeps the
/// generated file names away from reserved Windows device names.
pub fn sanitize_profile_name(raw: &str) -> Result<String, String> {
    let mut slug = String::new();
    for character in raw.trim().chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug
        .trim_matches('-')
        .chars()
        .take(MAX_PROFILE_NAME_LEN)
        .collect();
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        return Err("A profile name needs at least one letter or digit.".to_string());
    }
    Ok(slug)
}

/// Clean a display name for storage and display.
///
/// Display names are free-form and never reach the file system, so only
/// surrounding whitespace, control characters and the length cap are removed.
fn normalize_profile_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_PROFILE_DISPLAY_NAME_LEN)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Turn a slug into a readable default name: `home-net_2` becomes `Home Net 2`.
fn prettify_profile_id(id: &str) -> String {
    let mut name = String::new();
    for word in id.split(['-', '_']).filter(|word| !word.is_empty()) {
        if !name.is_empty() {
            name.push(' ');
        }
        let mut characters = word.chars();
        if let Some(first) = characters.next() {
            name.extend(first.to_uppercase());
            name.push_str(characters.as_str());
        }
    }
    if name.is_empty() {
        id.to_string()
    } else {
        name
    }
}

/// The display name to store for a profile, derived from its id when empty.
fn stored_profile_name(display_name: &str, id: &str) -> String {
    let name = normalize_profile_name(display_name);
    if name.is_empty() {
        prettify_profile_id(id)
    } else {
        name
    }
}

/// The name to show for a loaded profile.
///
/// Files written before profiles had display names have none, so one derived
/// from the id is used instead.
pub fn display_name(config: &Config, id: &str) -> String {
    stored_profile_name(&config.profile_name, id)
}

/// Append a postfix to a slug while staying inside the length cap, so the
/// result is still a canonical id that the profile list accepts.
fn with_postfix(base: &str, postfix: u32) -> String {
    let tail = format!("_{postfix}");
    let keep = MAX_PROFILE_NAME_LEN.saturating_sub(tail.len());
    let base: String = base.chars().take(keep).collect();
    format!("{}{tail}", base.trim_end_matches('-'))
}

/// The file name that stores a profile, shown as UI help text.
pub fn profile_file_name(name: &str) -> String {
    match sanitize_profile_name(name) {
        Ok(slug) => format!("{PROFILE_PREFIX}{slug}{PROFILE_EXTENSION}"),
        Err(_) => name.to_string(),
    }
}

/// Every readable profile in the profiles directory, sorted by id.
pub fn list_profiles_detailed() -> Vec<ProfileEntry> {
    store().list_profiles_detailed()
}

/// Read, validate and normalize one profile file.
pub fn load_profile(name: &str) -> io::Result<Config> {
    store().load_profile(name)
}

/// Overlay count per profile id, for every profile whose file parses.
///
/// Costs one file read per profile, so call it on a user action or on arrival at
/// the page that shows the counts. A profile that will not parse is left out of
/// the map rather than counted as zero.
pub fn profile_overlay_counts() -> HashMap<String, usize> {
    store().profile_overlay_counts()
}

/// Write one profile file atomically.
pub fn save_profile(name: &str, config: &Config) -> io::Result<()> {
    store().save_profile(name, config)
}

/// Create an empty profile under a display name and return its id and name.
///
/// The name does not have to be unique; a taken file name gets a postfix.
pub fn create_profile(display_name: &str) -> io::Result<ProfileEntry> {
    store().create_profile(display_name)
}

/// Rename a profile to a new display name and return its id and name.
///
/// The name does not have to be unique; a taken file name gets a postfix.
pub fn rename_profile(from: &str, display_name: &str) -> io::Result<ProfileEntry> {
    store().rename_profile(from, display_name)
}

/// Copy a profile's overlays into a new profile and return its id and name.
pub fn duplicate_profile(from: &str, display_name: &str) -> io::Result<ProfileEntry> {
    store().duplicate_profile(from, display_name)
}

/// Delete one profile file.
pub fn delete_profile(name: &str) -> io::Result<()> {
    store().delete_profile(name)
}

/// Remember the active profile for the next launch.
/// The app-wide preferences currently stored in `globalconfig.json`.
pub fn read_global_prefs() -> GlobalPrefs {
    store().read_global_prefs()
}

/// Store the app-wide preferences, leaving the active profile pointer in place.
pub fn write_global_prefs(prefs: &GlobalPrefs) -> io::Result<()> {
    store().write_global_prefs(prefs)
}

/// Path of the auto profile switching rules file.
pub fn rules_path() -> PathBuf {
    store().rules_path()
}

/// Read the auto profile switching rules; a missing file is an inert default.
pub fn load_rules() -> io::Result<AutoRules> {
    store().load_rules()
}

/// Write the auto profile switching rules atomically.
pub fn save_rules(rules: &AutoRules) -> io::Result<()> {
    store().save_rules(rules)
}

pub fn set_active_profile(name: &str) -> io::Result<()> {
    store().set_active_profile(name)
}

/// Structural validation of a parsed configuration.
///
/// Unknown keys are ignored so hand-edited and legacy files keep loading, and
/// out-of-range values are handled by [`Config::normalize`] instead of being
/// rejected here.
pub fn validate(config: &Config) -> Result<(), String> {
    let mut seen = HashSet::new();
    for overlay in &config.overlays {
        if overlay.id.trim().is_empty() {
            return Err("an overlay has an empty id".to_string());
        }
        if !seen.insert(overlay.id.as_str()) {
            return Err(format!(
                "overlay id \"{}\" is used more than once",
                overlay.id
            ));
        }
        // Checked per host rather than per overlay, and naming the position: a
        // group of four hosts is four chances to have a blank one, and "overlay
        // X has an empty probe host" does not say which of them.
        if overlay.targets.is_empty() {
            return Err(format!("overlay \"{}\" has no host to probe", overlay.id));
        }
        let mut target_ids = HashSet::new();
        for (index, target) in overlay.targets.iter().enumerate() {
            if target.id.trim().is_empty() {
                return Err(format!(
                    "overlay \"{}\" has a host at position {} with an empty id",
                    overlay.id,
                    index + 1
                ));
            }
            if !target_ids.insert(target.id.as_str()) {
                return Err(format!(
                    "target id \"{}\" is used more than once in overlay \"{}\"",
                    target.id, overlay.id
                ));
            }
            if target.probe.host().trim().is_empty() {
                return Err(format!(
                    "overlay \"{}\" host {} has an empty probe host",
                    overlay.id,
                    index + 1
                ));
            }
            if matches!(target.probe, ProbeConfig::Tcp { port: 0, .. }) {
                return Err(format!(
                    "overlay \"{}\" host {} ({}) is missing its TCP port",
                    overlay.id,
                    index + 1,
                    target.probe.host()
                ));
            }
        }
    }
    Ok(())
}

/// Load the active profile, creating the profiles layout on first run.
pub fn load() -> ConfigLoad {
    store().load(&legacy_config_dir())
}

/// Write `contents` through a temporary file so a reader never observes a
/// partially written configuration.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{stamp}", std::process::id()));

    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = rename_replace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Rename over the destination, replacing it where a plain rename refuses to
/// overwrite an existing file.
fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) => {
            if !to.exists() {
                return Err(error);
            }
            fs::remove_file(to)?;
            fs::rename(from, to)
        }
    }
}

fn strip_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

fn parse_config(raw: &str) -> Result<Config, serde_json::Error> {
    serde_json::from_str(strip_bom(raw))
}

/// Parse a file and check its overall shape before it is trusted as a profile.
///
/// Normalized **before** it is validated, not after, and the order is
/// load-bearing. A profile written before hosts were grouped has no `targets`
/// key at all, so validating first rejects every existing user's file — and
/// the failure is "overlay has no host to probe", which describes the migration
/// rather than anything the user did. Repairing first is also the natural order:
/// normalize only fixes what it can (`marginPx`, `smoothDelayMs`, the legacy
/// probe, an out-of-range number) and leaves everything validation is there to
/// catch exactly as it found it — a blank host stays blank and a TCP port of
/// zero stays zero.
fn parse_and_validate(raw: &str) -> Result<Config, String> {
    let value: serde_json::Value =
        serde_json::from_str(strip_bom(raw)).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("the configuration root must be a JSON object".to_string());
    }
    if let Some(overlays) = value.get("overlays") {
        if !overlays.is_array() {
            return Err("\"overlays\" must be an array".to_string());
        }
    }
    let mut config: Config = serde_json::from_value(value).map_err(|error| error.to_string())?;
    config.normalize();
    validate(&config)?;
    Ok(config)
}

fn migrate_legacy_config(
    new_dir: &Path,
    legacy_dir: &Path,
) -> io::Result<Option<MigrationOutcome>> {
    let new_path = new_dir.join(CONFIG_FILE);
    if new_path.exists() {
        return Ok(None);
    }

    let legacy_path = legacy_dir.join(CONFIG_FILE);
    if !legacy_path.is_file() {
        return Ok(None);
    }
    let legacy_contains_only_config = directory_contains_only_config(legacy_dir)?;
    fs::create_dir_all(new_dir)?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary_path = new_dir.join(format!(
        ".{CONFIG_FILE}.migrating-{}-{stamp}",
        std::process::id()
    ));

    let migration_result = (|| {
        let mut source = fs::File::open(&legacy_path)?;
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        io::copy(&mut source, &mut destination)?;
        destination.sync_all()?;
        drop(destination);

        let raw = fs::read_to_string(&temporary_path)?;
        parse_config(&raw)
            .map(|_| ())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::rename(&temporary_path, &new_path)
    })();
    if let Err(error) = migration_result {
        let _ = fs::remove_file(&temporary_path);
        if new_path.exists() {
            return Ok(None);
        }
        return Err(error);
    }

    let legacy_directory_retained = fs::remove_file(&legacy_path).is_err()
        || !(legacy_contains_only_config && fs::remove_dir(legacy_dir).is_ok());
    Ok(Some(if legacy_directory_retained {
        MigrationOutcome::LegacyDirectoryRetained
    } else {
        MigrationOutcome::LegacyDirectoryRemoved
    }))
}

fn directory_contains_only_config(directory: &Path) -> io::Result<bool> {
    let mut entries = fs::read_dir(directory)?;
    let Some(first) = entries.next() else {
        return Ok(false);
    };
    let first = first?;
    if entries.next().transpose()?.is_some() {
        return Ok(false);
    }
    Ok(first
        .file_name()
        .to_string_lossy()
        .eq_ignore_ascii_case(CONFIG_FILE)
        && first.file_type()?.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Condition, MatchMode, Part};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "ping-latency-overlay-{label}-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_config(directory: &Path, contents: &str) -> PathBuf {
        fs::create_dir_all(directory).expect("create config directory");
        let path = directory.join(CONFIG_FILE);
        fs::write(&path, contents).expect("write config");
        path
    }

    const VALID_CONFIG: &str =
        r#"{"overlays":[{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"}}]}"#;

    /// `PLO_CONFIG_DIR` moves the whole configuration, and the resolver is a
    /// function so this can be tested without mutating the process
    /// environment `diagnostics::log_path` also reads.
    #[test]
    fn the_config_dir_override_applies_only_when_it_names_a_path() {
        let fallback = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join(".PingLatencyOverlay");
        assert_eq!(resolve_config_dir(None), fallback);
        assert_eq!(
            resolve_config_dir(Some(std::ffi::OsStr::new(""))),
            fallback,
            "an empty override must not move the configuration to the current directory"
        );
        let sandbox = PathBuf::from(r"C:\plo-capture-sandbox");
        assert_eq!(
            resolve_config_dir(Some(sandbox.as_os_str())),
            sandbox,
            "a non-empty override has to be used as written"
        );
    }

    /// A rooted load reads the sandbox it is handed and nothing else, which is
    /// what lets a test or a capture session run without touching the user's
    /// own profiles.
    #[test]
    fn a_rooted_load_reads_the_sandbox_it_was_given() {
        let root = TestDir::new("load-rooted");
        let store = store_at(root.path());
        let config = Config {
            overlays: vec![OverlayConfig::new()],
            ..Config::default()
        };
        store
            .save_profile("default", &config)
            .expect("save sandbox profile");
        store
            .set_active_profile("default")
            .expect("point at the sandbox profile");

        let loaded = load_rooted(root.path());
        assert_eq!(loaded.active_profile, "default");
        assert_eq!(loaded.config.overlays.len(), 1);
        assert_eq!(loaded.config.overlays[0].name, "New overlay");
    }

    #[test]
    fn new_overlay_uses_documented_defaults() {
        let overlay = OverlayConfig::new();
        assert_eq!(overlay.name, "New overlay");
        assert!(overlay.enabled);
        assert_eq!(overlay.scale, 2);
        assert!(overlay.smooth_rendering);
        assert_eq!(overlay.smooth_fps, DEFAULT_SMOOTH_FPS);
        assert!(overlay.cosmetic_startup_prefill);
        assert_eq!(overlay.prefill_line_color, DEFAULT_PREFILL_LINE_COLOR);
        assert_eq!(overlay.prefill_animation_sec, DEFAULT_PREFILL_ANIMATION_SEC);
        assert_eq!(overlay.startup_border_effect, BorderEffect::RgbLoop);
        assert_eq!(overlay.border_animation_sec, DEFAULT_BORDER_ANIMATION_SEC);
        assert_eq!(overlay.border_fade_sec, DEFAULT_BORDER_FADE_SEC);
        assert_eq!(overlay.first_target().timeout_ms, 1_000);
        assert_eq!(overlay.graph_height_px, 60);
        assert_eq!(overlay.max_y_ms, 1_000);
        assert_eq!(overlay.horizontal_margin_px, 0);
        assert_eq!(overlay.vertical_margin_px, 0);
        assert_eq!(overlay.bg_opacity, 0);
        assert_eq!(overlay.line_stroke_px, DEFAULT_LINE_STROKE_PX);
        assert!(overlay.line_glow);
        assert_eq!(overlay.line_glow_intensity, DEFAULT_LINE_GLOW_INTENSITY);
        assert_eq!(overlay.line_glow_radius_px, DEFAULT_LINE_GLOW_RADIUS_PX);
    }

    /// The underglow is off for every profile written before it existed, and
    /// the two numbers the UI can push beyond their range settle at the ends.
    #[test]
    fn line_glow_defaults_off_and_clamps_to_its_limits() {
        let overlay: OverlayConfig =
            serde_json::from_str(r#"{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"}}"#)
                .expect("legacy config");
        assert!(!overlay.line_glow);
        assert_eq!(overlay.line_glow_intensity, DEFAULT_LINE_GLOW_INTENSITY);
        assert_eq!(overlay.line_glow_radius_px, DEFAULT_LINE_GLOW_RADIUS_PX);

        let mut config = Config {
            overlays: vec![OverlayConfig {
                line_glow: true,
                line_glow_intensity: 5_000,
                line_glow_radius_px: 0,
                ..OverlayConfig::new()
            }],
            ..Config::default()
        };
        config.normalize();
        assert_eq!(
            config.overlays[0].line_glow_intensity,
            MAX_LINE_GLOW_INTENSITY
        );
        assert_eq!(
            config.overlays[0].line_glow_radius_px,
            MIN_LINE_GLOW_RADIUS_PX
        );
    }

    /// The stroke width is the old fixed 1.5px for every profile written
    /// before the setting existed, and a value pushed out of range settles at
    /// an end. A non-finite one settles at the default rather than a limit:
    /// it would poison every transform it reached.
    #[test]
    fn line_stroke_defaults_and_clamps_to_its_limits() {
        let overlay: OverlayConfig =
            serde_json::from_str(r#"{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"}}"#)
                .expect("legacy config");
        assert_eq!(overlay.line_stroke_px, DEFAULT_LINE_STROKE_PX);

        let mut config = Config {
            overlays: vec![
                OverlayConfig {
                    line_stroke_px: 99.0,
                    ..OverlayConfig::new()
                },
                OverlayConfig {
                    line_stroke_px: 0.0,
                    ..OverlayConfig::new()
                },
                OverlayConfig {
                    line_stroke_px: f32::INFINITY,
                    ..OverlayConfig::new()
                },
            ],
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.overlays[0].line_stroke_px, MAX_LINE_STROKE_PX);
        assert_eq!(config.overlays[1].line_stroke_px, MIN_LINE_STROKE_PX);
        assert_eq!(config.overlays[2].line_stroke_px, DEFAULT_LINE_STROKE_PX);
    }

    #[test]
    fn overlay_config_without_smooth_fields_gets_defaults() {
        let overlay: OverlayConfig =
            serde_json::from_str(r#"{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"}}"#)
                .expect("legacy config");
        assert!(overlay.smooth_rendering);
        assert_eq!(overlay.smooth_fps, DEFAULT_SMOOTH_FPS);
        assert!(overlay.cosmetic_startup_prefill);
        assert_eq!(overlay.prefill_line_color, DEFAULT_PREFILL_LINE_COLOR);
        assert_eq!(overlay.prefill_animation_sec, DEFAULT_PREFILL_ANIMATION_SEC);
        assert_eq!(overlay.startup_border_effect, BorderEffect::RgbLoop);
        assert_eq!(overlay.border_animation_sec, DEFAULT_BORDER_ANIMATION_SEC);
        assert_eq!(overlay.border_fade_sec, DEFAULT_BORDER_FADE_SEC);
    }

    #[test]
    fn explicit_startup_prefill_disabled_is_preserved() {
        let overlay: OverlayConfig = serde_json::from_str(
            r#"{"id":"configured","probe":{"protocol":"icmp","host":"1.1.1.1"},"cosmeticStartupPrefill":false}"#,
        )
        .expect("configured overlay");
        assert!(!overlay.cosmetic_startup_prefill);
    }

    #[test]
    fn explicit_startup_border_effect_is_preserved() {
        let overlay: OverlayConfig = serde_json::from_str(
            r#"{"id":"configured","probe":{"protocol":"icmp","host":"1.1.1.1"},"startupBorderEffect":"disabled"}"#,
        )
        .expect("configured overlay");
        assert_eq!(overlay.startup_border_effect, BorderEffect::Disabled);
    }

    #[test]
    fn explicit_startup_rgb_noise_is_preserved() {
        let overlay: OverlayConfig = serde_json::from_str(
            r#"{"id":"configured","probe":{"protocol":"icmp","host":"1.1.1.1"},"startupBorderEffect":"rgbNoise"}"#,
        )
        .expect("configured overlay");
        assert_eq!(overlay.startup_border_effect, BorderEffect::RgbNoise);
    }

    #[test]
    fn legacy_smooth_delay_is_migrated_to_fps() {
        let mut config: Config = serde_json::from_str(
            r#"{"overlays":[{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"},"smoothDelayMs":8}]}"#,
        )
        .expect("legacy config");
        config.normalize();
        assert_eq!(config.overlays[0].smooth_fps, 125);
        let serialized = serde_json::to_value(&config).expect("serialized config");
        assert_eq!(serialized["overlays"][0]["smoothFps"], 125);
        assert!(serialized["overlays"][0].get("smoothDelayMs").is_none());
    }

    #[test]
    fn legacy_margin_is_mapped_relative_to_each_anchor() {
        let cases = [
            ("topLeft", 20, 20),
            ("topCenter", 0, 20),
            ("topRight", 20, 20),
            ("centerLeft", 20, 0),
            ("center", 0, 0),
            ("centerRight", 20, 0),
            ("bottomLeft", 20, 20),
            ("bottomCenter", 0, 20),
            ("bottomRight", 20, 20),
        ];
        for (position, expected_horizontal, expected_vertical) in cases {
            let raw = format!(
                r#"{{"id":"legacy","position":"{position}","probe":{{"protocol":"icmp","host":"1.1.1.1"}},"marginPx":20}}"#
            );
            let overlay: OverlayConfig = serde_json::from_str(&raw).expect("legacy config");
            let mut config = Config {
                overlays: vec![overlay],
                ..Config::default()
            };
            config.normalize();
            let overlay = &config.overlays[0];
            assert_eq!(
                overlay.horizontal_margin_px, expected_horizontal,
                "{position}"
            );
            assert_eq!(overlay.vertical_margin_px, expected_vertical, "{position}");
        }
    }

    #[test]
    fn legacy_margin_field_is_not_serialized_after_migration() {
        let mut overlay = OverlayConfig::new();
        overlay.position = Anchor::TopCenter;
        overlay.legacy_margin_px = Some(20);
        let mut config = Config {
            overlays: vec![overlay],
            ..Config::default()
        };
        config.normalize();
        let value = serde_json::to_value(&config.overlays[0]).expect("serialized config");
        assert!(value.get("marginPx").is_none());
        assert_eq!(value["horizontalMarginPx"], 0);
        assert_eq!(value["verticalMarginPx"], 20);
    }

    #[test]
    fn normalize_preserves_scale_above_slider_max() {
        let mut config = Config {
            overlays: vec![OverlayConfig {
                scale: 25,
                ..OverlayConfig::new()
            }],
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.overlays[0].scale, 25);
    }

    #[test]
    fn migration_moves_only_config_and_removes_legacy_directory() {
        let root = TestDir::new("only-config");
        let legacy = root.path().join("legacy");
        let new = root.path().join("new");
        let legacy_path = write_config(&legacy, VALID_CONFIG);

        assert_eq!(
            migrate_legacy_config(&new, &legacy).expect("migrate config"),
            Some(MigrationOutcome::LegacyDirectoryRemoved)
        );
        assert_eq!(
            fs::read_to_string(new.join(CONFIG_FILE)).expect("read migrated config"),
            VALID_CONFIG
        );
        assert!(!legacy_path.exists());
        assert!(!legacy.exists());
        assert_eq!(
            migrate_legacy_config(&new, &legacy).expect("repeat migration"),
            None
        );
    }

    #[test]
    fn migration_retains_legacy_directory_with_extra_files() {
        let root = TestDir::new("extra-files");
        let legacy = root.path().join("legacy");
        let new = root.path().join("new");
        fs::create_dir_all(&new).expect("create existing new directory");
        let legacy_path = write_config(&legacy, VALID_CONFIG);
        let extra = legacy.join("keep-me.txt");
        fs::write(&extra, "user data").expect("write extra file");

        assert_eq!(
            migrate_legacy_config(&new, &legacy).expect("migrate config"),
            Some(MigrationOutcome::LegacyDirectoryRetained)
        );
        assert!(new.join(CONFIG_FILE).is_file());
        assert!(!legacy_path.exists());
        assert_eq!(
            fs::read_to_string(extra).expect("read retained file"),
            "user data"
        );
    }

    #[test]
    fn migration_never_overwrites_an_existing_new_config() {
        let root = TestDir::new("existing-new");
        let legacy = root.path().join("legacy");
        let new = root.path().join("new");
        let legacy_path = write_config(&legacy, VALID_CONFIG);
        let new_path = write_config(&new, "{\"overlays\":[]}");

        assert_eq!(
            migrate_legacy_config(&new, &legacy).expect("check migration"),
            None
        );
        assert_eq!(
            fs::read_to_string(new_path).expect("read new config"),
            "{\"overlays\":[]}"
        );
        assert!(legacy_path.is_file());
    }

    #[test]
    fn migration_ignores_legacy_directory_without_config() {
        let root = TestDir::new("missing-config");
        let legacy = root.path().join("legacy");
        let new = root.path().join("new");
        fs::create_dir_all(&legacy).expect("create legacy directory");
        fs::write(legacy.join("keep-me.txt"), "user data").expect("write extra file");

        assert_eq!(
            migrate_legacy_config(&new, &legacy).expect("check migration"),
            None
        );
        assert!(legacy.join("keep-me.txt").is_file());
        assert!(!new.exists());
    }

    #[test]
    fn invalid_legacy_config_is_not_deleted() {
        let root = TestDir::new("invalid-config");
        let legacy = root.path().join("legacy");
        let new = root.path().join("new");
        let legacy_path = write_config(&legacy, "{not valid json");

        assert!(migrate_legacy_config(&new, &legacy).is_err());
        assert!(legacy_path.is_file());
        assert!(!new.join(CONFIG_FILE).exists());
        assert_eq!(fs::read_dir(&new).expect("read new directory").count(), 0);
    }

    /// A blank monitor name is not a monitor, and must not be one.
    ///
    /// `monitor_device` names a display to pin to, and a name that matches no
    /// attached display hides the overlay. A hand-edited `"monitorDevice": ""`
    /// or a stray space would therefore hide a graph with nothing on screen to
    /// say which setting did it, and it would keep doing so on every load
    /// because the empty name survives a round trip. `normalize` is the only
    /// place that can turn it back into "follow the primary", so it is the
    /// place this has to be enforced.
    #[test]
    fn normalize_turns_a_blank_monitor_name_back_into_following_the_primary() {
        for blank in ["", "   ", "\t"] {
            let mut config = Config {
                overlays: vec![OverlayConfig {
                    monitor_device: Some(blank.to_string()),
                    ..OverlayConfig::new()
                }],
                ..Config::default()
            };
            config.normalize();
            assert_eq!(
                config.overlays[0].monitor_device, None,
                "{blank:?} survived as a display name"
            );
        }
    }

    /// A real display name is left alone, including its case and its spelling.
    ///
    /// The counterpart to the test above: a "be tidy with whitespace" fix that
    /// also rewrote a genuine name would silently move a user's overlay.
    #[test]
    fn normalize_keeps_a_real_monitor_name() {
        let mut config = Config {
            overlays: vec![OverlayConfig {
                monitor_device: Some("\\\\.\\DISPLAY2".to_string()),
                ..OverlayConfig::new()
            }],
            ..Config::default()
        };
        config.normalize();
        assert_eq!(
            config.overlays[0].monitor_device.as_deref(),
            Some("\\\\.\\DISPLAY2")
        );
    }

    /// An overlay written before monitors were selectable has no field at all.
    ///
    /// `None` is the default rather than a required field, so a profile file
    /// from any earlier version loads, and "no field" and "an explicit null" are
    /// the same thing to the renderer.
    #[test]
    fn a_profile_without_a_monitor_field_loads_and_follows_the_primary() {
        let json = r#"{
            "profileName": "Desk",
            "overlays": [
                {
                    "id": "one",
                    "name": "Gateway",
                    "host": "192.168.1.1",
                    "probe": {"protocol": "icmp", "host": "192.168.1.1"}
                }
            ]
        }"#;
        let config: Config = serde_json::from_str(json).expect("a profile without a monitor field");
        assert_eq!(config.overlays[0].monitor_device, None);
        assert_eq!(OverlayConfig::new().monitor_device, None);
    }

    /// Display Mode says where an overlay is placed, and only one way at a time.
    ///
    /// It used to be a wallpaper boolean; an enum makes "wallpaper and sticky"
    /// unrepresentable rather than a combination someone has to remember to
    /// refuse.
    #[test]
    fn display_mode_defaults_to_global_and_round_trips() {
        assert_eq!(OverlayConfig::new().display_mode, DisplayMode::Global);

        for (mode, json_name) in [
            (DisplayMode::Global, "\"displayMode\":\"global\""),
            (DisplayMode::Sticky, "\"displayMode\":\"sticky\""),
            (DisplayMode::Wallpaper, "\"displayMode\":\"wallpaper\""),
        ] {
            let mut config = OverlayConfig::new();
            config.display_mode = mode;
            let json = serde_json::to_string(&config).expect("serialize");
            assert!(
                json.contains(json_name),
                "the mode did not reach the file: {json}"
            );
            let parsed: OverlayConfig = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed.display_mode, mode, "the mode did not survive a save");
        }

        // A profile written before the field existed must load on top of
        // everything, not fail on a missing member.
        let json = serde_json::to_string(&OverlayConfig::new()).expect("serialize");
        let without = json.replace("\"displayMode\":\"global\",", "");
        let parsed: OverlayConfig =
            serde_json::from_str(&without).expect("a profile without the field");
        assert_eq!(parsed.display_mode, DisplayMode::Global);
    }

    /// The old `wallpaperMode` key still means what it meant, and the new key
    /// wins when both are present.
    ///
    /// The translation is one-way: the legacy field is skipped when writing, so
    /// a save cannot produce a file that says both things.
    #[test]
    fn a_legacy_wallpaper_key_still_turns_wallpaper_mode_on() {
        let legacy = |wallpaper: bool| {
            let mut value = serde_json::to_value(OverlayConfig::new()).expect("serialize");
            value["wallpaperMode"] = serde_json::json!(wallpaper);
            serde_json::from_value::<OverlayConfig>(value).expect("the legacy key")
        };

        let mut on = Config {
            overlays: vec![legacy(true)],
            ..Config::default()
        };
        on.normalize();
        assert_eq!(
            on.overlays[0].display_mode,
            DisplayMode::Wallpaper,
            "wallpaperMode:true must still mean wallpaper"
        );

        let mut off = Config {
            overlays: vec![legacy(false)],
            ..Config::default()
        };
        off.normalize();
        assert_eq!(off.overlays[0].display_mode, DisplayMode::Global);

        // An explicit mode is not overridden by the stale key.
        let mut value = serde_json::to_value(OverlayConfig::new()).expect("serialize");
        value["displayMode"] = serde_json::json!("sticky");
        value["wallpaperMode"] = serde_json::json!(true);
        let mut explicit = Config {
            overlays: vec![serde_json::from_value(value).expect("both keys")],
            ..Config::default()
        };
        explicit.normalize();
        assert_eq!(explicit.overlays[0].display_mode, DisplayMode::Sticky);

        // And a save never writes the legacy key back.
        let mut mode = OverlayConfig::new();
        mode.display_mode = DisplayMode::Wallpaper;
        let json = serde_json::to_string(&mode).expect("serialize");
        assert!(
            !json.contains("wallpaperMode"),
            "the legacy key leaked into a save: {json}"
        );
    }

    /// A sticky target survives a save, and a blank box does not become a
    /// condition that matches everything.
    #[test]
    fn sticky_targets_round_trip_and_drop_blank_boxes() {
        let condition = |part, matcher, value: &str| Condition {
            part,
            matcher,
            value: value.to_string(),
        };
        let mut config = Config {
            overlays: vec![OverlayConfig {
                display_mode: DisplayMode::Sticky,
                sticky_target: Some(StickyTarget {
                    when: vec![
                        condition(Part::ProcessName, MatchMode::Exact, "chrome.exe"),
                        condition(Part::Title, MatchMode::Contains, "   "),
                        condition(Part::ClassName, MatchMode::Exact, "Chrome_WidgetWin_1"),
                    ],
                }),
                ..OverlayConfig::new()
            }],
            ..Config::default()
        };

        config.normalize();
        let target = config.overlays[0]
            .sticky_target
            .as_ref()
            .expect("a target survived normalization");
        assert_eq!(
            target.when.len(),
            2,
            "a blank value became a condition that matches everything"
        );
        assert_eq!(target.when[0].part, Part::ProcessName);
        assert_eq!(target.when[1].part, Part::ClassName);

        let json = serde_json::to_string(&config.overlays[0]).expect("serialize");
        assert!(
            json.contains("\"stickyTarget\""),
            "the target did not reach the file: {json}"
        );
        let parsed: OverlayConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.sticky_target, config.overlays[0].sticky_target);
    }

    #[test]
    fn normalize_clamps_invalid_values() {
        let mut config = Config {
            overlays: vec![OverlayConfig {
                window_seconds: 1,
                scale: MAX_SCALE_INPUT + 1,
                smooth_fps: 0,
                prefill_animation_sec: 0,
                border_animation_sec: 0,
                border_fade_sec: MAX_BORDER_FADE_SEC + 1,
                graph_height_px: 1,
                max_y_ms: 0,
                orientation: 45,
                horizontal_margin_px: MAX_MARGIN_OFFSET_PX + 1,
                vertical_margin_px: MIN_MARGIN_OFFSET_PX - 1,
                bg_opacity: 200,
                ..OverlayConfig::new()
            }],
            ..Config::default()
        };
        config.normalize();
        let overlay = &config.overlays[0];
        assert_eq!(overlay.window_seconds, MIN_WINDOW_SECONDS);
        assert_eq!(overlay.scale, MAX_SCALE_INPUT);
        assert_eq!(overlay.smooth_fps, MIN_SMOOTH_FPS);
        assert_eq!(overlay.prefill_animation_sec, MIN_PREFILL_ANIMATION_SEC);
        assert_eq!(overlay.border_animation_sec, MIN_BORDER_ANIMATION_SEC);
        assert_eq!(overlay.border_fade_sec, MAX_BORDER_FADE_SEC);
        assert_eq!(overlay.first_target().timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(overlay.graph_height_px, MIN_GRAPH_HEIGHT_PX);
        assert_eq!(overlay.max_y_ms, DEFAULT_MAX_Y_MS);
        assert_eq!(overlay.orientation, 0);
        assert_eq!(overlay.horizontal_margin_px, MAX_MARGIN_OFFSET_PX);
        assert_eq!(overlay.vertical_margin_px, MIN_MARGIN_OFFSET_PX);
        assert_eq!(overlay.bg_opacity, 100);
    }

    /// The migration that makes every existing overlay a group of one.
    ///
    /// Without this a profile written before targets existed would load as an
    /// overlay with nothing to probe, which is the "silently draws nothing"
    /// shape: no error, no warning, just an empty window.
    #[test]
    fn a_pre_targets_profile_becomes_one_target() {
        let mut config: Config = serde_json::from_str(
            r##"{"overlays":[{"id":"legacy","name":"Work","probe":{"protocol":"tcp","host":"10.0.0.1","port":443},"lineColor":"#123456","timeoutColor":"#654321","timeoutMs":2500,"maxYMs":500}]}"##,
        )
        .expect("legacy config");

        config.normalize();

        let overlay = &config.overlays[0];
        assert_eq!(overlay.targets.len(), 1);
        let target = overlay.first_target();
        assert_eq!(target.probe.host(), "10.0.0.1");
        assert_eq!(target.probe.port(), 443);
        assert_eq!(target.timeout_ms, 2500);
        assert_eq!(target.line_color, "#123456");
        assert_eq!(target.timeout_color, "#654321");
        assert!(target.enabled);
        assert!(!target.id.trim().is_empty());
        // The group-level settings are untouched by the migration, which is the
        // whole point: the window keeps the size and place the user chose.
        assert_eq!(overlay.max_y_ms, 500);
        assert_eq!(overlay.name, "Work");
    }

    /// A migrated profile keeps the keys an older build understands.
    ///
    /// The migration is only safe if it can be undone by the version that wrote
    /// the file, so the legacy keys are dropped on write and the targets
    /// written in their place. Anything else and downgrading would lose the
    /// host.
    ///
    /// Checked by looking at the overlay object's own keys rather than by
    /// searching the text: `probe` and `timeoutMs` legitimately appear inside
    /// every target, so a substring search finds them and reports a pass that
    /// is not one.
    #[test]
    fn a_migrated_profile_no_longer_writes_the_legacy_keys() {
        let mut config: Config = serde_json::from_str(
            r#"{"overlays":[{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"},"timeoutMs":2500}]}"#,
        )
        .expect("legacy config");
        config.normalize();

        let written = serde_json::to_value(&config).expect("serialize");
        let overlay = &written["overlays"][0];

        assert!(overlay.get("probe").is_none(), "{overlay}");
        assert!(overlay.get("timeoutMs").is_none(), "{overlay}");
        assert!(overlay.get("lineColor").is_none(), "{overlay}");
        assert!(overlay.get("timeoutColor").is_none(), "{overlay}");
        assert!(
            overlay.get("targets").is_some(),
            "the targets were not written at all: {overlay}"
        );
    }

    /// A profile that already has targets must not gain one from its own
    /// legacy keys, or reloading it would grow a host every launch.
    #[test]
    fn a_profile_that_already_has_targets_gains_none() {
        let mut config: Config = serde_json::from_str(
            r#"{"overlays":[{"id":"grouped","probe":{"protocol":"icmp","host":"9.9.9.9"},"targets":[{"id":"a","probe":{"protocol":"icmp","host":"1.1.1.1"}},{"id":"b","probe":{"protocol":"icmp","host":"8.8.8.8"}}]}]}"#,
        )
        .expect("config");

        config.normalize();

        let targets = &config.overlays[0].targets;
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].probe.host(), "1.1.1.1");
        assert_eq!(targets[1].probe.host(), "8.8.8.8");
    }

    /// An overlay with no hosts at all is refused, not given one.
    ///
    /// The tempting repair is to hand it `1.1.1.1`, the same starting host a new
    /// overlay gets. That would put a graph on somebody's screen that they never
    /// asked to ping, with no error anywhere — a confident wrong answer. So the
    /// file is rejected and kept, and the user is told.
    #[test]
    fn an_overlay_with_no_hosts_is_refused_rather_than_given_one() {
        let mut config: Config =
            serde_json::from_str(r#"{"overlays":[{"id":"empty"}]}"#).expect("config");
        config.normalize();

        assert!(
            config.overlays[0].targets.is_empty(),
            "a host was invented for an overlay that has none"
        );
        let error = validate(&config).expect_err("an overlay with no host must not load");
        assert!(error.contains("no host to probe"), "{error}");
    }

    /// A zero timeout on any host is repaired, not just the first one.
    #[test]
    fn a_zero_timeout_is_repaired_on_every_host() {
        let mut overlay = OverlayConfig::new();
        let mut second = TargetConfig::new();
        second.timeout_ms = 0;
        overlay.targets.push(second);
        let mut config = Config {
            overlays: vec![overlay],
            ..Config::default()
        };

        config.normalize();

        assert!(config.overlays[0]
            .targets
            .iter()
            .all(|target| target.timeout_ms == DEFAULT_TIMEOUT_MS));
    }

    /// Validation is per host, and it says which one. A group of four is four
    /// chances to have a blank host and "overlay X" does not narrow that down.
    #[test]
    fn validation_reports_the_host_that_is_broken() {
        let mut overlay = OverlayConfig::new();
        let mut second = TargetConfig::new();
        second.probe = ProbeConfig::Icmp {
            host: "   ".to_string(),
        };
        overlay.targets.push(second);
        let config = Config {
            overlays: vec![overlay],
            ..Config::default()
        };

        let error = validate(&config).expect_err("a blank host must not load");

        assert!(
            error.contains("host 2"),
            "the message does not say which host: {error}"
        );
        assert!(error.contains("empty probe host"), "{error}");
    }

    /// Two targets sharing an id are two buffers under one name, and the
    /// second silently overwrites the first's samples forever.
    #[test]
    fn validation_rejects_a_repeated_target_id() {
        let mut overlay = OverlayConfig::new();
        let mut second = TargetConfig::new();
        second.id = overlay.first_target().id.clone();
        overlay.targets.push(second);
        let config = Config {
            overlays: vec![overlay],
            ..Config::default()
        };

        let error = validate(&config).expect_err("a repeated id must not load");
        assert!(error.contains("more than once"), "{error}");
    }

    /// A TCP target with no port would otherwise be probed against port zero,
    /// which is not a thing you can connect to and fails forever quietly.
    #[test]
    fn validation_rejects_a_tcp_host_with_no_port() {
        let mut overlay = OverlayConfig::new();
        overlay.first_target_mut().probe = ProbeConfig::Tcp {
            host: "example.com".to_string(),
            port: 0,
        };
        let config = Config {
            overlays: vec![overlay],
            ..Config::default()
        };

        let error = validate(&config).expect_err("port zero must not load");
        assert!(error.contains("TCP port"), "{error}");
    }

    /// A host added next to an existing one gets its own colour, because two
    /// lines in the same colour are one line as far as the reader is concerned.
    #[test]
    fn an_added_host_does_not_inherit_its_neighbours_colour() {
        let mut overlay = OverlayConfig::new();
        overlay.first_target_mut().line_color = "#4ade80".to_string();

        let added = overlay.add_target().clone();

        assert_ne!(added.line_color, "#4ade80");
        assert!(is_hex_color(&added.line_color), "{}", added.line_color);
        assert_ne!(added.id, overlay.first_target().id);
    }

    /// Ids come from a clock and a counter, so two hosts added in the same
    /// millisecond must not collide.
    #[test]
    fn every_host_gets_its_own_id() {
        let mut overlay = OverlayConfig::new();
        for _ in 0..8 {
            overlay.add_target();
        }
        let mut ids: Vec<&str> = overlay
            .targets
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two hosts shared an id");
    }

    fn is_hex_color(value: &str) -> bool {
        value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit())
    }

    fn store_at(root: &Path) -> Store {
        Store::new(root.to_path_buf())
    }

    fn write_profile(store: &Store, name: &str, contents: &str) -> PathBuf {
        fs::create_dir_all(store.profiles_dir()).expect("create profiles directory");
        let path = store
            .profiles_dir()
            .join(format!("{PROFILE_PREFIX}{name}{PROFILE_EXTENSION}"));
        fs::write(&path, contents).expect("write profile");
        path
    }

    fn one_overlay_config(id: &str) -> Config {
        Config {
            overlays: vec![OverlayConfig {
                id: id.to_string(),
                ..OverlayConfig::new()
            }],
            ..Config::default()
        }
    }

    /// Expected profile list from `(id, display name)` pairs.
    fn entries(names: &[(&str, &str)]) -> Vec<ProfileEntry> {
        names
            .iter()
            .map(|(id, name)| ProfileEntry {
                id: (*id).to_string(),
                name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn profile_names_are_sanitized_into_safe_slugs() {
        assert_eq!(sanitize_profile_name("default").unwrap(), "default");
        assert_eq!(sanitize_profile_name("  Work VPN  ").unwrap(), "work-vpn");
        assert_eq!(sanitize_profile_name("My_Profile").unwrap(), "my_profile");
        assert_eq!(
            sanitize_profile_name("a  ...  b").unwrap(),
            "a-b",
            "separators collapse into one dash"
        );
        assert!(sanitize_profile_name("   ").is_err());
        assert!(sanitize_profile_name("///").is_err());
        assert!(sanitize_profile_name("..").is_err());
        assert_eq!(
            sanitize_profile_name(&"x".repeat(80)).unwrap().len(),
            MAX_PROFILE_NAME_LEN
        );
        // Slugs can never escape the profiles directory.
        let store = store_at(&std::env::temp_dir());
        assert!(store.profile_path("../escape").is_ok());
        assert_eq!(
            store.profile_path("../../escape").unwrap(),
            store
                .profiles_dir()
                .join(format!("{PROFILE_PREFIX}escape{PROFILE_EXTENSION}")),
            "path separators are collapsed, not honoured"
        );
    }

    #[test]
    fn config_json_is_validated_and_moved_into_the_profiles_directory() {
        let root = TestDir::new("import-valid");
        let store = store_at(root.path());
        write_config(root.path(), VALID_CONFIG);

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, DEFAULT_PROFILE);
        assert_eq!(loaded.profiles, entries(&[(DEFAULT_PROFILE, "Default")]));
        assert_eq!(loaded.config.overlays.len(), 1);
        assert_eq!(loaded.config.overlays[0].id, "legacy");
        assert!(loaded.notices.contains(&ConfigNotice::ProfileImported));
        assert!(
            !store.config_path().exists(),
            "the old file is removed once the profile exists"
        );
        assert!(store
            .profile_path(DEFAULT_PROFILE)
            .expect("default path")
            .is_file());
    }

    #[test]
    fn invalid_config_json_is_kept_and_reported() {
        for (label, contents) in [
            ("not-json", "{not valid json"),
            ("root-array", "[]"),
            ("overlays-object", r#"{"overlays":{}}"#),
            (
                "empty-id",
                r#"{"overlays":[{"id":"","probe":{"protocol":"icmp","host":"1.1.1.1"}}]}"#,
            ),
            (
                "duplicate-id",
                r#"{"overlays":[{"id":"same","probe":{"protocol":"icmp","host":"1.1.1.1"}},{"id":"same","probe":{"protocol":"icmp","host":"1.1.1.1"}}]}"#,
            ),
            ("no-probe", r#"{"overlays":[{"id":"a"}]}"#),
            (
                "empty-host",
                r#"{"overlays":[{"id":"a","probe":{"protocol":"icmp","host":" "}}]}"#,
            ),
            (
                "missing-port",
                r#"{"overlays":[{"id":"a","probe":{"protocol":"tcp","host":"1.1.1.1","port":0}}]}"#,
            ),
        ] {
            let root = TestDir::new(label);
            let store = store_at(root.path());
            write_config(root.path(), contents);

            let loaded = store.load(&root.path().join("missing-legacy"));

            assert!(
                store.config_path().is_file(),
                "{label}: config.json must be preserved"
            );
            assert!(
                loaded
                    .notices
                    .iter()
                    .any(|notice| matches!(notice, ConfigNotice::ProfileImportFailed(_))),
                "{label}: the failure must be reported"
            );
            assert!(
                loaded.config.overlays.is_empty(),
                "{label}: an unusable file must not be loaded"
            );
        }
    }

    #[test]
    fn an_existing_default_profile_keeps_the_old_config_file() {
        let root = TestDir::new("import-skip");
        let store = store_at(root.path());
        write_config(root.path(), VALID_CONFIG);
        write_profile(&store, DEFAULT_PROFILE, r#"{"overlays":[]}"#);

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert!(store.config_path().is_file());
        assert!(loaded.notices.contains(&ConfigNotice::ProfileImportSkipped));
        assert_eq!(loaded.config.overlays.len(), 0);
    }

    #[test]
    fn a_fresh_install_creates_an_empty_default_profile_and_global_config() {
        let root = TestDir::new("fresh");
        let store = store_at(root.path());

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, DEFAULT_PROFILE);
        assert_eq!(loaded.profiles, entries(&[(DEFAULT_PROFILE, "Default")]));
        assert!(loaded.config.overlays.is_empty());
        assert!(loaded.notices.is_empty());
        assert_eq!(
            fs::read_to_string(store.global_config_path()).expect("read global config"),
            "{}",
            "global preferences stay empty until something is stored"
        );
    }

    #[test]
    fn global_prefs_default_when_absent() {
        let root = TestDir::new("prefs-absent");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        assert_eq!(store.read_global_prefs(), GlobalPrefs::default());
        let loaded = store.load(&root.path().join("missing-legacy"));
        assert_eq!(loaded.prefs, GlobalPrefs::default());
    }

    #[test]
    fn the_title_version_preference_defaults_to_off_and_round_trips() {
        let root = TestDir::new("prefs-title-version");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        assert!(
            !store.read_global_prefs().ui.show_version_in_title,
            "the window title is already long, so the version is opt-in"
        );

        let prefs = GlobalPrefs {
            ui: UiPrefs {
                show_version_in_title: true,
                ..UiPrefs::default()
            },
        };
        store.write_global_prefs(&prefs).expect("write prefs");
        assert!(
            store.read_global_prefs().ui.show_version_in_title,
            "the preference did not survive a round trip"
        );

        // A file written before the preference existed must not fail to load.
        let raw = store.global_config_path().to_string_lossy().to_string();
        std::fs::write(&raw, "{\"ui\":{\"railCollapsed\":true}}").expect("write a partial ui key");
        assert!(
            !store.read_global_prefs().ui.show_version_in_title,
            "a ui key without the preference must fall back to the default"
        );
    }

    #[test]
    fn the_selection_border_animation_defaults_on_and_round_trips() {
        let root = TestDir::new("prefs-selection-border");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        assert!(
            store.read_global_prefs().ui.selection_border_animation,
            "the border preview is on unless it is turned off"
        );

        let prefs = GlobalPrefs {
            ui: UiPrefs {
                selection_border_animation: false,
                ..UiPrefs::default()
            },
        };
        store.write_global_prefs(&prefs).expect("write prefs");
        assert!(
            !store.read_global_prefs().ui.selection_border_animation,
            "the preference did not survive a round trip"
        );

        // A file written before the preference existed must not fail to load,
        // and must not silently turn the animation off.
        let raw = store.global_config_path().to_string_lossy().to_string();
        std::fs::write(&raw, "{\"ui\":{\"railCollapsed\":true}}").expect("write a partial ui key");
        assert!(
            store.read_global_prefs().ui.selection_border_animation,
            "a ui key without the preference must fall back to the default"
        );
    }

    #[test]
    fn the_background_tracking_preference_defaults_on_and_round_trips() {
        let root = TestDir::new("prefs-background-tracking");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        assert!(
            store.read_global_prefs().ui.background_tracking,
            "background profiles are tracked unless the setting is turned off"
        );

        let prefs = GlobalPrefs {
            ui: UiPrefs {
                background_tracking: false,
                ..UiPrefs::default()
            },
        };
        store.write_global_prefs(&prefs).expect("write prefs");
        assert!(
            !store.read_global_prefs().ui.background_tracking,
            "the preference did not survive a round trip"
        );

        // A file written before the preference existed must not fail to load,
        // and must not silently stop probing profiles the user switched away
        // from.
        let raw = store.global_config_path().to_string_lossy().to_string();
        std::fs::write(&raw, "{\"ui\":{\"railCollapsed\":true}}").expect("write a partial ui key");
        assert!(
            store.read_global_prefs().ui.background_tracking,
            "a ui key without the preference must fall back to the default"
        );
    }

    #[test]
    fn global_prefs_round_trip_through_global_config() {
        let root = TestDir::new("prefs-round-trip");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Work VPN").expect("create profile");
        store
            .set_active_profile("work-vpn")
            .expect("set active profile");

        let prefs = GlobalPrefs {
            ui: UiPrefs {
                rail_collapsed: true,
                ..UiPrefs::default()
            },
        };
        store.write_global_prefs(&prefs).expect("write prefs");

        let loaded = store.load(&root.path().join("missing-legacy"));
        assert!(loaded.prefs.ui.rail_collapsed);
        // The preferences live beside the active profile pointer, so writing
        // them must leave that pointer alone.
        assert_eq!(loaded.active_profile, "work-vpn");
    }

    #[test]
    fn unknown_global_keys_survive_a_prefs_update() {
        let root = TestDir::new("prefs-unknown-keys");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        fs::write(
            store.global_config_path(),
            r#"{"futurePreference":true,"activeProfile":"work"}"#,
        )
        .expect("seed global config");

        store
            .write_global_prefs(&GlobalPrefs {
                ui: UiPrefs {
                    rail_collapsed: true,
                    ..UiPrefs::default()
                },
            })
            .expect("write prefs");

        let raw = fs::read_to_string(store.global_config_path()).expect("read global config");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parse global config");
        assert_eq!(value["futurePreference"], true);
        assert_eq!(value["activeProfile"], "work");
        assert_eq!(value[UI_PREFS_KEY]["railCollapsed"], true);
    }

    #[test]
    fn a_missing_rules_file_loads_as_inert_defaults() {
        let root = TestDir::new("rules-missing");
        let store = store_at(root.path());

        assert_eq!(store.rules_path(), root.path().join(RULES_FILE));
        let rules = store
            .load_rules()
            .expect("a missing rules file is not an error");
        assert_eq!(rules, AutoRules::default());
        assert!(
            !rules.enabled,
            "switching must be off until it is turned on"
        );
        assert!(rules.rules.is_empty());
    }

    #[test]
    fn rules_round_trip_through_an_atomic_write() {
        let root = TestDir::new("rules-round-trip");
        let store = store_at(root.path());
        let rules = AutoRules {
            enabled: true,
            fallback_profile: Some("default".to_string()),
            rules: vec![crate::rules::Rule {
                name: "CS2".to_string(),
                scope: crate::rules::Scope::AnyWindow,
                combine: crate::rules::Combine::All,
                when: vec![crate::rules::Condition {
                    part: crate::rules::Part::ProcessName,
                    matcher: crate::rules::MatchMode::Exact,
                    value: "cs2.exe".to_string(),
                }],
                profile: "gaming".to_string(),
            }],
        };
        store.save_rules(&rules).expect("save rules");

        assert_eq!(store.load_rules().expect("load rules"), rules);
        // The temporary name `write_atomic` uses must not survive the write:
        // the tray watches this file's modification time, and a leftover
        // partial file next to it is exactly the state that watching is
        // supposed to be safe against.
        let leftovers: Vec<String> = fs::read_dir(root.path())
            .expect("read root")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "left over: {leftovers:?}");
    }

    #[test]
    fn a_broken_rules_file_is_an_error_and_is_left_alone() {
        let root = TestDir::new("rules-broken");
        let store = store_at(root.path());
        store.save_rules(&AutoRules::default()).expect("seed rules");
        let path = store.rules_path();
        fs::write(&path, "{ this is not json").expect("break the file");

        let error = store
            .load_rules()
            .expect_err("unparseable rules must error");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "{ this is not json",
            "loading a broken rules file modified it"
        );
    }

    #[test]
    fn the_active_profile_is_remembered_across_loads() {
        let root = TestDir::new("active");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Work VPN").expect("create profile");
        store
            .set_active_profile("work-vpn")
            .expect("set active profile");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, "work-vpn");
        assert_eq!(
            loaded.profiles,
            entries(&[(DEFAULT_PROFILE, "Default"), ("work-vpn", "Work VPN")])
        );
        assert!(loaded.notices.is_empty());
        let global: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(store.global_config_path()).expect("read global config"),
        )
        .expect("parse global config");
        assert_eq!(global["activeProfile"], "work-vpn");
        assert_eq!(
            global[ACTIVE_PROFILE_FILE_KEY], "profile_work-vpn.json",
            "the file name is stored next to the id"
        );
    }

    #[test]
    fn the_stored_file_name_finds_the_profile_without_an_id() {
        let root = TestDir::new("active-file");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        let mut config = one_overlay_config("work");
        config.profile_name = "Work".to_string();
        store.save_profile("work", &config).expect("write profile");
        fs::write(
            store.global_config_path(),
            format!(r#"{{"{ACTIVE_PROFILE_FILE_KEY}":"profile_work.json"}}"#),
        )
        .expect("write global config");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, "work");
        assert_eq!(loaded.config.overlays.len(), 1);
        assert!(loaded.notices.is_empty());
        let global: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(store.global_config_path()).expect("read global config"),
        )
        .expect("parse global config");
        assert_eq!(global[ACTIVE_PROFILE_FILE_KEY], "profile_work.json");
    }

    #[test]
    fn a_postfixed_profile_id_round_trips_through_the_stored_file_name() {
        let root = TestDir::new("active-postfix");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Home").expect("create profile");
        let postfixed = store.create_profile("Home").expect("create duplicate");
        assert_eq!(postfixed.id, "home_2");
        let mut config = one_overlay_config("home");
        config.profile_name = "Home".to_string();
        store
            .save_profile(&postfixed.id, &config)
            .expect("write duplicate");
        store
            .set_active_profile(&postfixed.id)
            .expect("set active profile");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, "home_2");
        assert_eq!(loaded.config.overlays.len(), 1);
        assert_eq!(
            loaded.config.profile_name, "Home",
            "the duplicate keeps the same display name"
        );
        assert!(loaded.notices.is_empty());
    }

    #[test]
    fn a_deleted_stored_profile_falls_back_to_the_default_not_to_another_one() {
        let root = TestDir::new("active-deleted");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        write_profile(&store, "work", VALID_CONFIG);
        write_profile(&store, "other", VALID_CONFIG);
        write_profile(&store, DEFAULT_PROFILE, r#"{"overlays":[]}"#);
        store
            .set_active_profile("work")
            .expect("set active profile");
        store.delete_profile("work").expect("delete profile");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(
            loaded.active_profile, DEFAULT_PROFILE,
            "the stored key is the only source, so no other profile is picked"
        );
        assert!(loaded.config.overlays.is_empty());
        assert_eq!(
            fs::read_to_string(store.global_config_path()).expect("read global config"),
            "{}",
            "the stale pointer is cleared"
        );
    }

    #[test]
    fn a_stored_file_name_that_is_not_a_profile_file_falls_back() {
        let root = TestDir::new("active-bad-file");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        write_profile(&store, DEFAULT_PROFILE, VALID_CONFIG);
        fs::write(
            store.global_config_path(),
            r#"{"activeProfileFile":"../escape.json"}"#,
        )
        .expect("write global config");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, DEFAULT_PROFILE);
        assert!(loaded.notices.iter().any(
            |notice| matches!(notice, ConfigNotice::ProfileFallback { profile, .. } if profile
                    == "../escape.json")
        ));
    }

    #[test]
    fn unknown_global_keys_survive_an_active_profile_update() {
        let root = TestDir::new("global-keys");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("work").expect("create profile");
        fs::write(
            store.global_config_path(),
            r#"{"futurePreference":true,"activeProfile":"work"}"#,
        )
        .expect("write global config");

        store
            .set_active_profile("default")
            .expect("set active profile");

        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(store.global_config_path()).expect("read global config"),
        )
        .expect("parse global config");
        assert_eq!(value["futurePreference"], true);
        assert!(
            value.get("activeProfile").is_none(),
            "the default profile is implied by an absent key"
        );
        assert!(
            value.get(ACTIVE_PROFILE_FILE_KEY).is_none(),
            "the file name is cleared together with the id"
        );
    }

    #[test]
    fn an_unusable_active_profile_falls_back_to_the_default() {
        let root = TestDir::new("fallback");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        write_profile(&store, DEFAULT_PROFILE, VALID_CONFIG);
        write_profile(&store, "work", r#"{"overlays":"broken"}"#);
        fs::write(store.global_config_path(), r#"{"activeProfile":"work"}"#)
            .expect("write global config");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, DEFAULT_PROFILE);
        assert_eq!(loaded.config.overlays.len(), 1);
        assert!(loaded.notices.iter().any(
            |notice| matches!(notice, ConfigNotice::ProfileFallback { profile, .. } if profile
                    == "work")
        ));
    }

    #[test]
    fn a_missing_active_profile_falls_back_and_corrects_the_stored_key() {
        let root = TestDir::new("missing-active");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        fs::write(store.global_config_path(), r#"{"activeProfile":"gone"}"#)
            .expect("write global config");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert_eq!(loaded.active_profile, DEFAULT_PROFILE);
        assert!(loaded
            .notices
            .iter()
            .any(|notice| matches!(notice, ConfigNotice::ProfileFallback { profile, .. } if profile == "gone")));
        assert_eq!(
            fs::read_to_string(store.global_config_path()).expect("read global config"),
            "{}",
            "the stale key is cleared"
        );
    }

    #[test]
    fn profiles_can_be_created_renamed_and_deleted() {
        let root = TestDir::new("crud");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        let created = store.create_profile("Work VPN").expect("create profile");
        assert_eq!(created.id, "work-vpn");
        assert_eq!(created.name, "Work VPN");
        let config = store
            .load_profile(&created.id)
            .expect("load created profile");
        assert!(config.overlays.is_empty());
        assert_eq!(config.profile_name, "Work VPN");

        store
            .save_profile("work-vpn", &one_overlay_config("kept"))
            .expect("save profile");
        let renamed = store.rename_profile("work-vpn", "Office").expect("rename");
        assert_eq!(renamed.id, "office");
        assert_eq!(renamed.name, "Office");
        let renamed_config = store.load_profile("office").expect("load renamed profile");
        assert_eq!(renamed_config.overlays[0].id, "kept");
        assert_eq!(
            renamed_config.profile_name, "Office",
            "the new name is stored in the file"
        );
        assert!(
            store.load_profile("work-vpn").is_err(),
            "the old id is gone"
        );
        assert!(store.rename_profile("missing", "any").is_err());

        store.delete_profile("office").expect("delete profile");
        assert_eq!(store.list_profiles(), vec![DEFAULT_PROFILE.to_string()]);
        assert!(store.load_profile("office").is_err());
    }

    #[test]
    fn a_profile_can_be_duplicated() {
        let root = TestDir::new("duplicate");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Home").expect("create profile");
        // `create_profile` wrote the name, but saving a config built by hand
        // replaces the whole file, so the name is set again here.
        let mut source_config = one_overlay_config("kept");
        source_config.profile_name = "Home".to_string();
        store
            .save_profile("home", &source_config)
            .expect("save profile");

        let copy = store
            .duplicate_profile("home", "Home 2")
            .expect("duplicate");
        // A space becomes a dash in the id, while the display name keeps it.
        assert_eq!(copy.id, "home-2");
        assert_eq!(copy.name, "Home 2");
        let copied = store.load_profile("home-2").expect("load copy");
        assert_eq!(copied.overlays[0].id, "kept", "overlays are copied");
        assert_eq!(copied.profile_name, "Home 2", "the copy is named");
        let source = store.load_profile("home").expect("load source");
        assert_eq!(source.profile_name, "Home", "the source keeps its own name");
        assert_eq!(source.overlays[0].id, "kept");

        // Duplicating again with the same name postfixes the id, like any other
        // collision, and leaves the source alone.
        let second = store
            .duplicate_profile("home", "Home 2")
            .expect("duplicate again");
        assert_eq!(second.id, "home-2_2");
        assert_eq!(store.list_profiles_detailed().len(), 4);
        assert!(store.duplicate_profile("missing", "Any").is_err());
    }

    #[test]
    fn duplicate_profile_names_get_a_postfix() {
        let root = TestDir::new("duplicates");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        for expected in ["home", "home_2", "home_3"] {
            let created = store.create_profile("Home").expect("create profile");
            assert_eq!(created.id, expected);
            assert_eq!(created.name, "Home");
        }

        // Display names may repeat, so only the ids differ.
        for id in ["home", "home_2", "home_3"] {
            let config = store.load_profile(id).expect("load profile");
            assert_eq!(config.profile_name, "Home");
            assert!(config.overlays.is_empty());
        }
        assert_eq!(
            store.list_profiles_detailed(),
            entries(&[
                ("default", "Default"),
                ("home", "Home"),
                ("home_2", "Home"),
                ("home_3", "Home"),
            ])
        );
    }

    #[test]
    fn renaming_onto_a_taken_name_keeps_the_existing_profile() {
        let root = TestDir::new("rename-collision");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Home").expect("create home");
        store.create_profile("Office").expect("create office");
        store
            .save_profile("office", &one_overlay_config("office-overlay"))
            .expect("save office");

        let renamed = store.rename_profile("office", "Home").expect("rename");

        assert_eq!(renamed.id, "home_2", "the taken id gets a postfix");
        assert_eq!(renamed.name, "Home");
        assert_eq!(
            store.load_profile("home_2").expect("load renamed").overlays[0].id,
            "office-overlay",
            "the overlays move with the file"
        );
        assert_eq!(
            store
                .load_profile("home")
                .expect("load untouched")
                .profile_name,
            "Home"
        );
        assert!(store.load_profile("office").is_err(), "the old id is gone");
    }

    #[test]
    fn renaming_a_profile_to_its_own_name_keeps_the_file() {
        let root = TestDir::new("rename-self");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        store.create_profile("Home Net").expect("create profile");

        for name in ["Home Net", "home net", "HOME-NET"] {
            let renamed = store.rename_profile("home-net", name).expect("rename");
            assert_eq!(
                renamed.id, "home-net",
                "the same id is not a collision with itself"
            );
        }
        assert_eq!(store.list_profiles().len(), 2, "no postfix file appears");
        assert_eq!(
            store
                .load_profile("home-net")
                .expect("load profile")
                .profile_name,
            "HOME-NET",
            "the name is free-form, only the id is normalized"
        );
    }

    #[test]
    fn postfixes_stay_inside_the_id_length_cap() {
        let long = "x".repeat(MAX_PROFILE_NAME_LEN);
        let with_postfix_value = with_postfix(&long, 2);
        assert_eq!(
            with_postfix_value.len(),
            MAX_PROFILE_NAME_LEN,
            "the id stays canonical so the profile list accepts it"
        );
        assert!(with_postfix_value.ends_with("_2"));
        assert_eq!(
            sanitize_profile_name(&with_postfix_value).as_deref(),
            Ok(with_postfix_value.as_str())
        );
    }

    #[test]
    fn display_names_fall_back_to_the_id() {
        assert_eq!(prettify_profile_id("home-net_2"), "Home Net 2");
        assert_eq!(prettify_profile_id("default"), "Default");
        assert_eq!(
            display_name(&Config::default(), "work_vpn"),
            "Work Vpn",
            "a file written before profiles had names still shows something"
        );
        let config = Config {
            profile_name: "  Home\u{7} Net  ".to_string(),
            ..Config::default()
        };
        assert_eq!(display_name(&config, "home"), "Home Net");
    }

    #[test]
    fn existing_profiles_without_a_name_get_one_on_load() {
        let root = TestDir::new("backfill");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        write_profile(&store, "work-vpn", r#"{"overlays":[]}"#);
        let broken = write_profile(&store, "broken", "{not json");

        let loaded = store.load(&root.path().join("missing-legacy"));

        assert!(loaded
            .notices
            .contains(&ConfigNotice::ProfileNamesBackfilled { count: 1 }));
        assert_eq!(
            loaded.profiles,
            entries(&[
                ("broken", "Broken"),
                ("default", "Default"),
                ("work-vpn", "Work Vpn"),
            ]),
            "an unreadable profile is still listed under a derived name"
        );
        assert_eq!(
            fs::read_to_string(broken).expect("read broken profile"),
            "{not json",
            "a file that cannot be parsed is never rewritten"
        );
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(store.profile_path("work-vpn").expect("path"))
                .expect("read profile"),
        )
        .expect("parse profile");
        assert_eq!(value["profileName"], "Work Vpn");
    }

    #[test]
    fn unrelated_files_in_the_profiles_directory_are_ignored() {
        let root = TestDir::new("foreign");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));
        write_profile(&store, "Work VPN", r#"{"overlays":[]}"#);
        fs::write(store.profiles_dir().join("notes.txt"), "hello").expect("write note");
        fs::write(store.profiles_dir().join("profile_UPPER.json"), "{}").expect("write odd name");
        fs::create_dir_all(store.profiles_dir().join("profile_dir")).expect("write directory");

        assert_eq!(store.list_profiles(), vec![DEFAULT_PROFILE.to_string()]);
    }

    #[test]
    fn saving_a_profile_replaces_it_atomically() {
        let root = TestDir::new("atomic-save");
        let store = store_at(root.path());
        store.load(&root.path().join("missing-legacy"));

        store
            .save_profile(DEFAULT_PROFILE, &one_overlay_config("first"))
            .expect("first save");
        store
            .save_profile(DEFAULT_PROFILE, &one_overlay_config("second"))
            .expect("second save");

        assert_eq!(
            store
                .load_profile(DEFAULT_PROFILE)
                .expect("load profile")
                .overlays[0]
                .id,
            "second"
        );
        let leftovers: Vec<String> = fs::read_dir(store.profiles_dir())
            .expect("read profiles directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            vec![format!(
                "{PROFILE_PREFIX}{DEFAULT_PROFILE}{PROFILE_EXTENSION}"
            )],
            "no temporary file may be left behind"
        );
    }
}
