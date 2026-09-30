//! Colours for the Config window, loaded from a file rather than compiled in.
//!
//! The window is the only themed surface in the app. The overlay graph draws
//! with per-overlay colours the user already chose, the tray menu is a native
//! `HMENU` that Windows paints from the system theme, and the tray icon and the
//! About logo are brand artwork. So a theme is exactly this: the Config window's
//! palette.
//!
//! ## Where a theme comes from
//!
//! `default` ships with the app and is **repaired, not protected**: its two files
//! are embedded in the binary and written to disk at startup when the copy on
//! disk is missing or unreadable. A file that parses is honoured, so editing one
//! is a real thing you can do. The alternative — always overwriting — destroys
//! an edit without saying so, and a theme you cannot experiment with is not
//! worth having on disk at all.
//!
//! Any other directory under `themes/` is a user theme. Phase 1 has no UI for
//! browsing them; the format is the contract for that.
//!
//! ## Two things that are easy to get wrong here
//!
//! The first is the `Visuals` **base**. `Visuals::light()` and `Visuals::dark()`
//! are not the same palette with the ends swapped: much of egui's widget
//! drawing — checkbox ticks, scrollbar grips, selection handles, shaded
//! non-interactive text — is derived from that base rather than from the fields
//! below. Repainting the fifteen colours onto a dark base produces a light
//! background sitting on dark internals, which is the classic half-themed
//! window. `visuals` switches the base and *then* applies the palette.
//!
//! The second is that a theme can be unreadable and still load perfectly. A
//! `text` the same colour as `background` is not a parse error, it is a
//! confident wrong window, so `contrast_ratio` and the checks on the built-ins
//! are part of this module rather than a nicety.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32};
use ping_latency_overlay_core::render::parse_hex_color;
use serde::Deserialize;

/// The light theme, embedded. Written to disk when the copy there is missing or
/// unreadable.
pub const CORE_LIGHT: &str = include_str!("../assets/themes/default/core.json");
/// The dark theme, embedded. Optional on disk; without it the dark theme is the
/// embedded one either way.
pub const CORE_DARK: &str = include_str!("../assets/themes/default/core-dark.json");

/// The directory under `themes/` that the app ships and repairs.
pub const BUILTIN_THEME_DIR: &str = "default";

/// Which of a theme's two files to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    fn file_name(self) -> &'static str {
        match self {
            Mode::Light => "core.json",
            Mode::Dark => "core-dark.json",
        }
    }

    /// The mode that Windows is asking for.
    ///
    /// `AppsUseLightTheme` is 1 for light and 0 for dark. A missing or
    /// unreadable value is treated as light, because that is what a machine with
    /// no such key has been showing all along.
    pub fn from_system(read: impl FnOnce() -> Option<u32>) -> Mode {
        match read() {
            Some(0) => Mode::Dark,
            _ => Mode::Light,
        }
    }
}

/// The user's Windows app theme, from the registry.
///
/// Read on demand rather than cached at startup, so flipping Windows between
/// light and dark follows through to the window while it is open — which is the
/// point of `System` being the default.
///
/// Declared here rather than taken from a crate: this is the application
/// shell, so a binding crate would be a dependency bought for one `RegGetValueW`
/// call, and the tray has already shown what hand-declared Win32 costs (a
/// helper thread and a message pump). This is the cheaper direction.
#[cfg(windows)]
pub fn windows_app_mode() -> Mode {
    #[allow(non_snake_case, clippy::upper_case_acronyms)]
    mod win {
        use core::ffi::c_void;

        pub type DWORD = u32;
        pub type HKEY = *mut c_void;
        pub type LSTATUS = i32;
        pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as HKEY;
        pub const RRF_RT_REG_DWORD: u32 = 0x0000_0018;

        #[link(name = "advapi32")]
        unsafe extern "system" {
            pub fn RegGetValueW(
                hkey: HKEY,
                lpsubkey: *const u16,
                lpvalue: *const u16,
                dwflags: DWORD,
                pdwtype: *mut DWORD,
                pvdata: *mut c_void,
                pcbdata: *mut DWORD,
            ) -> LSTATUS;
        }
    }

    /// `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
    const SUBKEY: &[u16] = &[
        83u16, 111, 102, 116, 119, 97, 114, 101, 92, 77, 105, 99, 114, 111, 115, 111, 102, 116, 92,
        87, 105, 110, 100, 111, 119, 115, 92, 67, 117, 114, 114, 101, 110, 116, 86, 101, 114, 115,
        105, 111, 110, 92, 84, 104, 101, 109, 101, 115, 92, 80, 101, 114, 115, 111, 110, 97, 108,
        105, 122, 101, 0,
    ];
    /// `AppsUseLightTheme`
    const VALUE: &[u16] = &[
        65u16, 112, 112, 115, 85, 115, 101, 76, 105, 103, 104, 116, 84, 104, 101, 109, 101, 0,
    ];

    unsafe {
        let mut value: win::DWORD = 0;
        let mut size = std::mem::size_of::<win::DWORD>() as win::DWORD;
        let status = win::RegGetValueW(
            win::HKEY_CURRENT_USER,
            SUBKEY.as_ptr(),
            VALUE.as_ptr(),
            win::RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut win::DWORD).cast(),
            &mut size,
        );
        // ERROR_SUCCESS, or nothing at all, both mean "light": a machine with
        // no such value has been showing a light Windows this whole time, and
        // second-guessing that with a dark window would be the surprising
        // outcome.
        if status == 0 {
            Mode::from_system(|| Some(value))
        } else {
            Mode::from_system(|| None)
        }
    }
}

