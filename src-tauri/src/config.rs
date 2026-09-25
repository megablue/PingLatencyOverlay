use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
/// Default delay between smooth overlay redraws.
pub const DEFAULT_SMOOTH_DELAY_MS: u32 = 16;
/// Smallest allowed smooth redraw delay.
pub const MIN_SMOOTH_DELAY_MS: u32 = 1;
/// Largest allowed smooth redraw delay.
pub const MAX_SMOOTH_DELAY_MS: u32 = 1_000;
/// Default gap between the overlay and the screen edge, in logical pixels.
pub const DEFAULT_MARGIN_PX: u32 = 20;
/// Default overlay background color.
pub const DEFAULT_BG_COLOR: &str = "#0f172a";
/// Default overlay background opacity (0 = fully transparent).
pub const DEFAULT_BG_OPACITY: u32 = 0;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
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
    pub probe: ProbeConfig,

    /// Graph rotation: 0, 90, 180 or 270 degrees.
    #[serde(default)]
    pub orientation: u16,
    /// Mirror the graph (combines with any orientation).
    #[serde(default)]
    pub mirrored: bool,
    #[serde(default = "default_line_color")]
    pub line_color: String,
    #[serde(default = "default_timeout_color")]
    pub timeout_color: String,
    #[serde(default)]
    pub position: Anchor,

    /// Visible time window in seconds (each tick = one ping, one tick per second).
    #[serde(default = "default_window_seconds")]
    pub window_seconds: u32,
    /// Visual scale multiplier: pixels per tick.
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Continuously redraw the graph between timestamped samples.
    #[serde(default)]
    pub smooth_rendering: bool,
    /// Delay between smooth redraws, in milliseconds.
    #[serde(default = "default_smooth_delay_ms")]
    pub smooth_delay_ms: u32,
    /// Ping timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
    /// Height of the Y axis on screen, in logical pixels.
    #[serde(default = "default_graph_height_px")]
    pub graph_height_px: u32,
    /// Latency ceiling in milliseconds; higher pings clamp to the top.
    #[serde(default = "default_max_y_ms")]
    pub max_y_ms: u32,
    /// Gap between the overlay and the screen edge, in logical pixels.
    #[serde(default = "default_margin_px")]
    pub margin_px: u32,
    /// Background color drawn behind the graph.
    #[serde(default = "default_bg_color")]
    pub bg_color: String,
    /// Background opacity, 0 (transparent) to 100 (opaque).
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: u32,
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

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayConfig {
    /// A sensible starting point for a newly added overlay.
    pub fn new() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            id: format!("{millis}-{sequence}"),
            name: "New overlay".to_string(),
            enabled: true,
            probe: ProbeConfig::Icmp {
                host: "1.1.1.1".to_string(),
            },
            orientation: 0,
            mirrored: false,
            line_color: default_line_color(),
            timeout_color: default_timeout_color(),
            position: Anchor::TopRight,
            window_seconds: default_window_seconds(),
            scale: default_scale(),
            smooth_rendering: false,
            smooth_delay_ms: default_smooth_delay_ms(),
            timeout_ms: default_timeout_ms(),
            graph_height_px: default_graph_height_px(),
            max_y_ms: default_max_y_ms(),
            margin_px: default_margin_px(),
            bg_color: default_bg_color(),
            bg_opacity: default_bg_opacity(),
        }
    }
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
fn default_smooth_delay_ms() -> u32 {
    DEFAULT_SMOOTH_DELAY_MS
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
fn default_margin_px() -> u32 {
    DEFAULT_MARGIN_PX
}
fn default_bg_color() -> String {
    DEFAULT_BG_COLOR.to_string()
}
fn default_bg_opacity() -> u32 {
    DEFAULT_BG_OPACITY
}

impl Config {
    /// Clamp values that the UI might send out of range.
    pub fn normalize(&mut self) {
        for o in &mut self.overlays {
            o.window_seconds = o.window_seconds.max(MIN_WINDOW_SECONDS);
            o.scale = o.scale.clamp(1, MAX_SCALE_INPUT);
            o.smooth_delay_ms = o
                .smooth_delay_ms
                .clamp(MIN_SMOOTH_DELAY_MS, MAX_SMOOTH_DELAY_MS);
            if o.timeout_ms == 0 {
                o.timeout_ms = DEFAULT_TIMEOUT_MS;
            }
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
        }
    }
}

/// `~/.PingLatencyOverlay`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".PingLatencyOverlay")
}

