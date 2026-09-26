use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
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
            horizontal_margin_px: DEFAULT_HORIZONTAL_MARGIN_PX,
            vertical_margin_px: DEFAULT_VERTICAL_MARGIN_PX,
            legacy_margin_px: None,
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
fn default_bg_color() -> String {
    DEFAULT_BG_COLOR.to_string()
}
fn default_bg_opacity() -> u32 {
    DEFAULT_BG_OPACITY
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
const PROFILES_DIR_NAME: &str = "profiles";
const PROFILE_PREFIX: &str = "profile_";
const PROFILE_EXTENSION: &str = ".json";
const GLOBAL_CONFIG_FILE: &str = "globalconfig.json";
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

/// Result of loading the configuration, including any one-time migrations.
pub struct ConfigLoad {
    pub config: Config,
    pub active_profile: String,
    pub profiles: Vec<ProfileEntry>,
    pub notices: Vec<ConfigNotice>,
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
/// temporary directory in tests.
struct Store {
    root: PathBuf,
}

impl Store {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join(PROFILES_DIR_NAME)
    }

    /// Only the migration source for the profiles directory; new
    /// configurations are stored as profile files.
    fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    fn global_config_path(&self) -> PathBuf {
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
    fn list_profiles_detailed(&self) -> Vec<ProfileEntry> {
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
    fn load_profile(&self, name: &str) -> io::Result<Config> {
        let path = self.profile_path(name)?;
        let raw = fs::read_to_string(&path)?;
        let mut config = parse_and_validate(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        config.normalize();
        Ok(config)
    }

    /// Write one profile file atomically.
    fn save_profile(&self, name: &str, config: &Config) -> io::Result<()> {
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
    fn create_profile(&self, display_name: &str) -> io::Result<ProfileEntry> {
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
    fn rename_profile(&self, from: &str, display_name: &str) -> io::Result<ProfileEntry> {
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
    fn duplicate_profile(&self, from: &str, display_name: &str) -> io::Result<ProfileEntry> {
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

    fn delete_profile(&self, name: &str) -> io::Result<()> {
        fs::remove_file(self.profile_path(name)?)
    }

    /// Remember the active profile for the next launch.
    ///
    /// Both the id and the file name are stored, and the file name is the only
    /// thing [`Store::load`] needs to find the profile again. Both keys are
    /// removed when the default profile is active, so a fresh install keeps
    /// `globalconfig.json` as an empty object until something is actually
    /// stored.
    fn set_active_profile(&self, name: &str) -> io::Result<()> {
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
    fn load(&self, legacy_dir: &Path) -> ConfigLoad {
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
        }
    }
}

fn store() -> Store {
    Store::new(config_dir())
}

fn canonical_profile_name(name: &str) -> io::Result<String> {
    sanitize_profile_name(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// `~/.config/.PingLatencyOverlay`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(".PingLatencyOverlay")
}

/// `~/.config/.PingLatencyOverlay/profiles`
pub fn profiles_dir() -> PathBuf {
    store().profiles_dir()
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
        if overlay.probe.host().trim().is_empty() {
            return Err(format!(
                "overlay \"{}\" has an empty probe host",
                overlay.id
            ));
        }
        if matches!(overlay.probe, ProbeConfig::Tcp { port: 0, .. }) {
            return Err(format!(
                "overlay \"{}\" is missing its TCP port",
                overlay.id
            ));
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
    let config: Config = serde_json::from_value(value).map_err(|error| error.to_string())?;
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
        assert_eq!(overlay.horizontal_margin_px, 0);
        assert_eq!(overlay.vertical_margin_px, 0);
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
        assert_eq!(overlay.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(overlay.graph_height_px, MIN_GRAPH_HEIGHT_PX);
        assert_eq!(overlay.max_y_ms, DEFAULT_MAX_Y_MS);
        assert_eq!(overlay.orientation, 0);
        assert_eq!(overlay.horizontal_margin_px, MAX_MARGIN_OFFSET_PX);
        assert_eq!(overlay.vertical_margin_px, MIN_MARGIN_OFFSET_PX);
        assert_eq!(overlay.bg_opacity, 100);
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