/// What the user asked for, as stored in `globalconfig.json`.
///
/// The enum itself lives in `config.rs` because `UiPrefs` holds it; see the note
/// there.
pub use ping_latency_overlay_core::config::ThemeMode;

/// Resolve a stored preference against what the system is currently doing.
///
/// A free function rather than a method: `ThemeMode` is declared in `config.rs`
/// because `UiPrefs` holds it, and an `impl` for a type from another crate is
/// not allowed. `Mode` is the other way round, so neither type can own the
/// combination.
pub fn resolve(preference: ThemeMode, system: Mode) -> Mode {
    match preference {
        ThemeMode::System => system,
        ThemeMode::Light => Mode::Light,
        ThemeMode::Dark => Mode::Dark,
    }
}

/// The resolved palette, as egui colours.
///
/// Every field here is one of the fifteen constants that used to be `const`s in
/// `ui.rs`. There is no third place a colour can come from, which is the point:
/// a new surface has to pick from this list rather than invent a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub surface_hover: Color32,
    pub input_background: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub selection: Color32,
    pub scrollbar: Color32,
    pub scrollbar_hover: Color32,
    pub danger: Color32,
    pub danger_strong: Color32,
}

/// The colours as they appear in a theme file: hex strings, every one optional.
///
/// `Option` rather than a `#[serde(default)]` value on purpose. A missing key
/// has to fall back to the built-in palette *for the mode being loaded*, and a
/// `Default` impl cannot know that; a light theme with one missing key would
/// otherwise get a dark value for it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColorsFile {
    background: Option<String>,
    surface: Option<String>,
    surface_alt: Option<String>,
    surface_hover: Option<String>,
    input_background: Option<String>,
    border: Option<String>,
    text: Option<String>,
    text_secondary: Option<String>,
    accent: Option<String>,
    accent_strong: Option<String>,
    selection: Option<String>,
    scrollbar: Option<String>,
    scrollbar_hover: Option<String>,
    danger: Option<String>,
    danger_strong: Option<String>,
}

/// One file in a theme directory, as parsed.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    colors: ColorsFile,
    #[serde(default)]
    assets: AssetsFile,
}

/// Artwork a theme supplies, as it appears in a theme file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetsFile {
    #[serde(default)]
    position_picker: Option<PickerFile>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PickerFile {
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    hover: Option<String>,
    #[serde(default)]
    selected: Option<String>,
}

/// Which picture the position picker draws in one of its three states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerState {
    Default,
    Hover,
    Selected,
}

/// The artwork a theme resolved to, by state. A state the theme says nothing
/// about is `None`, which is the signal to draw the picker with painted
/// primitives instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeAssets {
    pub position_picker: [Option<PathBuf>; 3],
}

impl ThemeAssets {
    pub fn picker(&self, state: PickerState) -> Option<&Path> {
        self.position_picker[state as usize].as_deref()
    }
}