/// `~/.PingLatencyOverlay/config.json`
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Load the config from disk, falling back to defaults on any error.
pub fn load() -> Config {
    let mut cfg = match fs::read_to_string(config_path()) {
        Ok(raw) => {
            // Tolerate a UTF-8 BOM (e.g. files edited by Notepad/PowerShell).
            let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
            serde_json::from_str::<Config>(raw).unwrap_or_default()
        }
        Err(_) => Config::default(),
    };
    cfg.normalize();
    cfg
}

/// Persist the config to disk (creating the directory if needed).
pub fn save(cfg: &Config) -> io::Result<()> {
    fs::create_dir_all(config_dir())?;
    let json = serde_json::to_string_pretty(cfg).map_err(io::Error::other)?;
    fs::write(config_path(), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_overlay_uses_documented_defaults() {
        let overlay = OverlayConfig::new();
        assert_eq!(overlay.name, "New overlay");
        assert!(overlay.enabled);
        assert_eq!(overlay.scale, 2);
        assert!(!overlay.smooth_rendering);
        assert_eq!(overlay.smooth_delay_ms, DEFAULT_SMOOTH_DELAY_MS);
        assert_eq!(overlay.timeout_ms, 1_000);
        assert_eq!(overlay.graph_height_px, 60);
        assert_eq!(overlay.max_y_ms, 1_000);
        assert_eq!(overlay.margin_px, 20);
        assert_eq!(overlay.bg_opacity, 0);
    }

    #[test]
    fn legacy_overlay_config_gets_smooth_defaults() {
        let overlay: OverlayConfig =
            serde_json::from_str(r#"{"id":"legacy","probe":{"protocol":"icmp","host":"1.1.1.1"}}"#)
                .expect("legacy config");
        assert!(!overlay.smooth_rendering);
        assert_eq!(overlay.smooth_delay_ms, DEFAULT_SMOOTH_DELAY_MS);
    }

    #[test]
    fn normalize_preserves_scale_above_slider_max() {
        let mut config = Config {
            overlays: vec![OverlayConfig {
                scale: 25,
                ..OverlayConfig::new()
            }],
        };
        config.normalize();
        assert_eq!(config.overlays[0].scale, 25);
    }

    #[test]
    fn normalize_clamps_invalid_values() {
        let mut config = Config {
            overlays: vec![OverlayConfig {
                window_seconds: 1,
                scale: MAX_SCALE_INPUT + 1,
                smooth_delay_ms: 0,
                timeout_ms: 0,
                graph_height_px: 1,
                max_y_ms: 0,
                orientation: 45,
                bg_opacity: 200,
                ..OverlayConfig::new()
            }],
        };
        config.normalize();
        let overlay = &config.overlays[0];
        assert_eq!(overlay.window_seconds, MIN_WINDOW_SECONDS);
        assert_eq!(overlay.scale, MAX_SCALE_INPUT);
        assert_eq!(overlay.smooth_delay_ms, MIN_SMOOTH_DELAY_MS);
        assert_eq!(overlay.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(overlay.graph_height_px, MIN_GRAPH_HEIGHT_PX);
        assert_eq!(overlay.max_y_ms, DEFAULT_MAX_Y_MS);
        assert_eq!(overlay.orientation, 0);
        assert_eq!(overlay.bg_opacity, 100);
    }
}
