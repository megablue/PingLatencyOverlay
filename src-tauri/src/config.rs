use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
            smooth_rendering: true,
            smooth_fps: default_smooth_fps(),
            legacy_smooth_delay_ms: None,
            cosmetic_startup_prefill: true,
            prefill_line_color: default_prefill_line_color(),
            prefill_animation_sec: default_prefill_animation_sec(),
            startup_border_effect: default_startup_border_effect(),
            border_animation_sec: default_border_animation_sec(),
            border_fade_sec: default_border_fade_sec(),
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

const CONFIG_FILE: &str = "config.json";

/// Result of loading the configuration, including any one-time migration.
pub struct ConfigLoad {
    pub config: Config,
    pub notice: Option<ConfigNotice>,
}

/// User-visible result of attempting to migrate the legacy config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigNotice {
    Migrated { legacy_directory_retained: bool },
    MigrationFailed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MigrationOutcome {
    LegacyDirectoryRemoved,
    LegacyDirectoryRetained,
}

/// `~/.config/.PingLatencyOverlay`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".PingLatencyOverlay")
}

/// `~/.config/.PingLatencyOverlay/config.json`
pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Legacy `~/.PingLatencyOverlay` directory.
fn legacy_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".PingLatencyOverlay")
}

/// Load the new config location, migrating the legacy directory when needed.
pub fn load() -> ConfigLoad {
    let path = config_path();
    let notice = if path.exists() {
        None
    } else {
        match migrate_legacy_config(&config_dir(), &legacy_config_dir()) {
            Ok(Some(MigrationOutcome::LegacyDirectoryRemoved)) => Some(ConfigNotice::Migrated {
                legacy_directory_retained: false,
            }),
            Ok(Some(MigrationOutcome::LegacyDirectoryRetained)) => Some(ConfigNotice::Migrated {
                legacy_directory_retained: true,
            }),
            Ok(None) => None,
            Err(error) => Some(ConfigNotice::MigrationFailed(error.to_string())),
        }
    };

    let mut config = load_from_path(&path);
    config.normalize();
    ConfigLoad { config, notice }
}

/// Persist the config to disk (creating the directory if needed).
pub fn save(cfg: &Config) -> io::Result<()> {
    fs::create_dir_all(config_dir())?;
    let json = serde_json::to_string_pretty(cfg).map_err(io::Error::other)?;
    fs::write(config_path(), json)
}

fn load_from_path(path: &Path) -> Config {
    match fs::read_to_string(path) {
        Ok(raw) => parse_config(&raw).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

fn parse_config(raw: &str) -> Result<Config, serde_json::Error> {
    // Tolerate a UTF-8 BOM (e.g. files edited by Notepad/PowerShell).
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    serde_json::from_str(raw)
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
        assert_eq!(overlay.timeout_ms, 1_000);
        assert_eq!(overlay.graph_height_px, 60);
        assert_eq!(overlay.max_y_ms, 1_000);
        assert_eq!(overlay.margin_px, 20);
        assert_eq!(overlay.bg_opacity, 0);
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
        assert_eq!(overlay.smooth_fps, MIN_SMOOTH_FPS);
        assert_eq!(overlay.prefill_animation_sec, MIN_PREFILL_ANIMATION_SEC);
        assert_eq!(overlay.border_animation_sec, MIN_BORDER_ANIMATION_SEC);
        assert_eq!(overlay.border_fade_sec, MAX_BORDER_FADE_SEC);
        assert_eq!(overlay.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(overlay.graph_height_px, MIN_GRAPH_HEIGHT_PX);
        assert_eq!(overlay.max_y_ms, DEFAULT_MAX_Y_MS);
        assert_eq!(overlay.orientation, 0);
        assert_eq!(overlay.bg_opacity, 100);
    }
}