/// A loaded theme.
#[derive(Clone, Debug)]
pub struct Theme {
    /// The theme's own name, from its file.
    ///
    /// Parsed but not shown: phase 1 ships the two built-in themes and no UI
    /// for choosing one, so there is nowhere to put a name yet. It is read here
    /// rather than dropped so that a theme file already carrying a name is not
    /// silently ignored, and so phase 2 only has to display it.
    #[allow(dead_code)]
    pub name: String,
    pub colors: Palette,
    pub assets: ThemeAssets,
    pub mode: Mode,
}

/// Reduce a user-supplied file name to a bare name inside the theme directory.
///
/// A theme file is data in a directory the user can write to, and its `assets`
/// block is a path. Without this, `"../../../../Windows/System32/drivers/etc/hosts"`
/// would be a perfectly ordinary-looking theme. Anything that is not a plain
/// file name is dropped, so the worst case is a missing picture and a painted
/// picker.
fn safe_asset_name(raw: &str) -> Option<PathBuf> {
    let name = raw.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name == "."
        || name == ".."
    {
        return None;
    }
    Some(PathBuf::from(name))
}

fn parse_theme_file(text: &str, mode: Mode, base: &Palette) -> Result<Theme, String> {
    let file: ThemeFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let pick = |value: &Option<String>, fallback: Color32| {
        let [r, g, b, _] = fallback.to_array();
        value
            .as_deref()
            .map(|hex| {
                let [r, g, b] = parse_hex_color(hex, [r, g, b]);
                Color32::from_rgb(r, g, b)
            })
            .unwrap_or(fallback)
    };
    let colors = &file.colors;
    let palette = Palette {
        background: pick(&colors.background, base.background),
        surface: pick(&colors.surface, base.surface),
        surface_alt: pick(&colors.surface_alt, base.surface_alt),
        surface_hover: pick(&colors.surface_hover, base.surface_hover),
        input_background: pick(&colors.input_background, base.input_background),
        border: pick(&colors.border, base.border),
        text: pick(&colors.text, base.text),
        text_secondary: pick(&colors.text_secondary, base.text_secondary),
        accent: pick(&colors.accent, base.accent),
        accent_strong: pick(&colors.accent_strong, base.accent_strong),
        selection: pick(&colors.selection, base.selection),
        scrollbar: pick(&colors.scrollbar, base.scrollbar),
        scrollbar_hover: pick(&colors.scrollbar_hover, base.scrollbar_hover),
        danger: pick(&colors.danger, base.danger),
        danger_strong: pick(&colors.danger_strong, base.danger_strong),
    };
    let picker = file.assets.position_picker.unwrap_or_default();
    let assets = ThemeAssets {
        position_picker: [
            picker.default.as_deref().and_then(safe_asset_name),
            picker.hover.as_deref().and_then(safe_asset_name),
            picker.selected.as_deref().and_then(safe_asset_name),
        ],
    };
    let name = file.name.unwrap_or_else(|| {
        format!(
            "Default {}",
            if mode == Mode::Light { "Light" } else { "Dark" }
        )
    });
    Ok(Theme {
        name,
        colors: palette,
        assets,
        mode,
    })
}

fn embedded(mode: Mode) -> &'static str {
    match mode {
        Mode::Light => CORE_LIGHT,
        Mode::Dark => CORE_DARK,
    }
}

/// The theme the app ships, from the copy embedded in the binary.
///
/// This never fails: the embedded files are compiled in, and a test parses both
/// of them so a malformed one fails the suite rather than the app.
pub fn builtin(mode: Mode) -> Theme {
    builtin_with(embedded(mode), mode)
}

fn builtin_with(text: &str, mode: Mode) -> Theme {
    // The palette a file is resolved against is the embedded one, so a missing
    // key can only ever inherit the shipped value. The all-black base is only
    // ever the parent of this first parse, never a value a theme can end up
    // with, because every field in the embedded file is set.
    let base = parse_theme_file(embedded(mode), mode, &Palette::black())
        .expect("the embedded theme parses; a test asserts this");
    parse_theme_file(text, mode, &base.colors).expect("the embedded theme parses")
}

impl Palette {
    /// A placeholder base so that resolving the embedded theme needs no values
    /// of its own. Only ever used as the parent of a first parse.
    const fn black() -> Palette {
        Palette {
            background: Color32::BLACK,
            surface: Color32::BLACK,
            surface_alt: Color32::BLACK,
            surface_hover: Color32::BLACK,
            input_background: Color32::BLACK,
            border: Color32::BLACK,
            text: Color32::BLACK,
            text_secondary: Color32::BLACK,
            accent: Color32::BLACK,
            accent_strong: Color32::BLACK,
            selection: Color32::BLACK,
            scrollbar: Color32::BLACK,
            scrollbar_hover: Color32::BLACK,
            danger: Color32::BLACK,
            danger_strong: Color32::BLACK,
        }
    }
}

/// Load the shipped theme, writing both of its files to disk when they are
/// missing or unreadable.
///
/// Both, not just the one in use. A theme directory holding only whichever file
/// the last launch happened to need is a directory whose contents depend on what
/// the machine was set to that day, and `core.json` is the required one — a user
/// opening the folder to look at it should find the light theme there whether or
/// not they have ever run the app in light mode.
///
/// Returns the theme plus a notice per thing written or repaired, so the window
/// can say so rather than silently rewriting a file the user was looking at.
pub fn load_builtin(root: &Path, mode: Mode) -> (Theme, Vec<String>) {
    let mut notices = Vec::new();
    let dir = root.join(BUILTIN_THEME_DIR);
    for each in [Mode::Light, Mode::Dark] {
        notices.extend(ensure_builtin_file(&dir, each));
    }

    let path = dir.join(mode.file_name());
    // A file that parses is honoured, so editing one is a real thing a user can
    // do. Only an unreadable one falls through to the embedded copy.
    let on_disk = std::fs::read_to_string(&path).ok();
    let theme = on_disk
        .as_deref()
        .and_then(|text| parse_theme_file(text, mode, &builtin(mode).colors).ok())
        .unwrap_or_else(|| builtin(mode));
    // A theme that loads perfectly and cannot be read is the failure this whole
    // check exists for, so it is checked on the way in rather than trusted.
    notices.extend(contrast_report(&theme.colors));
    (theme, notices)
}

/// Write one of the built-in theme's files if it is missing or unreadable, and
/// report what happened.
fn ensure_builtin_file(dir: &Path, mode: Mode) -> Vec<String> {
    let path = dir.join(mode.file_name());
    let name = if mode == Mode::Light { "light" } else { "dark" };

    match std::fs::read_to_string(&path) {
        Err(_) => match write_file(&path, embedded(mode)) {
            Ok(()) => vec![format!(
                "Wrote the built-in {name} theme to {}.",
                path.display()
            )],
            Err(error) => vec![unwritable(&path, &error, name)],
        },
        // Present. Repair it only if it does not parse, and say so, because a
        // silent rewrite of a file somebody is editing is the thing to avoid.
        Ok(existing) => {
            if serde_json::from_str::<ThemeFile>(&existing).is_ok() {
                return Vec::new();
            }
            match write_file(&path, embedded(mode)) {
                Ok(()) => vec![format!(
                    "{} could not be read as a theme, so the built-in {name} theme was restored.",
                    path.display()
                )],
                Err(error) => vec![unwritable(&path, &error, name)],
            }
        }
    }
}

fn unwritable(path: &Path, error: &std::io::Error, name: &str) -> String {
    format!(
        "Could not write {}: {error}. The built-in {name} theme is in use for this session.",
        path.display()
    )
}

fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

/// WCAG's floor for body text. Below this a pair is a real usability problem,
/// not a preference.
const AA_BODY: f32 = 4.5;

/// Report every pair in `colors` that is too close to read.
///
/// Returned as text so a caller can put it in the status bar. A theme can be
/// unreadable and still load without complaint, so without this the only way to
/// find out is to squint at the window and decide you are imagining it.
pub fn contrast_report(colors: &Palette) -> Vec<String> {
    readable_pairs(colors)
        .into_iter()
        .filter(|(_, foreground, background)| contrast_ratio(*foreground, *background) < AA_BODY)
        .map(|(name, _, _)| format!("{name} is hard to read in this theme"))
        .collect()
}

/// Hand a theme to egui.
///
/// This is the step that was wrong once, so it is a function with a test of its
/// own rather than a line inside `PingApp::sync_theme`.
///
/// egui keeps **two** styles, `Options::dark_style` and `Options::light_style`,
/// and `Context::set_visuals` is `style_mut_of(self.theme(), ..)`: it writes the
/// visuals into *whichever theme egui currently considers active*, and
/// `self.theme()` comes from egui's own `theme_preference`, which defaults to
/// `System` and is re-read from the OS on every pass.
///
/// So `set_visuals` on its own writes into one of two slots without choosing
/// which. A theme change that crossed egui's own detection left widgets reading
/// the other slot — one this app had never written — so they rendered with
/// egui's stock light or dark defaults: light buttons on a dark window, white
/// text on white. The painted half of the app was fine, because it reads the
/// thread-local palette, which is why the failure looked partial.
///
/// The fix is to fill **both** slots and then pin the preference, so egui's
/// detection has nothing left to decide. `set_theme` is what makes the switch
/// take effect; writing both is what makes it stay.
pub fn apply(ctx: &egui::Context, theme: &Theme) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals_for(theme, Mode::Dark));
    ctx.set_visuals_of(egui::Theme::Light, visuals_for(theme, Mode::Light));
    ctx.set_theme(match theme.mode {
        Mode::Light => egui::Theme::Light,
        Mode::Dark => egui::Theme::Dark,
    });
}

/// egui's widget styling for a palette in a given mode.
///
/// The base comes first and the palette goes on top. See the module comment:
/// skipping the base switch is what produces a light window with dark internals.
fn visuals_for(theme: &Theme, mode: Mode) -> egui::Visuals {
    let colors = &theme.colors;
    let mut visuals = match mode {
        Mode::Light => egui::Visuals::light(),
        Mode::Dark => egui::Visuals::dark(),
    };
    let border = egui::Stroke::new(1.0, colors.border);
    let text = egui::Stroke::new(1.0, colors.text);
    let accent_text = egui::Stroke::new(1.0, colors.accent);
    let radius = egui::CornerRadius::same(4);

    visuals.override_text_color = Some(colors.text);
    visuals.weak_text_color = Some(colors.text_secondary);
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.background;
    visuals.faint_bg_color = colors.surface;
    visuals.extreme_bg_color = colors.input_background;
    visuals.text_edit_bg_color = Some(colors.input_background);
    visuals.hyperlink_color = colors.accent;
    visuals.warn_fg_color = colors.danger;
    visuals.error_fg_color = colors.danger;
    visuals.selection.bg_fill = colors.selection;
    visuals.selection.stroke = accent_text;
    visuals.window_stroke = border;
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.menu_corner_radius = radius;
    visuals.text_cursor.stroke = accent_text;
    visuals.button_frame = true;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.disabled_alpha = 0.45;

    visuals.widgets.noninteractive.bg_fill = colors.surface;
    visuals.widgets.noninteractive.weak_bg_fill = colors.surface;
    visuals.widgets.noninteractive.bg_stroke = border;
    visuals.widgets.noninteractive.fg_stroke = text;
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = colors.scrollbar;
    visuals.widgets.inactive.weak_bg_fill = colors.surface_alt;
    visuals.widgets.inactive.bg_stroke = border;
    visuals.widgets.inactive.fg_stroke = text;
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = colors.scrollbar_hover;
    visuals.widgets.hovered.weak_bg_fill = colors.surface_hover;
    visuals.widgets.hovered.bg_stroke = border;
    visuals.widgets.hovered.fg_stroke = text;
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = colors.selection;
    visuals.widgets.active.weak_bg_fill = colors.selection;
    visuals.widgets.active.bg_stroke = accent_text;
    visuals.widgets.active.fg_stroke = text;
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.bg_fill = colors.surface_hover;
    visuals.widgets.open.weak_bg_fill = colors.surface_hover;
    visuals.widgets.open.bg_stroke = accent_text;
    visuals.widgets.open.fg_stroke = text;
    visuals.widgets.open.corner_radius = radius;

    visuals
}

// --- contrast -------------------------------------------------------------
// A theme that puts the same colour on text and background loads without a
// single complaint and renders an unreadable window, so the built-ins are held
// to a floor rather than left to look right by eye.

/// WCAG relative luminance.
pub fn relative_luminance(color: Color32) -> f32 {
    let channel = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = color.to_array();
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
///
/// Symmetric, and 1.0 means the two are indistinguishable.
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (first, second) = (relative_luminance(a), relative_luminance(b));
    let lighter = first.max(second);
    let darker = first.min(second);
    (lighter + 0.05) / (darker + 0.05)
}

/// The foreground/background pairs a window cannot be unreadable in.
///
/// Text on a surface is the common case, but the secondary text, the accent and
/// the danger colour are all used on the background too, and each one that is
/// too close to it is text nobody can read.
pub fn readable_pairs(colors: &Palette) -> Vec<(&'static str, Color32, Color32)> {
    vec![
        ("text on background", colors.text, colors.background),
        ("text on surface", colors.text, colors.surface),
        ("text on surface_alt", colors.text, colors.surface_alt),
        (
            "text on input_background",
            colors.text,
            colors.input_background,
        ),
        (
            "text_secondary on background",
            colors.text_secondary,
            colors.background,
        ),
        ("accent on background", colors.accent, colors.background),
        ("accent on surface", colors.accent, colors.surface),
        ("danger on background", colors.danger, colors.background),
        ("danger on surface", colors.danger, colors.surface),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's own floor for body text.
    const AA_BODY: f32 = 4.5;

    #[test]
    fn both_embedded_themes_parse() {
        // The embedded files are the app's fallback for everything, so a
        // malformed one is a broken app rather than a broken theme.
        builtin(Mode::Light);
        builtin(Mode::Dark);
        assert_eq!(builtin(Mode::Light).mode, Mode::Light);
    }

    #[test]
    fn the_dark_builtin_is_the_palette_the_app_has_shipped() {
        // Pins the current appearance, so a change to the dark file is a
        // deliberate change and not a typo nobody sees.
        let colors = builtin(Mode::Dark).colors;
        assert_eq!(colors.background, Color32::from_rgb(0x19, 0x19, 0x19));
        assert_eq!(colors.text, Color32::from_rgb(0xf2, 0xf2, 0xf2));
        assert_eq!(colors.accent, Color32::from_rgb(0x60, 0xcd, 0xff));
        assert_eq!(colors.selection, Color32::from_rgb(0x2d, 0x4f, 0x6d));
    }

    #[test]
    fn both_builtin_themes_are_readable() {
        for mode in [Mode::Light, Mode::Dark] {
            let theme = builtin(mode);
            for (name, foreground, background) in readable_pairs(&theme.colors) {
                let ratio = contrast_ratio(foreground, background);
                assert!(
                    ratio >= AA_BODY,
                    "{mode:?} theme: {name} has a contrast ratio of {ratio:.2}, below {AA_BODY}"
                );
            }
        }
    }

    /// And the shape of the failure this check exists for: it is not a parse
    /// error, it is a window that renders confidently and wrongly.
    #[test]
    fn an_unreadable_theme_loads_and_is_only_caught_by_contrast() {
        let text = r##"{"name":"Bad","colors":{"text":"#202020","background":"#202020"}}"##;
        let theme = parse_theme_file(text, Mode::Light, &builtin(Mode::Light).colors)
            .expect("an unreadable theme is still a valid file");
        assert_eq!(theme.colors.text, theme.colors.background);
        let ratio = contrast_ratio(theme.colors.text, theme.colors.background);
        assert!(
            ratio < AA_BODY,
            "the check that exists to catch this did not"
        );
    }

    #[test]
    fn a_missing_colour_falls_back_to_the_built_in_for_that_mode() {
        // The same partial file resolves differently per mode, which is the
        // whole reason the fields are Option rather than serde(default).
        let text = r##"{"colors":{"accent":"#ff0000"}}"##;
        let light = builtin_with(text, Mode::Light);
        let dark = builtin_with(text, Mode::Dark);
        assert_eq!(light.colors.accent, Color32::from_rgb(0xff, 0, 0));
        // `text` was not in the file, so it inherits per mode rather than
        // inheriting a single hardcoded default.
        assert_eq!(light.colors.text, Color32::from_rgb(0x1a, 0x1a, 0x1a));
        assert_eq!(dark.colors.text, Color32::from_rgb(0xf2, 0xf2, 0xf2));
    }

    #[test]
    fn contrast_is_symmetric_and_bounded() {
        let white = Color32::WHITE;
        let black = Color32::BLACK;
        assert!((contrast_ratio(white, black) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 0.001);
    }

    /// An asset name is a path, and the file saying so is in a directory the
    /// user can write to. Anything that climbs out of it is dropped.
    #[test]
    fn an_asset_cannot_point_outside_the_theme_directory() {
        for hostile in [
            "../secrets.png",
            "..\\secrets.png",
            "../../../../Windows/System32/drivers/etc/hosts",
            "sub/dir.png",
            "C:\\Windows\\win.ini",
            "",
            "   ",
            ".",
            "..",
        ] {
            assert_eq!(safe_asset_name(hostile), None, "{hostile:?} was accepted");
        }
        assert_eq!(
            safe_asset_name("picker.png"),
            Some(PathBuf::from("picker.png"))
        );
        assert_eq!(
            safe_asset_name(" picker.png "),
            Some(PathBuf::from("picker.png")),
            "surrounding whitespace is not an escape"
        );
    }

    #[test]
    fn assets_are_optional_and_absent_means_draw_it() {
        let theme = parse_theme_file(
            r#"{"colors":{}}"#,
            Mode::Light,
            &builtin(Mode::Light).colors,
        )
        .expect("a theme with no assets block is valid");
        for state in [
            PickerState::Default,
            PickerState::Hover,
            PickerState::Selected,
        ] {
            assert_eq!(theme.assets.picker(state), None);
        }
    }

    #[test]
    fn a_missing_theme_file_is_restored_and_an_edit_is_kept() {
        let root = std::env::temp_dir().join("plo-theme-restore");
        let _ = std::fs::remove_dir_all(&root);

        // Nothing on disk: the file is written.
        // Nothing on disk: BOTH files are written, not just the one in use.
        // `core.json` is the required one, so a theme directory whose contents
        // depend on what the machine happened to be set to that day is wrong.
        let (theme, notices) = load_builtin(&root, Mode::Dark);
        assert_eq!(theme.colors.background, Color32::from_rgb(0x19, 0x19, 0x19));
        let dir = root.join(BUILTIN_THEME_DIR);
        for file in ["core.json", "core-dark.json"] {
            assert!(
                dir.join(file).exists(),
                "{file} was not written, so a user opening the folder would not find it"
            );
        }
        assert_eq!(
            notices.iter().filter(|n| n.contains("Wrote")).count(),
            2,
            "expected a notice per file written, got {notices:?}"
        );
        assert!(
            !notices.iter().any(|n| n.contains("hard to read")),
            "the built-in themes failed their own contrast check: {notices:?}"
        );
        let path = dir.join("core.json");

        // A hand edit survives, because "immutable" means repaired, not
        // overwritten. A user who edits this is experimenting.
        //
        // The edit is deliberately an unreadable one — near-black background
        // under the inherited dark text — because that also pins the other half
        // of the contract: an edit that cannot be read is still honoured, and
        // the contrast check reports it rather than silently reverting it. A
        // theme that quietly undid your edit would be worse than one that
        // applied it and told you why it looks wrong.
        std::fs::write(&path, r##"{"colors":{"background":"#010203"}}"##).expect("edit");
        let (edited, notices) = load_builtin(&root, Mode::Light);
        assert_eq!(
            edited.colors.background,
            Color32::from_rgb(1, 2, 3),
            "an edited theme was overwritten on launch"
        );
        assert!(
            !notices
                .iter()
                .any(|n| n.contains("Wrote") || n.contains("restored")),
            "a readable, present file was rewritten: {notices:?}"
        );
        assert!(
            notices.iter().any(|n| n.contains("hard to read")),
            "an unreadable theme loaded without saying so: {notices:?}"
        );

        // A corrupt file is repaired, and says so.
        std::fs::write(&path, "{ not json").expect("corrupt");
        let (repaired, notices) = load_builtin(&root, Mode::Light);
        assert_eq!(
            repaired.colors.background,
            Color32::from_rgb(0xf3, 0xf3, 0xf3)
        );
        assert!(
            notices.iter().any(|notice| notice.contains("restored")),
            "repairing a broken theme said nothing: {notices:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A theme change has to reach the *widgets*, and stay there.
    ///
    /// This drives `apply` — the same call `sync_theme` makes — and reads a real
    /// button's resolved fill out of a frame, rather than checking that the mode
    /// changed or that `set_visuals` was called. Both of those passed while the
    /// window showed white text on white buttons, because the painted half reads
    /// the thread-local palette and only egui's widget half was stale.
    ///
    /// The sequence is the one that actually happens. egui re-reads its own
    /// `theme_preference` from the OS on every pass, so the slot it picks can
    /// change *after* `apply` has already written. The window is momentarily
    /// right — the palette is in the slot egui was using — and then the next
    /// pass switches slot and the widgets read one this app never wrote. So the
    /// test applies a theme, then lets egui switch slots, then reads a widget.
    #[test]
    fn a_theme_change_reaches_the_widgets_after_egui_switches_slots() {
        let ctx = egui::Context::default();
        let light = builtin(Mode::Light);

        // Applied while egui happens to be on the light slot.
        ctx.set_theme(egui::Theme::Light);
        apply(&ctx, &light);
        let while_on_light = fill_of_a_button(&ctx);
        assert_eq!(
            while_on_light,
            Some(inactive_widget_fill(&light.colors)),
            "the button did not take the light palette in the first place"
        );

        // Now egui re-reads the OS and picks the other slot. This is the whole
        // bug: the palette is only in the slot it happened to be written to.
        ctx.set_theme(egui::Theme::Dark);
        let after_switch = fill_of_a_button(&ctx);
        assert_eq!(
            after_switch, while_on_light,
            "the widget followed egui to its other theme slot, which the palette \
             was never written to"
        );
    }

    /// Both of egui's theme slots have to be filled, whichever one is active.
    ///
    /// The failure this holds is invisible until egui switches slots on its
    /// own, which is why it needs a test rather than a look at the window.
    #[test]
    fn both_egui_theme_slots_get_the_palette() {
        let ctx = egui::Context::default();
        for mode in [Mode::Light, Mode::Dark] {
            let theme = builtin(mode);
            apply(&ctx, &theme);
            for slot in [egui::Theme::Dark, egui::Theme::Light] {
                let stored = ctx.style_of(slot).visuals.widgets.noninteractive.bg_fill;
                assert_eq!(
                    stored, theme.colors.surface,
                    "the {slot:?} slot was not filled in {mode:?} mode"
                );
            }
        }
    }

    /// The fill a real widget resolves to, read out of a frame.
    ///
    /// A widget rather than a painted rect, because this is egui's own widget
    /// resolution and that is the thing that was broken; a test that read a
    /// colour this app painted would have passed throughout. A `ProgressBar`
    /// rather than a `Button` only because it draws no text, so the frame
    /// carries no texture deltas to dispose of.
    fn fill_of_a_button(ctx: &egui::Context) -> Option<Color32> {
        let mut seen = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.add(egui::ProgressBar::new(0.5));
            seen = Some(ui.visuals().widgets.inactive.bg_fill);
        });
        output.textures_delta.clear();
        seen
    }

    /// The fill `visuals_for` asks egui for an inactive widget.
    ///
    /// Written as the palette's own name rather than the literal, so a change to
    /// that mapping breaks these tests instead of quietly making them assert
    /// nothing.
    fn inactive_widget_fill(colors: &Palette) -> Color32 {
        colors.scrollbar
    }

    #[test]
    fn the_system_setting_chooses_the_mode() {
        assert_eq!(Mode::from_system(|| Some(0)), Mode::Dark);
        assert_eq!(Mode::from_system(|| Some(1)), Mode::Light);
        // A machine with no such key has been showing light all along.
        assert_eq!(Mode::from_system(|| None), Mode::Light);
    }

    #[test]
    fn the_preference_resolves_against_the_system() {
        assert_eq!(resolve(ThemeMode::System, Mode::Dark), Mode::Dark);
        assert_eq!(resolve(ThemeMode::System, Mode::Light), Mode::Light);
        // An explicit override is the only way the window and the native tray
        // menu can disagree, so it has to win.
        assert_eq!(resolve(ThemeMode::Light, Mode::Dark), Mode::Light);
        assert_eq!(resolve(ThemeMode::Dark, Mode::Light), Mode::Dark);
    }

    #[test]
    fn the_mode_really_switches_the_egui_base() {
        // The bug this guards: repainting fifteen colours onto the wrong base
        // gives a light background sitting on dark widget internals. Asserting
        // the base is genuinely different is the only way to notice.
        let light = builtin(Mode::Light);
        let dark = builtin(Mode::Dark);
        let light_visuals = visuals_for(&light, Mode::Light);
        let dark_visuals = visuals_for(&dark, Mode::Dark);
        assert!(!light_visuals.dark_mode);
        assert!(dark_visuals.dark_mode);
        assert_ne!(
            light_visuals.widgets.noninteractive.fg_stroke.color,
            Color32::WHITE
        );
    }
}
