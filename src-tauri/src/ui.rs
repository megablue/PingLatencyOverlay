use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ComboBox, Context, Frame, Grid, Layout, RichText, Ui, ViewportBuilder,
};
use eframe::{App, CreationContext, NativeOptions};

// Probing, rendering and the layered overlay windows live in the core crate so
// that a renderer process can be built from them without a GUI stack. Only
// This process is the configuration window and nothing else. The tray lives in
// its own process now, so there is no tray code here at all: menu choices
// arrive as pipe messages, and this window talks to the renderer over the
// named pipe rather than touching either of those types directly, which is the
// whole point — nothing in this binary can draw a graph.
use ping_latency_overlay_core::config::{
    self, Anchor, BorderEffect, Config, DisplayMode, OverlayConfig, ProbeConfig, StickyTarget,
    StickyZOrder, TargetConfig,
};
use ping_latency_overlay_core::monitors::{self, MonitorInfo};
use ping_latency_overlay_core::probes::{enabled_target_keys, TaskKey};
use ping_latency_overlay_core::rules::{
    self, AutoRules, Combine, Condition, MatchMode, Part, Rule, Scope, Snapshot, WindowInfo,
};
use ping_latency_overlay_core::transport::{Client, Message};
use ping_latency_overlay_core::winwatch::{self, WindowShape};

use crate::theme::{self, windows_app_mode, Mode, Theme, ThemeMode};

const SIDEBAR_WIDTH: f32 = 270.0;
/// Width a scroll bar's contents may use, the difference between the list
/// pane's allocation and the space its rows get. Reserving it keeps a row from
/// jumping sideways when a list outgrows the pane and a scroll bar appears.
const SCROLL_BAR_RESERVE: f32 = 8.0;
/// Height of pane 2's footer, which holds only the Overlays page's own actions.
/// Save and Discard live in the detail pane's footer, so this no longer covers
/// them.
const SIDEBAR_FOOTER_HEIGHT: f32 = 80.0;
/// Height of the profile switcher that heads pane 2.
const SIDEBAR_HEADER_HEIGHT: f32 = 40.0;
/// Height of the detail pane's sticky footer, which holds Save and Discard. It
/// mirrors pane 2's footer: both sit under a scrolling list, and keeping the
/// draft actions next to the thing they act on beats parking them in the status
/// bar a pane away.
const DETAIL_FOOTER_HEIGHT: f32 = 44.0;
const DETAIL_FOOTER_BUTTON_HEIGHT: f32 = 32.0;
const DETAIL_FOOTER_BUTTON_WIDTH: f32 = 96.0;
/// The pick button matches the footer's height and colours; it is wider
/// because the painted crosshair and the label both have to fit inside it.
const PICK_BUTTON_WIDTH: f32 = 128.0;
/// The pick crosshair's box, as a fraction of the button's height, and its
/// inset from the button's left edge in pixels.
const PICK_ICON_BOX: f32 = 0.5;
const PICK_ICON_INSET: f32 = 10.0;
/// The pick crosshair's geometry, all as fractions of its box: the ring's
/// stroke weight and radius, and how far the four ticks reach from and to.
const PICK_ICON_STROKE: f32 = 0.1;
const PICK_ICON_RADIUS: f32 = 0.24;
const PICK_ICON_TICK_INNER: f32 = 0.32;
const PICK_ICON_TICK_OUTER: f32 = 0.44;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const PROFILE_ROW_HEIGHT: f32 = 26.0;
const PROFILE_ACTION_WIDTH: f32 = 200.0;
/// Smallest width a read-only path on the Global page may shrink to.
const STORAGE_PATH_MIN: f32 = 120.0;
/// The Global rail glyph: `(vertical offset, knob position)`, both as a
/// fraction of the glyph's unit. The offsets sum to zero so the glyph is
/// centred, and the knob positions keep every knob inside its own track.
const GLOBAL_ICON_ROWS: [(f32, f32); 3] = [(-0.28, 0.55), (0.0, -0.55), (0.28, 0.05)];
/// Half the length of a Global glyph track, as a fraction of the unit.
const GLOBAL_ICON_TRACK_HALF: f32 = 0.35;
/// Radius of a Global glyph knob, as a fraction of the unit.
const GLOBAL_ICON_KNOB_RADIUS: f32 = 0.1;
/// The About rail glyph: `(vertical offset, half width)`, both as a fraction of
/// the glyph's unit.
///
/// A lower case `i` reads better than a piece of text at 20px, so it is drawn as
/// a dot over a stem. The offsets sum to zero and the widest part plus the stem
/// radius stays inside half a unit, which is the same pair of invariants the
/// Global glyph is held to.
const ABOUT_ICON_ROWS: [(f32, f32); 2] = [(-0.2, 0.0), (0.16, 0.16)];
/// Radius of the About glyph's dot, as a fraction of the unit.
const ABOUT_ICON_DOT_RADIUS: f32 = 0.12;
/// The theme tiles' mode glyphs, as fractions of the icon box. Fractions of a
/// unit rather than pixels so the glyph scales with the tile, and named so
/// `the_theme_glyphs_stay_inside_their_boxes` can hold the drawn extent — ray
/// or outline included — to half a box either way.
const THEME_ICON_STROKE: f32 = 0.06;
const THEME_SUN_CORE_RADIUS: f32 = 0.17;
const THEME_SUN_RAY_INNER: f32 = 0.28;
const THEME_SUN_RAY_OUTER: f32 = 0.45;
/// The sun's eight rays as unit vectors, so the painter and the balance check
/// read the same table.
const THEME_SUN_RAYS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    (0.0, 1.0),
    (
        -std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    (-1.0, 0.0),
    (
        -std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
    (0.0, -1.0),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
];
const THEME_MOON_RADIUS: f32 = 0.42;
const THEME_MOON_BITE_RADIUS: f32 = 0.33;
/// The moon's bite sits up and to the right, the way the reference does.
const THEME_MOON_BITE_OFFSET: (f32, f32) = (0.104, -0.104);
const THEME_SYSTEM_RADIUS: f32 = 0.44;
/// Segments the System glyph's filled semicircle is drawn with.
const THEME_SYSTEM_ARC_STEPS: usize = 16;
/// The About page's repository. This is the one line that opens anything: it is
/// the only link, and clicking it hands the address to Windows.
const ABOUT_REPOSITORY: &str = "https://github.com/megablue/PingLatencyOverlay";
/// Edge length of the app icon shown above the About page's text.
const ABOUT_LOGO_SIZE: f32 = 128.0;
/// Space above the About page's centred column, so it does not sit hard against
/// the top of the pane.
const ABOUT_TOP_GAP: f32 = 12.0;
/// Extra air before a new group of lines, so the name, tagline and version read
/// as one block and the link and copyright read as another.
const ABOUT_GROUP_GAP: f32 = 10.0;

/// The app version as the About page shows it, in the `v0.1.x` form.
///
/// One place formats it, so the page and the optional window title cannot
/// disagree about what the app is called.
/// The body text size, so the About page's sizes are relative to it rather than
/// hard-coded. Read from the style so a theme change cannot leave the page
/// shouting in a size the rest of the window has moved on from.
fn ui_text_size() -> f32 {
    egui::TextStyle::Body.resolve(&egui::Style::default()).size
}

fn app_version() -> String {
    format!("v{}", env!("APP_BUILD_VERSION"))
}

/// Where an About-page line opens when it is clicked, if it does at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AboutKind {
    /// Plain text, not clickable.
    Text,
    /// Opens `url` in the default browser when clicked.
    Link(&'static str),
}

/// One centred line of the About page.
///
/// The size is carried here rather than at the call site so a test can hold a
/// floor over the whole page. The page was criticised for using `.small()`,
/// and "do not use very small text here" is worth more as a rule in the suite
/// than as a preference in someone's head.
#[derive(Clone, Debug, PartialEq)]
struct AboutLine {
    text: String,
    size: f32,
    kind: AboutKind,
    /// Whether this line starts a new group and so takes the larger gap above
    /// it. Carried per line rather than decided by index, so inserting or
    /// removing a line cannot quietly move the wrong break.
    group_start: bool,
}

/// The text of every line on the About page, in order.
///
/// The page used to be a labelled table with accent section headers, which read
/// as settings rather than as an About box. It is one centred column now. The
/// facts are the ones the page has always carried: what this is, which version,
/// where the source is, who wrote it, under what licence. The copyright is
/// `APP_COPYRIGHT` from the root manifest, the same string the exes carry as
/// `LegalCopyright`, and it carries no year on purpose, so nothing here can go
/// stale between releases.
///
/// Only the repository is clickable. The licence is named in words and not
/// linked, because the text is not bundled with the app and a link to someone
/// else's copy of it invites the reader to trust that copy.
fn about_page_lines() -> Vec<AboutLine> {
    let body = ui_text_size();
    let line = |text: &str, size: f32, kind: AboutKind, group_start: bool| AboutLine {
        text: text.to_string(),
        size,
        kind,
        group_start,
    };
    vec![
        line("PingLatencyOverlay", body + 12.0, AboutKind::Text, false),
        line(
            "A small overlay that shows live network latency.",
            body,
            AboutKind::Text,
            false,
        ),
        line(&app_version(), body + 4.0, AboutKind::Text, false),
        line(
            "Github: github.com/megablue/PingLatencyOverlay",
            body,
            AboutKind::Link(ABOUT_REPOSITORY),
            true,
        ),
        line(env!("APP_COPYRIGHT"), body, AboutKind::Text, true),
        line(
            "LICENSE: GNU General Public License v3.0",
            body,
            AboutKind::Text,
            false,
        ),
    ]
}

/// Draws the About page's centred column.
///
/// The centring uses `ui.with_layout(Layout::top_down(Align::Center), ..)`
/// and NOT `ui.vertical_centered(..)`. `with_layout` re-lays-out the *existing*
/// `Ui` and creates no child, so there is no child min rect to overshoot and no
/// 18px `interact_size.y` band. `vertical_centered(..)` is a `scope_builder`
/// child, which is the exact shape that made the detail footer report 22px
/// more than its box and slide into the status bar. `show_detail_footer` uses
/// `with_layout` with two children and is correct, so the counter-example is
/// right here in the file.
///
/// For `Layout::top_down` the main axis is vertical, so `Align::Center` is the
/// cross axis and centres each line horizontally, which is what a centred
/// column needs.
fn about_page_column(ui: &mut Ui, logo: &egui::TextureHandle) {
    ui.add_space(ABOUT_TOP_GAP);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        {
            // `fit_to_exact_size` is what scales a texture down; without it the
            // image is drawn at its own 256px, which is larger than this pane
            // has to give.
            ui.add(
                egui::Image::new(logo)
                    .fit_to_exact_size(egui::vec2(ABOUT_LOGO_SIZE, ABOUT_LOGO_SIZE)),
            );
            ui.add_space(ABOUT_GROUP_GAP);
        }
        for (index, line) in about_page_lines().iter().enumerate() {
            if line.group_start {
                // More air between groups than between lines within one, so the
                // name, tagline and version read as a block of their own.
                ui.add_space(ABOUT_GROUP_GAP);
            } else if index > 0 {
                ui.add_space(2.0);
            }
            let font = egui::FontId::proportional(line.size);
            match line.kind {
                AboutKind::Text => {
                    let colour = if index == 1 {
                        UI_TEXT_SECONDARY()
                    } else {
                        UI_TEXT()
                    };
                    ui.label(RichText::new(&line.text).font(font).color(colour));
                }
                AboutKind::Link(url) => {
                    // A `Hyperlink` only emits `OutputCommand::OpenUrl`, and
                    // eframe's native runner ignores it, so the click is
                    // handled by `open_requested_urls` at the end of the frame.
                    ui.add(egui::Hyperlink::from_label_and_url(
                        RichText::new(&line.text).font(font),
                        url,
                    ));
                }
            }
        }
    });
}
/// The address an egui output command wants opened, if it wants one opened.
///
/// `egui::Hyperlink` does not open anything itself. On a click it emits
/// `OutputCommand::OpenUrl` and leaves it to the host, and eframe only honours
/// that command in its **web** runner: `src/native/*.rs` in eframe 0.36.2
/// contains no `OutputCommand` handling at all, so on a native glow window the
/// command is dropped and a link is inert. That is why the app drains the
/// command itself.
///
/// Pure, so a test can hold the mapping without opening a browser.
fn requested_url(command: &egui::OutputCommand) -> Option<&str> {
    match command {
        egui::OutputCommand::OpenUrl(open) => Some(open.url.as_str()),
        _ => None,
    }
}

/// Hands a URL to Windows, which routes it to whatever the user has set as
/// their default handler for `https`.
///
/// `ShellExecuteW` is that API. Returns `false` only when the call reports
/// failure, in which case the caller says so in the status bar: a link that
/// silently does nothing is exactly the bug this replaced.
fn open_url_in_browser(url: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // `ShellExecuteW` takes LPCWSTR, so the URL needs a NUL-terminated UTF-16
    // buffer, not a Rust `&str`.
    let wide: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call, and every pointer
    // argument is either null or a valid pointer into it. The window handle is
    // null because there is no parent window to associate the launched process
    // with, which is the documented way to hand a document to the shell.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a value greater than 32 on success; anything at or
    // below that is one of the documented error codes. `HINSTANCE` is a pointer
    // in windows-sys, so it is compared as an integer.
    result as isize > 32
}

/// Decode the bundled artwork for this window's title bar, taskbar and About
/// page.
///
/// The bytes come from `core::icon_png()`, the single `include_bytes!` in the
/// build, so the tray and this window share one copy of the artwork and one
/// place that names it. This used to be `tray::app_icon`, which was the reason
/// the window could not be split out of the tray's process: the helper
/// returned an `egui` type, so anything wanting the icon inherited eframe.
pub fn app_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(ping_latency_overlay_core::icon_png())
        .expect("bundled icon must be valid")
        .to_rgba8();
    let width = image.width();
    let height = image.height();
    eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

/// Decodes the bundled app icon into a texture for the About page.
///
/// `IconData` is already raw RGBA in row order, which is exactly what
/// `ColorImage::from_rgba_unmultiplied` wants.
fn about_logo_texture(ctx: &Context) -> egui::TextureHandle {
    let icon = app_icon();
    let size = [icon.width as usize, icon.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
    ctx.load_texture("about_logo", image, egui::TextureOptions::LINEAR)
}

/// Space kept free at the right of a profile row for its overlay count and the
/// active dot, so a long name truncates instead of running under them. The
/// gutter is two columns: the count, right aligned, and the dot's slot.
const PROFILE_ROW_TRAILING: f32 = 52.0;
/// Width of the active profile's dot at the right of a profile row.
const PROFILE_ROW_DOT_WIDTH: f32 = 12.0;
/// Gap between a profile row's overlay count and its dot.
const PROFILE_ROW_COUNT_GAP: f32 = 6.0;
/// Inner margin of a row in the list pane, which also comes out of its width.
/// Margin between a list pane row's fill and its contents, on every side.
///
/// Every list pane row uses this one margin, and its contents are laid out in
/// `row_inner(row)`, so a row cannot be painted at one size and filled at
/// another.
const ROW_MARGIN: f32 = 4.0;
/// Height of the area an overlay row's contents sit in.
const OVERLAY_ROW_HEIGHT: f32 = 28.0;
/// Width of an overlay row's delete and pause buttons.
const OVERLAY_ROW_ACTION_WIDTH: f32 = 28.0;

/// A host row in the detail pane: shorter than an overlay row, because it is
/// inside a pane that already scrolls vertically.
const TARGET_ROW_HEIGHT: f32 = 24.0;
const TARGET_ROW_ACTION_WIDTH: f32 = 24.0;
const TARGET_ROW_SWATCH: f32 = 10.0;
const TARGET_ROW_SWATCH_GAP: f32 = 6.0;
/// Height of an overlay row's delete confirmation buttons.
const OVERLAY_CONFIRM_HEIGHT: f32 = 26.0;
/// Width of the delete confirmation's OK and Cancel buttons.
const OVERLAY_CONFIRM_OK_WIDTH: f32 = 36.0;
const OVERLAY_CONFIRM_CANCEL_WIDTH: f32 = 28.0;
/// Padding inside the profile switcher, around its name and its arrow.
const SWITCHER_TEXT_LEFT_PAD: f32 = 10.0;
const SWITCHER_ARROW_PAD: f32 = 12.0;
const SWITCHER_TEXT_RIGHT_PAD: f32 = 28.0;
/// Padding at each side of the list pane's content, so its rows do not touch
/// the pane edges.
const LIST_PANE_INSET: f32 = 8.0;
/// Navigation rail widths: labelled, then icon only.
const RAIL_WIDTH: f32 = 148.0;
const RAIL_COLLAPSED_WIDTH: f32 = 44.0;
const RAIL_ROW_HEIGHT: f32 = 40.0;
const RAIL_ICON_SIZE: f32 = 20.0;
const RAIL_TEXT_SIZE: f32 = 14.0;
/// Theme-picker tiles: three share the detail pane's width, so a tile is
/// `(available - 2*gap)/3`, capped so a wide window gets buttons rather than
/// billboards.
const THEME_TILE_GAP: f32 = 10.0;
const THEME_TILE_MAX: f32 = 112.0;
/// The tile's icon box, its centre and the label's centre, as fractions of the
/// tile.
const THEME_TILE_ICON_SIZE: f32 = 0.42;
const THEME_TILE_ICON_CENTER_Y: f32 = 0.36;
const THEME_TILE_LABEL_CENTER_Y: f32 = 0.8;
/// The label shrinks with the tile rather than overflowing it.
const THEME_TILE_LABEL_FRACTION: f32 = 0.115;
const THEME_TILE_LABEL_SIZE_MIN: f32 = 10.0;
const THEME_TILE_LABEL_SIZE_MAX: f32 = 12.5;
/// Gap between two panes.
const PANE_GAP: f32 = 8.0;
/// Horizontal padding the central frame puts around the panes.
const PANE_MARGIN: f32 = 12.0;
const WINDOW_WIDTH: f32 = 860.0;
const WINDOW_HEIGHT: f32 = 660.0;
const WINDOW_MIN_WIDTH: f32 = 720.0;
const WINDOW_MIN_HEIGHT: f32 = 480.0;
const WINDOW_MAX_WIDTH: f32 = 1400.0;
const WINDOW_MAX_HEIGHT: f32 = 8192.0;
/// Stable widget id for the profile name field, so it keeps keyboard focus.
const PROFILE_NAME_FIELD_ID: &str = "profile-name-field";
/// Fixed popup width. The popup must never derive its width from
/// `available_width()`, because `Area` stores the content size and re-lays the
/// content out at it next frame, so any overflow feeds back and grows forever.
const PROFILE_POPUP_WIDTH: f32 = SIDEBAR_WIDTH - 40.0;
const REPAINT_INTERVAL: Duration = Duration::from_millis(100);

// The palette in force for the frame being drawn.
//
// These used to be `const`s, and there are 119 uses of them across this file.
// Threading a `&Palette` through every one of those call sites would be a large
// refactor whose diff would bury the actual change, and the alternative — a
// palette baked into each paint call — is the thing theming is for. So the
// palette is held once and read from here.
//
// A thread-local rather than a field on `PingApp` because most of the 119 are in
// free functions that already take a `&Ui` or a `&Painter` and have no way to
// reach the app. That is a real trade: the palette is global state, so a test
// that needs a particular palette has to set it, and a second UI thread would
// not see the same one. The egui UI is single-threaded and the palette does not
// change within a frame, so neither bites today.
thread_local! {
    static CURRENT_PALETTE: std::cell::RefCell<theme::Palette> =
        std::cell::RefCell::new(theme::builtin(Mode::Dark).colors);
}

/// Install the palette for subsequent frames.
fn set_palette(palette: theme::Palette) {
    CURRENT_PALETTE.with(|current| *current.borrow_mut() = palette);
}

/// The palette in force right now.
fn palette() -> theme::Palette {
    CURRENT_PALETTE.with(|current| *current.borrow())
}

macro_rules! palette_accessors {
    ($($name:ident => $field:ident),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($field), "` colour of the current theme.")]
            // The names stay the SCREAMING_CASE they were as constants, so the
            // diff that made them functions is a rename to `()` and nothing
            // else. A mechanical rename across 119 call sites is the kind of
            // thing to be able to audit by eye.
            #[allow(non_snake_case)]
            fn $name() -> Color32 {
                palette().$field
            }
        )*
    };
}

// Only the colours something in this file actually paints. `input_background`,
// `scrollbar` and `scrollbar_hover` are palette entries used inside
// `theme::visuals` and nowhere else, so they have no accessor here.
palette_accessors! {
    UI_BACKGROUND => background,
    UI_SURFACE => surface,
    UI_SURFACE_ALT => surface_alt,
    UI_SURFACE_HOVER => surface_hover,
    UI_BORDER => border,
    UI_TEXT => text,
    UI_TEXT_SECONDARY => text_secondary,
    UI_ACCENT => accent,
    UI_ACCENT_STRONG => accent_strong,
    UI_SELECTION => selection,
    UI_DANGER => danger,
    UI_DANGER_STRONG => danger_strong,
}

/// How often the attached-display list is re-read.
///
/// Long enough to keep `EnumDisplayMonitors` off the frame path, short enough
/// that plugging a monitor in while the window is open still puts it in the
/// list.
const MONITOR_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

const POSITION_PICKER_SIZE: f32 = 220.0;
const POSITION_PICKER_DISPLAY_SIZE: f32 = 180.0;
const POSITION_PICKER_PADDING: f32 = 4.0;
const POSITION_PICKER_CELL_SIZE: f32 = 40.0;
const POSITION_PICKER_GAP: f32 = 46.0;

struct PositionPicker {
    default_texture: egui::TextureHandle,
    hover_texture: egui::TextureHandle,
    selected_texture: egui::TextureHandle,
    /// Artwork the current theme supplies, per state, when it supplies any.
    ///
    /// A theme's `assets.positionPicker` is optional per state, so this is a
    /// `HashMap` rather than a fixed set: the built-in images are the fallback
    /// for every state a theme says nothing about. The key is the full path,
    /// so a theme change loads new files rather than reusing the old ones.
    themed_textures: HashMap<std::path::PathBuf, egui::TextureHandle>,
}

/// One entry in the monitor picker.
///
/// `device` is what gets stored: `None` means "follow the primary monitor",
/// which is the default and is therefore a real choice rather than the absence
/// of one. A pinned device that is not attached also gets an entry, because the
/// alternative is an overlay that is hidden with nothing on screen to say so.
struct MonitorChoice {
    device: Option<String>,
    label: String,
    hint: String,
}

impl MonitorChoice {
    fn is(&self, device: Option<&str>) -> bool {
        match (&self.device, device) {
            (None, None) => true,
            (Some(mine), Some(theirs)) => mine.eq_ignore_ascii_case(theirs),
            _ => false,
        }
    }
}

/// The primary-monitor entry, which is also what "follow the primary" reads as.
fn primary_choice() -> MonitorChoice {
    MonitorChoice {
        device: None,
        label: "Primary monitor (follow automatically)".to_string(),
        hint: "The overlay moves to whichever monitor Windows is calling primary. \
               This is what an overlay created before monitor selection existed does."
            .to_string(),
    }
}

/// How one attached display is named in the picker.
///
/// The device name alone is not checkable by eye, and the picker's whole job is
/// letting a user confirm the monitor they meant — especially after a port
/// change, where the name survives and the panel does not. So the label carries
/// the resolution, the scaling and where it sits relative to the primary, and
/// the name comes last where it is least likely to be read as the whole story.
fn monitor_label(monitor: &MonitorInfo, primary: Option<&MonitorInfo>) -> String {
    let resolution = format!("{} × {}", monitor.bounds.width(), monitor.bounds.height());
    let scaling = format!("{}%", monitors::scale_percent(monitor));
    let where_it_is = primary
        .filter(|primary| !std::ptr::eq(*primary, monitor))
        .map_or_else(String::new, |primary| {
            format!(" · {}", monitors::placement(monitor, primary))
        });
    format!("{resolution} ({scaling}){where_it_is} · {}", monitor.device)
}

/// Every entry the picker offers, in the order it offers them.
///
/// The primary monitor is not in this list as a device: "follow the primary" is
/// the entry for it, and offering both would be two ways of saying one thing
/// where one of them stops working when the primary changes.
///
/// A pinned device that is not attached is included, and first, so a hidden
/// overlay is visibly a hidden overlay. Dropping it would make the picker
/// silently disagree with what the profile says — a list that does not contain
/// the selected value has no way to show it, so the combobox would fall back to
/// its first row and read as though the user had chosen something else.
fn monitor_choices(monitors: &[MonitorInfo], selected: Option<&str>) -> Vec<MonitorChoice> {
    let primary = monitors::primary(monitors);
    let mut choices: Vec<MonitorChoice> = Vec::new();
    let selected_device = selected.map(str::trim).filter(|name| !name.is_empty());
    if let Some(device) = selected_device {
        if !monitors
            .iter()
            .any(|monitor| monitor.device.eq_ignore_ascii_case(device))
        {
            choices.push(MonitorChoice {
                device: Some(device.to_string()),
                label: format!("{device} — not connected"),
                hint: "This overlay is hidden while that display is not attached. \
                       It comes back where you left it when the display returns."
                    .to_string(),
            });
        }
    }
    choices.push(primary_choice());
    for monitor in monitors {
        if monitor.primary {
            continue;
        }
        choices.push(MonitorChoice {
            device: Some(monitor.device.clone()),
            label: monitor_label(monitor, primary),
            hint: format!(
                "Pins this overlay to {}. The scale and size are read from that \
                 display, not from the primary one.",
                monitor.device
            ),
        });
    }
    choices
}

/// The text the closed combobox shows, which is the selected entry's label.
///
/// Falls back to the first entry when the selected value is in none of them,
/// so the control can never show a blank or a value the user cannot find when
/// they open it.
fn monitor_choice_label(monitors: &[MonitorInfo], selected: Option<&str>) -> String {
    let choices = monitor_choices(monitors, selected);
    choices
        .iter()
        .find(|choice| choice.is(selected))
        .or_else(|| choices.first())
        .map(|choice| choice.label.clone())
        .unwrap_or_else(|| "Primary monitor (follow automatically)".to_string())
}

impl PositionPicker {
    fn new(ctx: &Context) -> Self {
        Self {
            default_texture: load_position_texture(
                ctx,
                "position-picker-default",
                include_bytes!("../../assets/overlay-position-ui-default.png"),
            ),
            hover_texture: load_position_texture(
                ctx,
                "position-picker-hover",
                include_bytes!("../../assets/overlay-position-ui-hover.png"),
            ),
            selected_texture: load_position_texture(
                ctx,
                "position-picker-selected",
                include_bytes!("../../assets/overlay-position-ui-selected.png"),
            ),
            themed_textures: HashMap::new(),
        }
    }

    /// The artwork to draw one state with: the theme's if it named a file that
    /// loads, the built-in otherwise.
    ///
    /// Two independent fallbacks, and both matter. A file that does not exist or
    /// does not decode is not worth failing a frame over — the built-in is a
    /// perfectly good picture — and a state the theme says nothing about is the
    /// common case, since `assets` is optional per state.
    fn texture_for(
        &mut self,
        ctx: &Context,
        theme: &Theme,
        state: theme::PickerState,
        fallback: &egui::TextureHandle,
    ) -> egui::TextureHandle {
        let Some(relative) = theme.assets.picker(state) else {
            return fallback.clone();
        };
        let path = config::themes_dir()
            .join(theme::BUILTIN_THEME_DIR)
            .join(relative);
        if let Some(texture) = self.themed_textures.get(&path) {
            return texture.clone();
        }
        let Ok(bytes) = std::fs::read(&path) else {
            log_line(
                "config",
                &format!(
                    "The theme names {} but it could not be read, so the built-in image is used.",
                    path.display()
                ),
            );
            return fallback.clone();
        };
        let name = format!("position-picker-{}", state as usize);
        let texture = load_position_texture(ctx, &name, &bytes);
        self.themed_textures.insert(path, texture.clone());
        texture
    }

    fn show(&mut self, ui: &mut Ui, current: Anchor, theme: &Theme) -> Option<Anchor> {
        let display_size = ui
            .available_width()
            .clamp(1.0, POSITION_PICKER_DISPLAY_SIZE);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(display_size, display_size), egui::Sense::click());
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        // The fallbacks are cloned out first: `texture_for` needs `&mut self`
        // to cache, and reading the built-in handle straight out of `self` while
        // that borrow is live is the conflict.
        let fallback = self.default_texture.clone();
        let default_texture =
            self.texture_for(ui.ctx(), theme, theme::PickerState::Default, &fallback);
        ui.painter()
            .image(default_texture.id(), rect, full_uv, Color32::WHITE);

        let hovered = response
            .hover_pos()
            .and_then(|point| position_cell_at(rect, point));
        if let Some(index) = hovered {
            let cell_rect = position_cell_rect(rect, index);
            let fallback = self.hover_texture.clone();
            let hover_texture =
                self.texture_for(ui.ctx(), theme, theme::PickerState::Hover, &fallback);
            ui.painter().image(
                hover_texture.id(),
                cell_rect,
                position_cell_uv(index),
                Color32::WHITE,
            );
        }

        let selected = position_index(current);
        let selected_rect = position_cell_rect(rect, selected);
        let fallback = self.selected_texture.clone();
        let selected_texture =
            self.texture_for(ui.ctx(), theme, theme::PickerState::Selected, &fallback);
        ui.painter().image(
            selected_texture.id(),
            selected_rect,
            position_cell_uv(selected),
            Color32::WHITE,
        );
        ui.painter().rect_stroke(
            selected_rect,
            8.0,
            egui::Stroke::new(1.5, UI_ACCENT()),
            egui::StrokeKind::Inside,
        );

        let clicked_anchor = response
            .clicked()
            .then(|| response.hover_pos())
            .flatten()
            .and_then(|point| position_cell_at(rect, point))
            .map(position_anchor);
        if let Some(index) = hovered {
            response.on_hover_text_at_pointer(position_name(position_anchor(index)));
        }
        clicked_anchor
    }
}

/// How often the crosshair re-reads the window list while it is armed.
///
/// The list costs a Toolhelp sweep and an `EnumWindows`, so it is refreshed on
/// a clock rather than on every pass; the cursor and the button are polled
/// every pass, which is free by comparison.
const PICK_CANDIDATE_REFRESH: Duration = Duration::from_millis(250);

/// The crosshair that fills the sticky target's boxes from a real window.
///
/// Pointing at a window without a global mouse hook: while a pick is armed
/// this window holds the mouse capture, so the click that ends the pick lands
/// here instead of in whatever is under the cursor, and every pass polls the
/// cursor and the button directly rather than waiting for this window's own
/// events — which is what makes it work over another process's windows.
///
/// The candidates are exactly the windows [`winwatch::shapes`] reports, which
/// is also exactly what the renderer's matcher will see, so a window that
/// cannot be picked could not have been followed either.
#[derive(Default)]
struct StickyPicker {
    /// The window holding the capture while a pick is in progress.
    capture: Option<windows_sys::Win32::Foundation::HWND>,
    /// Where the window sat before the pick moved it out of the way; `Some`
    /// exactly while it is displaced.
    saved_rect: Option<windows_sys::Win32::Foundation::RECT>,
    /// Whether the click that started the pick has been released yet; the
    /// next press is the one that picks.
    armed: bool,
    /// A press has been seen, so the next release commits the pick.
    held: bool,
    /// The candidate under the cursor on the last pass.
    hover: Option<WindowShape>,
    /// The candidate list, refreshed on `PICK_CANDIDATE_REFRESH`.
    candidates: Vec<WindowShape>,
    candidates_read_at: Option<Instant>,
}

impl StickyPicker {
    /// Draws the pick button, or the running hint, and returns a window when
    /// the user completed a pick on this pass.
    fn show(&mut self, ui: &mut Ui) -> Option<WindowShape> {
        if self.capture.is_none() {
            let button = ui
                .add_sized(
                    [PICK_BUTTON_WIDTH, DETAIL_FOOTER_BUTTON_HEIGHT],
                    egui::Button::new(RichText::new("Pick window").color(UI_TEXT()))
                        .fill(UI_ACCENT_STRONG()),
                )
                .on_hover_text(
                    "Point at a window and click, like a screen spy. Escape \
                     cancels, and the click is swallowed so it never reaches \
                     the window you are pointing at.",
                );
            draw_crosshair_icon(ui.painter(), pick_icon_rect(button.rect), UI_TEXT());
            if button.clicked() {
                self.start();
                ui.ctx().request_repaint();
            }
            return None;
        }

        self.poll(ui)
    }

    fn start(&mut self) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture;

        // The click that got here focused this window, so the capture belongs
        // to it. It is checked rather than trusted, and the fallback is our own
        // window: capturing a foreign window would send the pick's clicks
        // there instead of swallowing them.
        let Some(hwnd) = own_capture_window() else {
            return;
        };
        unsafe { SetCapture(hwnd) };
        self.capture = Some(hwnd);
        self.saved_rect = displace(hwnd);
        self.armed = false;
        self.held = false;
        self.hover = None;
        self.candidates_read_at = None;
    }

    fn release(&mut self) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;

        unsafe { ReleaseCapture() };
        if let (Some(hwnd), Some(rect)) = (self.capture, self.saved_rect.take()) {
            restore_displaced(hwnd, rect);
        }
        self.capture = None;
        self.armed = false;
        self.held = false;
        self.hover = None;
    }

    /// One pass of the crosshair: the candidate under the cursor and the
    /// button state, both read from Win32 rather than from egui's events.
    fn poll(&mut self, ui: &mut Ui) -> Option<WindowShape> {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, SetCapture, VK_ESCAPE, VK_LBUTTON,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetCursorPos, GetWindowRect, WindowFromPoint, GA_ROOT,
        };

        let capture = self.capture.expect("polled only while picking");
        // Escape is the only cancel. A lost capture is not one: winit releases
        // the capture on every button-up, so the release that ends a pick always
        // arrives with the capture already gone, and treating that as a cancel
        // is what made the boxes impossible to fill.
        if unsafe { GetAsyncKeyState(VK_ESCAPE as i32) } < 0 {
            self.release();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            return None;
        }

        if self
            .candidates_read_at
            .is_none_or(|read_at| read_at.elapsed() >= PICK_CANDIDATE_REFRESH)
        {
            self.candidates = pickable_windows();
            self.candidates_read_at = Some(Instant::now());
        }

        let mut point = POINT { x: 0, y: 0 };
        self.hover = (unsafe { GetCursorPos(&mut point) } != 0)
            .then(|| unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) })
            .and_then(|root| {
                self.candidates
                    .iter()
                    .find(|shape| shape.hwnd == root)
                    .cloned()
            });

        // The crosshair belongs to the Config window: the moment the pointer
        // leaves it the ordinary arrow comes back. While the window is
        // displaced for a pick there is nothing to leave, so the crosshair
        // stays — it is the only feedback that a pick is running.
        let mut window_rect = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let inside = unsafe { GetWindowRect(capture, &mut window_rect) } != 0
            && point.x >= window_rect.left
            && point.x < window_rect.right
            && point.y >= window_rect.top
            && point.y < window_rect.bottom;
        ui.ctx()
            .set_cursor_icon(pick_cursor(self.saved_rect.is_some(), inside));

        // The edges come before the capture is repaired, because a release is
        // also the moment winit takes the capture away.
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } < 0;
        if pick_step(&mut self.armed, &mut self.held, down) {
            let picked = self.hover.take();
            self.release();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            ui.ctx().request_repaint();
            return picked;
        }

        // While the button is up nothing else wants the capture, so taking it
        // back every pass keeps the next press ours to swallow. The press that
        // is down right now was already ours: the capture was held when it went
        // down, and the only drop is the up that commits.
        unsafe { SetCapture(capture) };
        ui.ctx().request_repaint();
        None
    }
}

/// Advances the pick's button tracking by one pass and reports whether this
/// pass completes a pick.
///
/// A free function over two flags, not a method, so the whole sequence — start,
/// release, press, release — is testable without Win32 and without a capture.
/// The capture is deliberately not part of the decision: winit releases it on
/// every button-up, so a pick that required it to still be held could never
/// commit.
fn pick_step(armed: &mut bool, held: &mut bool, down: bool) -> bool {
    if !*armed {
        // The click that started the pick has to be released first, or its own
        // press would be the one that picks.
        if !down {
            *armed = true;
        }
        false
    } else if down {
        *held = true;
        false
    } else if *held {
        *held = false;
        true
    } else {
        false
    }
}

/// The cursor for a running pick: the crosshair while the window is displaced
/// (there is no rect to leave, and it is the only feedback a pick is running),
/// and otherwise the crosshair only while the pointer is inside the window.
fn pick_cursor(displaced: bool, inside: bool) -> egui::CursorIcon {
    if displaced || inside {
        egui::CursorIcon::Crosshair
    } else {
        egui::CursorIcon::Default
    }
}

/// Moves the Config window out of the way for a pick, returning where it was.
///
/// The window is moved rather than hidden. Hiding it — which is what shipped
/// first — costs the pick three things at once: Windows hands the foreground
/// back to the previous window, the hidden window loses the mouse capture, and
/// a hidden window stops receiving frames, so the polling that drives the pick
/// sleeps. The first click then only focuses the window under the cursor and
/// the second one picks.
fn displace(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<windows_sys::Win32::Foundation::RECT> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER,
    };

    let mut rect = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            -32000,
            -32000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
        )
    };
    Some(rect)
}

/// Puts the Config window back where [`displace`] found it.
fn restore_displaced(
    hwnd: windows_sys::Win32::Foundation::HWND,
    rect: windows_sys::Win32::Foundation::RECT,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER,
    };

    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            rect.left,
            rect.top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
        )
    };
}

/// The window a pick captures with: the foreground window when it belongs to
/// this process, otherwise this process's first visible top-level window.
///
/// `GetForegroundWindow` is normally the Config window by the time a pick
/// starts — the click that starts it focused the window — but it is checked
/// rather than trusted, because capturing a window of another process would
/// send the pick's clicks there instead of swallowing them.
fn own_capture_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    };

    let own_pid = std::process::id();
    let belongs_to_us = |hwnd: HWND| {
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid == own_pid
    };

    let foreground = unsafe { GetForegroundWindow() };
    if belongs_to_us(foreground) {
        return Some(foreground);
    }

    unsafe extern "system" fn first_visible(hwnd: HWND, data: isize) -> i32 {
        let found = &mut *(data as *mut Option<HWND>);
        if found.is_some() {
            return 0;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id() && IsWindowVisible(hwnd) != 0 {
            *found = Some(hwnd);
            return 0;
        }
        1
    }

    let mut found: Option<HWND> = None;
    unsafe {
        EnumWindows(
            Some(first_visible),
            &mut found as *mut Option<HWND> as isize,
        )
    };
    found
}

/// The box the pick button's crosshair is painted in: a square against the
/// button's left edge, vertically centred, sized from the button's height.
fn pick_icon_rect(button: egui::Rect) -> egui::Rect {
    let side = button.height() * PICK_ICON_BOX;
    egui::Rect::from_center_size(
        egui::pos2(
            button.left() + PICK_ICON_INSET + side / 2.0,
            button.center().y,
        ),
        egui::vec2(side, side),
    )
}

/// The pick button's crosshair, painted rather than shipped as artwork: a
/// stroked ring with four ticks, like a screen spy's target.
///
/// The same reasoning as the rail and theme glyphs — no asset, and it takes
/// whatever colour the button's label does, so a theme cannot strand it.
fn draw_crosshair_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let unit = rect.width().min(rect.height());
    let centre = rect.center();
    let stroke = egui::Stroke::new(PICK_ICON_STROKE * unit, color);
    painter.circle_stroke(centre, PICK_ICON_RADIUS * unit, stroke);
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        painter.line_segment(
            [
                egui::pos2(
                    centre.x + dx * PICK_ICON_TICK_INNER * unit,
                    centre.y + dy * PICK_ICON_TICK_INNER * unit,
                ),
                egui::pos2(
                    centre.x + dx * PICK_ICON_TICK_OUTER * unit,
                    centre.y + dy * PICK_ICON_TICK_OUTER * unit,
                ),
            ],
            stroke,
        );
    }
}

/// The windows a pick may offer: everything the matcher could see, minus this
/// process's own windows — following the configuration window is never what
/// the user meant.
fn pickable_windows() -> Vec<WindowShape> {
    let own = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
    });
    winwatch::shapes()
        .into_iter()
        .filter(|shape| own.as_ref() != Some(&shape.window.process.to_ascii_lowercase()))
        .collect()
}

/// The text one of the sticky boxes shows: the value of the condition with
/// that part, or empty when there is none.
fn sticky_value(target: Option<&StickyTarget>, part: Part) -> String {
    target
        .and_then(|target| target.when.iter().find(|condition| condition.part == part))
        .map(|condition| condition.value.clone())
        .unwrap_or_default()
}

/// The mode a sticky box writes for its part.
///
/// Process and class are exact strings an executable and a window class are
/// already unique under; a title is not, so it matches as a substring.
fn sticky_mode(part: Part) -> MatchMode {
    match part {
        Part::Title => MatchMode::Contains,
        Part::ProcessName | Part::ClassName => MatchMode::Exact,
    }
}

/// Writes one sticky box.
///
/// The box being written takes the mode its part implies, but the other boxes
/// keep their conditions verbatim — a hand-edited matcher (a regex title, say)
/// survives editing a different box, and only the box the user actually types
/// in is normalized. An emptied box drops its condition, and a target left
/// with nothing to match goes back to `None`: an empty matcher means "no
/// target", which is what the editor's empty boxes mean too.
fn set_sticky_value(target: &mut Option<StickyTarget>, part: Part, value: String) {
    const KNOWN: [Part; 3] = [Part::ProcessName, Part::Title, Part::ClassName];
    let existing = target.take().map(|target| target.when).unwrap_or_default();
    let mut when: Vec<Condition> = Vec::new();
    for known in KNOWN {
        if known == part {
            if !value.trim().is_empty() {
                when.push(Condition {
                    part: known,
                    matcher: sticky_mode(known),
                    value: value.clone(),
                });
            }
        } else if let Some(condition) = existing.iter().find(|condition| condition.part == known) {
            when.push(condition.clone());
        }
    }
    *target = (!when.is_empty()).then_some(StickyTarget { when });
}

/// Fills a sticky target from a picked window: the process and the class, and
/// deliberately **not** the title.
///
/// The picked title is true only for the moment it was read. Windows 11's
/// Notepad reopens the document it last had, a browser's title follows the
/// page, a game's follows its state — and since the boxes are ANDed, a title
/// that has moved on is what strands an overlay on a window that is right in
/// front of the user (the overlay hides and never comes back, because nothing
/// matches). The title box stays editable for narrowing a target on purpose;
/// it is just not filled in for them.
fn fill_sticky_target(target: &mut Option<StickyTarget>, window: &WindowInfo) {
    set_sticky_value(target, Part::ProcessName, window.process.clone());
    set_sticky_value(target, Part::ClassName, window.class_name.clone());
    set_sticky_value(target, Part::Title, String::new());
}

fn load_position_texture(ctx: &Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let image = image::load_from_memory(bytes)
        .expect("bundled position picker asset must be valid")
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}

fn position_index(anchor: Anchor) -> usize {
    match anchor {
        Anchor::TopLeft => 0,
        Anchor::TopCenter => 1,
        Anchor::TopRight => 2,
        Anchor::CenterLeft => 3,
        Anchor::Center => 4,
        Anchor::CenterRight => 5,
        Anchor::BottomLeft => 6,
        Anchor::BottomCenter => 7,
        Anchor::BottomRight => 8,
    }
}

fn position_anchor(index: usize) -> Anchor {
    match index {
        0 => Anchor::TopLeft,
        1 => Anchor::TopCenter,
        2 => Anchor::TopRight,
        3 => Anchor::CenterLeft,
        4 => Anchor::Center,
        5 => Anchor::CenterRight,
        6 => Anchor::BottomLeft,
        7 => Anchor::BottomCenter,
        _ => Anchor::BottomRight,
    }
}

fn position_cell_at(rect: egui::Rect, point: egui::Pos2) -> Option<usize> {
    let scale_x = rect.width() / POSITION_PICKER_SIZE;
    let scale_y = rect.height() / POSITION_PICKER_SIZE;
    let x = (point.x - rect.left()) / scale_x;
    let y = (point.y - rect.top()) / scale_y;
    if !(0.0..POSITION_PICKER_SIZE).contains(&x) || !(0.0..POSITION_PICKER_SIZE).contains(&y) {
        return None;
    }

    for index in 0..9 {
        let cell = position_cell_rect(
            egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(POSITION_PICKER_SIZE, POSITION_PICKER_SIZE),
            ),
            index,
        );
        if cell.contains(egui::pos2(x, y)) {
            return Some(index);
        }
    }
    None
}

fn position_cell_rect(bounds: egui::Rect, index: usize) -> egui::Rect {
    let column = (index % 3) as f32;
    let row = (index / 3) as f32;
    let x = POSITION_PICKER_PADDING + column * (POSITION_PICKER_CELL_SIZE + POSITION_PICKER_GAP);
    let y = POSITION_PICKER_PADDING + row * (POSITION_PICKER_CELL_SIZE + POSITION_PICKER_GAP);
    let scale = bounds.width() / POSITION_PICKER_SIZE;
    egui::Rect::from_min_size(
        bounds.min + egui::vec2(x * scale, y * scale),
        egui::vec2(
            POSITION_PICKER_CELL_SIZE * scale,
            POSITION_PICKER_CELL_SIZE * scale,
        ),
    )
}

fn position_cell_uv(index: usize) -> egui::Rect {
    let column = (index % 3) as f32;
    let row = (index / 3) as f32;
    let x = POSITION_PICKER_PADDING + column * (POSITION_PICKER_CELL_SIZE + POSITION_PICKER_GAP);
    let y = POSITION_PICKER_PADDING + row * (POSITION_PICKER_CELL_SIZE + POSITION_PICKER_GAP);
    egui::Rect::from_min_max(
        egui::pos2(x / POSITION_PICKER_SIZE, y / POSITION_PICKER_SIZE),
        egui::pos2(
            (x + POSITION_PICKER_CELL_SIZE) / POSITION_PICKER_SIZE,
            (y + POSITION_PICKER_CELL_SIZE) / POSITION_PICKER_SIZE,
        ),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShutdownState {
    /// The window is open and nothing has been asked of it.
    Running,
    /// The three-way prompt is up because closing would lose a draft.
    ConfirmClose,
    /// The user has answered, and the close we asked for must now get through.
    ///
    /// A separate state because the frame that intercepts the close and the
    /// frame that lets it through cannot be the same one: cancelling and
    /// closing in the same frame cancels, since eframe only looks for
    /// `CancelClose` and never sees the `Close` behind it.
    ExitConfirmed,
}

/// Inline editor or confirmation shown in the Profiles page detail pane.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProfileDialog {
    Create { name: String },
    Rename { from: String, name: String },
    Duplicate { from: String, name: String },
    Delete { id: String, name: String },
}

/// A profile request raised by a page, applied after the widget closes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProfileAction {
    Switch(String),
    Create(String),
    Rename { from: String, to: String },
    Duplicate { from: String, to: String },
    Delete(String),
}

/// The pages the navigation rail switches between.
///
/// Pane 2 and pane 3 both change with the page: pane 2 holds the page's list
/// and pane 3 its detail. Switching pages is free, because the draft of the
/// Overlays page stays in memory; only switching *profile* is refused while
/// there are unsaved edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Overlays,
    Profiles,
    Global,
    About,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Overlays => "Overlays",
            Page::Profiles => "Profiles",
            Page::Global => "Global",
            Page::About => "About",
        }
    }
}

const PAGES: [Page; 4] = [Page::Overlays, Page::Profiles, Page::Global, Page::About];

/// Width of the navigation rail, labelled or icon only.
fn rail_width(collapsed: bool) -> f32 {
    if collapsed {
        RAIL_COLLAPSED_WIDTH
    } else {
        RAIL_WIDTH
    }
}

pub struct PingApp {
    page: Page,
    /// The page shown on the previous pass, so a change can be noticed.
    last_page: Page,
    rail_collapsed: bool,
    /// The store every profile, preference and rule read and write goes
    /// through.
    ///
    /// The app keeps one for its whole lifetime rather than calling the free
    /// `config::*` functions: a window pointed at a root must keep writing
    /// there, so a capture session launched with `PLO_CONFIG_DIR` set cannot
    /// stray back to the real config, and a test can drive the window against
    /// a temp directory without moving the process's own root.
    store: config::Store,
    config: Config,
    active_profile: String,
    profiles: Vec<config::ProfileEntry>,
    /// Overlay count per profile id, read from each profile file.
    ///
    /// A miss is not the same as zero and must never be drawn as one, so this
    /// starts empty and is filled by `sync_profiles` on arrival.
    profile_overlay_counts: HashMap<String, usize>,
    selected_profile: Option<String>,
    last_title: String,
    profile_menu_open: bool,
    profile_dialog: Option<ProfileDialog>,
    profile_name_focus: bool,
    selected_id: Option<String>,
    /// The target whose host fields the detail pane is editing.
    ///
    /// View-only, and subject to the same rules as `selected_id`: it starts as
    /// `None`, nothing auto-selects it, and clearing it never touches the draft,
    /// because staged edits live in `config.overlays[..].targets` and not here.
    /// A second selection rather than an index because a target id is stable and
    /// an index is not — the moment the list is reordered or one is deleted,
    /// every index after it points somewhere else.
    selected_target: Option<String>,
    /// Preferences as last written to `globalconfig.json`.
    prefs: config::GlobalPrefs,
    /// The staged copy the Global page edits, written on Save.
    prefs_draft: config::GlobalPrefs,
    prefs_dirty: bool,
    /// Auto profile switching as last written to `rules.json`.
    ///
    /// Its own draft and flag, exactly like the preferences: the Global page
    /// edits a copy, Save writes `rules.json`, and Discard puts the copy back.
    rules: AutoRules,
    rules_draft: AutoRules,
    rules_dirty: bool,
    /// A `rules.json` read failure from startup.
    ///
    /// The file is left exactly as it was found — an unreadable file is never
    /// silently rewritten — and the error stays visible until a Save succeeds,
    /// because the Save is the user explicitly saying "replace it with this".
    rules_error: Option<String>,
    /// What the engine would decide with the rules as currently edited, and
    /// when that was computed.
    ///
    /// On a clock rather than per frame: it enumerates the desktop's windows.
    /// This is the window-side mirror of the tray's decision, from the same
    /// pure `rules::decide`, so what the preview says is what the engine does.
    auto_preview: Option<String>,
    auto_preview_at: Option<Instant>,
    /// The debounced decision machine, run by the window itself while it is
    /// open.
    ///
    /// The tray stands down for this process's whole lifetime, so this is the
    /// only engine that can act in that time — and the window is then the only
    /// writer of `activeProfile`, which is exactly why that is safe. Seeded
    /// with the active profile so the first settled decision that names it is
    /// not a pointless reload.
    auto_engine: rules::Engine,
    /// When the engine last looked at the desktop.
    auto_switch_at: Option<Instant>,
    /// The profile a settled decision is waiting to switch to.
    ///
    /// Kept so the status line can say a switch is held once, rather than
    /// every second, while the user's unsaved edits are in the way.
    auto_hold: Option<String>,
    config_visible: bool,
    running: bool,
    status: String,
    dirty: bool,
    confirm_delete: Option<String>,
    /// What the renderer was last told, so `sync_runtime_config` can tell a
    /// change from a frame that merely followed another.
    last_pushed: Config,
    /// The enabled target keys the profile on disk has.
    ///
    /// A Save diffs the config being saved against this to find the removals it
    /// commits, and those are the probes the renderer must stop keeping.
    saved_keys: HashSet<TaskKey>,
    /// Committed removals the renderer has not acknowledged yet.
    ///
    /// Attached to every config send until one lands. The disk already holds
    /// the removal, so losing it to a failed write would leave a deleted host
    /// probed until the two ends happened to be reconnected.
    pending_retire: Vec<TaskKey>,
    /// The background-tracking preference the renderer was last told.
    ///
    /// The preference is saved like the rest and the renderer learns it by
    /// comparison each pass, so a change goes out once rather than every frame.
    last_sent_background: bool,
    /// The connection to the renderer process, when one is running.
    ///
    /// `None` means the renderer is gone and could not be brought back, which is
    /// a different situation from "not started yet" and has to be reported
    /// differently, because a save that reaches disk but never reaches the
    /// overlays is not really a save.
    ///
    /// There is deliberately no renderer process handle and no restart guard
    /// here. This window is a client: it starts the renderer once at launch if
    /// nothing else has, and the tray supervises it from then on. See
    /// `reconnect_renderer`.
    renderer: Option<Client>,
    /// The border preview the renderer was last told about, so it is only sent
    /// when it actually changes. Sending it per frame would be a pipe write per
    /// frame for no reason.
    border_preview: Option<String>,
    position_picker: PositionPicker,
    /// The sticky target's crosshair, alive only while a pick is armed. Kept
    /// on the app rather than in the editor so the capture survives a pass
    /// that redraws the section.
    sticky_picker: StickyPicker,
    /// The displays attached right now, and when they were last read.
    ///
    /// Cached because `EnumDisplayMonitors` is a round trip into the window
    /// manager and this runs every pass. A display can be plugged in or pulled
    /// out at any time without telling us, so this is re-read on a clock rather
    /// than on a save — the picks list has to be able to add a new monitor the
    /// user has not configured anything for yet.
    monitors: Vec<MonitorInfo>,
    monitors_read_at: Option<Instant>,
    /// The theme in force, and the Windows mode it was resolved against.
    ///
    /// Both are needed because `System` is the default: the window has to
    /// re-resolve when Windows changes, and it cannot do that by re-reading the
    /// preference alone, since the preference has not moved.
    theme: Theme,
    system_mode: Mode,
    shutdown_state: ShutdownState,
    /// The app icon, decoded once and shown above the About page's text.
    about_logo: egui::TextureHandle,
}

/// The three theme choices, in the order they are offered, with their labels.
///
/// The labels come from here rather than being written inline so a test can hold
/// them. They are short because they sit inside square tiles; the two pieces of
/// information the old System label carried — what it follows and what it
/// resolved to — now live in the tile's tooltip (`theme_choice_hint`).
fn theme_choices() -> [(ThemeMode, &'static str); 3] {
    [
        (ThemeMode::System, "System Theme"),
        (ThemeMode::Light, "Light Theme"),
        (ThemeMode::Dark, "Dark Theme"),
    ]
}

/// The tooltip for a theme tile.
///
/// "System Theme" on its own does not say whether the feature is working, so
/// the hint carries the answer — the question a user actually has about a
/// follow-the-system setting. The overrides carry the one consequence of
/// overriding: the tray menu is a native menu Windows paints from the system
/// setting, so it follows Windows whatever this is set to.
fn theme_choice_hint(system: Mode, choice: ThemeMode) -> String {
    match choice {
        ThemeMode::System => format!(
            "Follows Windows — currently {}.",
            if system == Mode::Dark {
                "dark"
            } else {
                "light"
            }
        ),
        ThemeMode::Light | ThemeMode::Dark => {
            "The tray's menu always follows Windows, so with this set the window and the tray \
             menu can look different."
                .to_string()
        }
    }
}

/// A theme tile's side: three tiles and their two gaps share the available
/// width, capped so a wide window gets buttons rather than billboards.
///
/// An uncapped side leaves `3*side + 2*gap` exactly at the available width, so
/// the row fits either way — `the_theme_tiles_fit_their_pane` measures it.
fn theme_tile_side(available: f32) -> f32 {
    ((available - THEME_TILE_GAP * 2.0) / 3.0).min(THEME_TILE_MAX)
}

/// The tile's label size: it shrinks with the tile rather than overflowing it.
fn theme_tile_label_size(side: f32) -> f32 {
    (side * THEME_TILE_LABEL_FRACTION).clamp(THEME_TILE_LABEL_SIZE_MIN, THEME_TILE_LABEL_SIZE_MAX)
}

/// The theme tiles, painted, returning the choice a click made.
///
/// Painted rather than three `selectable_label`s because the mode icon has to
/// sit inside the button: an egui widget has no image slot, and a glyph from the
/// font would depend on font coverage — the same reason the rail's glyphs are
/// painted.
///
/// A free function taking the values it needs, like `sync_profile_cache`, so a
/// test can drive it headlessly and measure what it lays out. The tiles' fill,
/// stroke and rounding come from `interact_selectable`, the same
/// `WidgetVisuals` a selectable label uses, so the row follows whatever a theme
/// does to buttons.
fn theme_tiles(ui: &mut Ui, current: ThemeMode, system: Mode) -> Option<ThemeMode> {
    let side = theme_tile_side(ui.available_width());
    let icon_size = side * THEME_TILE_ICON_SIZE;
    let label_size = theme_tile_label_size(side);
    let mut chosen = None;
    // The row's gap is the constant the side was computed from, so it is set
    // rather than left to `item_spacing`, and put back afterwards.
    let previous_gap = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = THEME_TILE_GAP;
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        for (choice, label) in theme_choices() {
            let picked = current == choice;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
            let visuals = ui.style().interact_selectable(&response, picked);
            let painter = ui.painter();
            painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
            painter.rect_stroke(
                rect,
                visuals.corner_radius,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            draw_theme_icon(
                painter,
                egui::Rect::from_center_size(
                    egui::pos2(
                        rect.center().x,
                        rect.top() + side * THEME_TILE_ICON_CENTER_Y,
                    ),
                    egui::Vec2::splat(icon_size),
                ),
                choice,
                visuals.fg_stroke.color,
                visuals.weak_bg_fill,
            );
            painter.text(
                egui::pos2(
                    rect.center().x,
                    rect.top() + side * THEME_TILE_LABEL_CENTER_Y,
                ),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(label_size),
                visuals.fg_stroke.color,
            );
            if let Some(cursor) = ui.visuals().interact_cursor {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(cursor);
                }
            }
            let response = response.on_hover_text(theme_choice_hint(system, choice));
            if response.clicked() && !picked {
                chosen = Some(choice);
            }
        }
    });
    ui.spacing_mut().item_spacing.x = previous_gap;
    chosen
}

/// Stage a theme choice and make it the live one at the same time.
///
/// Both halves, and the second is the easy one to leave out. `sync_theme` runs
/// every pass and resolves the mode from `prefs.ui.theme` — the SAVED value — so
/// a control that only wrote the draft would have its pick applied and then
/// silently undone on the next frame. That is the "briefly right, then it changes
/// back" shape again, and it is why the two Appearance checkboxes write
/// `self.prefs` as well as the draft.
///
/// The consequence is that this needs no apply call: the next `sync_theme` sees
/// the new live preference and re-applies. It is also what makes Discard work,
/// since `discard_prefs` restores `prefs` and the same pass puts the theme back.
fn choose_theme(
    prefs: &mut config::GlobalPrefs,
    draft: &mut config::GlobalPrefs,
    choice: ThemeMode,
) {
    draft.ui.theme = choice;
    prefs.ui.theme = choice;
}

/// Stage the selection-border toggle, and make it the live value at the same
/// time.
///
/// Both halves for the same reason `choose_theme` writes both: `sync_border_preview`
/// runs every pass and reads the live preference, so a control that only wrote
/// the draft would leave the border animating and the setting looking dead.
fn set_selection_border_animation(
    prefs: &mut config::GlobalPrefs,
    draft: &mut config::GlobalPrefs,
    enabled: bool,
) {
    draft.ui.selection_border_animation = enabled;
    prefs.ui.selection_border_animation = enabled;
}

/// Stage the background-tracking toggle, and make it the live value at the
/// same time.
///
/// The same both-halves rule as `set_selection_border_animation`:
/// `sync_runtime_config` compares the live preference against what the renderer
/// was told, so a draft-only write left the kept probes running after the box
/// was unticked — the setting looked live but only Save made it so.
fn set_background_tracking(
    prefs: &mut config::GlobalPrefs,
    draft: &mut config::GlobalPrefs,
    enabled: bool,
) {
    draft.ui.background_tracking = enabled;
    prefs.ui.background_tracking = enabled;
}

impl PingApp {
    pub fn new(cc: &CreationContext<'_>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // The one store this window writes through. It resolves the same
        // `PLO_CONFIG_DIR` override `config::load` does, so a window launched
        // against a sandbox (the capture script does this) reads and writes
        // that root and can never stray back to the user's real config.
        let store = config::Store::new(config::config_dir());
        let loaded = config::load();
        // Loaded separately from the profile: it is app-wide and the tray is
        // already reading it. A parse failure is shown in the status bar and
        // the section, and never rewrites the file on its own.
        let (rules, rules_error) = match config::load_rules() {
            Ok(rules) => (rules, None),
            Err(error) => (
                AutoRules::default(),
                Some(format!("rules.json could not be read: {error}")),
            ),
        };
        Ok(Self::build(
            &cc.egui_ctx,
            store,
            loaded,
            rules,
            rules_error,
            true,
        ))
    }

    /// The app a test drives: the real window minus the parts a test cannot
    /// own.
    ///
    /// A bare `egui::Context` and no process spawns, so the whole frame path
    /// runs headlessly, and `build` is shared with `new` so the two cannot
    /// drift. The theme is pinned to light so no test depends on the machine's
    /// Windows setting; the sandbox has no theme files, so the built-in light
    /// theme loads and the position picker never reads the disk.
    #[cfg(test)]
    fn for_test(ctx: &Context, root: &std::path::Path) -> Self {
        let mut loaded = config::load_rooted(root);
        loaded.prefs.ui.theme = ThemeMode::Light;
        Self::build(
            ctx,
            config::Store::new(root.to_path_buf()),
            loaded,
            AutoRules::default(),
            None,
            false,
        )
    }

    /// The shared body of `new` and `for_test`.
    ///
    /// `attach` is the one difference: a real launch brings the tray up,
    /// starts or attaches to the renderer, shows the window and pushes the
    /// config, while a test does none of those and runs the same code for
    /// everything else — the theme resolved from the loaded preference, the
    /// global style, the position picker, the About logo, the engine seed.
    fn build(
        ctx: &Context,
        store: config::Store,
        loaded: config::ConfigLoad,
        rules: AutoRules,
        rules_error: Option<String>,
        attach: bool,
    ) -> Self {
        let position_picker = PositionPicker::new(ctx);
        let config = loaded.config;
        let mut status = config_notices_status(&loaded.notices);
        let active_profile = loaded.active_profile;
        // The one place the rail's collapsed state survives a restart.
        let prefs = loaded.prefs;
        let rail_collapsed = prefs.ui.rail_collapsed;
        let profiles = loaded.profiles;
        // The theme is resolved before the first frame rather than lazily inside
        // it, so the window never paints one frame in the wrong colours, and the
        // resolved mode is kept so the first `logic()` pass is not a no-op that
        // decides it changed when it did not.
        //
        // The preference is read from `prefs`, not hardcoded to `System`: a
        // `globalconfig.json` a user edited by hand says `light`, and starting
        // up as System would override it on every launch and never say so.
        let (system_mode, theme) = sync_theme(None, prefs.ui.theme, &store.themes_dir());
        set_palette(theme.colors);
        theme::apply(ctx, &theme);
        ctx.global_style_mut(|style| {
            style.spacing.scroll.foreground_color = false;
        });
        // A real launch is the window, and it is only ever started because
        // somebody asked for a window, so it starts visible. It used to start
        // hidden behind a `--show-config` flag, which was right when the tray
        // lived in this process and revealed it from its menu — and became a
        // window nobody could reach once the tray moved out. The flag is still
        // accepted below so the documented development command does not
        // become an error, but it no longer hides anything.
        let show_config = attach;
        let _show_config_flag = std::env::args_os().any(|arg| arg == "--show-config");

        // A real launch is the one thing that can bring the tray up with it: a
        // user who double-clicks this executable should end up with a working
        // app, not a window and no tray. The renderer follows the same rule one
        // level down, and the renderer itself never starts either of them.
        // The tray is deliberately NOT tracked as a child to clean up: it is the
        // app, and it should outlive a window. Closing the window leaves it
        // running, which is the intended shape rather than a leak.
        let mut renderer = None;
        if attach {
            if let Some(failure) = start_or_attach_tray() {
                log_line("config", &failure);
            }
            // A renderer that is already running is somebody else's, and
            // attaching to it is the whole point of the pipe being the
            // rendezvous. Only when there is nothing listening does this shell
            // start one, and only a process it started is ever stopped by it.
            let (connection, _renderer_process, failure) = start_or_attach_renderer();
            renderer = connection;
            // Whatever the notices said at startup survives a renderer that
            // would not start, because both are things the user needs to see.
            status = append_status_message(status, failure.as_deref());
        }
        status = append_status_message(status, rules_error.as_deref());
        let running = true;

        // The window runs the engine while it is open, because the tray stands
        // down for this process's whole lifetime. Seeded with the profile that
        // is already loaded — and pushed to the renderer just below — so the
        // first settled decision that names it is not a pointless reload.
        let mut auto_engine = rules::Engine::default();
        auto_engine.mark_applied(&active_profile);

        if attach {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(true));
        }
        // The static title in `run` cannot know the profile, so the real one
        // is sent as soon as the app state exists.
        let mut app = Self {
            page: Page::Overlays,
            // Matching the initial page, so the first pass is not read as an
            // arrival and the startup is not spent reading every profile file.
            last_page: Page::Overlays,
            rail_collapsed,
            store,
            // Cloned before it is moved, and seeded with what is loaded: the
            // `push_config` at the end of the launch tells the renderer exactly
            // this, so the first `sync_runtime_config` has nothing to say.
            // Seeding it with `Config::default()` instead would send the whole
            // file once more on the first frame, which is harmless but means the
            // record and the renderer disagree at startup for no reason.
            last_pushed: config.clone(),
            saved_keys: enabled_target_keys(&config),
            pending_retire: Vec::new(),
            last_sent_background: prefs.ui.background_tracking,
            config,
            // Cloned before the id is moved into `active_profile`, so the
            // Profiles page opens on whatever is already loaded.
            selected_profile: Some(active_profile.clone()),
            active_profile,
            profiles,
            profile_overlay_counts: HashMap::new(),
            last_title: String::new(),
            profile_menu_open: false,
            profile_dialog: None,
            profile_name_focus: false,
            // Nothing is selected on the Overlays page until the user picks an
            // overlay. Selecting the first one automatically meant a border was
            // animating the moment the window opened, which read as the app
            // doing something nobody asked for. The selection is view-only:
            // staged edits live in `config.overlays`, so emptying pane 3
            // cannot lose any.
            selected_id: None,
            selected_target: None,
            prefs_draft: prefs.clone(),
            prefs,
            prefs_dirty: false,
            rules_draft: rules.clone(),
            rules,
            rules_dirty: false,
            rules_error,
            auto_preview: None,
            auto_preview_at: None,
            auto_engine,
            auto_switch_at: None,
            auto_hold: None,
            config_visible: show_config,
            running,
            status,
            dirty: false,
            confirm_delete: None,
            renderer,

            // The renderer starts with no border preview, so `None` here is
            // accurate rather than merely unknown and needs no first send.
            border_preview: None,
            position_picker,
            sticky_picker: StickyPicker::default(),
            monitors: Vec::new(),
            // `None` so the first `sync_monitors` reads, rather than showing an
            // empty monitor list until the clock comes round.
            monitors_read_at: None,
            theme,
            system_mode,
            shutdown_state: ShutdownState::Running,
            about_logo: about_logo_texture(ctx),
        };
        app.sync_window_title(ctx);
        if attach {
            app.push_config();
        }
        ctx.request_repaint();
        app
    }

    /// Send a message to the renderer, reporting failure in the status bar.
    ///
    /// Every path that changes what should be on screen goes through here, so
    /// there is exactly one place that knows the renderer may be unreachable
    /// and exactly one place that says so. A caller that needs to distinguish
    /// "the disk write worked" from "the renderer was told" uses
    /// [`PingApp::try_send`] instead.
    fn send(&mut self, message: Message) {
        if let Err(error) = self.try_send(&message) {
            self.status = format!("The renderer is not responding: {error}");
        }
    }

    /// Send a message, leaving the status bar to the caller.
    ///
    /// The pipe is an I/O channel and nothing else goes wrong here, so the
    /// error type says exactly that rather than a boxed trait object that would
    /// hide which failure it was.
    fn try_send(&mut self, message: &Message) -> std::io::Result<()> {
        match &mut self.renderer {
            Some(client) => client.send(message),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "no renderer is running",
            )),
        }
    }

    /// The config push, carrying the two facts that decide the fate of targets
    /// it no longer contains.
    ///
    /// `retire` is the removals a Save has committed and the renderer has not
    /// acknowledged; it is empty for every other send. The preference is read
    /// live, because it is staged like the rest and Save is what makes it so.
    fn config_message(&self, retire: Vec<TaskKey>) -> Message {
        Message::SetConfig {
            config: self.config.clone(),
            background_tracking: self.prefs.ui.background_tracking,
            retire,
        }
    }

    /// Hand the current configuration to the renderer, and record that we did.
    ///
    /// `last_pushed` is updated here rather than only at the call sites so that
    /// every path which sends a config leaves the window and the renderer
    /// agreeing about what the renderer has. `reconnect_renderer` in particular
    /// attaches to a renderer started by somebody else, which knows nothing
    /// about this profile; if it did not update the record, the next
    /// `sync_runtime_config` would either send a redundant copy or — if this
    /// send had failed — sit there convinced the renderer was already up to date.
    ///
    /// A committed removal rides this push too, so a renderer that reconnects
    /// after a failed save still retires what the file already dropped.
    fn push_config(&mut self) {
        let retire = std::mem::take(&mut self.pending_retire);
        match self.try_send(&self.config_message(retire.clone())) {
            Ok(()) => self.last_sent_background = self.prefs.ui.background_tracking,
            Err(error) => {
                self.status = format!("The renderer is not responding: {error}");
                self.pending_retire = retire;
            }
        }
        self.last_pushed = self.config.clone();
    }

    /// Push the draft to the renderer as soon as it changes, without saving it.
    ///
    /// This is what makes an edit visible while you are still making it. It used
    /// to happen by accident: before the renderer was its own process, `logic`
    /// called `sync_overlays()` every frame and the overlay manager re-read the
    /// in-memory config, so any staged change was on screen immediately. Moving
    /// the renderer behind the pipe turned that per-frame call into a message,
    /// and the message went only down the paths that used to save — so every
    /// appearance edit silently became save-only. The only edit that stayed live
    /// was the list pane's enable toggle, because that one had an explicit call
    /// added to it at the time.
    ///
    /// **Normalized in place, before the comparison and not just before the
    /// write.** Two reasons, and the second is the one that bites. `persist`
    /// clamps, so an un-normalized draft would move the overlay as you dragged
    /// it and then jump again on Save — the "briefly right, then it changes
    /// back" shape this app has produced twice. And comparing a normalized
    /// record against an un-normalized draft would never match, so the window
    /// would send the whole configuration on *every single frame* for as long as
    /// it stayed open. Normalizing into `self.config` first makes the two
    /// comparable; it is also correct in its own right, because a clamped value
    /// is what Save would write anyway, so the editor should show it.
    ///
    /// Returns whether anything was sent, which is what the test drives.
    fn sync_runtime_config(&mut self) -> bool {
        if !config_push_due(
            &mut self.config,
            &self.last_pushed,
            self.prefs.ui.background_tracking,
            self.last_sent_background,
            !self.pending_retire.is_empty(),
        ) {
            return false;
        }
        // Nothing to try without a client, and `reconnect_renderer` pushes
        // everything the moment one answers again — pending removals included.
        if self.renderer.is_none() {
            return false;
        }
        let retire = std::mem::take(&mut self.pending_retire);
        match self.try_send(&self.config_message(retire.clone())) {
            Ok(()) => {
                self.last_pushed = self.config.clone();
                self.last_sent_background = self.prefs.ui.background_tracking;
                true
            }
            Err(error) => {
                self.pending_retire = retire;
                self.status = format!("The renderer is not responding: {error}");
                false
            }
        }
    }

    /// The window title, which names the profile that is currently loaded.
    ///
    /// Reads the **live** `prefs`, not `prefs_draft`, so ticking the checkbox on
    /// the Global page updates the title immediately while the write stays
    /// staged until Save. `sync_window_title` runs every frame from `logic()`
    /// and only sends when the string actually changes, so this costs nothing
    /// per frame.
    fn window_title(&self) -> String {
        window_title(
            &self.active_profile_name(),
            self.prefs.ui.show_version_in_title,
        )
    }

    /// Push the title to the window when the active profile's name changed.
    fn sync_window_title(&mut self, ctx: &Context) {
        let title = self.window_title();
        if title == self.last_title {
            return;
        }
        self.last_title = title.clone();
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Title(title));
    }

    /// The display name of the loaded profile.
    fn active_profile_name(&self) -> String {
        self.profile_name(&self.active_profile)
    }

    /// The name to show for a profile id.
    ///
    /// The refreshed list is authoritative because it also covers renames of
    /// the active profile, which the in-memory config does not know about yet.
    fn profile_name(&self, id: &str) -> String {
        if let Some(profile) = self.profiles.iter().find(|profile| profile.id == id) {
            return profile.name.clone();
        }
        if id == self.active_profile {
            return config::display_name(&self.config, id);
        }
        id.to_string()
    }

    /// The window is only in this process while it is open, so "close" means
    /// exit rather than hide. What still matters is that the border preview is
    /// handed over as the window goes: the renderer's copy of it is a `Some`
    /// for as long as the window is on screen, and a process that exits
    /// without clearing it would leave the overlay's border animating with
    /// nothing on screen to explain it.
    fn release_border_preview(&mut self, ctx: &Context) {
        if self.border_preview.is_none() {
            return;
        }
        self.border_preview = None;
        let _ = self.try_send(&Message::SetBorderPreview { overlay_id: None });
        ctx.request_repaint();
    }

    /// Closing the window ends this process, so the window is not a window
    /// that hides.
    ///
    /// That is the whole point of the split: a hidden-but-alive Config window
    /// still holds the GL context, so "hide on close" would buy the memory
    /// back nothing at all. The tray survives, because it is a different
    /// process now.
    ///
    /// Every close is intercepted and cancelled first, and the close is only
    /// re-issued from a later frame once this process has decided. With a
    /// draft outstanding the decision is the user's, so the prompt goes up
    /// instead.
    fn handle_root_close(&mut self, ctx: &Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        match self.shutdown_state {
            ShutdownState::Running => {
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::CancelClose,
                );
                // The window is going away, so the border preview must not
                // outlive it. This is the one case where a close does not need
                // asking about first, so it is done here rather than on the way
                // out: a prompt that hangs around with no border behind it
                // would be answering a question about nothing.
                self.release_border_preview(ctx);
                if self.has_pending_edits() {
                    self.shutdown_state = ShutdownState::ConfirmClose;
                } else {
                    self.shutdown_state = ShutdownState::ExitConfirmed;
                }
            }
            ShutdownState::ConfirmClose => {
                // A second click on the title bar while the panel is up must
                // not stack a second panel, so this one is swallowed too. The
                // buttons on the panel are how the user answers it.
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::CancelClose,
                );
            }
            ShutdownState::ExitConfirmed => {
                // The close we already asked for is on its way. Cancelling it
                // here would trap the user in a window they cannot close.
            }
        }
    }

    /// The three-way prompt: keep the changes, throw them away, or stay.
    ///
    /// Save is the only branch that can fail, and a failure keeps the window
    /// open with the reason in the status bar, because the alternative is
    /// closing on a save that did not happen. Discard never fails the same
    /// way: it is asking to forget the draft, so a profile that will not
    /// reload is not a reason to keep the window open.
    fn show_close_prompt(&mut self, ctx: &Context) {
        let mut save = false;
        let mut discard = false;
        let mut stay = false;

        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .title_bar(false)
            .show(ctx, |ui| {
                ui.label("This profile has changes that have not been saved.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save and close").clicked() {
                        save = true;
                    }
                    if ui.button("Discard and close").clicked() {
                        discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        stay = true;
                    }
                });
            });

        if stay {
            self.shutdown_state = ShutdownState::Running;
            return;
        }
        if discard {
            self.discard_edits();
            self.shutdown_state = ShutdownState::ExitConfirmed;
            return;
        }
        if save && self.save_edits() {
            // Only now, with the write confirmed, does the window leave.
            self.shutdown_state = ShutdownState::ExitConfirmed;
        }
        // A save that failed leaves the state alone, so the panel stays up
        // and the status bar says why.
    }

    /// Tell the renderer which overlay's border should animate, when it changes.
    ///
    /// The animated border is a Config-window affordance, so with the overlay
    /// windows in another process it has to travel over the pipe instead of
    /// being a direct call. Only a change is sent: a send every frame would be a
    /// pipe write every frame for a value that almost never moves.
    fn sync_border_preview(&mut self) {
        let preview = selected_overlay_for_border(
            self.config_visible,
            self.page,
            self.selected_id.as_deref(),
            self.prefs.ui.selection_border_animation,
        )
        .map(str::to_string);
        if preview == self.border_preview {
            return;
        }
        self.border_preview = preview.clone();
        self.send(Message::SetBorderPreview {
            overlay_id: preview,
        });
    }

    /// Reconnect to the renderer, but never start one.
    ///
    /// This process is a **client, not a supervisor**. It starts the renderer
    /// once at launch if nothing else has, and from that moment the tray owns
    /// that process's life. When this used to restart a renderer that had died,
    /// the two processes competed for it: the tray's Exit would stop the
    /// renderer and this window would bring it straight back, so neither could
    /// ever win. It also carries no storm guard, because a storm guard exists
    /// to stop a *crash loop*, and this is not the process that would be
    /// crash-looping — the tray is, and it has its own.
    ///
    /// Reconnecting rather than doing nothing is worth it for one reason: a
    /// renderer the tray revived becomes usable again, and a window that cannot
    /// save is a window that lies to you. In the meantime the save path reports
    /// the truth — a profile written while no renderer is listening says so
    /// rather than claiming the overlays were updated.
    fn reconnect_renderer(&mut self) {
        if self.renderer.as_ref().is_some_and(Client::is_connected) {
            return;
        }
        match ping_latency_overlay_core::transport::Client::connect() {
            Ok(client) => {
                self.renderer = Some(client);
                // The renderer we just found is somebody else's, and it was
                // started with no knowledge of this profile.
                self.push_config();
            }
            Err(_) => self.renderer = None,
        }
    }

    fn apply_saved_config(&mut self, next: Config) {
        self.config = next;
        // The saved state a later save diffs against. A profile load is not a
        // removal, so nothing here is retired.
        self.saved_keys = enabled_target_keys(&self.config);
        // A profile load changes what should be on screen, so it goes out now
        // rather than waiting for the next pass of `sync_runtime_config`. The
        // function would get there on the same frame, but a load is a discrete
        // event and having it announce itself keeps the two paths from being
        // one obviously-correct one and one that happens to be in step.
        self.push_config();
    }

    /// Whether any draft has something to write.
    ///
    /// The profile draft, the preferences draft and the rules draft are
    /// independent, and pages stage into one of them, so the footer's buttons
    /// must follow all three.
    fn has_pending_edits(&self) -> bool {
        pending_edits(self.dirty, self.prefs_dirty, self.rules_dirty)
    }

    /// Write the active profile, but only when the profile draft has changed.
    ///
    /// The guard is not an optimisation. Once the footer was enabled for
    /// preferences too, an unguarded Save would rewrite the profile file from
    /// the in-memory draft even when only `globalconfig.json` had changed,
    /// silently clobbering any edit made to that file from outside the app
    /// while it was running. `save_prefs` has always had this guard; this is
    /// where the two writers were asymmetric. Returns true either way so
    /// `save_edits` still reports "Saved." for a preferences-only save.
    ///
    /// Writing the file and telling the renderer are two things that can fail
    /// separately, and only the first is under this process's control. So the
    /// status distinguishes them rather than reporting one confident "Saved."
    /// over both: a profile that reached disk while the renderer was never told
    /// is the exact shape of quiet wrongness this app has shipped before.
    fn persist_current(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        let mut next = self.config.clone();
        next.normalize();
        // Profiles are the only configuration storage now; the previous
        // single-file config.json was moved into the profiles directory.
        match self.store.save_profile(&self.active_profile, &next) {
            Ok(()) => {
                self.config = next;
                self.dirty = false;
                // The removals this save just committed, diffed against what
                // the file held a moment ago. The record then moves forward, so
                // a second save does not name the same host again — while the
                // pending list keeps one a failed send did not land.
                let retired = retired_targets(&self.saved_keys, &self.config);
                self.saved_keys = enabled_target_keys(&self.config);
                self.pending_retire.extend(retired);
                // Sent directly rather than through `push_config`, because this
                // path has to tell the two failures apart: the file reached disk
                // and the renderer was never told is a different thing from a
                // confident "Saved.", and the caller is what reports it. The
                // record is still updated, and only on success — a failed send
                // has to leave `sync_runtime_config` wanting to try again, with
                // the removals still pending.
                let retire = std::mem::take(&mut self.pending_retire);
                match self.try_send(&self.config_message(retire.clone())) {
                    Ok(()) => {
                        self.last_pushed = self.config.clone();
                        self.last_sent_background = self.prefs.ui.background_tracking;
                        self.status.clear();
                        true
                    }
                    Err(error) => {
                        self.pending_retire = retire;
                        self.status = format!(
                            "Saved, but the overlays were not updated: {error}. Save again once the renderer is back."
                        );
                        true
                    }
                }
            }
            Err(error) => {
                self.status = format!("Save failed: {error}");
                false
            }
        }
    }

    fn add_overlay(&mut self) {
        let overlay = OverlayConfig::new();
        self.selected_id = Some(overlay.id.clone());
        // A different overlay, so a host selected in the old one cannot still be
        // the "selected host" here — the editor looks it up by id and would find
        // nothing, so the pane would show neither.
        self.selected_target = None;
        self.config.overlays.push(overlay);
        self.dirty = true;
        self.status.clear();
    }

    fn toggle_overlay(&mut self, id: &str) {
        let mut changed = false;
        if let Some(overlay) = self
            .config
            .overlays
            .iter_mut()
            .find(|overlay| overlay.id == id)
        {
            overlay.enabled = !overlay.enabled;
            changed = true;
        }
        if changed {
            self.dirty = true;
            self.status.clear();
            // The toggle is on screen before the user lets go of the mouse now,
            // because `sync_runtime_config` sends anything the draft changed
            // since last frame. Nothing to send here.
        }
    }

    fn delete_overlay(&mut self, id: &str) {
        self.config.overlays.retain(|overlay| overlay.id != id);
        if self.selected_id.as_deref() == Some(id) {
            self.selected_id = self
                .config
                .overlays
                .first()
                .map(|overlay| overlay.id.clone());
        }
        let _ = self.persist_current();
    }

    /// Write whichever drafts are dirty.
    ///
    /// The profile draft, the preferences draft and the rules draft are
    /// independent: the Global page stages the latter two, so one Save can
    /// carry all three.
    ///
    /// Returns whether everything that needed writing was written, which is
    /// what the close prompt gates on: a save that reports success but failed
    /// to reach disk must not let the window close, or the draft is gone with
    /// nothing to show for it. "There was nothing to save" counts as success,
    /// so a Save on a clean window exits normally.
    fn save_edits(&mut self) -> bool {
        let profile_saved = self.persist_current();
        let prefs_saved = self.save_prefs();
        let rules_saved = self.persist_rules();
        if profile_saved || prefs_saved || rules_saved {
            self.status = "Saved.".to_string();
        }
        profile_saved && prefs_saved && rules_saved
    }

    /// Store the preferences draft, leaving the active profile pointer alone.
    fn save_prefs(&mut self) -> bool {
        if !self.prefs_dirty {
            return true;
        }
        match self.store.write_global_prefs(&self.prefs_draft) {
            Ok(()) => {
                self.prefs = self.prefs_draft.clone();
                self.prefs_dirty = false;
                true
            }
            Err(error) => {
                self.status = format!("Could not update globalconfig.json: {error}");
                false
            }
        }
    }

    /// Store the rules draft, clearing a startup read failure.
    ///
    /// The guard mirrors `save_prefs`: writing an untouched draft over a
    /// `rules.json` a user hand-edited from outside the window would discard
    /// that edit for no reason. A successful save is also the explicit
    /// "replace the broken file" permission, so the error notice clears here
    /// and only here.
    fn persist_rules(&mut self) -> bool {
        if !self.rules_dirty {
            return true;
        }
        match self.store.save_rules(&self.rules_draft) {
            Ok(()) => {
                self.rules = self.rules_draft.clone();
                self.rules_dirty = false;
                self.rules_error = None;
                true
            }
            Err(error) => {
                self.status = format!("Could not update rules.json: {error}");
                false
            }
        }
    }

    /// Drop unsaved edits by reloading the active profile and the preferences
    /// from disk.
    fn discard_edits(&mut self) {
        self.discard_prefs();
        self.discard_rules();
        match self.store.load_profile(&self.active_profile) {
            Ok(mut stored) => {
                stored.normalize();
                self.apply_saved_config(stored);
                self.dirty = false;
                self.status = "Changes discarded.".to_string();
            }
            Err(error) => {
                self.status = format!(
                    "Could not reload profile \"{}\": {error}",
                    self.active_profile
                );
            }
        }
    }

    /// Put the preferences back the way disk has them, rail included.
    fn discard_prefs(&mut self) {
        self.prefs = self.store.read_global_prefs();
        self.prefs_draft = self.prefs.clone();
        self.rail_collapsed = self.prefs.ui.rail_collapsed;
        self.prefs_dirty = false;
    }

    /// Put the rules back to the last load or save.
    ///
    /// `rules` is that value; a read failure left it at its default, which is
    /// what Discard comes back to. The error itself is kept, because it
    /// describes the file rather than the draft, and the file is untouched.
    fn discard_rules(&mut self) {
        self.rules_draft = self.rules.clone();
        self.rules_dirty = false;
    }

    /// Re-read the profile list, and with it the overlay count every profile
    /// row shows.
    ///
    /// The counts cost one file read per profile, so this only runs on a user
    /// action. Arriving on the Profiles page goes through `sync_profiles`;
    /// everything else that can change a count — creating, renaming,
    /// duplicating, deleting and switching a profile, and opening the switcher —
    /// calls this directly. Never per frame.
    ///
    /// A profile whose file will not parse is left out of the count map rather
    /// than counted as zero, so it draws no number instead of a wrong one.
    fn refresh_profiles(&mut self) {
        let snapshot = read_profile_snapshot(&self.store);
        self.profiles = snapshot.profiles;
        self.profile_overlay_counts = snapshot.counts;
    }

    /// Re-read the profile counts when arriving on the page that shows them.
    ///
    /// The Profiles page is the only place a per-profile count appears, and the
    /// count is a cache, so arriving there is what makes the cache true. Every
    /// other page shows nothing that goes stale, and staying put must not
    /// re-read: this runs every pass.
    fn sync_profiles(&mut self) {
        let last_page = &mut self.last_page;
        let page = self.page;
        let profiles = &mut self.profiles;
        let counts = &mut self.profile_overlay_counts;
        sync_profile_cache(last_page, page, profiles, counts, || {
            read_profile_snapshot(&self.store)
        });
    }

    /// Re-resolve the theme, and repaint the window when it changed.
    ///
    /// Runs every pass, like `sync_monitors`, and for the same reason: a Windows
    /// theme change arrives as nothing at all. There is no `WM_SETTINGCHANGE`
    /// here and no notification, so the only way to notice is to look, and a
    /// registry read is cheap enough to look with every few hundred
    /// milliseconds.
    ///
    /// The comparison is on the *mode*, not on the theme, because that is the
    /// only thing that can change without the user touching anything. A theme
    /// file edited on disk is not noticed until the next launch: reloading it
    /// would mean watching a directory, which is phase 2's problem.
    fn sync_theme(&mut self, ctx: &Context) {
        let previous = (self.system_mode, &self.theme);
        let (mode, theme) = sync_theme(
            Some(previous),
            self.prefs.ui.theme,
            &self.store.themes_dir(),
        );
        if mode == self.system_mode {
            return;
        }
        self.system_mode = mode;
        self.theme = theme;
        set_palette(self.theme.colors);
        theme::apply(ctx, &self.theme);
        // The window background is a `clear_color`, not a widget, so a repaint
        // with stale visuals would leave a band of the old theme around the
        // edges until something else asked for a frame.
        ctx.request_repaint();
    }

    /// Keep the list of attached displays fresh.
    ///
    /// A display change is not something the app hears about, and the Overlays
    /// page is the only place the list is drawn, so this is on a clock rather
    /// than on a save. Two seconds is a compromise: short enough that plugging
    /// a monitor in puts it in the list while the window is still open, long
    /// enough that the enumeration is not on the frame path.
    fn sync_monitors(&mut self) {
        let now = Instant::now();
        sync_monitor_list(
            &mut self.monitors,
            &mut self.monitors_read_at,
            now,
            monitors::enumerate,
        );
    }

    /// Refresh the auto switching preview, on the Global page only.
    ///
    /// The preview asks the same `rules::decide` the tray's engine does, so a
    /// rule that shows as matching here is a rule that will switch a profile
    /// when this window closes. It enumerates the desktop's windows, which is
    /// why it runs on a clock and only where it is drawn.
    fn sync_auto_preview(&mut self) {
        if self.page != Page::Global {
            return;
        }
        sync_auto_preview_text(
            &mut self.auto_preview,
            &mut self.auto_preview_at,
            Instant::now(),
            &self.rules_draft,
            &self.profiles,
            winwatch::snapshot,
        );
    }

    /// One evaluation of the engine that switches profiles while this window is
    /// open.
    ///
    /// Runs on every page, unlike the preview: the preview is only where it is
    /// drawn, but a switch has to happen wherever the user is looking. Decides
    /// from the **saved** rules — the preview is the draft — and holds while
    /// the profile or the rules draft has unsaved edits, so a switch can never
    /// land on top of a change in progress. Holding costs nothing: the engine
    /// is simply not told it applied, so it keeps returning the same decision
    /// and the switch lands by itself once the drafts are resolved.
    fn sync_auto_switch(&mut self) {
        let blocked = self.dirty || self.rules_dirty;
        match auto_switch_step(
            &mut self.auto_engine,
            &mut self.auto_switch_at,
            Instant::now(),
            &self.rules,
            blocked,
            winwatch::snapshot,
        ) {
            AutoSwitchStep::Idle => self.auto_hold = None,
            AutoSwitchStep::Held(profile) => {
                if self.auto_hold.as_deref() != Some(profile.as_str()) {
                    // The id is what the rules name; the display name is what
                    // the user reads everywhere else.
                    let name = self
                        .profiles
                        .iter()
                        .find(|entry| entry.id == profile)
                        .map(|entry| entry.name.as_str())
                        .unwrap_or(profile.as_str());
                    self.status = format!(
                        "Auto switch to \"{name}\" is waiting for your unsaved changes to be \
                         saved or discarded."
                    );
                    self.auto_hold = Some(profile);
                }
            }
            AutoSwitchStep::Apply(profile) => {
                self.auto_hold = None;
                // Recorded only on success, so a failed load or a renderer that
                // never took the config is retried on the next tick.
                if self.switch_profile(&profile) {
                    self.auto_engine.mark_applied(&profile);
                }
            }
        }
    }

    /// Make another profile the active one and remember the choice.
    ///
    /// Returns whether this profile is the active one afterwards, which is what
    /// the window's engine records as applied: a refused or failed switch must
    /// leave the engine wanting to retry rather than believing it landed.
    fn switch_profile(&mut self, id: &str) -> bool {
        self.profile_menu_open = false;
        self.selected_profile = Some(id.to_string());
        if id == self.active_profile {
            return true;
        }
        if !can_switch_profile(self.dirty) {
            self.status = "Save or discard your changes before switching profiles.".to_string();
            return false;
        }
        match self.store.load_profile(id) {
            Ok(mut stored) => {
                stored.normalize();
                self.active_profile = id.to_string();
                if let Err(error) = self.store.set_active_profile(&self.active_profile) {
                    self.status = format!("Could not update globalconfig.json: {error}");
                }
                self.selected_id = stored.overlays.first().map(|overlay| overlay.id.clone());
                // A different set of overlays entirely, so a host selected in the
                // profile being left behind is not a selection here.
                self.selected_target = None;
                self.apply_saved_config(stored);
                self.dirty = false;
                // Read after the load, so a name that is only stored in the
                // file is the one reported.
                self.refresh_profiles();
                self.status = format!("Loaded profile \"{}\".", self.active_profile_name());
                true
            }
            Err(error) => {
                self.status = format!("Could not load profile \"{id}\": {error}");
                false
            }
        }
    }

    /// Create a new empty profile, then load it. Returns false when the name
    /// was rejected.
    fn create_profile(&mut self, display_name: &str) -> bool {
        match self.store.create_profile(display_name) {
            Ok(created) => {
                self.refresh_profiles();
                self.selected_profile = Some(created.id.clone());
                if can_switch_profile(self.dirty) {
                    self.switch_profile(&created.id);
                } else {
                    self.status = format!(
                        "Created profile \"{}\" ({}). Save or discard your changes to load it.",
                        created.name, created.id
                    );
                }
                true
            }
            Err(error) => {
                self.status = format!("Could not create a profile: {error}");
                false
            }
        }
    }

    /// Rename a profile. Returns false when the new name was rejected.
    fn rename_profile(&mut self, from: &str, display_name: &str) -> bool {
        match self.store.rename_profile(from, display_name) {
            Ok(renamed) => {
                if self.active_profile == from {
                    self.active_profile = renamed.id.clone();
                    // Keep the draft in step with the file, so the next Save
                    // cannot write the previous name back into it.
                    self.config.profile_name = renamed.name.clone();
                    if let Err(error) = self.store.set_active_profile(&self.active_profile) {
                        self.status = format!("Could not update globalconfig.json: {error}");
                    }
                }
                // A rename can move the file, so the selection follows the id
                // rather than pointing at a profile that no longer exists.
                self.selected_profile = Some(renamed.id.clone());
                self.refresh_profiles();
                // Names may repeat, so the id is reported when the file moved.
                let moved = config::sanitize_profile_name(from).ok().as_deref()
                    != Some(renamed.id.as_str());
                self.status = if moved {
                    format!(
                        "Renamed profile \"{from}\" to \"{}\" ({}).",
                        renamed.name, renamed.id
                    )
                } else {
                    format!("Renamed profile \"{from}\" to \"{}\".", renamed.name)
                };
                true
            }
            Err(error) => {
                self.status = format!("Could not rename the profile: {error}");
                false
            }
        }
    }

    /// Copy a profile's overlays into a new profile. Returns false when the name
    /// was rejected.
    ///
    /// The copy is not loaded: switching is a separate, deliberate action, and
    /// doing it here would discard nothing but would still be a surprise.
    fn duplicate_profile(&mut self, from: &str, display_name: &str) -> bool {
        match self.store.duplicate_profile(from, display_name) {
            Ok(created) => {
                self.refresh_profiles();
                self.selected_profile = Some(created.id.clone());
                self.status = format!(
                    "Duplicated profile \"{}\" as \"{}\" ({}).",
                    self.profile_name(from),
                    created.name,
                    created.id
                );
                true
            }
            Err(error) => {
                self.status = format!("Could not duplicate the profile: {error}");
                false
            }
        }
    }

    fn delete_profile(&mut self, id: &str) {
        if self.profiles.len() <= 1 {
            self.status = "A profile cannot be deleted while it is the only one.".to_string();
            return;
        }
        // Move the selection off the profile that is about to disappear.
        if self.selected_profile.as_deref() == Some(id) {
            self.selected_profile = Some(self.active_profile.clone());
        }
        if id == self.active_profile && !can_switch_profile(self.dirty) {
            self.status =
                "Save or discard your changes before deleting the active profile.".to_string();
            return;
        }
        let name = self.profile_name(id);
        if let Err(error) = self.store.delete_profile(id) {
            self.status = format!("Could not delete profile \"{id}\": {error}");
            return;
        }
        self.refresh_profiles();
        if id == self.active_profile {
            // Prefer the default profile, otherwise fall back to whatever is
            // left so the app always has a configuration.
            let fallback = if self
                .profiles
                .iter()
                .any(|profile| profile.id == config::DEFAULT_PROFILE)
            {
                config::DEFAULT_PROFILE.to_string()
            } else {
                self.profiles
                    .first()
                    .map(|profile| profile.id.clone())
                    .unwrap_or_else(|| config::DEFAULT_PROFILE.to_string())
            };
            self.switch_profile(&fallback);
        } else {
            self.status = format!("Deleted profile \"{name}\".");
        }
    }

    fn toggle_running(&mut self) {
        self.running = !self.running;
        // The renderer owns the probes, so pausing is a command. `running` is
        // the window's mirror of it, which is what pane 2's Pause all button
        // reads back.
        self.send(Message::SetPaused {
            paused: !self.running,
        });
    }

    /// Pane 2 of the Overlays page: the profile switcher, the overlay list and
    /// the page's own actions.
    fn show_overlays_page(&mut self, ui: &mut Ui, height: f32) {
        let list_height = (height - SIDEBAR_HEADER_HEIGHT - SIDEBAR_FOOTER_HEIGHT).max(100.0);
        let column = list_pane_column(ui.available_width());
        let row_width = column.width;

        // The whole page is laid out in one centred column, so the switcher, the
        // rows and the footer cannot end up three different widths.
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(column.rect(ui, height)));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.allocate_ui(egui::vec2(row_width, SIDEBAR_HEADER_HEIGHT), |ui| {
                self.show_profile_switcher(ui, row_width);
            });
            ui.add_space(4.0);
            ui.allocate_ui(egui::vec2(column.scroll_width(), list_height), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.config.overlays.is_empty() {
                            ui.label(
                                RichText::new("No overlays yet. Add one to get started.")
                                    .color(UI_TEXT_SECONDARY()),
                            );
                        }
                        let rows: Vec<(String, String, Anchor, bool)> = self
                            .config
                            .overlays
                            .iter()
                            .map(|overlay| {
                                (
                                    overlay.id.clone(),
                                    overlay_row_label(overlay),
                                    overlay.position,
                                    overlay.enabled,
                                )
                            })
                            .collect();

                        for (id, name, indicator, enabled) in rows {
                            let active = self.selected_id.as_deref() == Some(id.as_str());
                            // Painted like a rail row: no border, and a fill only
                            // when the row is selected or hovered.
                            let (row_rect, row_response) = ui.allocate_exact_size(
                                egui::vec2(row_width, list_pane_row_height(OVERLAY_ROW_HEIGHT)),
                                egui::Sense::click(),
                            );
                            let background = if active {
                                Some(UI_SELECTION())
                            } else if row_response.hovered() {
                                Some(UI_SURFACE_ALT())
                            } else {
                                None
                            };
                            if let Some(background) = background {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(4),
                                    background,
                                );
                            }
                            let row = overlay_row_contents(
                                ui,
                                row_rect,
                                &name,
                                indicator,
                                enabled,
                                active,
                                self.confirm_delete.as_deref() == Some(id.as_str()),
                            );
                            let clicks = row.clicks;
                            if clicks.confirm_remove {
                                self.confirm_delete = None;
                                self.delete_overlay(&id);
                            } else if clicks.dismiss_confirm {
                                self.confirm_delete = None;
                            } else if clicks.name {
                                self.selected_id =
                                    toggled_selection(self.selected_id.as_deref(), id.as_str());
                                // The host selection belongs to the overlay it was
                                // made in, so it cannot follow the selection across
                                // to a different one.
                                self.selected_target = None;
                            } else if clicks.remove {
                                self.confirm_delete = Some(id.clone());
                            } else if clicks.toggle {
                                self.toggle_overlay(&id);
                            }
                        }

                        // Clicking the blank space under the last row clears the
                        // selection too. `ui.interact` registers a hit target
                        // without laying out a widget, so the strip cannot change
                        // the scroll area's content height and summon a scrollbar
                        // that the list was too short to deserve.
                        if let Some(strip) =
                            deselect_strip_rect(ui.max_rect(), ui.cursor().min.y, row_width)
                        {
                            let response = ui.interact(
                                strip,
                                ui.id().with("deselect_strip"),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_id = None;
                                self.selected_target = None;
                            }
                        }
                    });
            });

            ui.add_space(6.0);
            if ui
                .add_sized([row_width, 32.0], egui::Button::new("Add overlay"))
                .clicked()
            {
                self.add_overlay();
            }
            if ui
                .add_sized(
                    [row_width, 32.0],
                    egui::Button::new(if self.running {
                        "Pause all"
                    } else {
                        "Resume all"
                    }),
                )
                .clicked()
            {
                self.toggle_running();
            }
        });
    }

    /// Pane 2 of the Profiles page: the list of profiles and a create button.
    ///
    /// Selecting a row only selects it. Loading it is a separate action in the
    /// detail pane, because switching is refused while there are unsaved edits
    /// and a two-pane list should not have that side effect.
    fn show_profiles_page(&mut self, ui: &mut Ui, height: f32) {
        let list_height = (height - SIDEBAR_FOOTER_HEIGHT).max(100.0);
        let column = list_pane_column(ui.available_width());
        let row_width = column.width;
        let active = self.active_profile.clone();
        let selected = self.selected_profile.clone();
        let profiles = self.profiles.clone();
        let counts = self.profile_overlay_counts.clone();
        let dirty = self.dirty;

        // The same centred column the Overlays page uses, so the two pages' rows
        // and footers line up with each other as well as with their own pane.
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(column.rect(ui, height)));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.allocate_ui(egui::vec2(column.scroll_width(), list_height), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if profiles.is_empty() {
                            ui.label(
                                RichText::new("No profiles yet. Create one to get started.")
                                    .color(UI_TEXT_SECONDARY()),
                            );
                        }
                        let rows: Vec<(String, String, bool, bool, Option<usize>)> = profiles
                            .iter()
                            .map(|profile| {
                                let is_active = profile.id == active;
                                let count = counts.get(&profile.id).copied();
                                (
                                    profile.id.clone(),
                                    profile_row_label(profile, &profiles),
                                    is_active,
                                    selected.as_deref() == Some(profile.id.as_str()),
                                    count,
                                )
                            })
                            .collect();

                        for (id, label, is_active, is_selected, count) in rows {
                            // Painted like a rail row: no border, and a fill only
                            // when the row is selected or hovered.
                            let (row_rect, row_response) = ui.allocate_exact_size(
                                egui::vec2(row_width, list_pane_row_height(PROFILE_ROW_HEIGHT)),
                                egui::Sense::click(),
                            );
                            let background = if is_selected {
                                Some(UI_SELECTION())
                            } else if row_response.hovered() {
                                Some(UI_SURFACE_ALT())
                            } else {
                                None
                            };
                            if let Some(background) = background {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(4),
                                    background,
                                );
                            }
                            let row =
                                profile_row_contents(ui, row_rect, &label, count, is_active, dirty);
                            if row.clicked {
                                self.selected_profile = Some(id.clone());
                            }
                        }
                    });
            });

            ui.add_space(6.0);
            if ui
                .add_sized([row_width, 32.0], egui::Button::new("+ New profile"))
                .clicked()
            {
                self.profile_dialog = Some(ProfileDialog::Create {
                    name: String::new(),
                });
                self.profile_name_focus = true;
            }
        });
    }

    /// Pane 3 of the Profiles page: the selected profile's name, file and the
    /// actions that change it.
    ///
    /// An open editor or confirmation replaces the detail, so the pane never
    /// shows a form and the profile it acts on at the same time.
    fn show_profile_detail(&mut self, ui: &mut Ui) {
        if self.show_profile_dialog(ui) {
            return;
        }
        let Some(id) = self.selected_profile.clone() else {
            ui.label(
                RichText::new("Select a profile, or create a new one.").color(UI_TEXT_SECONDARY()),
            );
            return;
        };
        let entry = self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
            .unwrap_or_else(|| config::ProfileEntry {
                id: id.clone(),
                name: id.clone(),
            });
        let is_active = entry.id == self.active_profile;
        let count = self.profile_overlay_counts.get(&entry.id).copied();
        let only_profile = self.profiles.len() <= 1;
        let dirty = self.dirty;
        let mut action: Option<ProfileAction> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if entry.name.is_empty() {
                        "(unnamed profile)"
                    } else {
                        &entry.name
                    })
                    .heading()
                    .color(UI_TEXT()),
                );
                ui.add_space(2.0);
                // Names may repeat, so the file is what identifies the profile.
                ui.label(
                    RichText::new(config::profile_file_name(&entry.id))
                        .small()
                        .color(UI_TEXT_SECONDARY()),
                );
                // Nothing is said when the count is unknown, rather than a zero
                // that would be a confident lie.
                let count_label = overlay_count_label(count);
                if !count_label.is_empty() {
                    ui.label(
                        RichText::new(count_label)
                            .small()
                            .color(UI_TEXT_SECONDARY()),
                    );
                }
                ui.add_space(8.0);

                if is_active {
                    ui.label(
                        RichText::new("This is the active profile.").color(UI_TEXT_SECONDARY()),
                    );
                } else {
                    let mut switch_clicked = false;
                    ui.add_enabled_ui(can_switch_profile(dirty), |ui| {
                        switch_clicked = ui
                            .add_sized(
                                [PROFILE_ACTION_WIDTH, 32.0],
                                egui::Button::new(
                                    RichText::new("Switch to this profile").color(UI_TEXT()),
                                )
                                .fill(UI_ACCENT_STRONG()),
                            )
                            .clicked();
                    });
                    if switch_clicked {
                        action = Some(ProfileAction::Switch(entry.id.clone()));
                    }
                }
                if dirty {
                    // This page has no Save button of its own, so it has to say
                    // where the pending edits are and what to do with them.
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "The Overlays page has unsaved changes. Save or discard them there.",
                        )
                        .small()
                        .color(UI_TEXT_SECONDARY()),
                    );
                }

                ui.add_space(6.0);
                if ui
                    .add_sized([PROFILE_ACTION_WIDTH, 32.0], egui::Button::new("Rename"))
                    .clicked()
                {
                    self.profile_dialog = Some(ProfileDialog::Rename {
                        from: entry.id.clone(),
                        name: entry.name.clone(),
                    });
                    self.profile_name_focus = true;
                }
                if ui
                    .add_sized([PROFILE_ACTION_WIDTH, 32.0], egui::Button::new("Duplicate"))
                    .clicked()
                {
                    // Seeded with a distinct name, because duplicating onto the
                    // source's own name would just be a rename.
                    self.profile_dialog = Some(ProfileDialog::Duplicate {
                        from: entry.id.clone(),
                        name: format!("{} copy", entry.name),
                    });
                    self.profile_name_focus = true;
                }
                let mut delete_clicked = false;
                ui.add_enabled_ui(!only_profile, |ui| {
                    delete_clicked = ui
                        .add_sized(
                            [PROFILE_ACTION_WIDTH, 32.0],
                            egui::Button::new(RichText::new("Delete").color(UI_DANGER()))
                                .fill(UI_SURFACE_ALT()),
                        )
                        .clicked();
                });
                if delete_clicked {
                    self.profile_dialog = Some(ProfileDialog::Delete {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                    });
                }
            });

        if let Some(action) = action {
            // A rejected request keeps the page as it was.
            if self.apply_profile_action(action) {
                self.profile_dialog = None;
            }
        }
    }

    /// The profile switcher that heads pane 2: the active profile's display name
    /// with a painted arrow, opening the profile menu underneath.
    ///
    /// The menu anchors on this response, so it opens from the header instead of
    /// from a button in the footer.
    ///
    /// Painted rather than a `Button`, so the header carries no outline of its
    /// own and matches the rows below it. The fills are the ones the themed
    /// `Button` used, so it still looks like the control it replaced: the rest
    /// fill, the hover fill, and the selection while the menu is open.
    fn show_profile_switcher(&mut self, ui: &mut Ui, width: f32) {
        let name = self.active_profile_name();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
        let background = if self.profile_menu_open {
            UI_SELECTION()
        } else if response.hovered() {
            UI_SURFACE_HOVER()
        } else {
            UI_SURFACE_ALT()
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), background);
        let arrow = egui::Rect::from_min_max(
            egui::pos2(rect.right() - SWITCHER_TEXT_RIGHT_PAD, rect.top()),
            egui::pos2(rect.right() - SWITCHER_ARROW_PAD, rect.bottom()),
        );
        let text_width = arrow.left() - rect.left() - SWITCHER_TEXT_LEFT_PAD;
        let mut text = egui::text::LayoutJob::default();
        text.wrap.max_width = text_width.max(40.0);
        text.wrap.max_rows = 1;
        text.wrap.break_anywhere = true;
        text.append(
            &name,
            0.0,
            egui::TextFormat::simple(egui::TextStyle::Button.resolve(ui.style()), UI_TEXT()),
        );
        let galley = ui.painter().layout_job(text);
        let text_offset = (rect.height() - galley.size().y) / 2.0;
        ui.painter().galley(
            rect.left_top() + egui::vec2(SWITCHER_TEXT_LEFT_PAD, text_offset),
            galley,
            UI_TEXT(),
        );
        draw_dropdown_arrow(ui.painter(), arrow, UI_TEXT_SECONDARY());
        let response = response.on_hover_text(config::profile_file_name(&self.active_profile));
        if response.clicked() {
            self.profile_menu_open = !self.profile_menu_open;
            if self.profile_menu_open {
                self.refresh_profiles();
            }
        }
        self.show_profile_popup(ui, &response);
    }

    /// The navigation rail: one row per page plus the collapse toggle.
    ///
    /// Rows and glyphs are painted rather than built from buttons, so the label
    /// can sit beside the icon instead of under it and the collapsed rail needs
    /// no text at all.
    fn show_rail(&mut self, ui: &mut Ui) {
        let collapsed = self.rail_collapsed;
        let row_width = ui.available_width();
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add_space(4.0);
            for page in PAGES {
                let active = self.page == page;
                let color = if active {
                    UI_TEXT()
                } else {
                    UI_TEXT_SECONDARY()
                };
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(row_width, RAIL_ROW_HEIGHT),
                    egui::Sense::click(),
                );
                if active {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SELECTION());
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SURFACE_ALT());
                }
                let icon_rect = if collapsed {
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(RAIL_ICON_SIZE))
                } else {
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 14.0, rect.center().y - RAIL_ICON_SIZE / 2.0),
                        egui::Vec2::splat(RAIL_ICON_SIZE),
                    )
                };
                draw_nav_icon(
                    ui.painter(),
                    icon_rect,
                    page,
                    if active {
                        UI_ACCENT()
                    } else {
                        UI_TEXT_SECONDARY()
                    },
                );
                if !collapsed {
                    ui.painter().text(
                        egui::pos2(icon_rect.right() + 12.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        page.label(),
                        egui::FontId::proportional(RAIL_TEXT_SIZE),
                        color,
                    );
                }
                let mut hint = page.label().to_string();
                if page == Page::Overlays && self.dirty {
                    // The other pages have no Save button, so the rail is what
                    // says a draft is waiting on the Overlays page.
                    if !active {
                        draw_active_dot(ui.painter(), row_corner_dot_slot(rect), UI_ACCENT());
                    }
                    hint.push_str(" — unsaved changes");
                }
                let response = response.on_hover_text(hint);
                if response.clicked() {
                    self.page = page;
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(row_width, RAIL_ROW_HEIGHT),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SURFACE_ALT());
                }
                let icon_rect =
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(RAIL_ICON_SIZE));
                draw_collapse_chevron(ui.painter(), icon_rect, collapsed, UI_TEXT_SECONDARY());
                let hint = if collapsed {
                    "Expand the navigation rail"
                } else {
                    "Collapse the navigation rail"
                };
                let response = response.on_hover_text(hint);
                if response.clicked() {
                    self.rail_collapsed = !collapsed;
                }
            });
        });
    }

    /// Pane 2 for the current page. Only pages that have a list reach here.
    fn show_list_pane(&mut self, ui: &mut Ui, height: f32) {
        match self.page {
            Page::Overlays => self.show_overlays_page(ui, height),
            Page::Profiles => self.show_profiles_page(ui, height),
            // `config_ui` only allocates pane 2 when `page_has_list_pane` says
            // so, so a list-free page cannot reach this match. The arms are
            // still spelled out rather than folded into a catch-all so a new
            // page cannot be added without the compiler asking which case it is.
            Page::Global | Page::About => {}
        }
    }

    /// Pane 3 of the Global page: the app-wide preferences.
    ///
    /// The rail toggle applies immediately, so the user watches the rail
    /// collapse as they click, but the value is only written when the draft is
    /// saved, so Discard can put it back.
    fn show_global_page(&mut self, ui: &mut Ui) {
        let mut rail_collapsed = self.prefs_draft.ui.rail_collapsed;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("Global").heading().color(UI_TEXT()));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("These settings apply to the whole app, not to a profile.")
                        .color(UI_TEXT_SECONDARY()),
                );

                ui.add_space(12.0);
                ui.label(RichText::new("APPEARANCE").color(UI_ACCENT()));
                ui.separator();

                // Three painted tiles rather than a dropdown or selectable
                // labels: every option and the current one stay visible at once,
                // and the mode icon sits inside the button. Nothing here draws
                // its own glyph from the font, so no option depends on font
                // coverage — the same rule the rail's glyphs follow.
                if let Some(choice) = theme_tiles(ui, self.prefs_draft.ui.theme, self.system_mode) {
                    choose_theme(&mut self.prefs, &mut self.prefs_draft, choice);
                    self.prefs_dirty = true;
                    // `sync_theme` runs every pass and re-applies from the live
                    // preference, so the change lands on its own; this only asks
                    // for the frame sooner than the 100ms tick would.
                    ui.ctx().request_repaint();
                }

                let mut changed = false;
                ui.add_enabled_ui(true, |ui| {
                    changed = ui
                        .checkbox(&mut rail_collapsed, "Collapse the navigation rail")
                        .changed();
                });
                if changed {
                    self.prefs_draft.ui.rail_collapsed = rail_collapsed;
                    self.rail_collapsed = rail_collapsed;
                    self.prefs_dirty = true;
                }

                // The live value, not the draft, so the title changes as the box
                // is ticked. `discard_prefs` puts the stored value back, which
                // puts the title back with it.
                let mut show_version = self.prefs_draft.ui.show_version_in_title;
                if ui
                    .checkbox(&mut show_version, "Show the version in the window title")
                    .changed()
                {
                    self.prefs_draft.ui.show_version_in_title = show_version;
                    self.prefs.ui.show_version_in_title = show_version;
                    self.prefs_dirty = true;
                }

                // `sync_border_preview` reads the live preference every pass, so
                // this writes both through the helper, like `choose_theme`. The
                // border fades the moment it is unticked; Save is what makes
                // that survive a restart.
                let mut selection_border = self.prefs_draft.ui.selection_border_animation;
                if ui
                    .checkbox(
                        &mut selection_border,
                        "Animate the selected overlay's border",
                    )
                    .on_hover_text(
                        "Highlights the overlay whose settings are open. Switching \
                         profiles selects that profile's first overlay, so its \
                         border is shown then too. The startup border effect is a \
                         separate, per-overlay setting.",
                    )
                    .changed()
                {
                    set_selection_border_animation(
                        &mut self.prefs,
                        &mut self.prefs_draft,
                        selection_border,
                    );
                    self.prefs_dirty = true;
                }

                ui.add_space(12.0);
                self.show_auto_switch_section(ui);

                ui.add_space(12.0);
                ui.label(RichText::new("STORAGE").color(UI_ACCENT()));
                ui.separator();
                for (label, path) in [
                    (
                        "Config folder",
                        self.store.root().to_string_lossy().to_string(),
                    ),
                    (
                        "Profiles",
                        self.store.profiles_dir().to_string_lossy().to_string(),
                    ),
                    (
                        "Global config",
                        self.store
                            .global_config_path()
                            .to_string_lossy()
                            .to_string(),
                    ),
                ] {
                    ui.horizontal(|ui| {
                        let label_width = 96.0;
                        let gap = ui.spacing().item_spacing.x;
                        let path_width =
                            (ui.available_width() - label_width - gap - STORAGE_PATH_MIN).max(60.0);
                        ui.add_sized(
                            [label_width, 26.0],
                            egui::Label::new(RichText::new(label).color(UI_TEXT_SECONDARY())),
                        );
                        ui.add_sized(
                            [path_width, 26.0],
                            egui::Label::new(RichText::new(&path).color(UI_TEXT())).truncate(),
                        )
                        .on_hover_text(path.clone());
                    });
                }
            });
    }

    /// The Global page's auto profile switching section.
    ///
    /// Edits stage into `rules_draft` and follow the page's Save/Discard the
    /// way the preferences do: nothing here reaches `rules.json` until Save,
    /// and nothing here reaches the engine until this window closes, because
    /// the tray pauses its engine while the window is open.
    fn show_auto_switch_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("AUTO PROFILE SWITCHING").color(UI_ACCENT()));
        ui.separator();
        ui.label(
            RichText::new(
                "Switch the active profile to match the windows that are open. Rules are \
                 applied while this window is closed.",
            )
            .color(UI_TEXT_SECONDARY()),
        );

        // Live as well as staged, like the Appearance checkboxes: the renderer
        // learns the value by comparing it against what it was told on every
        // pass, so a draft-only write left kept probes running after the box
        // was unticked. `set_background_tracking` is the one place that writes
        // it, so the two halves cannot drift.
        ui.add_space(4.0);
        let mut background = self.prefs_draft.ui.background_tracking;
        if ui
            .checkbox(&mut background, "Keep tracking profiles in the background")
            .on_hover_text(
                "A profile you switch away from keeps probing, so its graph is continuous \
                 when you switch back. A host you disable or delete keeps probing until you \
                 save; saving the removal stops it for good. One probe per second per kept \
                 host; Pause still stops all probing.",
            )
            .changed()
        {
            set_background_tracking(&mut self.prefs, &mut self.prefs_draft, background);
            self.prefs_dirty = true;
        }

        // An unreadable file is never rewritten silently, so the error stays
        // visible until the user either fixes the file or edits a rule and
        // saves over it.
        if let Some(error) = &self.rules_error {
            ui.label(
                RichText::new(format!(
                    "{error} The file has been left as it is; editing a rule and saving \
                     replaces it."
                ))
                .color(UI_DANGER()),
            );
        }

        if auto_switch_section(
            ui,
            &mut self.rules_draft,
            &self.profiles,
            &self.active_profile,
        ) {
            self.rules_dirty = true;
        }

        ui.add_space(6.0);
        match &self.auto_preview {
            Some(line) => {
                ui.label(RichText::new(format!("Preview: {line}")).color(UI_TEXT_SECONDARY()));
            }
            None => {
                ui.label(
                    RichText::new("Switching is off; no profile is chosen automatically.")
                        .color(UI_TEXT_SECONDARY()),
                );
            }
        }
    }

    /// Pane 3 of the About page: what this is, which version, and who wrote it.
    ///
    /// Read-only, and it has no list pane, so it gets the full width like the
    /// Global page. It is one centred column rather than a labelled table,
    /// because the page used to read as settings instead of as an About box.
    /// The facts come from `about_page_lines` so they can be tested; the
    /// centring deliberately uses `with_layout`, see the note there.
    fn show_about_page(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                about_page_column(ui, &self.about_logo);
            });
    }

    /// Opens any URL the frame's widgets asked for.
    ///
    /// Drained at the END of the UI pass, not in `logic`, because the command is
    /// produced while the About page is being drawn; `logic` runs before the
    /// panes are laid out, so draining there would open the link a frame late.
    fn open_requested_urls(&mut self, ctx: &Context) {
        let mut requested: Vec<String> = Vec::new();
        ctx.output_mut(|output| {
            output.commands.retain(|command| {
                if let Some(url) = requested_url(command) {
                    requested.push(url.to_string());
                    false
                } else {
                    true
                }
            });
        });
        for url in requested {
            // Reported either way, so a click is never silently ignored. A
            // successful launch leaves the URL in the status bar too, which is
            // the only confirmation a user gets that the click was read at all.
            self.status = if open_url_in_browser(&url) {
                format!("Opened {url}.")
            } else {
                format!("Could not open {url}.")
            };
        }
    }

    /// Profile switcher menu shown under the pane 2 header.
    ///
    /// The menu only switches, because switching is the one profile action that
    /// can be wrong: it is refused while there are unsaved edits. Creating,
    /// renaming, duplicating and deleting need room for an editor and a
    /// confirmation, so they live on the Profiles page.
    fn show_profile_popup(&mut self, ui: &mut Ui, anchor: &egui::Response) {
        let active = self.active_profile.clone();
        let profiles = self.profiles.clone();
        let dirty = self.dirty;
        let mut switch_to: Option<String> = None;
        let mut manage_clicked = false;

        // egui owns the open flag through `open_bool`, so it can close the popup
        // on a click outside or Escape without treating the click that opened
        // it as an outside click.
        let _popup = egui::containers::Popup::from_response(anchor)
            .open_bool(&mut self.profile_menu_open)
            .close_behavior(egui::containers::PopupCloseBehavior::CloseOnClickOutside)
            .layout(Layout::top_down(Align::Min))
            .width(PROFILE_POPUP_WIDTH)
            .frame(Frame::popup(ui.style()).fill(UI_SURFACE()))
            .show(|ui| {
                ui.set_width(PROFILE_POPUP_WIDTH);
                let header = ui.label(RichText::new("Profiles").strong().color(UI_TEXT()));
                header.on_hover_text(self.store.profiles_dir().display().to_string());
                ui.separator();

                for profile in &profiles {
                    let current = profile.id == active;
                    // Pending edits dim every other profile, because
                    // switching is refused until the draft is saved.
                    let color = if current {
                        UI_ACCENT()
                    } else if dirty {
                        UI_TEXT_SECONDARY()
                    } else {
                        UI_TEXT()
                    };
                    let row = ui.add_sized(
                        [ui.available_width(), PROFILE_ROW_HEIGHT],
                        egui::Button::new(
                            RichText::new(profile_row_label(profile, &profiles)).color(color),
                        )
                        .truncate(),
                    );
                    if row.clicked() {
                        switch_to = Some(profile.id.clone());
                    }
                    row.on_hover_text(config::profile_file_name(&profile.id));
                }

                ui.separator();
                if ui
                    .add_sized(
                        [ui.available_width(), PROFILE_ROW_HEIGHT],
                        egui::Button::new("Manage profiles"),
                    )
                    .clicked()
                {
                    manage_clicked = true;
                }

                if dirty {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("Save or discard changes before switching profiles.")
                            .small()
                            .color(UI_TEXT_SECONDARY()),
                    );
                }
            });

        // The menu is only a switcher, so the request runs after it closes.
        if manage_clicked {
            self.page = Page::Profiles;
            self.profile_menu_open = false;
        }
        if let Some(id) = switch_to {
            self.switch_profile(&id);
        }
    }

    /// The inline editor or confirmation for a profile request, drawn in place
    /// of the Profiles detail. Returns true while one is open.
    ///
    /// The dialog is taken out for the frame so the editor can mutate it
    /// without borrowing the app state, then the edited value is stored back.
    /// Snapshotting it here would discard whatever was typed.
    fn show_profile_dialog(&mut self, ui: &mut Ui) -> bool {
        if self.profile_dialog.is_none() {
            return false;
        }
        // Taken, not copied: the field is focused exactly once, when the editor
        // opens, so a later frame must not steal the caret.
        let focus_field = std::mem::take(&mut self.profile_name_focus);
        let mut dialog = self.profile_dialog.take();
        let mut action: Option<ProfileAction> = None;
        // Assigned after the match, which borrows the dialog.
        let mut close_dialog = false;

        match &mut dialog {
            Some(ProfileDialog::Create { name }) => {
                ui.add_space(4.0);
                ui.label(RichText::new("New profile").heading().color(UI_TEXT()));
                ui.label(
                    RichText::new("A new profile starts empty and gets its own file.")
                        .small()
                        .color(UI_TEXT_SECONDARY()),
                );
                let (submit, cancel) = profile_name_field(ui, name, "Create", focus_field);
                if submit {
                    action = Some(ProfileAction::Create(name.clone()));
                }
                close_dialog = cancel;
            }
            Some(ProfileDialog::Rename { from, name }) => {
                ui.add_space(4.0);
                ui.label(RichText::new("Rename profile").heading().color(UI_TEXT()));
                // Names may repeat, so the file is shown: a taken name gives the
                // new profile a postfixed file instead of an error.
                ui.label(
                    RichText::new(format!(
                        "{} will be written as a new file if the name is taken.",
                        config::profile_file_name(from)
                    ))
                    .small()
                    .color(UI_TEXT_SECONDARY()),
                );
                let (submit, cancel) = profile_name_field(ui, name, "Rename", focus_field);
                if submit {
                    action = Some(ProfileAction::Rename {
                        from: from.clone(),
                        to: name.clone(),
                    });
                }
                close_dialog = cancel;
            }
            Some(ProfileDialog::Duplicate { from, name }) => {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Duplicate profile")
                        .heading()
                        .color(UI_TEXT()),
                );
                ui.label(
                    RichText::new(format!(
                        "The overlays of {} are copied into a new profile.",
                        config::profile_file_name(from)
                    ))
                    .small()
                    .color(UI_TEXT_SECONDARY()),
                );
                let (submit, cancel) = profile_name_field(ui, name, "Duplicate", focus_field);
                if submit {
                    action = Some(ProfileAction::Duplicate {
                        from: from.clone(),
                        to: name.clone(),
                    });
                }
                close_dialog = cancel;
            }
            Some(ProfileDialog::Delete { id, name }) => {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Delete \"{name}\"?"))
                        .heading()
                        .color(UI_DANGER()),
                );
                // Names may repeat, so the file confirms what goes.
                ui.label(
                    RichText::new(config::profile_file_name(id))
                        .small()
                        .color(UI_TEXT_SECONDARY()),
                );
                if id == &self.active_profile {
                    ui.label(
                        RichText::new("This is the active profile, so another one is loaded next.")
                            .small()
                            .color(UI_TEXT_SECONDARY()),
                    );
                }
                let mut confirm = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    confirm = ui
                        .add_sized(
                            [96.0, DETAIL_FOOTER_BUTTON_HEIGHT],
                            egui::Button::new(RichText::new("Delete").color(UI_TEXT()))
                                .fill(UI_DANGER_STRONG()),
                        )
                        .clicked();
                    cancel = ui
                        .add_sized(
                            [96.0, DETAIL_FOOTER_BUTTON_HEIGHT],
                            egui::Button::new("Cancel"),
                        )
                        .clicked();
                });
                if confirm {
                    action = Some(ProfileAction::Delete(id.clone()));
                }
                close_dialog = cancel;
            }
            None => {}
        }
        if close_dialog {
            dialog = None;
        }
        self.profile_name_focus = focus_field;
        self.profile_dialog = dialog;

        if let Some(action) = action {
            // A rejected name keeps the editor open so it can be corrected.
            if self.apply_profile_action(action) {
                self.profile_dialog = None;
            }
        }
        self.profile_dialog.is_some()
    }

    /// Apply a profile request. Returns false when it was rejected.
    fn apply_profile_action(&mut self, action: ProfileAction) -> bool {
        match action {
            ProfileAction::Switch(name) => {
                self.switch_profile(&name);
                true
            }
            ProfileAction::Create(name) => self.create_profile(&name),
            ProfileAction::Rename { from, to } => self.rename_profile(&from, &to),
            ProfileAction::Duplicate { from, to } => self.duplicate_profile(&from, &to),
            ProfileAction::Delete(name) => {
                self.delete_profile(&name);
                true
            }
        }
    }

    fn show_editor(&mut self, ui: &mut Ui) {
        // This is the pane's default state, not a dead end: nothing is selected
        // until the user picks an overlay, and picking an already-selected row
        // (or the blank space under the list) comes back here.
        let Some(selected_id) = self.selected_id.clone() else {
            empty_editor(ui);
            return;
        };
        let Some(index) = self
            .config
            .overlays
            .iter()
            .position(|overlay| overlay.id == selected_id)
        else {
            // The selected overlay is gone, so there is nothing to show. Fall
            // back to the empty state rather than silently focusing a different
            // overlay, which would start a border nobody asked for.
            self.selected_id = None;
            self.selected_target = None;
            return;
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if self.config.overlays[index].name.is_empty() {
                        "(unnamed overlay)"
                    } else {
                        &self.config.overlays[index].name
                    })
                    .heading()
                    .color(UI_TEXT()),
                );
                ui.add_space(8.0);
                self.show_target_list(ui, index);
                ui.add_space(4.0);
                let mut changed = false;
                let selected_target = self.selected_target.clone();
                edit_overlay(
                    ui,
                    &mut self.config.overlays[index],
                    selected_target.as_deref(),
                    &mut changed,
                    &mut self.position_picker,
                    &mut self.sticky_picker,
                    &self.monitors,
                    &self.theme,
                );
                if changed {
                    self.dirty = true;
                    self.status.clear();
                }
            });
    }

    /// The hosts of the selected overlay, above the editor for the chosen one.
    ///
    /// In the detail pane rather than the list pane because a list row has one
    /// click target and this has four, and because the two selections are
    /// nested: which overlay you are editing, and which of its hosts.
    fn show_target_list(&mut self, ui: &mut Ui, overlay_index: usize) {
        // Cloned because the rows are painted and the closure that handles their
        // clicks needs `&mut self`, which the borrow of the overlay forbids.
        let targets = self.config.overlays[overlay_index].targets.clone();
        let selected = self.selected_target.clone();
        let mut remove: Option<usize> = None;
        let mut toggle: Option<usize> = None;
        let mut select: Option<usize> = None;

        section(ui, "Hosts", |ui| {
            for (index, target) in targets.iter().enumerate() {
                let active = selected.as_deref() == Some(target.id.as_str());
                let row = ui.allocate_exact_size(
                    egui::vec2(
                        ui.available_width(),
                        list_pane_row_height(TARGET_ROW_HEIGHT),
                    ),
                    egui::Sense::click(),
                );
                if active {
                    ui.painter()
                        .rect_filled(row.0, egui::CornerRadius::same(4), UI_SELECTION());
                } else if row.1.hovered() {
                    ui.painter()
                        .rect_filled(row.0, egui::CornerRadius::same(4), UI_SURFACE_ALT());
                }
                let clicks = target_row_contents(ui, row.0, target, active, index + 1);
                if clicks.remove {
                    remove = Some(index);
                } else if clicks.toggle {
                    toggle = Some(index);
                } else if clicks.label {
                    select = Some(index);
                }
            }
            ui.add_space(4.0);
            if ui
                .add_sized([ui.available_width(), 28.0], egui::Button::new("Add host"))
                .clicked()
            {
                self.config.overlays[overlay_index].add_target();
                self.dirty = true;
                self.status.clear();
            }
        });

        // A row click toggles, exactly as the overlay rows do, so clicking the
        // host you are already editing puts the editor back to nothing rather
        // than leaving a selection that looks stuck.
        if let Some(index) = select {
            let id = targets[index].id.clone();
            self.selected_target = toggled_selection(selected.as_deref(), id.as_str());
        }
        if let Some(index) = toggle {
            let target = &mut self.config.overlays[overlay_index].targets[index];
            target.enabled = !target.enabled;
            self.dirty = true;
            self.status.clear();
        }
        if let Some(index) = remove {
            let id = targets[index].id.clone();
            let last = targets.len() == 1;
            if last {
                // An overlay with no targets has nothing to draw and nothing to
                // probe, and `normalize` would put one back on the next load —
                // so the last host is not removable, and says why.
                self.status = "The last host cannot be removed.".to_string();
            } else {
                self.config.overlays[overlay_index].targets.remove(index);
                if self.selected_target.as_deref() == Some(id.as_str()) {
                    self.selected_target = None;
                }
                self.dirty = true;
                self.status.clear();
            }
        }
    }

    /// Pane 3: the page's own content, plus the sticky Save/Discard footer on the
    /// pages that stage a draft.
    ///
    /// The footer is not drawn on the Profiles page, which has nothing to save, so
    /// that page's detail gets the full height. Its unsaved-overlay hint and the
    /// dot on the Overlays rail row are what make a pending draft visible there.
    fn show_detail_pane(&mut self, ui: &mut Ui, height: f32) {
        let has_footer = page_has_detail_footer(self.page);
        let content_height = if has_footer {
            (height - DETAIL_FOOTER_HEIGHT).max(120.0)
        } else {
            height
        };
        ui.allocate_ui(
            egui::vec2(ui.available_width(), content_height),
            |ui| match self.page {
                Page::Overlays => self.show_editor(ui),
                Page::Profiles => self.show_profile_detail(ui),
                Page::Global => self.show_global_page(ui),
                Page::About => self.show_about_page(ui),
            },
        );
        if has_footer {
            ui.add_space(4.0);
            self.show_detail_footer(ui);
        }
    }

    /// Save and Discard, right aligned under the detail pane.
    ///
    /// They mirror pane 2's footer, which holds that page's own actions, and both
    /// are enabled only while there is something to write, which is also how
    /// pending edits stay visible.
    fn show_detail_footer(&mut self, ui: &mut Ui) {
        // Either draft counts, not just the profile one. The Global page stages
        // into `prefs_dirty` and never touches `dirty`, so gating on `dirty`
        // alone left Save and Discard permanently greyed out there, which meant
        // every Global preference was silently discarded on close. That shipped
        // for the rail-collapse preference from its first release.
        let pending = self.has_pending_edits();
        ui.allocate_ui(
            egui::vec2(ui.available_width(), DETAIL_FOOTER_HEIGHT),
            |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Right to left, so Save lands rightmost as the primary
                    // action and Discard sits to its left.
                    let mut save_clicked = false;
                    ui.add_enabled_ui(pending, |ui| {
                        save_clicked = ui
                            .add_sized(
                                [DETAIL_FOOTER_BUTTON_WIDTH, DETAIL_FOOTER_BUTTON_HEIGHT],
                                egui::Button::new(RichText::new("Save").color(UI_TEXT()))
                                    .fill(UI_ACCENT_STRONG()),
                            )
                            .clicked();
                    });
                    let mut discard_clicked = false;
                    ui.add_enabled_ui(pending, |ui| {
                        discard_clicked = ui
                            .add_sized(
                                [DETAIL_FOOTER_BUTTON_WIDTH, DETAIL_FOOTER_BUTTON_HEIGHT],
                                egui::Button::new("Discard"),
                            )
                            .clicked();
                    });
                    if save_clicked {
                        self.save_edits();
                    } else if discard_clicked {
                        self.discard_edits();
                    }
                });
            },
        );
    }

    /// The Config window: a navigation rail, a list pane and a detail pane.
    ///
    /// The Global page has no list, so pane 2 is dropped and the detail pane
    /// takes its place. The status bar spans the full width underneath.
    fn config_ui(&mut self, ui: &mut Ui) {
        let frame = Frame::central_panel(ui.style())
            .fill(UI_BACKGROUND())
            .inner_margin(egui::Margin {
                left: PANE_MARGIN as i8,
                right: PANE_MARGIN as i8,
                top: PANE_MARGIN as i8,
                bottom: 0,
            });
        frame.show(ui, |ui| {
            ui.vertical(|ui| {
                let content_height = (ui.available_height() - STATUS_BAR_HEIGHT).max(160.0);
                ui.horizontal_top(|ui| {
                    ui.set_height(content_height);
                    let rail_width = rail_width(self.rail_collapsed);
                    ui.allocate_ui(egui::vec2(rail_width, content_height), |ui| {
                        self.show_rail(ui);
                    });
                    draw_pane_divider(ui);
                    if page_has_list_pane(self.page) {
                        ui.allocate_ui(egui::vec2(SIDEBAR_WIDTH, content_height), |ui| {
                            self.show_list_pane(ui, content_height);
                            // A list pane page draws itself as a positioned
                            // child, and a positioned child registers nothing
                            // with the layout that allocated it: `advance_after_rects`
                            // sets the cursor from the pane's own `min_rect`, which
                            // is empty. So the pane claims its width here. Without
                            // it the next divider and the detail pane are laid out
                            // from the list pane's left edge, on top of it.
                            ui.advance_cursor_after_rect(ui.max_rect());
                        });
                        draw_pane_divider(ui);
                    }
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.set_height(content_height);
                        self.show_detail_pane(ui, content_height);
                    });
                });
                self.show_status_bar(ui);
                // Last, so a click on the About page's link is read in the same
                // frame that drew it.
                self.open_requested_urls(ui.ctx());
            });
        });
    }

    /// The status bar spans the whole window and carries transient messages
    /// beside the right-aligned version label.
    ///
    /// Save and Discard used to live here so they would be reachable from every
    /// page. They are in the detail pane's footer now, next to the edits they
    /// write, so this only reads state and needs no clone.
    /// The bottom row: transient operation messages only.
    ///
    /// The version used to be right-aligned here and now lives on the About
    /// page, optionally in the window title. It is a paint-only function, so
    /// unlike the rest of the window layout there is no test that can hold it
    /// to shape, which is worth knowing before changing what goes in it.
    fn show_status_bar(&self, ui: &mut Ui) {
        ui.allocate_ui(egui::vec2(ui.available_width(), STATUS_BAR_HEIGHT), |ui| {
            let message = self.status.as_str();
            let response = ui.add(egui::Label::new(message).truncate());
            if !message.is_empty() {
                response.on_hover_text(message);
            }
        });
    }
}

/// One-time startup results, rendered into the Config status bar.
/// Append a status-bar message to whatever is already there.
///
/// A space only when there is something to separate: an empty status plus a
/// message must not come out as a leading space, which reads as a missing
/// word rather than as a boundary.
fn append_status_message(status: String, message: Option<&str>) -> String {
    match message {
        Some(message) if status.is_empty() => message.to_string(),
        Some(message) => format!("{status} {message}"),
        None => status,
    }
}

fn config_notices_status(notices: &[config::ConfigNotice]) -> String {
    notices
        .iter()
        .map(config_notice_status)
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn config_notice_status(notice: &config::ConfigNotice) -> String {
    match notice {
        config::ConfigNotice::Migrated {
            legacy_directory_retained: false,
        } => "Config migrated to ~/.config/.PingLatencyOverlay.".to_string(),
        config::ConfigNotice::Migrated {
            legacy_directory_retained: true,
        } => "Config migrated to ~/.config/.PingLatencyOverlay; legacy directory was retained."
            .to_string(),
        config::ConfigNotice::MigrationFailed(error) => {
            format!("Config migration failed: {error}. Legacy config was kept.")
        }
        config::ConfigNotice::ProfileImported => {
            "config.json moved to profiles/profile_default.json.".to_string()
        }
        config::ConfigNotice::ProfileImportSkipped => {
            "config.json was kept because a default profile already exists.".to_string()
        }
        config::ConfigNotice::ProfileImportFailed(error) => {
            format!("Profile import failed: {error}. config.json was kept.")
        }
        config::ConfigNotice::ProfileNamesBackfilled { count } => {
            let plural = if *count == 1 { "profile" } else { "profiles" };
            format!("Added a display name to {count} existing {plural}.")
        }
        config::ConfigNotice::ProfileFallback { profile, reason } => {
            format!("Could not use profile \"{profile}\" ({reason}); using the default profile.")
        }
        config::ConfigNotice::GlobalConfigFailed(error) => {
            format!("Could not update globalconfig.json: {error}")
        }
    }
}

/// Pending edits must be resolved before another profile can be loaded.
fn can_switch_profile(dirty: bool) -> bool {
    !dirty
}

/// Whether any draft has something to write.
///
/// There are three independent drafts: the profile draft (`dirty`), the
/// app-wide preferences draft (`prefs_dirty`), and the auto profile switching
/// rules (`rules_dirty`). A page stages into one of them, so anything asking
/// "is there anything to save?" has to ask about all three. Free function so a
/// test can drive the rule rather than assert a predicate nothing acts on,
/// which is how the two-draft version shipped broken.
fn pending_edits(dirty: bool, prefs_dirty: bool, rules_dirty: bool) -> bool {
    dirty || prefs_dirty || rules_dirty
}

/// The Config window title, which names the active profile and optionally carries
/// the app version after it.
fn window_title(profile_name: &str, show_version: bool) -> String {
    if show_version {
        format!(
            "PingLatencyOverlay - Current Profile: {profile_name} (v{})",
            env!("APP_BUILD_VERSION")
        )
    } else {
        format!("PingLatencyOverlay - Current Profile: {profile_name}")
    }
}

/// Row text for a profile.
///
/// Display names may repeat, so a row that shares its name with another one also
/// shows its id, which is the only part that is unique.
fn profile_row_label(profile: &config::ProfileEntry, profiles: &[config::ProfileEntry]) -> String {
    let shared_name = profiles
        .iter()
        .filter(|other| other.name == profile.name)
        .count()
        > 1;
    if shared_name {
        format!("{} ({})", profile.name, profile.id)
    } else {
        profile.name.clone()
    }
}

/// Width of the name button in a profile row.
///
/// The frame's margin, the gap between the two widgets and the trailing gutter all
/// come out of the row, so the name truncates instead of pushing the overlay count
/// and the active dot out of the pane.
/// The width the list pane's header, rows and footer all use.
///
/// Every one of them goes through here, so they line up with each other and
/// with the pane edges. The scroll bar's width is always reserved, otherwise
/// the rows would jump sideways the moment a list outgrew the pane.
/// The height of a painted list pane row.
///
/// A painted rect does not size itself to its contents the way an
/// `egui::Frame` does, so the margin has to come out of the row's own height.
/// Without this the child widgets fill the row edge to edge and the buttons
/// touch the fill.
fn list_pane_row_height(content_height: f32) -> f32 {
    content_height + ROW_MARGIN * 2.0
}

fn list_pane_row_width(pane_width: f32, bar_width: f32) -> f32 {
    (pane_width - LIST_PANE_INSET * 2.0 - bar_width).max(120.0)
}

/// The list pane's rows, given the width it was allocated.
///
/// `LIST_PANE_INSET` comes off each side and the scroll bar's home comes off the
/// right, so the row is narrower than its pane on both sides. `list_pane_column`
/// is what puts it back on the left.
fn list_pane_row_width_for(allocated: f32) -> f32 {
    list_pane_row_width(allocated, SCROLL_BAR_RESERVE)
}

/// Where the list pane's content sits inside the pane it was allocated.
///
/// One column, centred, so both of the pane's boundaries get the same margin.
/// The rows used to be allocated flush against the pane's left edge while
/// `LIST_PANE_INSET` was only ever subtracted from their width, so the entire
/// shortfall landed on the right: the left boundary read 4px and the right one
/// 40px, which is what made the panes look unevenly spaced.
struct ListPaneColumn {
    /// Distance from the pane's left edge to the column's left edge.
    inset: f32,
    /// The width shared by the switcher, the rows and the footer.
    width: f32,
}

impl ListPaneColumn {
    /// The width of the scrolling area that holds the rows.
    ///
    /// One `SCROLL_BAR_RESERVE` wider than the column, so the bar has a home of
    /// its own and never covers a row or shifts one sideways when the list
    /// outgrows the pane. It is the only thing in the pane allowed past the
    /// column's right edge, and it has nothing painted in it but the bar.
    fn scroll_width(&self) -> f32 {
        self.width + SCROLL_BAR_RESERVE
    }

    /// The column's rect, `height` tall, starting at the pane's top left.
    fn rect(&self, pane: &Ui, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            pane.min_rect().left_top() + egui::vec2(self.inset, 0.0),
            egui::vec2(self.width, height),
        )
    }
}

fn list_pane_column(allocated: f32) -> ListPaneColumn {
    let width = list_pane_row_width_for(allocated);
    ListPaneColumn {
        inset: ((allocated - width) / 2.0).max(0.0),
        width,
    }
}

/// The hairline and the gap that separate two panes.
///
/// Both boundaries in the window go through this, so the middle pane is spaced
/// the same on either side of it.
fn draw_pane_divider(ui: &mut Ui) {
    let x = ui.cursor().max.x + PANE_GAP / 2.0;
    ui.painter().vline(
        x,
        ui.min_rect().y_range(),
        egui::Stroke::new(1.0, UI_BORDER()),
    );
    ui.add_space(PANE_GAP);
}

/// The area a list pane row's contents live in: the row's painted fill minus the
/// same margin on every side.
///
/// Both halves of a row are measured from this one rect, so the name on the left
/// and the controls on the right cannot disagree about where the row ends.
fn row_inner(row: egui::Rect) -> egui::Rect {
    row.shrink2(egui::Vec2::splat(ROW_MARGIN))
}

/// Which part of an overlay row the user clicked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OverlayRowClicks {
    name: bool,
    remove: bool,
    toggle: bool,
    confirm_remove: bool,
    dismiss_confirm: bool,
}

/// What one overlay row laid out, and which part was clicked.
///
/// The rects are reported so a test can measure where the row really put things.
/// `name` starts at the row's left edge and `controls` ends at its right edge;
/// both live inside `row_inner(row)`. Only the clicks are read at runtime.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct OverlayRow {
    clicks: OverlayRowClicks,
    name: egui::Rect,
    controls: egui::Rect,
}

/// Lay out one overlay row's contents inside an already painted `row`.
///
/// The name takes whatever the row's left edge and its controls leave over, and
/// the controls are pinned to its right edge, so neither side can push the other
/// out of the fill. This lives in one function so the geometry can be measured in
/// a test: a row's contents shipped outside its fill twice, once because the row
/// was allocated too short and once because a nested `ui.horizontal` shifted
/// everything down by half a button.
fn overlay_row_contents(
    ui: &mut Ui,
    row: egui::Rect,
    name: &str,
    anchor: Anchor,
    enabled: bool,
    active: bool,
    confirming: bool,
) -> OverlayRow {
    let inner = row_inner(row);
    let gap = ui.spacing().item_spacing.x;
    let mut clicks = OverlayRowClicks::default();
    let mut ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if confirming {
        let label = ui.label(RichText::new(format!("Delete \"{name}\"?")).color(UI_DANGER()));
        // The pair is pinned to the row's right edge and the filler between the
        // question and the pair takes whatever is left, so neither can overflow.
        let controls = right_anchored(inner, confirm_controls_width(gap));
        let filler = (controls.left() - label.rect.right() - gap).max(0.0);
        let filler_response = ui.allocate_response(
            egui::vec2(filler, OVERLAY_CONFIRM_HEIGHT),
            egui::Sense::click(),
        );
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(controls)
                .layout(Layout::left_to_right(Align::Center)),
        );
        clicks.confirm_remove = buttons
            .add_sized(
                [OVERLAY_CONFIRM_OK_WIDTH, OVERLAY_CONFIRM_HEIGHT],
                egui::Button::new(RichText::new("OK").color(UI_TEXT())).fill(UI_DANGER_STRONG()),
            )
            .clicked();
        clicks.dismiss_confirm = label.clicked()
            || filler_response.clicked()
            || buttons
                .add_sized(
                    [OVERLAY_CONFIRM_CANCEL_WIDTH, OVERLAY_CONFIRM_HEIGHT],
                    egui::Button::new("X"),
                )
                .clicked();
        return OverlayRow {
            clicks,
            name: label.rect,
            controls,
        };
    }

    let (tab_rect, mut tab_response) = ui.allocate_exact_size(
        egui::vec2(overlay_name_width(inner.width(), gap), OVERLAY_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let indicator_size = 22.0;
    let indicator_rect = egui::Rect::from_min_size(
        tab_rect.left_top(),
        egui::vec2(indicator_size, tab_rect.height()),
    );
    // Reuse the existing blue palette: muted default, bright active selection.
    let indicator_color = if active {
        UI_ACCENT()
    } else {
        UI_ACCENT_STRONG()
    };
    draw_position_indicator(ui.painter(), indicator_rect, anchor, indicator_color);
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(indicator_rect.right() + 6.0, tab_rect.top()),
        tab_rect.right_bottom(),
    );
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut tab_text = egui::text::LayoutJob::default();
    tab_text.wrap.max_width = text_rect.width();
    tab_text.wrap.max_rows = 1;
    tab_text.wrap.break_anywhere = true;
    tab_text.append(name, 0.0, egui::TextFormat::simple(font_id, UI_TEXT()));
    let galley = ui.painter().layout_job(tab_text);
    let text_offset = (text_rect.height() - galley.size().y) / 2.0;
    ui.painter().galley(
        text_rect.left_top() + egui::vec2(0.0, text_offset),
        galley,
        UI_TEXT(),
    );
    if tab_response.hovered() {
        tab_response = tab_response.on_hover_text(position_name(anchor));
    }
    clicks.name = tab_response.clicked();

    let controls = right_anchored(inner, overlay_controls_width(gap));
    let mut actions = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls)
            .layout(Layout::left_to_right(Align::Center)),
    );
    clicks.remove = actions
        .add_sized(
            [OVERLAY_ROW_ACTION_WIDTH, OVERLAY_ROW_HEIGHT],
            egui::Button::new(RichText::new("X").color(UI_DANGER())),
        )
        .clicked();
    clicks.toggle = actions
        .add_sized(
            [OVERLAY_ROW_ACTION_WIDTH, OVERLAY_ROW_HEIGHT],
            egui::Button::new(if enabled { "||" } else { ">" }),
        )
        .clicked();
    OverlayRow {
        clicks,
        name: tab_rect,
        controls,
    }
}

/// The host the detail pane's per-host editor is showing.
///
/// Returns `None` when the overlay has no such host — the selection is stale,
/// which happens when a host is deleted while the window is closed to the
/// Profiles page. The editor treats that as "nothing selected" rather than
/// reaching for a different host, so a stale id can never show somebody else's
/// settings under the name of the one they clicked.
fn selected_target_in<'a>(
    overlay: &'a OverlayConfig,
    selected: Option<&str>,
) -> Option<&'a TargetConfig> {
    let id = selected?;
    overlay.targets.iter().find(|target| target.id == id)
}

/// The name a list-pane row shows for an overlay.
///
/// A group carries its host count, because a row reading like any other is how
/// four separate overlays and one overlay of four hosts end up looking the
/// same from across the room. One host shows nothing, which keeps the common
/// case exactly as it was.
fn overlay_row_label(overlay: &OverlayConfig) -> String {
    let name = if overlay.name.is_empty() {
        "(unnamed)".to_string()
    } else {
        overlay.name.clone()
    };
    let enabled = overlay
        .targets
        .iter()
        .filter(|target| target.enabled)
        .count();
    let total = overlay.targets.len();
    match (enabled, total) {
        (1, 1) => name,
        _ => format!("{name} ({enabled}/{total} hosts)"),
    }
}

/// What a host row's controls did, kept out of the borrow.
#[derive(Default)]
struct TargetRowClicks {
    label: bool,
    toggle: bool,
    remove: bool,
}

/// The inside of a host row: a colour swatch, the host, and two buttons.
///
/// Built the same way as an overlay row and for the same reasons: painted
/// rather than framed, allocated at its own height rather than its content's,
/// and with both halves taken from one rect so the name and the buttons cannot
/// disagree about where the row ends.
fn target_row_contents(
    ui: &mut Ui,
    row: egui::Rect,
    target: &TargetConfig,
    active: bool,
    position: usize,
) -> TargetRowClicks {
    let inner = row_inner(row);
    let gap = ui.spacing().item_spacing.x;
    let mut clicks = TargetRowClicks::default();
    let mut ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );

    // The swatch is the row's own identification: on screen the only thing that
    // says which line is which is its colour, so the list has to show it.
    let swatch = ui.allocate_exact_size(
        egui::vec2(TARGET_ROW_SWATCH, TARGET_ROW_SWATCH),
        egui::Sense::hover(),
    );
    ui.painter().circle_filled(
        swatch.0.center(),
        TARGET_ROW_SWATCH / 2.0,
        parse_color(&target.line_color),
    );
    ui.painter().circle_stroke(
        swatch.0.center(),
        TARGET_ROW_SWATCH / 2.0,
        egui::Stroke::new(1.0, UI_BORDER()),
    );

    let controls_width = TARGET_ROW_ACTION_WIDTH * 2.0 + gap;
    let label_width =
        (inner.width() - swatch.0.width() - TARGET_ROW_SWATCH_GAP - controls_width - gap).max(24.0);
    let (label_rect, mut label_response) = ui.allocate_exact_size(
        egui::vec2(label_width, TARGET_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let label = host_row_label(target, position);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = label_rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.append(&label, 0.0, egui::TextFormat::simple(font_id, UI_TEXT()));
    let galley = ui.painter().layout_job(job);
    let text_offset = (label_rect.height() - galley.size().y) / 2.0;
    ui.painter().galley(
        label_rect.left_top() + egui::vec2(0.0, text_offset),
        galley,
        if active { UI_ACCENT() } else { UI_TEXT() },
    );
    if label_response.hovered() {
        label_response = label_response.on_hover_text(target.probe.host());
    }
    clicks.label = label_response.clicked();

    let controls = right_anchored(inner, controls_width);
    let mut actions = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls)
            .layout(Layout::left_to_right(Align::Center)),
    );
    clicks.remove = actions
        .add_sized(
            [TARGET_ROW_ACTION_WIDTH, TARGET_ROW_HEIGHT],
            egui::Button::new(RichText::new("X").color(UI_DANGER())),
        )
        .clicked();
    clicks.toggle = actions
        .add_sized(
            [TARGET_ROW_ACTION_WIDTH, TARGET_ROW_HEIGHT],
            egui::Button::new(if target.enabled { "||" } else { "> " }),
        )
        .clicked();
    clicks
}

/// The right-hand area of an overlay row: the two action buttons and the gap
/// between them, anchored to the row's right edge.
fn overlay_controls_width(gap: f32) -> f32 {
    OVERLAY_ROW_ACTION_WIDTH * 2.0 + gap
}

/// The name area of an overlay row: the row's contents minus its action buttons
/// and the two gaps between the three of them.
fn overlay_name_width(inner_width: f32, gap: f32) -> f32 {
    (inner_width - overlay_controls_width(gap) - gap).max(60.0)
}

fn profile_name_width(inner_width: f32, gap: f32) -> f32 {
    (inner_width - PROFILE_ROW_TRAILING - gap).max(80.0)
}

/// What one profile row laid out, and whether the user clicked it.
///
/// Clicking only selects the profile; loading it is a separate action in the
/// detail pane. The rects are reported so a test can measure the row; only
/// `clicked` is read at runtime.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct ProfileRow {
    clicked: bool,
    name: egui::Rect,
    /// The count and active dot, pinned to the row's right edge.
    gutter: egui::Rect,
}

/// Lay out one profile row's contents inside an already painted `row`.
///
/// The name takes whatever the row's left edge and the gutter leave over, and the
/// gutter is pinned to the row's right edge, so a long name truncates instead of
/// running underneath the count and the dot, and neither can escape the fill.
/// `count` is `None` until the profile file has been read, and draws nothing then.
fn profile_row_contents(
    ui: &mut Ui,
    row: egui::Rect,
    label: &str,
    count: Option<usize>,
    is_active: bool,
    dirty: bool,
) -> ProfileRow {
    let inner = row_inner(row);
    let gap = ui.spacing().item_spacing.x;
    let name_width = profile_name_width(inner.width(), gap);
    let mut ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The name is painted, not a button, so the row has no outline of its own.
    // The row background is the hover cue instead.
    let color = if is_active {
        UI_ACCENT()
    } else if dirty {
        UI_TEXT_SECONDARY()
    } else {
        UI_TEXT()
    };
    let (name_rect, response) = ui.allocate_exact_size(
        egui::vec2(name_width, PROFILE_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut name_text = egui::text::LayoutJob::default();
    name_text.wrap.max_width = name_width;
    name_text.wrap.max_rows = 1;
    name_text.wrap.break_anywhere = true;
    name_text.append(label, 0.0, egui::TextFormat::simple(font_id, color));
    let galley = ui.painter().layout_job(name_text);
    let text_offset = (name_rect.height() - galley.size().y) / 2.0;
    ui.painter().galley(
        name_rect.left_top() + egui::vec2(0.0, text_offset),
        galley,
        color,
    );
    let clicked = response.clicked();

    let trailing = ui
        .new_child(
            egui::UiBuilder::new()
                .max_rect(right_anchored(inner, PROFILE_ROW_TRAILING))
                .layout(Layout::left_to_right(Align::Center)),
        )
        .allocate_response(
            egui::vec2(PROFILE_ROW_TRAILING, PROFILE_ROW_HEIGHT),
            egui::Sense::hover(),
        );
    let painter = ui.painter();
    // The gutter is two columns: the count, right aligned against a fixed slot
    // for the active dot. That way the numbers line up down the list and the dot
    // never nudges them.
    let dot_slot = egui::Rect::from_min_max(
        egui::pos2(
            trailing.rect.right() - PROFILE_ROW_DOT_WIDTH,
            trailing.rect.top(),
        ),
        trailing.rect.right_bottom(),
    );
    // No count means it has not been read, which is not the same as none, so
    // nothing is drawn rather than a zero that would be a confident lie.
    if let Some(count) = count {
        painter.text(
            egui::pos2(
                dot_slot.left() - PROFILE_ROW_COUNT_GAP,
                trailing.rect.center().y,
            ),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::TextStyle::Small.resolve(ui.style()),
            UI_TEXT_SECONDARY(),
        );
    }
    if is_active {
        // A dot, because the accent name alone is a weak cue in a long list.
        draw_active_dot(painter, dot_slot, UI_ACCENT());
    }
    ProfileRow {
        clicked,
        name: name_rect,
        gutter: trailing.rect,
    }
}

/// A strip of `width` at the right-hand end of a row's contents.
///
/// Every right-hand control in the list pane is placed with this instead of
/// being left to accumulate out of the layout cursor, so a row's controls cannot
/// drift past the painted fill however the left-hand side is sized.
fn right_anchored(inner: egui::Rect, width: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(inner.right() - width, inner.top()),
        egui::pos2(inner.right(), inner.bottom()),
    )
}

/// The width of an overlay row's delete confirmation: its two buttons and the gap
/// between them, pinned to the row's right edge.
fn confirm_controls_width(gap: f32) -> f32 {
    OVERLAY_CONFIRM_OK_WIDTH + OVERLAY_CONFIRM_CANCEL_WIDTH + gap
}

/// Whether a page's detail pane ends in the sticky Save/Discard footer.
///
/// Only a page that stages a draft gets one: the Overlays page writes the active
/// profile and the Global page writes `globalconfig.json`, while the Profiles page
/// changes profile files as soon as an action is confirmed and has nothing to save.
fn page_has_detail_footer(page: Page) -> bool {
    match page {
        Page::Overlays | Page::Global => true,
        Page::Profiles | Page::About => false,
    }
}

/// Whether the page has a list in the middle pane.
///
/// The Global and About pages are a single wide pane with nothing to list, so
/// `config_ui` drops the list pane for them and the page gets the full width.
/// This is a function rather than `self.page != Page::Global` at the call site
/// so a new list-free page is a one-line change here instead of an edit buried
/// in the layout code, which is how a fifth page would otherwise be forgotten.
fn page_has_list_pane(page: Page) -> bool {
    match page {
        Page::Overlays | Page::Profiles => true,
        Page::Global | Page::About => false,
    }
}

/// The detail pane's message when nothing is selected, as one string.
///
/// One `Label` with a newline in it, because `Ui::centered_and_justified` is a
/// one-widget builder: adding a second label and an `add_space` made it report
/// a min rect taller than the box it was given, which shoved the detail pane's
/// footer down into the status bar. Free so the test can assert it is one
/// string rather than a list of widgets.
fn empty_editor_message(_style: &egui::Style) -> &'static str {
    "No overlay selected.\nPick one from the list on the left."
}

/// Draws the detail pane's message when no overlay is selected.
///
/// One `Label` carrying both lines, inside `ui.centered_and_justified`, which by
/// contract holds exactly one widget. Two labels and an `add_space` between them
/// broke that contract, and the builder then reported a min rect 22px *taller*
/// than the box it had been given, which pushed Save and Discard 19.5px down
/// into the status bar. See `the_detail_footer_stays_under_the_detail_pane`.
fn empty_editor(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(empty_editor_message(ui.style())).color(UI_TEXT_SECONDARY()));
    });
}

/// The profile list and the overlay count that goes with it, read together.
///
/// They are one read because both come from the same files, and pairing them
/// here means no caller can refresh one and forget the other.
struct ProfileSnapshot {
    profiles: Vec<config::ProfileEntry>,
    counts: HashMap<String, usize>,
}

/// Read every profile's name and overlay count from disk.
///
/// Takes the store because the snapshot must come from the root the window is
/// pointed at: reading through the free functions would show a test the real
/// profiles while it writes into its own sandbox.
fn read_profile_snapshot(store: &config::Store) -> ProfileSnapshot {
    ProfileSnapshot {
        profiles: store.list_profiles_detailed(),
        counts: store.profile_overlay_counts(),
    }
}

/// Whether arriving on `to` has to re-read the profile list.
///
/// Only the Profiles page shows a per-profile overlay count, and that count is a
/// cache, so arriving there is what makes the cache true. `from != to` is the
/// part that keeps this off the per-frame path: it runs every pass, and the
/// Profiles page stays the current one for as long as the user reads it.
fn arriving_page_needs_profiles(from: Page, to: Page) -> bool {
    from != to && to == Page::Profiles
}

/// Whether the renderer has to be told about the draft on this frame.
///
/// Free rather than a method so it can be driven without a `PingApp`: the
/// window owns a renderer connection and a texture, so a test that had to build
/// one to exercise a two-field decision would not be worth writing.
///
/// `draft` is normalized **in place** before the comparison, and that is the
/// load-bearing half. Comparing a normalized record against an un-normalized
/// draft never matches, so the window would send the entire configuration on
/// every frame for as long as it stayed open — which is not slow enough to look
/// wrong and is exactly that: a pipe write and a full config parse ten times a
/// second, forever. Normalizing first also means the editor shows the clamped
/// value that Save would write, so a drag does not move the overlay and then
/// jump back.
fn runtime_config_changed(draft: &mut Config, last_pushed: &Config) -> bool {
    draft.normalize();
    draft != last_pushed
}

/// Whether a pass has anything new to hand the renderer.
///
/// Three things count: an edit, a background-tracking change the renderer has
/// not been told, and a committed removal it has not acknowledged. The first is
/// what makes edits live; the other two are what make a Save's removals survive
/// a failed send instead of being lost to it.
fn config_push_due(
    draft: &mut Config,
    last_pushed: &Config,
    background_tracking: bool,
    last_sent_background: bool,
    retiring: bool,
) -> bool {
    // `runtime_config_changed` first and on its own, so the draft is normalized
    // in place on every pass that asks — a `||` chain short-circuiting around it
    // would leave the editor showing values Save would clamp.
    runtime_config_changed(draft, last_pushed)
        || background_tracking != last_sent_background
        || retiring
}

/// The enabled target keys a save removes: what the profile on disk has that
/// the config being saved does not.
///
/// These are exactly the probes the renderer must stop keeping, because it
/// keeps every departure probed while the setting is on. Sorted, because a
/// `HashSet` has no order and the wire, the log and a test all read better when
/// the list does.
fn retired_targets(saved: &HashSet<TaskKey>, draft: &Config) -> Vec<TaskKey> {
    let enabled = enabled_target_keys(draft);
    let mut retired: Vec<TaskKey> = saved.difference(&enabled).cloned().collect();
    retired.sort_by(|a, b| (&a.overlay_id, &a.target_id).cmp(&(&b.overlay_id, &b.target_id)));
    retired
}

/// How a host is labelled in the detail pane's list.
///
/// A blank host says so rather than showing nothing: a row with no text is a
/// layout bug, and "no host" tells the user which field to fill in.
fn host_row_label(target: &TargetConfig, position: usize) -> String {
    let host = target.label();
    if host == "(no host)" {
        format!("{position}. (no host yet)")
    } else {
        format!("{position}. {host}")
    }
}

/// The body of `sync_profiles`, with the disk read passed in.
///
/// The read is a parameter so a test can hand it a counter and prove the read
/// actually happens on arrival. That is the whole point: the counts shipped a
/// release reading zero because the page never asked for them, and a test that
/// only checked the *predicate* above would still have passed with the call to
/// `read` deleted. Returns whether a read happened.
fn sync_profile_cache(
    last_page: &mut Page,
    page: Page,
    profiles: &mut Vec<config::ProfileEntry>,
    counts: &mut HashMap<String, usize>,
    read: impl FnOnce() -> ProfileSnapshot,
) -> bool {
    let arriving = arriving_page_needs_profiles(*last_page, page);
    // Advanced whether or not a read happened, so leaving the Profiles page and
    // coming back counts as arriving again.
    *last_page = page;
    if !arriving {
        return false;
    }
    let snapshot = read();
    *profiles = snapshot.profiles;
    *counts = snapshot.counts;
    true
}

/// The body of `sync_theme`, with the system-mode read passed in.
///
/// Same shape and the same reason as `sync_profile_cache`: a test that only
/// checked the "did the mode change" predicate would pass with the load
/// deleted, and the load is what turns a mode into colours.
///
/// `previous` is what the app last applied. `None` means nothing has been
/// applied yet — the startup case, which must always load. When the mode has not
/// moved, the previous theme is returned **unchanged**: re-reading the file
/// would be a disk read every pass, and returning the *built-in* instead of the
/// one on disk would quietly undo any edit the moment the mode stayed put.
///
/// Returns the resolved mode and the theme it resolved to. The caller compares
/// the mode itself, so an unchanged mode costs no repaint.
fn sync_theme(
    previous: Option<(Mode, &Theme)>,
    preference: ThemeMode,
    themes_dir: &std::path::Path,
) -> (Mode, Theme) {
    let system = windows_app_mode();
    let mode = theme::resolve(preference, system);
    if let Some((last_mode, last_theme)) = previous {
        if last_mode == mode {
            return (mode, last_theme.clone());
        }
    }
    let (loaded, notices) = theme::load_builtin(themes_dir, mode);
    for notice in notices {
        log_line("config", &notice);
    }
    (mode, loaded)
}

/// The body of `sync_monitors`, with the enumeration passed in.
///
/// Same shape and the same reason as `sync_profile_cache`: a test that only
/// checked the due/not-due predicate would pass with the read deleted, which is
/// the failure the profile counts shipped. Returns whether a read happened.
fn sync_monitor_list(
    monitors: &mut Vec<MonitorInfo>,
    read_at: &mut Option<Instant>,
    now: Instant,
    read: impl FnOnce() -> Vec<MonitorInfo>,
) -> bool {
    let due = read_at.is_none_or(|at| now.duration_since(at) >= MONITOR_REFRESH_INTERVAL);
    if !due {
        return false;
    }
    *monitors = read();
    // Set after the read, so a slow enumeration is not counted as having
    // happened before it did.
    *read_at = Some(now);
    true
}

/// How often the auto switching checks re-read the desktop, in seconds.
///
/// One, matching the tray's own evaluation cadence, so neither the preview nor
/// the window's engine is ever more than a second behind what the tray would
/// do with the same rules.
const AUTO_SWITCH_INTERVAL: Duration = Duration::from_secs(1);

/// The body of `sync_auto_preview`, with the window enumeration passed in.
///
/// Same shape and the same reason as `sync_monitor_list`: a test that only
/// checked the clock would pass with the enumeration deleted. The text is
/// computed from `rules::decide` — the same pure function the tray's engine
/// calls — so the preview cannot drift from the behaviour.
fn sync_auto_preview_text(
    text: &mut Option<String>,
    read_at: &mut Option<Instant>,
    now: Instant,
    rules: &AutoRules,
    profiles: &[config::ProfileEntry],
    read: impl FnOnce() -> Snapshot,
) {
    let due = read_at.is_none_or(|at| now.duration_since(at) >= AUTO_SWITCH_INTERVAL);
    if !due {
        return;
    }
    if !rules.enabled {
        // Nothing to say, and nothing worth enumerating for.
        *text = None;
        *read_at = Some(now);
        return;
    }
    let snapshot = read();
    *text = Some(auto_preview_line(rules, &snapshot, profiles));
    *read_at = Some(now);
}

/// What one evaluation of the window-side engine decided.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AutoSwitchStep {
    /// Nothing settled, or the settled decision is already applied.
    Idle,
    /// A settled decision is waiting for unsaved edits to be resolved.
    Held(String),
    /// Apply this profile, and record it as applied only if it lands.
    Apply(String),
}

/// The body of `sync_auto_switch`, with the window enumeration passed in.
///
/// Same shape and the same reason as `sync_auto_preview_text`: the due check,
/// the "only enumerate when a rule could match" gate and the debounce all live
/// here, so a test can drive the whole mechanism without a desktop to look at.
///
/// `blocked` is the unsaved-edits hold, and it deliberately never reaches the
/// engine: `step` returns `Apply` every tick until `mark_applied`, so a held
/// switch is not lost — it lands by itself once the drafts are resolved.
fn auto_switch_step(
    engine: &mut rules::Engine,
    read_at: &mut Option<Instant>,
    now: Instant,
    file: &AutoRules,
    blocked: bool,
    read: impl FnOnce() -> Snapshot,
) -> AutoSwitchStep {
    let due = read_at.is_none_or(|at| now.duration_since(at) >= AUTO_SWITCH_INTERVAL);
    if !due {
        return AutoSwitchStep::Idle;
    }
    *read_at = Some(now);
    let compiled = rules::compile(file);
    let decision = if compiled.enabled && compiled.rules.iter().any(|rule| rule.error.is_none()) {
        // Only enumerate the desktop when a rule could use the answer: with
        // switching off, or nothing usable, the decision is `None` whatever
        // the windows are and the snapshot is the expensive half.
        let snapshot = read();
        rules::decide(&compiled, &snapshot)
    } else {
        None
    };
    match engine.step(decision.as_ref().map(|decision| decision.profile.as_str())) {
        rules::Tick::Apply { profile } if !blocked => AutoSwitchStep::Apply(profile),
        rules::Tick::Apply { profile } => AutoSwitchStep::Held(profile),
        rules::Tick::Idle => AutoSwitchStep::Idle,
    }
}

/// What the rules as edited would decide against this desktop.
///
/// Wording, not logic: `rules::decide` answers the question, and this only
/// names the answer. A rule that cannot match is named as such rather than
/// quietly reported as "nothing matched", because the two are different
/// problems with different fixes.
fn auto_preview_line(
    rules: &AutoRules,
    snapshot: &Snapshot,
    profiles: &[config::ProfileEntry],
) -> String {
    let compiled = rules::compile(rules);
    if compiled.rules.iter().all(|rule| rule.error.is_some()) && !compiled.rules.is_empty() {
        return "No rule can match while its conditions are incomplete.".to_string();
    }
    match rules::decide(&compiled, snapshot) {
        Some(decision) => match decision.rule_index {
            Some(index) => {
                let rule = &compiled.rules[index];
                let name = if rule.name.trim().is_empty() {
                    format!("rule {}", index + 1)
                } else {
                    format!("rule {} \"{}\"", index + 1, rule.name)
                };
                format!(
                    "Matches {name} -> {}",
                    profile_display_name(profiles, &decision.profile)
                )
            }
            None => format!(
                "Nothing matches -> {}",
                profile_display_name(profiles, &decision.profile)
            ),
        },
        None => "Add a rule to start switching.".to_string(),
    }
}

/// The display name for a profile id, saying when there is no such profile.
///
/// A rule can name a profile that was renamed or deleted; the preview has to
/// say that rather than draw a name that no longer exists, because "the rule
/// points at nothing" is the thing the user needs to fix.
fn profile_display_name(profiles: &[config::ProfileEntry], id: &str) -> String {
    match profiles.iter().find(|profile| profile.id == id) {
        Some(profile) => profile.name.clone(),
        None => format!("{id} (no such profile)"),
    }
}

/// The auto profile switching editor's widgets.
///
/// A free function taking the rules and the profile list as parameters, so the
/// widget code does not have to reach through the app for either. Returns
/// whether anything changed, which is what stages the rules draft.
fn auto_switch_section(
    ui: &mut Ui,
    file: &mut AutoRules,
    profiles: &[config::ProfileEntry],
    active_profile: &str,
) -> bool {
    let mut changed = false;

    changed |= ui
        .checkbox(&mut file.enabled, "Enable auto profile switching")
        .changed();

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("When no rule matches, switch to:");
        if let Some(chosen) = profile_pick(
            ui,
            "auto-fallback",
            profiles,
            file.fallback_profile.as_deref(),
            true,
        ) {
            file.fallback_profile = chosen;
            changed = true;
        }
    });
    if file.enabled && !file.rules.is_empty() && file.fallback_profile.is_none() {
        ui.label(
            RichText::new("No fallback is set: when nothing matches, the profile in force stays.")
                .color(UI_DANGER()),
        );
    }

    let total = file.rules.len();
    let mut remove = None;
    let mut move_up = None;
    let mut move_down = None;
    for (index, rule) in file.rules.iter_mut().enumerate() {
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Rule {}", index + 1)).strong());
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut rule.name)
                        .hint_text("name (optional)")
                        .id_salt(("rule-name", index))
                        .desired_width(160.0),
                )
                .changed();
            ui.add_enabled_ui(index > 0, |ui| {
                if ui.button("Up").clicked() {
                    move_up = Some(index);
                }
            });
            ui.add_enabled_ui(index + 1 < total, |ui| {
                if ui.button("Down").clicked() {
                    move_down = Some(index);
                }
            });
            if ui
                .button(RichText::new("Remove").color(UI_DANGER()))
                .clicked()
            {
                remove = Some(index);
            }
        });

        ui.horizontal(|ui| {
            ui.label("Look at");
            changed |= scope_combo(ui, index, &mut rule.scope);
            ui.label("and require");
            changed |= combine_combo(ui, index, &mut rule.combine);
            ui.label("of these conditions:");
        });

        // Every condition below is asked about the same window; `combine`
        // decides how their answers join. That is why they are one list and
        // not one row per source.
        let mut drop_condition = None;
        for (position, condition) in rule.when.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= part_combo(ui, index, position, &mut condition.part);
                changed |= match_combo(ui, index, position, &mut condition.matcher);
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut condition.value)
                            .hint_text(part_hint(condition.part))
                            .id_salt(("condition-value", index, position))
                            .desired_width(200.0),
                    )
                    .changed();
                if ui.button("X").clicked() {
                    drop_condition = Some(position);
                }
            });
        }
        if let Some(position) = drop_condition {
            rule.when.remove(position);
            changed = true;
        }
        if ui.button("+ Add condition").clicked() {
            rule.when.push(Condition::default());
            changed = true;
        }

        ui.horizontal(|ui| {
            ui.label("Switch to");
            if let Some(chosen) = profile_pick(
                ui,
                ("rule-profile", index),
                profiles,
                Some(rule.profile.as_str()),
                false,
            ) {
                rule.profile = chosen.unwrap_or_default();
                changed = true;
            }
        });

        // From the same compiler the engine uses, so the editor cannot claim
        // a rule is fine while the tray skips it.
        if let Some(error) = rule_error(rule) {
            ui.label(RichText::new(format!("Cannot match: {error}")).color(UI_DANGER()));
        }
    }

    // Structural edits after the iteration borrow ends.
    if let Some(index) = remove {
        file.rules.remove(index);
        changed = true;
    }
    if let Some(index) = move_up {
        file.rules.swap(index - 1, index);
        changed = true;
    }
    if let Some(index) = move_down {
        file.rules.swap(index, index + 1);
        changed = true;
    }

    ui.add_space(8.0);
    if ui.button("+ Add rule").clicked() {
        file.rules.push(Rule {
            name: String::new(),
            scope: Scope::default(),
            combine: Combine::default(),
            when: vec![Condition::default()],
            profile: active_profile.to_string(),
        });
        // A fallback is required in spirit once a rule exists, so the first
        // rule sets one from the profiles that are there rather than leaving
        // a warning to fill in later.
        if file.fallback_profile.is_none() {
            file.fallback_profile = Some(default_fallback(profiles, active_profile));
        }
        changed = true;
    }

    changed
}

/// Why a rule cannot match, from the same compiler the engine uses.
fn rule_error(rule: &Rule) -> Option<String> {
    let probe = AutoRules {
        enabled: true,
        fallback_profile: None,
        rules: vec![rule.clone()],
    };
    rules::compile(&probe)
        .rules
        .into_iter()
        .next()
        .and_then(|compiled| compiled.error)
}

/// The fallback a new rule set starts with: `default` when it exists, then
/// the first profile, then whatever is in force.
fn default_fallback(profiles: &[config::ProfileEntry], active_profile: &str) -> String {
    profiles
        .iter()
        .find(|profile| profile.id == config::DEFAULT_PROFILE)
        .or_else(|| profiles.first())
        .map(|profile| profile.id.clone())
        .unwrap_or_else(|| active_profile.to_string())
}

/// A dropdown of profiles, showing display names and resolving to ids.
///
/// The value is the profile **id**; display names may repeat and a rule that
/// stored one would silently follow the wrong file. An id that no longer
/// resolves is drawn as such rather than omitted, because a `ComboBox` whose
/// entries do not contain the selected value falls back to its first row —
/// which would show a different profile as chosen. Returns the picked value,
/// or `None` when nothing was clicked; `Some(None)` is the explicit "not set"
/// choice, which only the fallback picker offers.
fn profile_pick(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    profiles: &[config::ProfileEntry],
    current: Option<&str>,
    allow_none: bool,
) -> Option<Option<String>> {
    let selected = match current {
        Some(id) => profile_display_name(profiles, id),
        None => "(not set)".to_string(),
    };
    let mut chosen = None;
    ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if allow_none
                && ui
                    .selectable_label(current.is_none(), "(not set)")
                    .clicked()
            {
                chosen = Some(None);
            }
            for profile in profiles {
                let picked = current == Some(profile.id.as_str());
                if ui.selectable_label(picked, &profile.name).clicked() {
                    chosen = Some(Some(profile.id.clone()));
                }
            }
        });
    chosen
}

fn scope_combo(ui: &mut Ui, index: usize, scope: &mut Scope) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(("rule-scope", index))
        .selected_text(scope_label(*scope))
        .show_ui(ui, |ui| {
            for option in [Scope::AnyWindow, Scope::Foreground] {
                changed |= ui
                    .selectable_value(scope, option, scope_label(option))
                    .changed();
            }
        });
    changed
}

fn combine_combo(ui: &mut Ui, index: usize, combine: &mut Combine) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(("rule-combine", index))
        .selected_text(combine_label(*combine))
        .show_ui(ui, |ui| {
            for option in [Combine::All, Combine::Any] {
                changed |= ui
                    .selectable_value(combine, option, combine_label(option))
                    .changed();
            }
        });
    changed
}

fn part_combo(ui: &mut Ui, index: usize, position: usize, part: &mut Part) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(("condition-part", index, position))
        .selected_text(part_label(*part))
        .width(120.0)
        .show_ui(ui, |ui| {
            for option in [Part::ProcessName, Part::Title, Part::ClassName] {
                changed |= ui
                    .selectable_value(part, option, part_label(option))
                    .changed();
            }
        });
    changed
}

fn match_combo(ui: &mut Ui, index: usize, position: usize, matcher: &mut MatchMode) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(("condition-match", index, position))
        .selected_text(match_label(*matcher))
        .width(110.0)
        .show_ui(ui, |ui| {
            for option in [MatchMode::Exact, MatchMode::Contains, MatchMode::Regex] {
                changed |= ui
                    .selectable_value(matcher, option, match_label(option))
                    .changed();
            }
        });
    changed
}

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::AnyWindow => "any window",
        Scope::Foreground => "the foreground window",
    }
}

fn combine_label(combine: Combine) -> &'static str {
    match combine {
        Combine::All => "all",
        Combine::Any => "any",
    }
}

fn part_label(part: Part) -> &'static str {
    match part {
        Part::ProcessName => "process name",
        Part::Title => "window title",
        Part::ClassName => "window class",
    }
}

fn match_label(matcher: MatchMode) -> &'static str {
    match matcher {
        MatchMode::Exact => "is exactly",
        MatchMode::Contains => "contains",
        MatchMode::Regex => "matches regex",
    }
}

/// Placeholder text for a condition's value, naming the kind of thing that
/// goes there: the three parts are not interchangeable, and the field is the
/// one a user has to know what to type in.
fn part_hint(part: Part) -> &'static str {
    match part {
        Part::ProcessName => "cs2.exe",
        Part::Title => "Counter-Strike",
        Part::ClassName => "Chrome_WidgetWin_1",
    }
}

/// How the detail pane words an overlay count, or nothing when it is unknown.
///
/// A count that has not been read is not the same as a profile with no overlays,
/// so it says nothing instead of saying zero.
fn overlay_count_label(count: Option<usize>) -> String {
    match count {
        Some(1) => "1 overlay".to_string(),
        Some(count) => format!("{count} overlays"),
        None => String::new(),
    }
}

/// A filled dot centred in the rect it is given, marking the active profile.
///
/// The rect decides where the dot lands, so a caller that wants it in a corner
/// passes a small rect in that corner. Centring it is what keeps it on the same
/// line as the text beside it.
fn draw_active_dot(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let radius = 3.5;
    painter.circle_filled(rect.center(), radius, color);
}

/// A small rect in the top right corner of `row`, where the rail's unsaved
/// changes dot goes. Keeping it here means [`draw_active_dot`] can centre.
fn row_corner_dot_slot(row: egui::Rect) -> egui::Rect {
    egui::Rect::from_center_size(
        egui::pos2(row.right() - 9.5, row.top() + 9.5),
        egui::vec2(PROFILE_ROW_DOT_WIDTH, PROFILE_ROW_DOT_WIDTH),
    )
}

/// A single-line profile name editor with a submit and a cancel button.
fn profile_name_field(
    ui: &mut Ui,
    name: &mut String,
    submit_label: &str,
    focus: bool,
) -> (bool, bool) {
    let mut submit = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 136.0).max(40.0);
        let edit = ui.add_sized(
            [width, PROFILE_ROW_HEIGHT],
            egui::TextEdit::singleline(name)
                .hint_text("Profile name")
                // A fixed id keeps the caret in the field when the editor
                // replaces the "+ New profile" row and shifts its position.
                .id(egui::Id::new(PROFILE_NAME_FIELD_ID)),
        );
        if focus {
            ui.ctx().memory_mut(|memory| memory.request_focus(edit.id));
        }
        // Enter submits while the name field has focus.
        let enter = edit.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        submit = enter
            || ui
                .add_sized([64.0, PROFILE_ROW_HEIGHT], egui::Button::new(submit_label))
                .clicked();
        cancel = ui
            .add_sized([56.0, PROFILE_ROW_HEIGHT], egui::Button::new("Cancel"))
            .clicked();
    });
    (submit, cancel)
}

/// The overlay whose border is previewed for the current selection, if any.
///
/// Four things have to line up: the Config window open, the Overlays page on
/// screen, an overlay selected, and the selection border animation enabled.
/// The page condition went missing for a release, leaving the last overlay's
/// border animating on the Profiles and Global pages.
///
/// The animation flag belongs here rather than at the send site so the table
/// test over this function still covers it.
///
/// Unlike the profile-count trigger in `sync_profile_cache`, a table over this
/// function is the whole mechanism rather than a predicate standing in for one:
/// `sync_border_preview` calls it every frame and sends only what changed, so
/// nothing has to act on the result for the test to mean anything.
fn selected_overlay_for_border(
    config_visible: bool,
    page: Page,
    selected_id: Option<&str>,
    animation_enabled: bool,
) -> Option<&str> {
    if config_visible && page == Page::Overlays && animation_enabled {
        selected_id
    } else {
        None
    }
}

/// The selection after the user clicks the row named `clicked`.
///
/// Clicking the row that is already selected clears the selection, which is the
/// only way to stop the border preview and empty pane 3. This is a free function
/// rather than an `if` in the row loop so a test can drive it: a predicate-only
/// test would pass with the action deleted, which is exactly how the profile
/// counts shipped reading zero.
fn toggled_selection(current: Option<&str>, clicked: &str) -> Option<String> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked.to_string())
    }
}

/// The blank strip under the last row that clears the selection when clicked.
///
/// The rect is only ever a *hit target*: it is registered with `ui.interact`,
/// which draws no widget and so adds nothing to the scroll area's content
/// height. Allocating the leftover as a real widget instead would push the
/// content past the viewport and conjure a scrollbar the moment the list exactly
/// fitted, which is the sort of thing that only shows up once the list is the
/// wrong length.
///
/// Returns `None` when the rows already fill the viewport, so a list long enough
/// to scroll simply has no strip and the row toggle carries on alone.
fn deselect_strip_rect(
    viewport: egui::Rect,
    rows_bottom: f32,
    row_width: f32,
) -> Option<egui::Rect> {
    if rows_bottom >= viewport.bottom() {
        return None;
    }
    let strip = egui::Rect::from_min_max(
        egui::pos2(viewport.left(), rows_bottom),
        egui::pos2(viewport.left() + row_width, viewport.bottom()),
    );
    Some(strip.intersect(viewport))
}

/// The renderer's executable, and how to start it if it is not already there.
///
/// Both live in `core::transport` rather than here, because the tray process
/// has to do exactly the same thing and two copies of "only ever stop a process
/// you started" is one copy too many.
use ping_latency_overlay_core::diagnostics::log_line;
use ping_latency_overlay_core::transport::{start_or_attach_renderer, start_or_attach_tray};

/// The per-frame path, split from the `App` trait so a test can drive it.
///
/// `eframe` hands the trait methods a `Frame`, and both ignored it: `logic` is
/// all state work and `ui` is all drawing. A method without the frame is
/// therefore the same code, and calling it directly is what lets the driving
/// tests run the real per-frame path headlessly — no window, no GPU, and no
/// `eframe::Frame` to construct, which a test cannot do.
impl PingApp {
    fn frame_logic(&mut self, ctx: &Context) {
        if self.shutdown_state == ShutdownState::ExitConfirmed {
            // Issued from a frame that did not also cancel, so this one gets
            // through and the process ends. If it somehow does not, the next
            // pass sends it again rather than leaving the window unclosable.
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            return;
        }
        if self.shutdown_state == ShutdownState::ConfirmClose {
            // The prompt is up. Keep the window alive and its buttons
            // responsive, but stop the per-frame work: nothing behind the
            // panel is going to change.
            ctx.request_repaint();
            return;
        }

        self.reconnect_renderer();
        if self.sync_runtime_config() {
            // A pushed config asks the renderer for a frame, so nothing here
            // needs to — but the window's own layout does, and this is the only
            // place that knows something changed.
            ctx.request_repaint();
        }
        self.sync_border_preview();
        self.sync_profiles();
        self.sync_monitors();
        self.sync_auto_preview();
        self.sync_auto_switch();
        self.sync_theme(ctx);
        self.sync_window_title(ctx);
        // The renderer owns every animation now: smooth rendering, the
        // cosmetic prefill and the startup border all repaint themselves at
        // whatever rate they need. This interval only has to keep the window's
        // own UI alive, so it is a fixed cost rather than one derived from the
        // overlays.
        ctx.request_repaint_after(REPAINT_INTERVAL);
    }

    fn frame_ui(&mut self, ui: &mut Ui) {
        self.handle_root_close(ui.ctx());
        if self.shutdown_state == ShutdownState::ExitConfirmed {
            // The close is on its way; drawing anything now would be a frame
            // of the window nobody is going to see.
            return;
        }
        // The window itself is still drawn behind the prompt, so the user can
        // see what they are about to lose. The prompt goes up afterwards, on
        // top, as its own little window rather than as a child of the panes --
        // a centred child is a `scope_builder`, which is the shape that made
        // the detail footer report more than its box and slide into the
        // status bar.
        self.config_ui(ui);
        if self.shutdown_state == ShutdownState::ConfirmClose {
            self.show_close_prompt(ui.ctx());
        }
    }
}

impl App for PingApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.frame_logic(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        UI_BACKGROUND().to_normalized_gamma_f32()
    }
}

impl Drop for PingApp {
    fn drop(&mut self) {
        // The renderer is NOT stopped here, even when this process started it.
        //
        // It used to be, on the reasoning that only a process you started is
        // yours to stop. That reasoning was about *killing* somebody else's
        // renderer, and it does not apply to the ordinary case: launching the
        // window starts the renderer, so closing the window sent it a Shutdown
        // and took the overlays off the screen with it. The renderer belongs to
        // the tray now, and the tray-less case deliberately wants it to survive.
        // Only the tray's Exit stops it, and "close the window" no longer means
        // "close the app".
        let _ = &self.renderer;
    }
}

/// The host fields of one target: protocol, host, port and timeout.
///
/// Its own function because these are per host rather than per overlay, and
/// because the same four controls would otherwise have to appear twice — once
/// for the selected target in the detail pane and once per row of the target
/// list, where only a compact form makes sense.
fn edit_target(ui: &mut Ui, target: &mut TargetConfig, changed: &mut bool) {
    Grid::new("target-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Protocol");
            let mut protocol = match target.probe {
                ProbeConfig::Icmp { .. } => "icmp",
                ProbeConfig::Tcp { .. } => "tcp",
            };
            ComboBox::from_id_salt("target-protocol")
                .selected_text(if protocol == "icmp" {
                    "ICMP echo"
                } else {
                    "TCP connect"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut protocol, "icmp", "ICMP echo");
                    ui.selectable_value(&mut protocol, "tcp", "TCP connect");
                });
            if protocol == "tcp" && matches!(target.probe, ProbeConfig::Icmp { .. }) {
                let host = target.probe.host().to_string();
                target.probe = ProbeConfig::Tcp { host, port: 80 };
                *changed = true;
            } else if protocol == "icmp" && matches!(target.probe, ProbeConfig::Tcp { .. }) {
                let host = target.probe.host().to_string();
                target.probe = ProbeConfig::Icmp { host };
                *changed = true;
            }
            ui.end_row();

            ui.label("Target host / IP");
            let mut host = target.probe.host().to_string();
            if ui
                .add(egui::TextEdit::singleline(&mut host).desired_width(f32::INFINITY))
                .changed()
            {
                set_probe_host(&mut target.probe, host);
                *changed = true;
            }
            ui.end_row();

            ui.label("Port (TCP only)");
            let mut port = target.probe.port().max(1) as i64;
            if matches!(target.probe, ProbeConfig::Icmp { .. }) {
                ui.add_enabled(false, egui::DragValue::new(&mut port).range(1..=65535));
            } else if ui
                .add(egui::DragValue::new(&mut port).range(1..=65535))
                .changed()
            {
                if let ProbeConfig::Tcp { port: value, .. } = &mut target.probe {
                    *value = port.clamp(1, 65535) as u16;
                }
                *changed = true;
            }
            ui.end_row();

            ui.label("Timeout");
            let mut timeout = target.timeout_ms.max(1) as i64;
            if ui
                .add(
                    egui::DragValue::new(&mut timeout)
                        .range(1..=600_000)
                        .suffix(" ms"),
                )
                .changed()
            {
                target.timeout_ms = timeout.clamp(1, 600_000) as u32;
                *changed = true;
            }
            ui.end_row();
        });
}

/// The per-target colours, in the same grid shape as the rest of the editor.
fn edit_target_colors(ui: &mut Ui, target: &mut TargetConfig, changed: &mut bool) {
    Grid::new("target-colors-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            color_field(ui, "Line color", &mut target.line_color, changed);
            ui.end_row();
            color_field(ui, "Timeout color", &mut target.timeout_color, changed);
            ui.end_row();
        });
}

/// The Sticky Overlay controls: the crosshair, the three boxes that are the
/// target, and the z-order choice.
fn show_sticky_editor(
    ui: &mut Ui,
    overlay: &mut OverlayConfig,
    sticky_picker: &mut StickyPicker,
    changed: &mut bool,
) {
    Grid::new("sticky-target-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            let boxes = [
                (
                    "Process",
                    Part::ProcessName,
                    "The executable's file name, matched exactly.",
                ),
                (
                    "Window title",
                    Part::Title,
                    "A substring of the title. Titles change with the document or \
                     the page, so an empty box is the durable choice.",
                ),
                (
                    "Window class",
                    Part::ClassName,
                    "The Win32 class name, matched exactly.",
                ),
            ];
            for (label, part, hint) in boxes {
                ui.label(label).on_hover_text(hint);
                let mut text = sticky_value(overlay.sticky_target.as_ref(), part);
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(f32::INFINITY)
                            .hint_text("(any)"),
                    )
                    .changed()
                {
                    set_sticky_value(&mut overlay.sticky_target, part, text);
                    *changed = true;
                }
                ui.end_row();
            }
        });
    ui.add_space(6.0);
    if let Some(picked) = sticky_picker.show(ui) {
        fill_sticky_target(&mut overlay.sticky_target, &picked.window);
        *changed = true;
    }
    if overlay.sticky_target.is_none() {
        ui.label(
            RichText::new(
                "No target yet: the overlay hides until a window matches. \
                 Pick a window, or fill in a box.",
            )
            .color(UI_TEXT_SECONDARY()),
        );
    }
    ui.add_space(6.0);
    let mut z_order = overlay.sticky_z_order;
    ui.radio_value(
        &mut z_order,
        StickyZOrder::AboveEverything,
        "Always above other windows",
    )
    .on_hover_text("The overlay floats above everything, focused or not.");
    ui.radio_value(
        &mut z_order,
        StickyZOrder::FollowWindow,
        "In front of the followed window",
    )
    .on_hover_text(
        "The overlay shares the window's place in the z-order, so switching \
         to another app takes it off the screen with the window it belongs to.",
    );
    if z_order != overlay.sticky_z_order {
        overlay.sticky_z_order = z_order;
        *changed = true;
    }
}

// The parameters are the editor's borrowed contexts and its draft, one each;
// bundling them into a struct would only move the same list somewhere else.
#[allow(clippy::too_many_arguments)]
fn edit_overlay(
    ui: &mut Ui,
    overlay: &mut OverlayConfig,
    selected_target: Option<&str>,
    changed: &mut bool,
    position_picker: &mut PositionPicker,
    sticky_picker: &mut StickyPicker,
    attached: &[MonitorInfo],
    theme: &Theme,
) {
    section(ui, "General", |ui| {
        Grid::new("general-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                let mut name = overlay.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY))
                    .changed()
                {
                    overlay.name = name;
                    *changed = true;
                }
                ui.end_row();
            });
    });

    // The lookup is by id, so a selection that is not one of this overlay's
    // hosts draws nothing at all. That is the whole point: the pane must never
    // show host 2's fields while host 1 is the one highlighted in the list.
    if let Some(target) = selected_target_in(overlay, selected_target) {
        let target = target.clone();
        let index = overlay
            .targets
            .iter()
            .position(|candidate| candidate.id == target.id)
            .expect("the lookup just found it");
        section(ui, "Selected host", |ui| {
            edit_target(ui, &mut overlay.targets[index], changed);
            ui.add_space(6.0);
            edit_target_colors(ui, &mut overlay.targets[index], changed);
        });
    }

    section(ui, "Position", |ui| {
        if let Some(anchor) = position_picker.show(ui, overlay.position, theme) {
            if anchor != overlay.position {
                overlay.position = anchor;
                *changed = true;
            }
        }
        Grid::new("position-offsets-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Horizontal margin").on_hover_text(
                    "Positive shifts right on centered anchors or inward from a left/right edge; negative shifts the opposite way.",
                );
                let mut horizontal_margin = overlay.horizontal_margin_px as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut horizontal_margin)
                            .range(
                                config::MIN_MARGIN_OFFSET_PX as i64
                                    ..=config::MAX_MARGIN_OFFSET_PX as i64,
                            )
                            .suffix(" px"),
                    )
                    .changed()
                {
                    overlay.horizontal_margin_px = horizontal_margin.clamp(
                        config::MIN_MARGIN_OFFSET_PX as i64,
                        config::MAX_MARGIN_OFFSET_PX as i64,
                    ) as i32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Vertical margin").on_hover_text(
                    "Positive shifts down on centered anchors or inward from a top/bottom edge; negative shifts the opposite way.",
                );
                let mut vertical_margin = overlay.vertical_margin_px as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut vertical_margin)
                            .range(
                                config::MIN_MARGIN_OFFSET_PX as i64
                                    ..=config::MAX_MARGIN_OFFSET_PX as i64,
                            )
                            .suffix(" px"),
                    )
                    .changed()
                {
                    overlay.vertical_margin_px = vertical_margin.clamp(
                        config::MIN_MARGIN_OFFSET_PX as i64,
                        config::MAX_MARGIN_OFFSET_PX as i64,
                    ) as i32;
                    *changed = true;
                }
                ui.end_row();
            });
    });

    section(ui, "Display Mode", |ui| {
        let mut mode = overlay.display_mode;
        ui.radio_value(&mut mode, DisplayMode::Global, "Global overlay")
            .on_hover_text("Placed on a display, pinned by name when you choose one.");
        ui.radio_value(&mut mode, DisplayMode::Sticky, "Sticky overlay")
            .on_hover_text(
                "Follows a window: the overlay moves and resizes with its \
                 client area, and hides while the window is minimized or gone.",
            );
        ui.radio_value(&mut mode, DisplayMode::Wallpaper, "Wallpaper mode")
            .on_hover_text(
                "Keeps the overlay on the desktop: above the wallpaper, below \
                 the desktop icons and every normal window, with the taskbar \
                 above it. It is not visible over fullscreen or borderless \
                 apps — that is the mode, not a fault.",
            );
        if mode != overlay.display_mode {
            overlay.display_mode = mode;
            *changed = true;
        }
        ui.add_space(6.0);

        match overlay.display_mode {
            DisplayMode::Global => {
                let mut chosen = overlay.monitor_device.clone();
                ComboBox::from_id_salt("monitor-device")
                    .selected_text(monitor_choice_label(attached, chosen.as_deref()))
                    .show_ui(ui, |ui| {
                        for choice in monitor_choices(attached, chosen.as_deref()) {
                            let picked = choice.is(chosen.as_deref());
                            if ui
                                .selectable_label(picked, choice.label)
                                .on_hover_text(choice.hint)
                                .clicked()
                                && !picked
                            {
                                chosen = choice.device;
                            }
                        }
                    });
                if chosen != overlay.monitor_device {
                    overlay.monitor_device = chosen;
                    *changed = true;
                }
            }
            DisplayMode::Sticky => show_sticky_editor(ui, overlay, sticky_picker, changed),
            DisplayMode::Wallpaper => {
                ui.label(
                    RichText::new("The overlay sits on the desktop itself.")
                        .color(UI_TEXT_SECONDARY()),
                );
            }
        }
    });

    section(ui, "Graph", |ui| {
        Grid::new("graph-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Sampling");
                let mut window_seconds = overlay.window_seconds as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut window_seconds)
                            .range(30..=86_400)
                            .suffix(" seconds"),
                    )
                    .changed()
                {
                    overlay.window_seconds = window_seconds.clamp(30, 86_400) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("X axis scale");
                let mut scale = overlay.scale.max(1) as i64;
                let mut slider_scale = scale.clamp(1, config::MAX_SCALE as i64);
                let mut scale_changed = false;
                ui.horizontal(|ui| {
                    let slider_changed = ui
                        .add(
                            egui::Slider::new(&mut slider_scale, 1..=config::MAX_SCALE as i64)
                                .suffix("x")
                                .step_by(1.0),
                        )
                        .changed();
                    let input_changed = ui
                        .add(
                            egui::DragValue::new(&mut scale)
                                .range(1..=config::MAX_SCALE_INPUT as i64)
                                .suffix("x"),
                        )
                        .changed();
                    if slider_changed {
                        scale = slider_scale;
                    }
                    scale_changed = slider_changed || input_changed;
                });
                if scale_changed {
                    overlay.scale = scale.clamp(1, config::MAX_SCALE_INPUT as i64) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Y axis height");
                let mut graph_height = overlay.graph_height_px.max(10) as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut graph_height)
                            .range(10..=10_000)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    overlay.graph_height_px = graph_height.clamp(10, 10_000) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Latency ceiling");
                let mut max_y = overlay.max_y_ms.max(1) as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut max_y)
                            .range(1..=1_000_000)
                            .suffix(" ms"),
                    )
                    .changed()
                {
                    overlay.max_y_ms = max_y.clamp(1, 1_000_000) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Orientation");
                let mut orientation = overlay.orientation;
                ComboBox::from_id_salt("orientation")
                    .selected_text(format!("{orientation} deg"))
                    .show_ui(ui, |ui| {
                        for value in [0u16, 90, 180, 270] {
                            ui.selectable_value(&mut orientation, value, format!("{value} deg"));
                        }
                    });
                if orientation != overlay.orientation {
                    overlay.orientation = orientation;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Mirrored");
                ui.horizontal(|ui| {
                    if ui.selectable_label(!overlay.mirrored, "No").clicked() && overlay.mirrored {
                        overlay.mirrored = false;
                        *changed = true;
                    }
                    if ui.selectable_label(overlay.mirrored, "Yes").clicked() && !overlay.mirrored {
                        overlay.mirrored = true;
                        *changed = true;
                    }
                });
                ui.end_row();

                ui.label("Smooth rendering");
                if ui
                    .checkbox(&mut overlay.smooth_rendering, "Enabled")
                    .changed()
                {
                    *changed = true;
                }
                ui.end_row();

                ui.label("Smooth FPS").on_hover_text(
                    "Target redraw rate for the graph; this does not change the probe cadence.",
                );
                let mut smooth_fps = overlay.smooth_fps as i64;
                if ui
                    .add_enabled(
                        overlay.smooth_rendering,
                        egui::DragValue::new(&mut smooth_fps)
                            .range(config::MIN_SMOOTH_FPS as i64..=config::MAX_SMOOTH_FPS as i64)
                            .suffix(" FPS"),
                    )
                    .changed()
                {
                    overlay.smooth_fps = smooth_fps
                        .clamp(config::MIN_SMOOTH_FPS as i64, config::MAX_SMOOTH_FPS as i64)
                        as u32;
                    *changed = true;
                }
                ui.end_row();
            });
    });

    section(ui, "Colors", |ui| {
        Grid::new("colors-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                color_field(ui, "Background color", &mut overlay.bg_color, changed);
                ui.end_row();

                ui.label("Background opacity");
                let mut opacity = overlay.bg_opacity.min(100) as i64;
                if ui
                    .add(
                        egui::Slider::new(&mut opacity, 0..=100)
                            .suffix("%")
                            .step_by(1.0),
                    )
                    .changed()
                {
                    overlay.bg_opacity = opacity.clamp(0, 100) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Stroke width")
                    .on_hover_text("Width of every line and of the timeout markers.");
                let mut stroke_width = overlay.line_stroke_px;
                if ui
                    .add(
                        egui::Slider::new(
                            &mut stroke_width,
                            config::MIN_LINE_STROKE_PX..=config::MAX_LINE_STROKE_PX,
                        )
                        .suffix(" px")
                        .step_by(0.5),
                    )
                    .changed()
                {
                    overlay.line_stroke_px =
                        stroke_width.clamp(config::MIN_LINE_STROKE_PX, config::MAX_LINE_STROKE_PX);
                    *changed = true;
                }
                ui.end_row();

                ui.label("Line glow").on_hover_text(
                    "Cast a soft glow under each line, in the line's own colour.\n\
                     The glow rotates with the graph and the box reserves room \
                     for it past the zero line; lowering the intensity pulls \
                     the glow, and its room, back toward the line.",
                );
                if ui.checkbox(&mut overlay.line_glow, "Enabled").changed() {
                    *changed = true;
                }
                ui.end_row();

                ui.label("Glow intensity");
                let mut glow_intensity = overlay.line_glow_intensity as i64;
                if ui
                    .add_enabled(
                        overlay.line_glow,
                        egui::Slider::new(
                            &mut glow_intensity,
                            config::MIN_LINE_GLOW_INTENSITY as i64
                                ..=config::MAX_LINE_GLOW_INTENSITY as i64,
                        )
                        .suffix("%")
                        .step_by(1.0),
                    )
                    .on_hover_text(
                        "Strength of the glow. A fainter glow also reaches \
                         less far: the radius is its reach at full intensity.",
                    )
                    .changed()
                {
                    overlay.line_glow_intensity = glow_intensity.clamp(
                        config::MIN_LINE_GLOW_INTENSITY as i64,
                        config::MAX_LINE_GLOW_INTENSITY as i64,
                    ) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Glow radius")
                    .on_hover_text("How far the glow reaches past each line at full intensity.");
                let mut glow_radius = overlay.line_glow_radius_px as i64;
                if ui
                    .add_enabled(
                        overlay.line_glow,
                        egui::Slider::new(
                            &mut glow_radius,
                            config::MIN_LINE_GLOW_RADIUS_PX as i64
                                ..=config::MAX_LINE_GLOW_RADIUS_PX as i64,
                        )
                        .suffix(" px")
                        .step_by(1.0),
                    )
                    .changed()
                {
                    overlay.line_glow_radius_px = glow_radius.clamp(
                        config::MIN_LINE_GLOW_RADIUS_PX as i64,
                        config::MAX_LINE_GLOW_RADIUS_PX as i64,
                    ) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Sample cursor").on_hover_text(
                    "Mark each line's newest drawn sample with a triangle.\n\
                     The cursor sits at the last value the line drew and \
                     rotates with the graph.",
                );
                if ui.checkbox(&mut overlay.sample_cursor, "Enabled").changed() {
                    *changed = true;
                }
                ui.end_row();

                ui.label("Cursor size")
                    .on_hover_text("Length of the triangle's body.");
                let mut cursor_size = overlay.sample_cursor_size_px as i64;
                if ui
                    .add_enabled(
                        overlay.sample_cursor,
                        egui::Slider::new(
                            &mut cursor_size,
                            config::MIN_SAMPLE_CURSOR_SIZE_PX as i64
                                ..=config::MAX_SAMPLE_CURSOR_SIZE_PX as i64,
                        )
                        .suffix(" px")
                        .step_by(1.0),
                    )
                    .changed()
                {
                    overlay.sample_cursor_size_px = cursor_size.clamp(
                        config::MIN_SAMPLE_CURSOR_SIZE_PX as i64,
                        config::MAX_SAMPLE_CURSOR_SIZE_PX as i64,
                    ) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Blink on timeout").on_hover_text(
                    "Pulse the cursor in the blink color while the newest sample \
                     is a timeout.\nThe pulse starts with the first failed probe \
                     instead of waiting for smooth rendering to reveal it.",
                );
                if ui
                    .add_enabled(
                        overlay.sample_cursor,
                        egui::Checkbox::new(&mut overlay.cursor_timeout_blink, "Enabled"),
                    )
                    .on_hover_text("Requires the sample cursor.")
                    .changed()
                {
                    *changed = true;
                }
                ui.end_row();

                ui.add_enabled_ui(
                    overlay.sample_cursor && overlay.cursor_timeout_blink,
                    |ui| {
                        color_field(
                            ui,
                            "Blink color",
                            &mut overlay.cursor_timeout_blink_color,
                            changed,
                        );
                    },
                );
                ui.end_row();
            });
    });

    section(ui, "Startup Behaviors", |ui| {
        Grid::new("startup-behaviors-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Cosmetic startup prefill")
                    .on_hover_text("Show a cosmetic fake graph before real samples arrive.");
                if ui
                    .checkbox(&mut overlay.cosmetic_startup_prefill, "Enabled")
                    .changed()
                {
                    *changed = true;
                }
                ui.end_row();

                color_field(
                    ui,
                    "Prefill line color",
                    &mut overlay.prefill_line_color,
                    changed,
                );
                ui.end_row();

                ui.label("Prefill animation");
                let mut prefill_animation = overlay.prefill_animation_sec as i64;
                if ui
                    .add_enabled(
                        overlay.cosmetic_startup_prefill,
                        egui::DragValue::new(&mut prefill_animation)
                            .range(
                                config::MIN_PREFILL_ANIMATION_SEC as i64
                                    ..=config::MAX_PREFILL_ANIMATION_SEC as i64,
                            )
                            .suffix(" seconds"),
                    )
                    .changed()
                {
                    overlay.prefill_animation_sec = prefill_animation.clamp(
                        config::MIN_PREFILL_ANIMATION_SEC as i64,
                        config::MAX_PREFILL_ANIMATION_SEC as i64,
                    ) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Startup border effect").on_hover_text(
                    "The selected tab always activates the RGB loop; this controls startup only.",
                );
                let mut border_effect = overlay.startup_border_effect;
                ComboBox::from_id_salt("startup-border-effect")
                    .selected_text(border_effect_label(border_effect))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut border_effect, BorderEffect::RgbLoop, "RGB loop");
                        ui.selectable_value(
                            &mut border_effect,
                            BorderEffect::RgbNoise,
                            "RGB noise",
                        );
                        ui.selectable_value(&mut border_effect, BorderEffect::Disabled, "Disabled");
                    });
                if border_effect != overlay.startup_border_effect {
                    overlay.startup_border_effect = border_effect;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Border animation");
                let mut border_animation = overlay.border_animation_sec as i64;
                if ui
                    .add_enabled(
                        overlay.startup_border_effect != BorderEffect::Disabled,
                        egui::DragValue::new(&mut border_animation)
                            .range(
                                config::MIN_BORDER_ANIMATION_SEC as i64
                                    ..=config::MAX_BORDER_ANIMATION_SEC as i64,
                            )
                            .suffix(" seconds"),
                    )
                    .changed()
                {
                    overlay.border_animation_sec = border_animation.clamp(
                        config::MIN_BORDER_ANIMATION_SEC as i64,
                        config::MAX_BORDER_ANIMATION_SEC as i64,
                    ) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Border fade out");
                let mut border_fade = overlay.border_fade_sec as i64;
                if ui
                    .add_enabled(
                        overlay.startup_border_effect != BorderEffect::Disabled,
                        egui::DragValue::new(&mut border_fade)
                            .range(0..=config::MAX_BORDER_FADE_SEC as i64)
                            .suffix(" seconds"),
                    )
                    .changed()
                {
                    overlay.border_fade_sec =
                        border_fade.clamp(0, config::MAX_BORDER_FADE_SEC as i64) as u32;
                    *changed = true;
                }
                ui.end_row();
            });
    });
}

fn section(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .strong()
            .color(UI_ACCENT()),
    );
    ui.separator();
    add_contents(ui);
}

fn color_field(ui: &mut Ui, label: &str, value: &mut String, changed: &mut bool) {
    ui.label(label);
    let mut color = parse_color(value);
    if ui.color_edit_button_srgba(&mut color).changed() {
        *value = color_to_hex(color);
        *changed = true;
    }
}

fn set_probe_host(probe: &mut ProbeConfig, host: String) {
    match probe {
        ProbeConfig::Icmp { host: current } | ProbeConfig::Tcp { host: current, .. } => {
            *current = host;
        }
    }
}

fn parse_color(value: &str) -> Color32 {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return Color32::from_rgb(74, 222, 128);
    }
    let channel = |offset: usize| u8::from_str_radix(&value[offset..offset + 2], 16).unwrap_or(0);
    Color32::from_rgb(channel(0), channel(2), channel(4))
}

fn color_to_hex(color: Color32) -> String {
    let value = color.to_array();
    format!("#{:02x}{:02x}{:02x}", value[0], value[1], value[2])
}

fn border_effect_label(effect: BorderEffect) -> &'static str {
    match effect {
        BorderEffect::RgbLoop => "RGB loop",
        BorderEffect::RgbNoise => "RGB noise",
        BorderEffect::Disabled => "Disabled",
    }
}

fn draw_position_indicator(
    painter: &egui::Painter,
    rect: egui::Rect,
    anchor: Anchor,
    color: Color32,
) {
    let center = rect.center();
    let arm = rect.width().min(rect.height()) * 0.30;
    let stroke = egui::Stroke::new(1.5, color);
    let direction = match anchor {
        Anchor::TopLeft => Some(egui::vec2(-1.0, -1.0)),
        Anchor::TopCenter => Some(egui::vec2(0.0, -1.0)),
        Anchor::TopRight => Some(egui::vec2(1.0, -1.0)),
        Anchor::CenterLeft => Some(egui::vec2(-1.0, 0.0)),
        Anchor::Center => None,
        Anchor::CenterRight => Some(egui::vec2(1.0, 0.0)),
        Anchor::BottomLeft => Some(egui::vec2(-1.0, 1.0)),
        Anchor::BottomCenter => Some(egui::vec2(0.0, 1.0)),
        Anchor::BottomRight => Some(egui::vec2(1.0, 1.0)),
    };

    if let Some(direction) = direction {
        let direction = direction.normalized();
        let tip = center + direction * arm;
        let tail = center - direction * arm;
        let head = arm * 0.55;
        let perpendicular = egui::vec2(-direction.y, direction.x) * head * 0.7;
        painter.line_segment([tail, tip], stroke);
        painter.line_segment([tip, tip - direction * head + perpendicular], stroke);
        painter.line_segment([tip, tip - direction * head - perpendicular], stroke);
    } else {
        let radius = arm * 0.65;
        let tick = arm * 0.35;
        painter.circle_stroke(center, radius, stroke);
        painter.line_segment(
            [
                center - egui::vec2(tick, 0.0),
                center + egui::vec2(tick, 0.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                center - egui::vec2(0.0, tick),
                center + egui::vec2(0.0, tick),
            ],
            stroke,
        );
    }
}

fn position_name(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "topLeft",
        Anchor::TopCenter => "topCenter",
        Anchor::TopRight => "topRight",
        Anchor::CenterLeft => "centerLeft",
        Anchor::Center => "center",
        Anchor::CenterRight => "centerRight",
        Anchor::BottomLeft => "bottomLeft",
        Anchor::BottomCenter => "bottomCenter",
        Anchor::BottomRight => "bottomRight",
    }
}

/// Painted rail glyphs, so the rail never depends on font coverage.
fn draw_nav_icon(painter: &egui::Painter, rect: egui::Rect, page: Page, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    let unit = rect.width().min(rect.height());
    let center = rect.center();
    let radius = egui::CornerRadius::same(2);
    match page {
        // Two offset squares, read as a stack of overlays.
        Page::Overlays => {
            let size = unit * 0.58;
            let offset = unit * 0.16;
            for direction in [1.0, -1.0] {
                let square = egui::Rect::from_center_size(
                    center + egui::vec2(offset * direction, -offset * direction),
                    egui::Vec2::splat(size),
                );
                painter.rect_stroke(square, radius, stroke, egui::StrokeKind::Middle);
            }
        }
        // Three stacked cards, read as saved profiles.
        Page::Profiles => {
            let width = unit * 0.7;
            let height = unit * 0.19;
            let step = unit * 0.3;
            for row in -1..=1 {
                let card = egui::Rect::from_center_size(
                    egui::pos2(center.x, center.y + row as f32 * step),
                    egui::vec2(width, height),
                );
                painter.rect_stroke(card, radius, stroke, egui::StrokeKind::Middle);
            }
        }
        // Three tracks with a knob each, read as global preferences. The
        // vertical offsets are symmetric around the centre and every knob sits
        // inside the track it belongs to.
        Page::Global => {
            let half = GLOBAL_ICON_TRACK_HALF * unit;
            for (row, knob) in GLOBAL_ICON_ROWS {
                let y = center.y + row * unit;
                painter.line_segment(
                    [
                        egui::pos2(center.x - half, y),
                        egui::pos2(center.x + half, y),
                    ],
                    stroke,
                );
                painter.circle_filled(
                    egui::pos2(center.x + half * knob, y),
                    GLOBAL_ICON_KNOB_RADIUS * unit,
                    color,
                );
            }
        }
        // A lower case `i`, drawn as a dot over a short stem. The two vertical
        // offsets do not sum to zero because the dot and the stem are not the
        // same shape, so centring is checked against the drawn extent rather
        // than the offsets: `the_about_glyph_stays_inside_its_box` holds the
        // span to half a unit either way.
        Page::About => {
            for (row, half_width) in ABOUT_ICON_ROWS {
                let y = center.y + row * unit;
                let half = half_width * unit;
                if half <= 0.0 {
                    // The dot: a filled circle, so it needs no stroke.
                    painter.circle_filled(
                        egui::pos2(center.x, y),
                        ABOUT_ICON_DOT_RADIUS * unit,
                        color,
                    );
                } else {
                    // The stem.
                    painter.line_segment(
                        [
                            egui::pos2(center.x - half, y),
                            egui::pos2(center.x + half, y),
                        ],
                        stroke,
                    );
                }
            }
        }
    }
}

/// A theme tile's mode icon, painted rather than shipped as artwork.
///
/// Painted for the same reason the rail's glyphs are: it needs no image assets,
/// no light and dark variants, and it picks the theme's colours up for free.
///
/// `fill` is the colour the tile was filled with behind the glyph. The moon is
/// a disc with a bite taken out, and egui has no subtractive clip, so the bite
/// is painted in the tile's fill — exact here because the tile painted itself
/// and hands the same colour back. Anywhere else the bite would show.
fn draw_theme_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    mode: ThemeMode,
    color: Color32,
    fill: Color32,
) {
    let unit = rect.width().min(rect.height());
    let center = rect.center();
    let stroke = egui::Stroke::new(THEME_ICON_STROKE * unit, color);
    match mode {
        // A sun: a core disc with eight rays.
        ThemeMode::Light => {
            painter.circle_filled(center, THEME_SUN_CORE_RADIUS * unit, color);
            for (ray_x, ray_y) in THEME_SUN_RAYS {
                let direction = egui::vec2(ray_x, ray_y);
                painter.line_segment(
                    [
                        center + direction * (THEME_SUN_RAY_INNER * unit),
                        center + direction * (THEME_SUN_RAY_OUTER * unit),
                    ],
                    stroke,
                );
            }
        }
        // A crescent: one disc with a second, offset disc cut out of it.
        ThemeMode::Dark => {
            painter.circle_filled(center, THEME_MOON_RADIUS * unit, color);
            let (bite_x, bite_y) = THEME_MOON_BITE_OFFSET;
            painter.circle_filled(
                center + egui::vec2(bite_x, bite_y) * unit,
                THEME_MOON_BITE_RADIUS * unit,
                fill,
            );
        }
        // A half-filled circle: the left half full, the whole thing outlined.
        ThemeMode::System => {
            let radius = THEME_SYSTEM_RADIUS * unit;
            let mut points = Vec::with_capacity(THEME_SYSTEM_ARC_STEPS + 1);
            for step in 0..=THEME_SYSTEM_ARC_STEPS {
                let angle = std::f32::consts::FRAC_PI_2
                    + step as f32 * std::f32::consts::PI / THEME_SYSTEM_ARC_STEPS as f32;
                points.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
            }
            // A closed semicircle is convex, so it can be filled; the outline
            // goes on afterwards so the stroke is not covered by the fill.
            painter.add(egui::Shape::convex_polygon(
                points,
                color,
                egui::Stroke::NONE,
            ));
            painter.circle_stroke(center, radius, stroke);
        }
    }
}

/// The chevron that collapses the rail, pointing away from where it goes.
fn draw_collapse_chevron(
    painter: &egui::Painter,
    rect: egui::Rect,
    collapsed: bool,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let center = rect.center();
    let arm = rect.width() * 0.24;
    let tip = center + egui::vec2(if collapsed { arm } else { -arm }, 0.0);
    for offset in [-arm, arm] {
        painter.line_segment([tip, center + egui::vec2(0.0, offset)], stroke);
    }
}

/// The painted arrow that marks a widget as opening a menu.
fn draw_dropdown_arrow(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let tip = egui::pos2(rect.right() - 14.0, rect.center().y + 3.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(tip.x - 5.0, tip.y - 5.0),
            egui::pos2(tip.x + 5.0, tip.y - 5.0),
            tip,
        ],
        color,
        egui::Stroke::NONE,
    ));
}

pub fn run() {
    ping_latency_overlay_core::overlay::enable_dpi_awareness();
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id("ping-latency-overlay")
            .with_title("PingLatencyOverlay - Config")
            .with_inner_size(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT))
            .with_min_inner_size(egui::vec2(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT))
            .with_max_inner_size(egui::vec2(WINDOW_MAX_WIDTH, WINDOW_MAX_HEIGHT))
            .with_resizable(true)
            .with_visible(false)
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "PingLatencyOverlay",
        native_options,
        Box::new(|cc| PingApp::new(cc).map(|app| Box::new(app) as Box<dyn App>)),
    )
    .expect("failed to start PingLatencyOverlay");
}

#[cfg(test)]
mod tests {
    use super::{
        about_page_lines, app_version, append_status_message, auto_preview_line,
        auto_switch_section, auto_switch_step, can_switch_profile, choose_theme, config,
        config_notice_status, config_notices_status, config_push_due, default_fallback,
        deselect_strip_rect, draw_pane_divider, empty_editor, enabled_target_keys,
        fill_sticky_target, host_row_label, list_pane_column, list_pane_row_height,
        list_pane_row_width_for, monitor_choice_label, monitor_choices, overlay_count_label,
        overlay_name_width, overlay_row_contents, overlay_row_label, page_has_detail_footer,
        page_has_list_pane, pending_edits, pick_cursor, pick_step, profile_name_width,
        profile_row_contents, profile_row_label, rail_width, requested_url, retired_targets,
        row_inner, rule_error, runtime_config_changed, selected_overlay_for_border,
        selected_target_in, set_background_tracking, set_selection_border_animation,
        set_sticky_value, sticky_value, sync_auto_preview_text, sync_monitor_list,
        sync_profile_cache, sync_theme, theme, theme_choice_hint, theme_choices,
        theme_tile_label_size, theme_tile_side, theme_tiles, toggled_selection, ui_text_size,
        window_title, AboutKind, AutoSwitchStep, Frame, Mode, Page, PingApp, ProfileSnapshot,
        ThemeMode, ABOUT_ICON_DOT_RADIUS, ABOUT_ICON_ROWS, ABOUT_REPOSITORY,
        DETAIL_FOOTER_BUTTON_HEIGHT, DETAIL_FOOTER_BUTTON_WIDTH, DETAIL_FOOTER_HEIGHT,
        GLOBAL_ICON_KNOB_RADIUS, GLOBAL_ICON_ROWS, GLOBAL_ICON_TRACK_HALF, LIST_PANE_INSET,
        MONITOR_REFRESH_INTERVAL, OVERLAY_ROW_HEIGHT, PAGES, PANE_GAP, PANE_MARGIN,
        PICK_ICON_RADIUS, PICK_ICON_STROKE, PICK_ICON_TICK_INNER, PICK_ICON_TICK_OUTER,
        PROFILE_ROW_HEIGHT, PROFILE_ROW_TRAILING, RAIL_ROW_HEIGHT, RAIL_WIDTH, ROW_MARGIN,
        SCROLL_BAR_RESERVE, SIDEBAR_WIDTH, STATUS_BAR_HEIGHT, TARGET_ROW_HEIGHT, THEME_ICON_STROKE,
        THEME_MOON_BITE_OFFSET, THEME_MOON_BITE_RADIUS, THEME_MOON_RADIUS, THEME_SUN_CORE_RADIUS,
        THEME_SUN_RAYS, THEME_SUN_RAY_OUTER, THEME_SYSTEM_RADIUS, THEME_TILE_GAP, UI_BACKGROUND,
        WINDOW_HEIGHT, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH, WINDOW_WIDTH,
    };
    use eframe::egui;
    use ping_latency_overlay_core::config::{
        Anchor, Config, ConfigNotice, OverlayConfig, ProbeConfig, ProfileEntry, TargetConfig,
    };
    use ping_latency_overlay_core::monitors::{self, MonitorInfo};
    use ping_latency_overlay_core::probes::TaskKey;
    use ping_latency_overlay_core::rules::{
        AutoRules, Combine, Condition, Engine, MatchMode, Part, Rule, Scope, Snapshot, WindowInfo,
    };
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    /// A staged edit reaches the renderer without a Save.
    ///
    /// This is the behaviour that went missing when the renderer became its own
    /// process: the per-frame `sync_overlays()` the old in-process overlay
    /// manager used was what made every edit live, and replacing it with a pipe
    /// message put the message only on the save path. The regression was silent
    /// — the overlay still appeared on Save — so nothing failed, the app just
    /// quietly stopped feeling live.
    #[test]
    fn a_staged_edit_reaches_the_renderer_without_saving() {
        let mut draft = Config::default();
        let mut last_pushed = draft.clone();

        // Nothing staged, nothing to send.
        assert!(
            !runtime_config_changed(&mut draft, &last_pushed),
            "an untouched window sends the configuration every frame"
        );

        draft.overlays.push(OverlayConfig::new());
        assert!(
            runtime_config_changed(&mut draft, &last_pushed),
            "a staged overlay never reached the renderer"
        );
        last_pushed = draft.clone();

        // A colour, which is the edit that was silently save-only.
        draft.overlays[0].first_target_mut().line_color = "#123456".to_string();
        assert!(
            runtime_config_changed(&mut draft, &last_pushed),
            "a colour change never reached the renderer"
        );
        last_pushed = draft.clone();

        // A size, which resizes the layered window.
        draft.overlays[0].window_seconds = 120;
        assert!(
            runtime_config_changed(&mut draft, &last_pushed),
            "a size change never reached the renderer"
        );
        last_pushed = draft.clone();

        // A host, which is a probe change and not only an appearance one.
        draft.overlays[0].add_target();
        assert!(
            runtime_config_changed(&mut draft, &last_pushed),
            "adding a host never reached the renderer"
        );
    }

    /// A frame where nothing changed sends nothing.
    ///
    /// The other half of the same behaviour, and the one that decides whether
    /// it is affordable: this runs on every pass, so a version that pushed
    /// unconditionally would serialise the whole configuration ten times a
    /// second while the user reads the status bar.
    #[test]
    fn an_unchanged_window_sends_nothing() {
        let mut draft = Config::default();
        draft.overlays.push(OverlayConfig::new());
        runtime_config_changed(&mut draft, &Config::default());
        let mut last_pushed = draft.clone();

        for _ in 0..50 {
            assert!(
                !runtime_config_changed(&mut draft, &last_pushed),
                "an idle window is still sending"
            );
            last_pushed = draft.clone();
        }
    }

    /// Normalization happens **before** the comparison, not only before a save.
    ///
    /// A draft holding an out-of-range value normalizes to the same thing the
    /// renderer was last told, so it settles instead of resending the whole
    /// configuration on every frame for as long as the window stays open. The
    /// alternative is not a visible failure — it is a pipe write and a full
    /// config parse ten times a second, forever.
    #[test]
    fn an_out_of_range_draft_settles_instead_of_resending() {
        let mut draft = Config::default();
        let mut overlay = OverlayConfig::new();
        overlay.window_seconds = 5;
        draft.overlays.push(overlay);

        // The renderer was last told the clamped value.
        let mut last_pushed = draft.clone();
        last_pushed.normalize();

        let mut sends = 0;
        for _ in 0..20 {
            if runtime_config_changed(&mut draft, &last_pushed) {
                sends += 1;
                last_pushed = draft.clone();
            }
        }

        assert_eq!(
            sends, 0,
            "a draft that normalizes to what the renderer already has sent \
             {sends} times in 20 frames"
        );
        assert_eq!(
            draft.overlays[0].window_seconds,
            ping_latency_overlay_core::config::MIN_WINDOW_SECONDS,
            "the draft was not normalized in place, so the editor is still \
             showing a value Save would clamp"
        );
    }

    /// A save retires exactly the removals it commits.
    ///
    /// This is the sender half of the retire list: the renderer cannot tell a
    /// saved removal from a profile switched away from, so the window has to
    /// tell it. A wrong diff would either stop a probe whose overlay is still
    /// on screen or keep a deleted host measuring.
    #[test]
    fn a_save_retires_the_targets_it_removed() {
        let mut before = Config::default();
        before.overlays.push(OverlayConfig::new());
        before.overlays[0].add_target();
        let saved = enabled_target_keys(&before);

        // Nothing changed: nothing to retire.
        assert!(
            retired_targets(&saved, &before).is_empty(),
            "a save that changes nothing retired something"
        );

        // A removal is retired, and it is named.
        let mut after = before.clone();
        after.overlays[0].targets.truncate(1);
        assert_eq!(
            retired_targets(&saved, &after),
            vec![TaskKey::new(
                &after.overlays[0].id,
                &before.overlays[0].targets[1].id
            )],
            "a removed host was not retired"
        );

        // An addition is not: the diff is one-directional, or adding a host
        // would stop the probe the user just asked for.
        let mut grown = before.clone();
        grown.overlays[0].add_target();
        assert!(
            retired_targets(&saved, &grown).is_empty(),
            "an added host was retired"
        );

        // Disabling is a removal too, because the renderer stops probing it.
        let mut disabled = before.clone();
        disabled.overlays[0].enabled = false;
        assert_eq!(
            retired_targets(&saved, &disabled).len(),
            saved.len(),
            "disabling an overlay did not retire its hosts"
        );
    }

    /// A background-tracking change is sent once, not every frame.
    ///
    /// The preference is staged and the renderer learns it by comparison, so
    /// the comparison has to settle: a version that did not record the send
    /// would push a full config on every pass for as long as the window is
    /// open — ten parses a second, with nothing to see but the CPU.
    #[test]
    fn a_background_tracking_change_is_sent_once_and_settles() {
        let mut draft = Config::default();
        let mut last_pushed = draft.clone();
        let mut last_sent = true;
        let mut sends = 0;

        for _ in 0..20 {
            if config_push_due(&mut draft, &last_pushed, true, last_sent, false) {
                sends += 1;
                last_sent = true;
                last_pushed = draft.clone();
            }
        }
        assert_eq!(sends, 0, "an untouched window kept sending");

        // The setting is saved off: the renderer is told once, then the
        // comparison matches and the window goes quiet again.
        for _ in 0..20 {
            if config_push_due(&mut draft, &last_pushed, false, last_sent, false) {
                sends += 1;
                last_sent = false;
                last_pushed = draft.clone();
            }
        }
        assert_eq!(
            sends, 1,
            "the preference change was sent {sends} times, not once"
        );
    }

    /// A committed removal keeps the push due until it lands.
    ///
    /// The disk already dropped the host, so a send that never happened has to
    /// be retried rather than forgotten; this is the half of the retire list
    /// that makes "saved means stopped" true across a dead renderer.
    #[test]
    fn a_pending_removal_keeps_the_push_due() {
        let mut draft = Config::default();
        let last_pushed = draft.clone();
        assert!(
            !config_push_due(&mut draft, &last_pushed, true, true, false),
            "nothing changed, so nothing is due"
        );
        assert!(
            config_push_due(&mut draft, &last_pushed, true, true, true),
            "a committed removal the renderer never took left the push idle"
        );
    }

    /// The value on screen while dragging is the value that gets written.
    ///
    /// These are the same claim as the test above seen from the other side: if
    /// normalization happened only on the way to the renderer, the overlay would
    /// move as the user dragged and then jump again on Save — the "briefly
    /// right, then it changes back" shape this app has produced twice already.
    #[test]
    fn a_dragged_value_is_clamped_before_it_is_shown() {
        let mut draft = Config::default();
        let mut overlay = OverlayConfig::new();
        overlay.scale = ping_latency_overlay_core::config::MAX_SCALE_INPUT + 500;
        overlay.bg_opacity = 200;
        draft.overlays.push(overlay);

        assert!(runtime_config_changed(&mut draft, &Config::default()));

        assert_eq!(
            draft.overlays[0].scale,
            ping_latency_overlay_core::config::MAX_SCALE_INPUT
        );
        assert_eq!(draft.overlays[0].bg_opacity, 100);
    }

    /// A host added next to an existing one is never given its colour.
    ///
    /// Two lines in one colour are one line as far as the reader is concerned,
    /// so the host you just added must not look like the one it sits next to.
    /// Adjacency is the property that holds at any size: past the size of the
    /// palette the rotation cycles and two hosts further apart can share a
    /// colour, and pretending otherwise would be a claim the code cannot keep.
    #[test]
    fn a_host_added_next_to_another_never_shares_its_colour() {
        let mut overlay = OverlayConfig::new();
        for _ in 0..12 {
            overlay.add_target();
        }

        let colours: Vec<&str> = overlay
            .targets
            .iter()
            .map(|target| target.line_color.as_str())
            .collect();
        for (index, pair) in colours.windows(2).enumerate() {
            assert_ne!(
                pair[0],
                pair[1],
                "hosts {} and {} were both given {}",
                index + 1,
                index + 2,
                pair[0]
            );
        }

        // Within one palette's worth they are all distinct, which is what makes
        // the neighbours of the common case separable rather than merely
        // different from each other.
        let first_six = &colours[..6];
        let mut sorted = first_six.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            first_six.len(),
            "the palette does not have six distinct colours"
        );
    }

    fn overlay_with_hosts(count: usize) -> OverlayConfig {
        let mut overlay = OverlayConfig::new();
        for index in 1..count {
            let mut target = overlay.first_target().clone();
            target.id = format!("{}-host-{index}", overlay.id);
            target.probe = ProbeConfig::Tcp {
                host: format!("host-{index}.example"),
                port: 443,
            };
            overlay.targets.push(target);
        }
        overlay
    }

    /// The host editor shows the host that is selected, and nothing else.
    ///
    /// The lookup is by id rather than by index, so a stale selection — a host
    /// deleted while the pane was showing another overlay — draws nothing rather
    /// than showing whichever host happens to be first. Showing host 1's fields
    /// under host 2's highlight is a worse failure than showing none: it is
    /// saved to host 1 and the user is editing host 2.
    #[test]
    fn the_host_editor_shows_only_the_selected_host() {
        let overlay = overlay_with_hosts(3);
        let second = overlay.targets[1].id.clone();
        let third = overlay.targets[2].id.clone();

        assert_eq!(
            selected_target_in(&overlay, Some(second.as_str())).map(|target| target.id.as_str()),
            Some(second.as_str())
        );
        assert_eq!(
            selected_target_in(&overlay, Some(third.as_str())).map(|target| target.probe.host()),
            Some("host-2.example")
        );
        assert!(
            selected_target_in(&overlay, None).is_none(),
            "no selection showed a host anyway"
        );
        assert!(
            selected_target_in(&overlay, Some("not-a-host")).is_none(),
            "an unknown id showed a host anyway"
        );
    }

    /// Two overlays may hold the same host id, so an id identifies a host only
    /// together with its overlay — which is why the editor never resolves one
    /// without the other.
    #[test]
    fn a_host_id_is_only_unique_within_its_overlay() {
        let first = overlay_with_hosts(2);
        let mut second = overlay_with_hosts(2);
        // The same id in both, which `validate` allows: it checks uniqueness per
        // overlay, and probes key on the pair.
        second.targets[1].id = first.targets[1].id.clone();

        let shared = first.targets[1].id.clone();
        assert_eq!(
            selected_target_in(&first, Some(shared.as_str())).map(|target| target.probe.host()),
            Some("host-1.example")
        );
        assert_eq!(
            selected_target_in(&second, Some(shared.as_str())).map(|target| target.probe.host()),
            Some("host-1.example"),
            "the two overlays are not distinguished by the lookup"
        );
    }

    /// A row that has not been given a host says so.
    ///
    /// The alternative is a row with no text in it, which reads as a broken row
    /// rather than as a host that needs filling in.
    #[test]
    fn a_row_with_no_host_says_so() {
        let mut target = TargetConfig::new();
        target.probe = ProbeConfig::Icmp {
            host: "   ".to_string(),
        };
        assert_eq!(host_row_label(&target, 2), "2. (no host yet)");

        target.probe = ProbeConfig::Icmp {
            host: "1.1.1.1".to_string(),
        };
        assert_eq!(host_row_label(&target, 2), "2. 1.1.1.1");
    }

    /// A group reads as a group in the list.
    ///
    /// Four overlays of one host each and one overlay of four hosts look
    /// identical otherwise, and that is exactly the confusion grouping
    /// introduces. One host shows no count, so the common case is unchanged.
    #[test]
    fn an_overlay_row_says_how_many_hosts_are_enabled() {
        let mut one = overlay_with_hosts(1);
        assert_eq!(overlay_row_label(&one), "New overlay");

        let mut four = overlay_with_hosts(4);
        assert_eq!(overlay_row_label(&four), "New overlay (4/4 hosts)");

        four.targets[1].enabled = false;
        four.targets[2].enabled = false;
        assert_eq!(overlay_row_label(&four), "New overlay (2/4 hosts)");

        // An overlay with nothing enabled still says so, rather than reading as
        // the single-host case with the "1" omitted.
        for target in &mut one.targets {
            target.enabled = false;
        }
        assert_eq!(overlay_row_label(&one), "New overlay (0/1 hosts)");
    }

    /// A host row is a row: it carries its own margin, so its controls are not
    /// flush against the fill.
    ///
    /// The same rule `a_painted_row_carries_its_margin` holds for the overlay
    /// rows, and for the same reason — `rect_filled` does not size itself, so a
    /// row allocated at its content height hands the child less room than it
    /// asked for.
    #[test]
    fn a_host_row_carries_its_margin() {
        let height = list_pane_row_height(TARGET_ROW_HEIGHT);
        assert!(
            height > TARGET_ROW_HEIGHT,
            "the host row is allocated at exactly its content height, so its \
             controls sit flush against the fill"
        );
    }

    /// A display with no appbar, so `bounds` and `work` agree and a test about
    /// the picker is not also a test of taskbar geometry.
    fn panel(device: &str, x: i32, y: i32, width: i32, height: i32, dpi: u32) -> MonitorInfo {
        let rect = monitors::Rect {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        MonitorInfo {
            device: device.to_string(),
            bounds: rect,
            work: rect,
            dpi,
            primary: false,
        }
    }

    fn primary_panel(
        device: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        dpi: u32,
    ) -> MonitorInfo {
        MonitorInfo {
            primary: true,
            ..panel(device, x, y, width, height, dpi)
        }
    }

    /// The installer and the app must report the same version.
    ///
    /// `build.rs` computes what the About page shows and
    /// `scripts/build-nsis.ps1` computes what the installer is *named*. They
    /// are two implementations of one rule, in two languages, reading one
    /// `Cargo.toml` — and an app that reports `0.2.3` inside a file called
    /// `0.1.77-setup.exe` is exactly the sort of drift nobody notices until a
    /// user files a bug about it. So this asks the script, rather than
    /// re-deriving the rule a third time here.
    ///
    /// Comparing against `env!("APP_BUILD_VERSION")` is the whole point: that
    /// is the value compiled into the binary, so this catches a script that
    /// has drifted from the build as well as a rule that changed on one side.
    #[test]
    fn the_installer_and_the_app_agree_on_the_version() {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has a parent")
            .join("scripts")
            .join("build-nsis.ps1");
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(["-VersionOnly"])
            .output()
            .expect("powershell is available on a Windows build");
        assert!(
            output.status.success(),
            "the packaging script could not report its version: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let scripted = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Compared against the raw compiled value rather than `app_version()`,
        // which only prefixes a `v` for display. The claim is the same one the
        // About page makes, just without the decoration, so nothing is
        // normalised away and a real difference cannot hide in formatting.
        assert_eq!(
            scripted,
            env!("APP_BUILD_VERSION"),
            "the installer would be named with {scripted} while the app reports {}",
            app_version()
        );
    }

    /// The shell looks for the renderer next to itself.
    ///
    /// The path decides whether the renderer is ever found at all, and nothing
    /// at runtime says otherwise: a wrong directory produces an app that starts
    /// cleanly and shows no overlays. Asserted on the function rather than on the
    /// name constant, because a constant can only be compared to another
    /// constant, which is documentation wearing a test's clothes.
    #[test]
    fn the_renderer_is_a_sibling_exe() {
        let path = ping_latency_overlay_core::transport::sibling_exe(
            ping_latency_overlay_core::transport::RENDERER_EXE,
        );
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(ping_latency_overlay_core::transport::RENDERER_EXE),
            "the shell must spawn the renderer by this exact name, not a guess"
        );
        let shell = std::env::current_exe().expect("the shell is running from somewhere");
        assert_eq!(
            path.parent(),
            shell.parent(),
            "the renderer has to be a sibling of the shell, so the same path works \
             installed and under `cargo run`"
        );
    }

    /// Where a list pane row's contents landed, measured through the real row
    /// functions rather than through arithmetic on the constants.
    ///
    /// A row's contents shipped outside its painted fill twice: once because the
    /// row was allocated shorter than its contents needed, and once because a
    /// nested `ui.horizontal` shifted everything down by half a row. Both were
    /// invisible to the constant arithmetic and obvious here, so the geometry is
    /// now measured instead of derived.
    fn assert_row_contents_stay_inside_the_row(
        row: egui::Rect,
        name: egui::Rect,
        controls: egui::Rect,
        what: &str,
    ) {
        let inner = row_inner(row);
        let tolerance = 0.01;
        for (label, rect) in [("name", name), ("controls", controls)] {
            assert!(
                rect.left() >= inner.left() - tolerance
                    && rect.right() <= inner.right() + tolerance
                    && rect.top() >= inner.top() - tolerance
                    && rect.bottom() <= inner.bottom() + tolerance,
                "the {what} row's {label} at {rect:?} escapes its contents {inner:?} \
                 (row {row:?}, margin {ROW_MARGIN}px)"
            );
        }
        assert!(
            (controls.right() - inner.right()).abs() <= tolerance,
            "the {what} row's controls end at {} but its contents end at {}",
            controls.right(),
            inner.right()
        );
        assert!(
            (name.left() - inner.left()).abs() <= tolerance,
            "the {what} row's name starts at {} but its contents start at {}",
            name.left(),
            inner.left()
        );
    }

    #[test]
    fn a_list_pane_row_puts_its_contents_inside_its_own_fill() {
        let row_width = list_pane_row_width_for(SIDEBAR_WIDTH);
        let ctx = egui::Context::default();
        let mut measured: Vec<(egui::Rect, egui::Rect, egui::Rect)> = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            // Two of each, so the columns are checked to line up down a list and
            // not merely to fit inside a single row.
            for _ in 0..2 {
                let (row, _) = ui.allocate_exact_size(
                    egui::vec2(row_width, list_pane_row_height(OVERLAY_ROW_HEIGHT)),
                    egui::Sense::click(),
                );
                let laid_out =
                    overlay_row_contents(ui, row, "Cloudflare", Anchor::TopLeft, true, true, false);
                measured.push((row, laid_out.name, laid_out.controls));
            }
            for _ in 0..2 {
                let (row, _) = ui.allocate_exact_size(
                    egui::vec2(row_width, list_pane_row_height(PROFILE_ROW_HEIGHT)),
                    egui::Sense::click(),
                );
                let laid_out = profile_row_contents(ui, row, "Gaming", Some(4), true, false);
                measured.push((row, laid_out.name, laid_out.gutter));
            }
        });
        // egui hands back the font atlas deltas it built, and a dropped delta is a
        // panic, so they are cleared instead.
        output.textures_delta.clear();

        let overlay_rows = &measured[..2];
        for (row, name, controls) in overlay_rows {
            assert_row_contents_stay_inside_the_row(*row, *name, *controls, "overlay");
        }
        for (row, name, gutter) in &measured[2..] {
            assert_row_contents_stay_inside_the_row(*row, *name, *gutter, "profile");
        }
        for pair in [overlay_rows, &measured[2..]] {
            assert_eq!(
                pair[0].2.left(),
                pair[1].2.left(),
                "two rows' right-hand controls are not in the same column"
            );
        }

        // The delete confirmation replaces the row's contents, so its buttons get
        // the same treatment.
        let ctx = egui::Context::default();
        let mut confirm = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(row_width, list_pane_row_height(OVERLAY_ROW_HEIGHT)),
                egui::Sense::click(),
            );
            let laid_out =
                overlay_row_contents(ui, row, "Cloudflare", Anchor::TopLeft, true, false, true);
            confirm = Some((row, laid_out.name, laid_out.controls));
        });
        output.textures_delta.clear();
        let (row, name, controls) = confirm.expect("the row was laid out");
        assert_row_contents_stay_inside_the_row(row, name, controls, "delete confirmation");
    }

    /// The rail is the leftmost pane, so the window has to be wide enough for the
    /// rail, the list pane and a usable detail pane at the same time.
    ///
    /// Both boundaries count, because the list pane is now separated from the
    /// detail pane by the same divider and gap as the rail is.
    #[test]
    fn the_window_fits_the_rail_the_list_and_the_detail_pane() {
        let boundaries = PANE_GAP * 2.0;
        let detail = WINDOW_MIN_WIDTH - PANE_MARGIN * 2.0 - RAIL_WIDTH - boundaries - SIDEBAR_WIDTH;
        assert!(
            detail >= 240.0,
            "detail pane would be only {detail}px wide at the minimum window size"
        );
    }

    /// The detail pane ends in a sticky Save/Discard footer, so the scrolling
    /// part of it has to stay usable at the minimum window height too.
    #[test]
    fn the_detail_pane_keeps_its_scrolling_area_above_the_footer() {
        let scrolling = WINDOW_MIN_HEIGHT - PANE_MARGIN - STATUS_BAR_HEIGHT - DETAIL_FOOTER_HEIGHT;
        assert!(
            scrolling >= 200.0,
            "detail pane would scroll in only {scrolling}px at the minimum window size"
        );
    }

    /// A painted row is taller than its contents by the margin on each side.
    ///
    /// This is not a restatement of the constants: a painted rect does not size
    /// itself the way an `egui::Frame` does, so leaving the margin out made
    /// every row's buttons sit flush against the fill.
    #[test]
    fn a_painted_row_carries_its_margin() {
        let row = list_pane_row_height(40.0);
        assert!(
            (row - 48.0).abs() < f32::EPSILON,
            "a row around 40px of content came out {row}px tall"
        );
    }

    /// The list pane's header, rows and footer must all be the same width, and
    /// that width has to fit inside the pane with its insets and the scroll
    /// bar's reserve still accounted for.
    #[test]
    fn a_list_pane_row_fits_its_pane() {
        let row_width = list_pane_row_width_for(SIDEBAR_WIDTH);
        let used = LIST_PANE_INSET * 2.0 + SCROLL_BAR_RESERVE + row_width;
        assert!(
            used <= SIDEBAR_WIDTH,
            "a list pane row needs {used}px but the pane offers {SIDEBAR_WIDTH}px"
        );
    }

    /// Arriving on the Profiles page is what makes the overlay-count cache true.
    ///
    /// The counts are read one file per profile, so they cannot be read per
    /// frame, and the Profiles page is the only place one is shown. The page
    /// shipped showing `0` on every row because nothing ever triggered the read:
    /// the cache started empty and only the profile mutations and the switcher
    /// filled it, so arriving from the rail left it empty and a missing count
    /// was drawn as zero.
    ///
    /// This drives `sync_profile_cache` itself with a counting reader, not just
    /// `arriving_page_needs_profiles`. Checking the predicate alone would still
    /// have passed with the call to `read` deleted, which is exactly the bug: the
    /// condition was right and nothing acted on it.
    #[test]
    fn arriving_on_the_profiles_page_re_reads_the_counts() {
        for (from, to, expected) in [
            (Page::Overlays, Page::Profiles, true),
            (Page::Global, Page::Profiles, true),
            (Page::Profiles, Page::Profiles, false),
            (Page::Profiles, Page::Overlays, false),
            (Page::Overlays, Page::Overlays, false),
        ] {
            let mut last_page = from;
            let mut profiles = Vec::new();
            let mut counts = HashMap::new();
            let mut reads = 0;
            let read = || {
                reads += 1;
                ProfileSnapshot {
                    profiles: vec![ProfileEntry {
                        id: "home".to_string(),
                        name: "Home".to_string(),
                    }],
                    counts: HashMap::from([("home".to_string(), 3)]),
                }
            };
            let refreshed =
                sync_profile_cache(&mut last_page, to, &mut profiles, &mut counts, read);

            assert_eq!(
                refreshed,
                expected,
                "going from {} to {} should{} re-read the profile counts",
                from.label(),
                to.label(),
                if expected { "" } else { " not" },
            );
            assert_eq!(
                reads,
                usize::from(expected),
                "going from {} to {} made {reads} disk reads, expected {}",
                from.label(),
                to.label(),
                usize::from(expected),
            );
            assert_eq!(
                last_page,
                to,
                "the page marker did not follow the page after going from {} to {}",
                from.label(),
                to.label(),
            );
            if expected {
                assert_eq!(
                    counts.get("home"),
                    Some(&3),
                    "arriving on the Profiles page did not publish the counts it read"
                );
                assert_eq!(
                    profiles.len(),
                    1,
                    "arriving on the Profiles page did not publish the profile list it read"
                );
            } else {
                assert!(
                    counts.is_empty() && profiles.is_empty(),
                    "staying off the Profiles page filled the cache anyway"
                );
            }
        }
    }

    /// The attached-display list is read once and then only on a clock, and the
    /// read has to actually happen rather than merely be due.
    ///
    /// A display can be plugged in while the window is open and nothing tells
    /// us, so this cannot be tied to a save or to arriving on a page. The first
    /// pass reads unconditionally — `monitors_read_at` starts as `None` — because
    /// a list that is empty until the clock comes round would show the picker
    /// with no monitors in it on the first frame, and the user would be looking
    /// at a combobox that claims there is nothing to choose.
    #[test]
    fn the_monitor_list_is_read_once_and_then_on_the_clock() {
        let start = Instant::now();
        let mut list: Vec<MonitorInfo> = Vec::new();
        let mut read_at = None;

        let first = sync_monitor_list(&mut list, &mut read_at, start, || {
            vec![panel("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96)]
        });
        assert!(first, "the first pass did not read the display list");
        assert_eq!(list.len(), 1, "the read list was not published");
        assert_eq!(
            read_at,
            Some(start),
            "the read time was not recorded, so the next pass would read again"
        );

        // Just inside the interval: no read, and the previous list is kept.
        let soon = start + MONITOR_REFRESH_INTERVAL / 2;
        let again = sync_monitor_list(&mut list, &mut read_at, soon, Vec::new);
        assert!(!again, "the display list was re-read before its interval");
        assert_eq!(list.len(), 1, "a skipped read dropped the cached list");

        // Past the interval: read, and a newly attached display is published.
        let later = start + MONITOR_REFRESH_INTERVAL;
        let due = sync_monitor_list(&mut list, &mut read_at, later, || {
            vec![
                panel("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96),
                panel("\\\\.\\DISPLAY2", 1920, 0, 2560, 1440, 144),
            ]
        });
        assert!(due, "a monitor plugged in did not reach the list");
        assert_eq!(
            list.len(),
            2,
            "the newly attached display was not published to the picker"
        );
    }

    /// A pinned display that is not attached stays in the picker, and stays
    /// selected.
    ///
    /// This is the whole mitigation for hiding an overlay whose monitor is
    /// missing. A combobox whose entry list does not contain the selected value
    /// falls back to its first row, so dropping the entry would not just hide
    /// the state — it would report a *different* monitor as chosen, and the user
    /// saving from there would silently re-pin the overlay. The entry is first so
    /// it is visible without scrolling.
    #[test]
    fn a_disconnected_pin_is_still_listed_and_still_selected() {
        let attached = [panel("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96)];
        let choices = monitor_choices(&attached, Some("\\\\.\\DISPLAY2"));

        assert_eq!(choices[0].device.as_deref(), Some("\\\\.\\DISPLAY2"));
        assert!(
            choices[0].label.contains("not connected"),
            "the disconnected display is not labelled as such: {}",
            choices[0].label
        );
        assert!(choices[0].is(Some("\\\\.\\DISPLAY2")));
        assert!(
            monitor_choice_label(&attached, Some("\\\\.\\DISPLAY2")).contains("not connected"),
            "the closed combobox does not show that the pinned display is gone"
        );

        // And the shape this must not become: a list without the pinned entry,
        // where the combobox has to fall back to the primary row.
        let without = monitor_choices(&attached, None);
        assert!(
            !without
                .iter()
                .any(|choice| choice.device.as_deref() == Some("\\\\.\\DISPLAY2")),
            "the disconnected pin was dropped from the list"
        );
    }

    /// The list names displays in a way a user can check, and offers exactly one
    /// way to be on the primary monitor.
    ///
    /// Two entries for the primary — a device and a "follow" — would be two
    /// spellings of one choice where one of them quietly stops working when
    /// Windows changes which display is primary.
    #[test]
    fn the_picker_names_each_display_and_offers_the_primary_once() {
        let desk = vec![
            primary_panel("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96),
            panel("\\\\.\\DISPLAY2", 1920, 0, 2560, 1440, 144),
        ];
        let choices = monitor_choices(&desk, None);

        assert_eq!(
            choices.len(),
            2,
            "expected one primary entry and one display"
        );
        assert_eq!(
            choices[0].device, None,
            "the first entry does not follow the primary"
        );
        assert!(
            choices[1].label.contains("2560 × 1440"),
            "the display is not named by its resolution: {}",
            choices[1].label
        );
        assert!(
            choices[1].label.contains("150%"),
            "the display is not named by its scaling: {}",
            choices[1].label
        );
        assert!(
            choices[1].label.contains("right of the primary"),
            "the display does not say where it sits: {}",
            choices[1].label
        );
        assert!(
            choices[1].label.contains("\\\\.\\DISPLAY2"),
            "the device name is missing from the label: {}",
            choices[1].label
        );
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.device.is_none())
                .count(),
            1,
            "the primary monitor is offered more than once"
        );
    }

    /// The theme is reloaded when the mode moves, and left alone when it does
    /// not.
    ///
    /// Two directions, because the two ways to be wrong look identical from
    /// outside: a reload that never happens leaves the window in the old theme,
    /// and a reload that always happens is a disk read every frame plus a theme
    /// that reverts under any hand edit.
    #[test]
    fn the_theme_is_reloaded_only_when_the_mode_moves() {
        let dir = std::env::temp_dir().join("plo-sync-theme");
        let _ = std::fs::remove_dir_all(&dir);
        let light = theme::builtin(Mode::Light);

        // Startup: no previous theme, so it must load.
        let (mode, first) = sync_theme(None, ThemeMode::Dark, &dir);
        assert_eq!(mode, Mode::Dark);
        assert_eq!(first.mode, Mode::Dark);

        // An explicit preference beats what Windows is doing.
        let (mode, _) = sync_theme(Some((mode, &first)), ThemeMode::Light, &dir);
        assert_eq!(mode, Mode::Light, "an explicit preference was ignored");
        let (mode, light_theme) = sync_theme(Some((Mode::Light, &light)), ThemeMode::Light, &dir);
        assert_eq!(mode, Mode::Light);

        // Unchanged: the very same theme comes back, not a re-read one. A
        // hand-edited theme must survive a pass that did not need to change.
        let (_, again) = sync_theme(Some((Mode::Light, &light_theme)), ThemeMode::Light, &dir);
        assert_eq!(
            again.colors, light_theme.colors,
            "an unchanged mode reloaded the theme from disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Choosing a theme has to change what is actually applied, and it has to
    /// survive the next pass.
    ///
    /// The second half is the one that matters. `sync_theme` runs every pass and
    /// resolves the mode from the **saved** preference, so a control that staged
    /// into `prefs_draft` alone would apply the pick and then have it undone a
    /// frame later — the "briefly right, then it changes back" shape this
    /// codebase has now produced twice.
    ///
    /// Driven through `choose_theme` and then through the real `sync_theme`, so
    /// a fix that only satisfies the first assertion cannot pass.
    #[test]
    fn choosing_a_theme_changes_it_and_survives_the_next_pass() {
        let dir = std::env::temp_dir().join("plo-choose-theme");
        let _ = std::fs::remove_dir_all(&dir);

        let mut prefs = config::GlobalPrefs::default();
        let mut draft = config::GlobalPrefs::default();
        // Explicit preferences, never `System`: `System` resolves from the
        // machine's own Windows setting, so a test that used it would pass on a
        // light desktop and fail on a dark one. That is a test about the machine.
        prefs.ui.theme = ThemeMode::Light;
        draft.ui.theme = ThemeMode::Light;
        let (start_mode, start_theme) = sync_theme(None, prefs.ui.theme, &dir);
        assert_eq!(
            start_mode,
            Mode::Light,
            "an explicit Light must resolve light"
        );

        choose_theme(&mut prefs, &mut draft, ThemeMode::Dark);
        // The draft is what Save writes, and the live one is what the next pass
        // reads. Assert both, because they are separate mistakes.
        assert_eq!(draft.ui.theme, ThemeMode::Dark, "the draft was not staged");
        assert_eq!(
            prefs.ui.theme,
            ThemeMode::Dark,
            "the live value was not set"
        );

        let (mode, applied) = sync_theme(Some((start_mode, &start_theme)), prefs.ui.theme, &dir);
        assert_eq!(mode, Mode::Dark, "the pick was undone by the next pass");
        assert_eq!(applied.colors, theme::builtin(Mode::Dark).colors);

        // And again, with the state a second pass would actually see.
        let (_, again) = sync_theme(Some((mode, &applied)), prefs.ui.theme, &dir);
        assert_eq!(again.colors, applied.colors, "the theme drifted");

        // The failure this guards, stated as the test rather than as a comment:
        // a picker that stages Dark but leaves the live value at Light, which is
        // the mode the pass then goes on using.
        let mut prefs_only = config::GlobalPrefs::default();
        prefs_only.ui.theme = ThemeMode::Light;
        let mut draft_only = config::GlobalPrefs::default();
        draft_only.ui.theme = ThemeMode::Dark;
        assert_eq!(draft_only.ui.theme, ThemeMode::Dark);
        assert_eq!(prefs_only.ui.theme, ThemeMode::Light);
        let (mode, _) = sync_theme(Some((start_mode, &start_theme)), prefs_only.ui.theme, &dir);
        assert_eq!(
            mode,
            Mode::Light,
            "staging without setting the live value should not change the mode, \
             which is exactly why choose_theme has to do both"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The selection-border toggle is written to the draft and to the live
    /// preference, for the same reason the theme pick is.
    ///
    /// `sync_border_preview` runs every pass and reads the live preference, so
    /// a checkbox that only staged into `prefs_draft` would leave the border
    /// animating — the setting would look dead until something else re-synced.
    #[test]
    fn the_selection_border_toggle_is_staged_and_live() {
        let mut prefs = config::GlobalPrefs::default();
        let mut draft = config::GlobalPrefs::default();
        assert!(
            prefs.ui.selection_border_animation,
            "the selection border animates unless it is turned off"
        );

        set_selection_border_animation(&mut prefs, &mut draft, false);
        assert!(
            !draft.ui.selection_border_animation,
            "the draft was not staged, so Save would write the old value"
        );
        assert!(
            !prefs.ui.selection_border_animation,
            "the live value was not set, so the next pass would keep animating the border"
        );

        // And the direction Enable has to work in.
        set_selection_border_animation(&mut prefs, &mut draft, true);
        assert!(draft.ui.selection_border_animation);
        assert!(prefs.ui.selection_border_animation);
    }

    /// The background-tracking toggle is written to the draft and to the live
    /// preference, for the same reason the selection-border toggle is.
    ///
    /// `sync_runtime_config` reads the live preference every pass and sends the
    /// difference to the renderer, so a draft-only write left kept probes
    /// running after the box was unticked — the setting only worked on Save,
    /// which is exactly the shape this class of bug always has.
    #[test]
    fn the_background_tracking_toggle_is_staged_and_live() {
        let mut prefs = config::GlobalPrefs::default();
        let mut draft = config::GlobalPrefs::default();
        assert!(
            prefs.ui.background_tracking,
            "background tracking is on unless it is turned off"
        );

        set_background_tracking(&mut prefs, &mut draft, false);
        assert!(
            !draft.ui.background_tracking,
            "the draft was not staged, so Save would write the old value"
        );
        assert!(
            !prefs.ui.background_tracking,
            "the live value was not set, so the renderer would keep the probes running"
        );

        // And the direction Enable has to work in.
        set_background_tracking(&mut prefs, &mut draft, true);
        assert!(draft.ui.background_tracking);
        assert!(prefs.ui.background_tracking);
    }

    /// The labels are the short ones the tiles use, and the tooltips carry what
    /// the labels cannot: what System follows and resolved to, and the tray-menu
    /// caveat for an override.
    ///
    /// "System Theme" on its own does not say whether the feature is working,
    /// so the hint carries the answer.
    #[test]
    fn the_theme_choice_labels_and_hints_say_what_the_tiles_cannot() {
        // All three, in a fixed order, and nothing longer than the reference
        // tiles' own wording — the labels have to fit inside a square.
        assert_eq!(
            theme_choices(),
            [
                (ThemeMode::System, "System Theme"),
                (ThemeMode::Light, "Light Theme"),
                (ThemeMode::Dark, "Dark Theme"),
            ]
        );

        for (system, expected) in [
            (Mode::Light, "currently light"),
            (Mode::Dark, "currently dark"),
        ] {
            let hint = theme_choice_hint(system, ThemeMode::System);
            assert!(hint.contains("Follows Windows"), "{hint:?}");
            assert!(
                hint.contains(expected),
                "{hint:?} does not say {expected:?}"
            );
        }

        for choice in [ThemeMode::Light, ThemeMode::Dark] {
            let hint = theme_choice_hint(Mode::Dark, choice);
            assert!(
                hint.contains("tray's menu always follows Windows"),
                "{hint:?} does not warn about the tray menu"
            );
        }
    }

    /// Leaving the Profiles page and coming back is arriving again.
    ///
    /// `last_page` has to advance even on the frames that read nothing, or the
    /// page would only ever be refreshed the first time and the counts would go
    /// stale for the rest of the session after a create or a delete.
    #[test]
    fn coming_back_to_the_profiles_page_reads_again() {
        let mut last_page = Page::Profiles;
        let mut profiles = Vec::new();
        let mut counts = HashMap::new();
        let snapshot = || ProfileSnapshot {
            profiles: Vec::new(),
            counts: HashMap::new(),
        };

        // Sit on the Profiles page for a few frames, which must not read.
        for _ in 0..3 {
            assert!(!sync_profile_cache(
                &mut last_page,
                Page::Profiles,
                &mut profiles,
                &mut counts,
                snapshot,
            ));
        }
        // Leave and come back, which must read.
        assert!(!sync_profile_cache(
            &mut last_page,
            Page::Overlays,
            &mut profiles,
            &mut counts,
            snapshot,
        ));
        assert!(sync_profile_cache(
            &mut last_page,
            Page::Profiles,
            &mut profiles,
            &mut counts,
            snapshot,
        ));
    }

    /// A count that has not been read says nothing, and one that has been read
    /// as zero says zero.
    ///
    /// Those are different facts. The bug this came from rendered the first as
    /// the second, which is why every profile read `0 overlays` and the real
    /// counts only appeared once the switcher had been opened by hand.
    #[test]
    fn an_unknown_overlay_count_says_nothing() {
        assert_eq!(overlay_count_label(None), "");
        assert_eq!(overlay_count_label(Some(0)), "0 overlays");
        assert_eq!(overlay_count_label(Some(1)), "1 overlay");
        assert_eq!(overlay_count_label(Some(7)), "7 overlays");
    }

    /// The three pane rects `config_ui` lays out, mirroring its pane sequence.
    ///
    /// This is a mirror and not a call: `config_ui` is a method on `PingApp`, and
    /// standing one up needs a creation context, live probes and a tray, which is
    /// not worth it for a layout test. It uses the same `draw_pane_divider`, the
    /// same `rail_width`, the same `SIDEBAR_WIDTH`, the same frame margin and the
    /// same `list_pane_column`, and its list pane draws itself as a positioned
    /// child exactly as the real pages do.
    fn pane_rects(claim_pane_width: bool) -> (egui::Rect, egui::Rect, egui::Rect, egui::Rect) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let mut panes = (
            egui::Rect::ZERO,
            egui::Rect::ZERO,
            egui::Rect::ZERO,
            egui::Rect::ZERO,
        );
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(input, |ui| {
            let height = 600.0;
            Frame::central_panel(ui.style())
                .fill(UI_BACKGROUND())
                .inner_margin(egui::Margin {
                    left: PANE_MARGIN as i8,
                    right: PANE_MARGIN as i8,
                    top: PANE_MARGIN as i8,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_height(height);
                        ui.allocate_ui(egui::vec2(rail_width(false), height), |ui| {
                            panes.0 = ui.max_rect();
                            ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), RAIL_ROW_HEIGHT),
                                egui::Sense::hover(),
                            );
                        });
                        draw_pane_divider(ui);
                        ui.allocate_ui(egui::vec2(SIDEBAR_WIDTH, height), |ui| {
                            panes.1 = ui.max_rect();
                            let column = list_pane_column(ui.available_width());
                            panes.2 = column.rect(ui, height);
                            let mut page = ui.new_child(egui::UiBuilder::new().max_rect(panes.2));
                            page.allocate_exact_size(
                                egui::vec2(column.width, list_pane_row_height(OVERLAY_ROW_HEIGHT)),
                                egui::Sense::hover(),
                            );
                            if claim_pane_width {
                                ui.advance_cursor_after_rect(ui.max_rect());
                            }
                        });
                        draw_pane_divider(ui);
                        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                            panes.3 = ui.max_rect();
                            ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), height),
                                egui::Sense::hover(),
                            );
                        });
                    });
                });
        });
        // `FullOutput` owns a `TexturesDelta` whose `Drop` panics if the deltas
        // were never applied, and there is no headless pass to apply them in.
        output.textures_delta.clear();
        panes
    }

    /// A pane that draws itself as a positioned child must have the pane claim its
    /// own width, because `Ui::new_child` reports nothing to the layout that
    /// allocated it.
    ///
    /// This shipped as a full-on overlap of the detail pane on top of the list
    /// pane. `allocate_ui` advances the cursor, then `scope_dyn` sets it again
    /// from the child's `min_rect`, which a page that painted itself in a
    /// grandchild never grows, so the cursor went back to the pane's left edge.
    /// Both halves are pinned here: the panes must come out disjoint, and
    /// dropping the claim has to actually break them, or the test proves nothing.
    #[test]
    fn a_pane_that_draws_itself_keeps_its_place() {
        let (rail, list, column, detail) = pane_rects(true);
        assert!(
            rail.right() <= list.left(),
            "the rail at {rail:?} and the list pane at {list:?} overlap"
        );
        assert!(
            list.right() <= detail.left(),
            "the list pane at {list:?} and the detail pane at {detail:?} overlap"
        );

        // The spacing that has to look even is content to content across each
        // boundary, which is the rail's rows to the column and the column to the
        // detail pane's content. Measuring hairline to nearest content instead
        // reads uneven (16px on one side, 24px on the other) purely because the
        // rail and the detail pane lay out flush to their pane edges while the
        // column is inset, so that metric is the wrong one to design against.
        let left = column.left() - rail.right();
        let right = detail.left() - column.right();
        assert!(
            (left - right).abs() < f32::EPSILON,
            "the list pane is spaced {left}px from the rail and {right}px from the detail pane"
        );

        let (_, list, _, detail) = pane_rects(false);
        assert!(
            list.intersects(detail),
            "an unclaimed list pane at {list:?} was expected to be overrun by the detail pane at {detail:?}"
        );
    }

    /// Save and Discard follow *every* draft.
    ///
    /// The profile draft, the preferences draft and the rules draft are
    /// separate flags and pages stage into one of them, so gating the footer
    /// on the profile flag alone left every Global preference unsaveable. The
    /// rail-collapse preference has been in that state since it shipped, and
    /// the rules section would have repeated it.
    #[test]
    fn the_footer_follows_every_draft() {
        let cases = [
            (false, false, false, false),
            (true, false, false, true),
            (false, true, false, true),
            (false, false, true, true),
            (true, true, true, true),
        ];
        for (dirty, prefs_dirty, rules_dirty, expected) in cases {
            assert_eq!(
                pending_edits(dirty, prefs_dirty, rules_dirty),
                expected,
                "with the profile draft {dirty}, the preferences draft {prefs_dirty} and the \
                 rules draft {rules_dirty}, Save and Discard should be {}",
                if expected { "enabled" } else { "disabled" }
            );
        }
    }

    /// A status message must not start with a space.
    ///
    /// The startup path joins several independent messages, and an empty
    /// status plus one message is the ordinary case for a clean install.
    /// A leading space reads as a missing word at the front of the status bar.
    #[test]
    fn a_status_message_does_not_start_with_a_space() {
        assert_eq!(append_status_message(String::new(), Some("hello")), "hello");
        assert_eq!(
            append_status_message("one".to_string(), Some("two")),
            "one two"
        );
        assert_eq!(append_status_message("one".to_string(), None), "one");
    }

    /// The preview is the engine's own decision, named.
    ///
    /// It runs `rules::decide` — the same pure function the tray's engine
    /// calls — so the preview cannot describe behaviour the engine does not
    /// have. The cases are the ones a user needs told apart: a rule matched,
    /// nothing matched and the fallback applies, and nothing can match at all.
    #[test]
    fn the_preview_names_what_the_engine_would_do() {
        let profiles = vec![
            ProfileEntry {
                id: "default".to_string(),
                name: "Default".to_string(),
            },
            ProfileEntry {
                id: "gaming".to_string(),
                name: "Gaming".to_string(),
            },
        ];
        let mut file = AutoRules {
            enabled: true,
            fallback_profile: Some("default".to_string()),
            rules: vec![Rule {
                name: "CS2".to_string(),
                scope: Scope::AnyWindow,
                combine: Combine::All,
                when: vec![Condition {
                    part: Part::ProcessName,
                    matcher: MatchMode::Exact,
                    value: "cs2.exe".to_string(),
                }],
                profile: "gaming".to_string(),
            }],
        };
        let matched = Snapshot {
            windows: vec![WindowInfo {
                process: "cs2.exe".to_string(),
                title: "Counter-Strike 2".to_string(),
                class_name: "SDL_app".to_string(),
            }],
            foreground: None,
        };
        let empty = Snapshot::default();

        assert_eq!(
            auto_preview_line(&file, &matched, &profiles),
            "Matches rule 1 \"CS2\" -> Gaming"
        );
        assert_eq!(
            auto_preview_line(&file, &empty, &profiles),
            "Nothing matches -> Default"
        );

        // A rule pointing at a deleted profile must say so rather than draw a
        // name that no longer exists.
        file.rules[0].profile = "gone".to_string();
        assert_eq!(
            auto_preview_line(&file, &matched, &profiles),
            "Matches rule 1 \"CS2\" -> gone (no such profile)"
        );

        // Enabled with no usable rule is inert, not "always fall back".
        file.rules[0].when.clear();
        assert_eq!(
            auto_preview_line(&file, &empty, &profiles),
            "No rule can match while its conditions are incomplete."
        );
    }

    /// The preview re-reads the desktop on a clock, and not when switching is
    /// off.
    ///
    /// The enumeration is the cost, so this is the half worth pinning: a due
    /// check that never fires leaves a stale preview, and one that always
    /// fires enumerates every frame.
    #[test]
    fn the_preview_only_reads_the_desktop_when_it_is_due() {
        let profiles: Vec<ProfileEntry> = Vec::new();
        let enabled = AutoRules {
            enabled: true,
            fallback_profile: None,
            rules: Vec::new(),
        };
        let mut text = None;
        let mut read_at = None;
        let start = Instant::now();
        let reads = std::cell::Cell::new(0);
        // A fresh closure per call: the count is shared through the `Cell`,
        // and `sync_auto_preview_text` takes its read by value.
        let count_read = || {
            reads.set(reads.get() + 1);
            Snapshot::default()
        };

        sync_auto_preview_text(
            &mut text,
            &mut read_at,
            start,
            &enabled,
            &profiles,
            count_read,
        );
        assert_eq!(reads.get(), 1, "the first pass should read");
        sync_auto_preview_text(
            &mut text,
            &mut read_at,
            start + Duration::from_millis(500),
            &enabled,
            &profiles,
            count_read,
        );
        assert_eq!(
            reads.get(),
            1,
            "a pass inside the interval re-read the desktop"
        );
        sync_auto_preview_text(
            &mut text,
            &mut read_at,
            start + Duration::from_secs(2),
            &enabled,
            &profiles,
            count_read,
        );
        assert_eq!(reads.get(), 2, "the interval elapsed and nothing re-read");

        // Off says nothing and costs nothing. The clock moves forward, as a
        // real one does: a due check against a time that went backwards
        // saturates rather than firing, and that is not the case under test.
        let off = AutoRules::default();
        sync_auto_preview_text(
            &mut text,
            &mut read_at,
            start + Duration::from_secs(4),
            &off,
            &profiles,
            count_read,
        );
        assert_eq!(reads.get(), 2, "switching off still enumerated the desktop");
        assert_eq!(text, None);
    }

    /// The window's engine debounces like the tray's, holds while the drafts
    /// are dirty, and lands by itself once they are resolved.
    ///
    /// The hold is the point: it must not be a dropped switch. The engine is
    /// simply not told it applied, so the same settled decision comes back on
    /// every tick until the caller can take it.
    #[test]
    fn the_window_engine_holds_a_switch_until_the_drafts_are_resolved() {
        let file = AutoRules {
            enabled: true,
            fallback_profile: None,
            rules: vec![Rule {
                name: "CS2".to_string(),
                scope: Scope::AnyWindow,
                combine: Combine::All,
                when: vec![Condition {
                    part: Part::ProcessName,
                    matcher: MatchMode::Exact,
                    value: "cs2.exe".to_string(),
                }],
                profile: "gaming".to_string(),
            }],
        };
        let desktop = || Snapshot {
            windows: vec![WindowInfo {
                process: "cs2.exe".to_string(),
                title: "Counter-Strike 2".to_string(),
                class_name: "SDL_app".to_string(),
            }],
            foreground: None,
        };
        let start = Instant::now();

        let mut engine = Engine::default();
        let mut read_at = None;
        assert_eq!(
            auto_switch_step(&mut engine, &mut read_at, start, &file, false, desktop),
            AutoSwitchStep::Idle,
            "one evaluation only sets the candidate"
        );
        let settled = start + Duration::from_secs(1);
        assert_eq!(
            auto_switch_step(&mut engine, &mut read_at, settled, &file, false, desktop),
            AutoSwitchStep::Apply("gaming".to_string()),
            "the second consecutive evaluation settles the decision"
        );
        engine.mark_applied("gaming");
        assert_eq!(
            auto_switch_step(
                &mut engine,
                &mut read_at,
                settled + Duration::from_secs(1),
                &file,
                false,
                desktop
            ),
            AutoSwitchStep::Idle,
            "an applied decision is not applied again"
        );

        // Dirty drafts: held rather than applied, and still held next tick.
        let mut held = Engine::default();
        let mut held_at = None;
        assert_eq!(
            auto_switch_step(&mut held, &mut held_at, start, &file, true, desktop),
            AutoSwitchStep::Idle
        );
        let held_tick = start + Duration::from_secs(1);
        assert_eq!(
            auto_switch_step(&mut held, &mut held_at, held_tick, &file, true, desktop),
            AutoSwitchStep::Held("gaming".to_string())
        );
        assert_eq!(
            auto_switch_step(
                &mut held,
                &mut held_at,
                held_tick + Duration::from_secs(1),
                &file,
                true,
                desktop
            ),
            AutoSwitchStep::Held("gaming".to_string()),
            "a held switch must keep coming back rather than be lost"
        );
        // Saving or discarding releases it on the next tick.
        let free_tick = held_tick + Duration::from_secs(2);
        assert_eq!(
            auto_switch_step(&mut held, &mut held_at, free_tick, &file, false, desktop),
            AutoSwitchStep::Apply("gaming".to_string())
        );
        held.mark_applied("gaming");
        assert_eq!(
            auto_switch_step(
                &mut held,
                &mut held_at,
                free_tick + Duration::from_secs(1),
                &file,
                false,
                desktop
            ),
            AutoSwitchStep::Idle
        );

        // Switching off decides nothing and never enumerates the desktop.
        let off = AutoRules::default();
        let mut off_engine = Engine::default();
        let mut off_at = None;
        assert_eq!(
            auto_switch_step(
                &mut off_engine,
                &mut off_at,
                start,
                &off,
                false,
                || unreachable!("the desktop must not be read with switching off")
            ),
            AutoSwitchStep::Idle
        );
    }

    /// The fallback a new rule set starts with is the `default` profile when
    /// it exists, because that is the one every install has.
    #[test]
    fn a_new_rule_set_falls_back_to_default_when_it_exists() {
        let with_default = vec![
            ProfileEntry {
                id: "gaming".to_string(),
                name: "Gaming".to_string(),
            },
            ProfileEntry {
                id: "default".to_string(),
                name: "Default".to_string(),
            },
        ];
        assert_eq!(default_fallback(&with_default, "gaming"), "default");

        let without_default = vec![ProfileEntry {
            id: "work".to_string(),
            name: "Work".to_string(),
        }];
        assert_eq!(default_fallback(&without_default, "work"), "work");
        assert_eq!(default_fallback(&[], "work"), "work");
    }

    /// Drawing the editor must not stage a draft by itself.
    ///
    /// Every widget in the section can report a change, and the caller turns
    /// that into `rules_dirty`, which enables Save. A widget that fires on its
    /// own — a combo that reports a change for merely being drawn, an id
    /// collision between two rows — would make every visit to the Global page
    /// stage a `rules.json` write. This runs the whole section headlessly with
    /// a representative file, including a rule that cannot match, and holds
    /// both halves: the drawing itself must not report a change.
    #[test]
    fn drawing_the_rules_editor_does_not_stage_a_draft() {
        let profiles = vec![
            ProfileEntry {
                id: "default".to_string(),
                name: "Default".to_string(),
            },
            ProfileEntry {
                id: "gaming".to_string(),
                name: "Gaming".to_string(),
            },
        ];
        let mut draft = AutoRules {
            enabled: true,
            fallback_profile: Some("default".to_string()),
            rules: vec![
                Rule {
                    name: "CS2".to_string(),
                    scope: Scope::AnyWindow,
                    combine: Combine::All,
                    when: vec![
                        Condition {
                            part: Part::ProcessName,
                            matcher: MatchMode::Exact,
                            value: "cs2.exe".to_string(),
                        },
                        Condition {
                            part: Part::Title,
                            matcher: MatchMode::Regex,
                            value: "(?i)counter".to_string(),
                        },
                    ],
                    profile: "gaming".to_string(),
                },
                // A second rule, so the loop's ids are exercised more than
                // once, carrying the incomplete condition the editor is
                // expected to report rather than hide.
                Rule {
                    name: String::new(),
                    scope: Scope::Foreground,
                    combine: Combine::Any,
                    when: vec![Condition::default()],
                    profile: "gone".to_string(),
                },
            ],
        };

        let ctx = egui::Context::default();
        let mut staged = true;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            staged = auto_switch_section(ui, &mut draft, &profiles, "default");
        });
        output.textures_delta.clear();

        assert!(
            !staged,
            "drawing the rules editor staged a draft without being touched"
        );
    }

    /// The editor's per-rule warning comes from the engine's own compiler.
    #[test]
    fn a_rule_that_cannot_match_says_why() {
        let valid = Rule {
            name: "CS2".to_string(),
            scope: Scope::AnyWindow,
            combine: Combine::All,
            when: vec![Condition {
                part: Part::ProcessName,
                matcher: MatchMode::Exact,
                value: "cs2.exe".to_string(),
            }],
            profile: "gaming".to_string(),
        };
        assert_eq!(rule_error(&valid), None);

        let mut unfinished = valid.clone();
        unfinished.when = vec![Condition::default()];
        assert_eq!(
            rule_error(&unfinished).as_deref(),
            Some("condition 1 has an empty value")
        );

        let mut bad_regex = valid;
        bad_regex.when[0].matcher = MatchMode::Regex;
        bad_regex.when[0].value = "(".to_string();
        assert!(rule_error(&bad_regex)
            .as_deref()
            .is_some_and(|error| error.contains("not a valid regular expression")));
    }

    /// No line on the About page may be smaller than the rest of the window's
    /// body text.
    ///
    /// The page shipped one line at `.small()` and was criticised for it, which
    /// is a preference everyone shares and only one person remembers. This makes
    /// it a rule: any line below the body size fails, so the floor cannot be
    /// quietly lowered to make a new line fit.
    #[test]
    fn no_about_page_line_is_tiny() {
        let floor = ui_text_size();
        for line in about_page_lines() {
            assert!(
                line.size >= floor,
                "the About page's {:?} is {}px, below the {floor}px body size",
                line.text,
                line.size
            );
        }
    }

    /// The repository is the only thing on the About page that opens anything.
    ///
    /// The display text drops the `https://` prefix, so a typo in a URL would
    /// not be visible on the page at all; only the click would fail, and then in
    /// the user's browser rather than in this app. Checking the constants here
    /// catches that. The licence is named in words and deliberately not linked,
    /// so this also fails if a line quietly becomes or stops being clickable.
    #[test]
    fn only_the_repository_is_a_link() {
        let lines = about_page_lines();
        let linked: Vec<&str> = lines
            .iter()
            .filter_map(|line| match line.kind {
                AboutKind::Link(url) => Some(url),
                AboutKind::Text => None,
            })
            .collect();
        assert_eq!(
            linked,
            vec![ABOUT_REPOSITORY],
            "the About page's links changed, so check the text says what each one opens"
        );
        for url in linked {
            assert!(
                url.starts_with("https://"),
                "{url} is shown on the About page and would not open"
            );
        }
    }

    /// A click on a link reaches the opener.
    ///
    /// `requested_url` is the whole reason the About page's link works. egui
    /// emits `OutputCommand::OpenUrl` and expects the host to act on it, and
    /// eframe's **native** runner has no `OutputCommand` handling at all — only
    /// its web runner does — so without this mapping the link is inert. That
    /// shipped once: the page claimed its links opened a browser, and they did
    /// nothing at all.
    #[test]
    fn a_link_click_becomes_a_url_to_open() {
        let open = egui::OpenUrl::same_tab(ABOUT_REPOSITORY);
        assert_eq!(
            requested_url(&egui::OutputCommand::OpenUrl(open)),
            Some(ABOUT_REPOSITORY),
            "a clicked link must reach the opener as a URL"
        );
        // The other command egui can emit is a clipboard copy, and it must be
        // left in place: draining the list must not swallow the app's other
        // output. `OutputCommand` has exactly three variants, so covering
        // `CopyText` and `CopyImage` covers everything that is not a URL.
        assert_eq!(
            requested_url(&egui::OutputCommand::CopyText("copied".to_string())),
            None,
            "a command that is not about a URL must be left in place"
        );
        assert_eq!(
            requested_url(&egui::OutputCommand::CopyImage(egui::ColorImage::new(
                [1, 1],
                vec![egui::Color32::TRANSPARENT],
            ))),
            None,
            "a command that is not about a URL must be left in place"
        );
    }

    /// The list pane's content is one column, centred in its pane.
    ///
    /// The rows used to be allocated flush against the pane's left edge while
    /// `LIST_PANE_INSET` was only ever subtracted from their width, so the whole
    /// of the slack landed on the right: the left boundary read 4px and the
    /// right one 40px, which is what made the three panes look unevenly spaced.
    #[test]
    fn the_list_pane_column_is_centred_in_its_pane() {
        for allocated in [SIDEBAR_WIDTH, 200.0, 140.0] {
            let column = list_pane_column(allocated);
            let left = column.inset;
            let right = allocated - column.inset - column.width;
            assert!(
                (left - right).abs() < f32::EPSILON,
                "a {allocated}px pane left {left}px beside its column and {right}px on the other side"
            );
            assert!(
                column.inset >= 0.0,
                "a {allocated}px pane put its column {left}px outside itself"
            );
            assert!(
                column.inset + column.scroll_width() <= allocated,
                "the {}px scroll area in a {allocated}px pane runs {}px past the pane's edge",
                column.scroll_width(),
                column.inset + column.scroll_width() - allocated
            );
        }
    }

    /// The name, the `item_spacing` gap and the two action buttons all come out
    /// of an overlay row's contents, and the controls are pinned to the right
    /// edge, so the two sides have to add up to the row's contents exactly.
    #[test]
    fn an_overlay_row_fits_its_pane() {
        let row_width = list_pane_row_width_for(SIDEBAR_WIDTH);
        let inner_width = row_width - ROW_MARGIN * 2.0;
        let gap = 8.0;
        let used = overlay_name_width(inner_width, gap) + gap + super::overlay_controls_width(gap);
        assert!(
            used <= inner_width,
            "an overlay row needs {used}px of contents but the pane offers {inner_width}px"
        );
        assert!(
            used + ROW_MARGIN * 2.0 <= row_width,
            "an overlay row needs {used}px but the pane offers {row_width}px"
        );
    }

    /// The name, the `item_spacing` gap and the count/dot gutter all come out of
    /// a profile row's contents, so forgetting one of them overflows the row.
    #[test]
    fn a_profile_row_fits_its_pane() {
        let row_width = list_pane_row_width_for(SIDEBAR_WIDTH);
        let inner_width = row_width - ROW_MARGIN * 2.0;
        let gap = 8.0;
        let used = profile_name_width(inner_width, gap) + gap + PROFILE_ROW_TRAILING;
        assert!(
            used <= inner_width,
            "a profile row needs {used}px of contents but the pane offers {inner_width}px"
        );
        assert!(
            used + ROW_MARGIN * 2.0 <= row_width,
            "a profile row needs {used}px but the pane offers {row_width}px"
        );
    }

    /// The Global glyph shipped low and asymmetric, and one knob poked past
    /// the end of its own track, so the geometry is pinned here.
    #[test]
    fn the_global_glyph_is_centred_and_stays_inside_its_box() {
        let total: f32 = GLOBAL_ICON_ROWS.iter().map(|(row, _)| row).sum();
        assert!(
            total.abs() < f32::EPSILON,
            "vertical offsets {GLOBAL_ICON_ROWS:?} do not sum to zero, so the glyph is not centred"
        );

        let lowest: f32 = GLOBAL_ICON_ROWS
            .iter()
            .map(|(row, _)| row.abs())
            .fold(0.0, f32::max);
        assert!(
            lowest + GLOBAL_ICON_KNOB_RADIUS <= 0.5,
            "the glyph reaches {lowest} of the half box, past the knob radius"
        );

        let widest: f32 = GLOBAL_ICON_ROWS
            .iter()
            .map(|(_, knob)| knob.abs())
            .fold(0.0, f32::max);
        assert!(
            widest * GLOBAL_ICON_TRACK_HALF + GLOBAL_ICON_KNOB_RADIUS <= GLOBAL_ICON_TRACK_HALF,
            "a knob reaches past the end of its track"
        );
    }

    /// The theme tiles' glyphs reach no further than half the icon box, and the
    /// sun's rays balance around the centre.
    ///
    /// The same pair of invariants the rail's glyphs are held to, and for the
    /// same reason: a glyph that pokes out of its box reads as a rendering bug
    /// long before anyone can say which constant is wrong.
    #[test]
    fn the_theme_glyphs_stay_inside_their_boxes() {
        // The sun: the rays come from a table, so the reach and the balance are
        // summed from the same numbers the painter reads.
        let (mut dx, mut dy) = (0.0_f32, 0.0_f32);
        let mut reach = THEME_SUN_CORE_RADIUS;
        for (ray_x, ray_y) in THEME_SUN_RAYS {
            dx += ray_x;
            dy += ray_y;
            let length = (ray_x * ray_x + ray_y * ray_y).sqrt();
            reach = reach.max(length * THEME_SUN_RAY_OUTER);
        }
        assert!(
            dx.abs() < 1e-3,
            "the sun's rays do not balance horizontally: {dx}"
        );
        assert!(
            dy.abs() < 1e-3,
            "the sun's rays do not balance vertically: {dy}"
        );
        let sun = reach + THEME_ICON_STROKE / 2.0;
        assert!(sun <= 0.5, "the sun reaches {sun} of its box");

        // The moon: the disc is what shows, and the bite has to stay in the box
        // too, or it would paint the tile's fill outside the icon.
        let (bite_x, bite_y) = THEME_MOON_BITE_OFFSET;
        let bite = bite_x.abs().max(bite_y.abs()) + THEME_MOON_BITE_RADIUS;
        let moon = THEME_MOON_RADIUS.max(bite);
        assert!(moon <= 0.5, "the moon reaches {moon} of its box");

        // The System disc, outline included.
        let system = THEME_SYSTEM_RADIUS + THEME_ICON_STROKE / 2.0;
        assert!(system <= 0.5, "the system disc reaches {system} of its box");
    }

    /// The three theme tiles and their gaps fit the detail pane at both ends of
    /// the window's width range, and their labels are measured to fit inside
    /// them.
    ///
    /// Measured rather than deduced: the tiles are laid out for real in a
    /// headless context — the same `theme_tiles` the window calls — and the
    /// label widths come from the font atlas at the size the tiles actually use.
    #[test]
    fn the_theme_tiles_fit_their_pane() {
        let ctx = egui::Context::default();
        let window_widths = [WINDOW_MIN_WIDTH, WINDOW_WIDTH];
        let mut measured: Vec<(f32, egui::Vec2)> = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (index, window_width) in window_widths.iter().enumerate() {
                let available = window_width
                    - PANE_MARGIN * 2.0
                    - RAIL_WIDTH
                    - PANE_GAP * 2.0
                    - SIDEBAR_WIDTH
                    - SCROLL_BAR_RESERVE;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(0.0, index as f32 * 160.0),
                    egui::vec2(available, 150.0),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                theme_tiles(&mut child, ThemeMode::System, Mode::Dark);
                measured.push((available, child.min_rect().size()));
            }
        });
        output.textures_delta.clear();

        for (available, extent) in &measured {
            let side = theme_tile_side(*available);
            let expected = side * 3.0 + THEME_TILE_GAP * 2.0;
            assert!(
                (extent.x - expected).abs() < 1.0,
                "the row laid out {}px wide but three {side}px tiles and two gaps are {expected}px",
                extent.x
            );
            assert!(
                extent.x <= available + 0.5,
                "the row overflows the pane: {}px of tiles in {available}px",
                extent.x
            );
            assert!(
                extent.y >= side - 0.5,
                "a {side}px tile only reported {}px of height",
                extent.y
            );
        }

        // The labels, measured at the size the tiles draw them, have to keep
        // clear of the tile's rounded corners.
        for (available, _) in &measured {
            let side = theme_tile_side(*available);
            let font = egui::FontId::proportional(theme_tile_label_size(side));
            for (_, label) in theme_choices() {
                let width = ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                });
                assert!(
                    width <= side - 12.0,
                    "{label:?} is {width}px wide inside a {side}px tile, which leaves no margin"
                );
            }
        }
    }

    /// Only a page that stages a draft carries the Save/Discard footer. The
    /// Profiles page acts on files straight away, so it has nothing to save.
    #[test]
    fn only_the_overlays_and_global_pages_carry_the_detail_footer() {
        assert!(page_has_detail_footer(Page::Overlays));
        assert!(page_has_detail_footer(Page::Global));
        assert!(!page_has_detail_footer(Page::Profiles));
    }

    #[test]
    fn the_rail_offers_every_page_once() {
        let labels: Vec<&str> = PAGES.iter().map(|page| page.label()).collect();
        assert_eq!(labels, vec!["Overlays", "Profiles", "Global", "About"]);
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());
    }

    /// Overlays and Profiles list something; Global and About are a single wide
    /// pane. This is a function rather than a `page != Global` test at the layout
    /// call site so a fifth page is one line here instead of an edit buried in
    /// `config_ui` that nothing would point at.
    #[test]
    fn only_the_pages_with_a_list_get_one() {
        let cases = [
            (Page::Overlays, true),
            (Page::Profiles, true),
            (Page::Global, false),
            (Page::About, false),
        ];
        for (page, expected) in cases {
            assert_eq!(
                page_has_list_pane(page),
                expected,
                "{} {}",
                page.label(),
                if expected { "lists" } else { "has no list" }
            );
        }
    }

    /// The About glyph is a lower case `i`: a dot over a short stem. Its two
    /// vertical offsets deliberately do NOT sum to zero, because the dot and the
    /// stem are different shapes, so the check is against the drawn extent rather
    /// than the offsets. The Global glyph shipped with its offsets read as loop
    /// indices and one knob poking past its track, so this is worth pinning from
    /// the first release rather than after a screenshot.
    #[test]
    fn the_about_glyph_stays_inside_its_box() {
        let dot = ABOUT_ICON_ROWS
            .iter()
            .find(|(_, half)| *half <= 0.0)
            .map(|(row, _)| *row)
            .expect("the glyph needs a dot");
        let stem = ABOUT_ICON_ROWS
            .iter()
            .find(|(_, half)| *half > 0.0)
            .map(|(row, _)| *row)
            .expect("the glyph needs a stem");
        assert!(
            dot.abs() + ABOUT_ICON_DOT_RADIUS <= 0.5,
            "the dot at {dot} of the half box plus its radius leaves the box"
        );
        assert!(
            stem.abs() <= 0.5,
            "the stem at {stem} of the half box leaves the box"
        );
        assert!(
            dot < stem,
            "the dot at {dot} must sit above the stem at {stem}, or it is not an i"
        );
    }

    #[test]
    fn a_shared_profile_name_shows_its_id() {
        let profiles = vec![
            ProfileEntry {
                id: "home".to_string(),
                name: "Home".to_string(),
            },
            ProfileEntry {
                id: "home_2".to_string(),
                name: "Home".to_string(),
            },
            ProfileEntry {
                id: "office".to_string(),
                name: "Office".to_string(),
            },
        ];
        assert_eq!(profile_row_label(&profiles[0], &profiles), "Home (home)");
        assert_eq!(
            profile_row_label(&profiles[1], &profiles),
            "Home (home_2)",
            "the id is the only part that differs"
        );
        assert_eq!(
            profile_row_label(&profiles[2], &profiles),
            "Office",
            "a unique name needs no id"
        );
    }

    #[test]
    fn the_rail_narrows_to_icons_only() {
        assert_eq!(rail_width(false), RAIL_WIDTH);
        assert_eq!(rail_width(true), 44.0);
        assert!(rail_width(true) < rail_width(false));
    }

    /// The strip's two runtime facts, measured through a real scroll area rather
    /// than assumed: the inner `Ui`'s `max_rect` is the viewport, so the strip can
    /// be sized from it, and `ui.interact` leaves the cursor where it found it, so
    /// the strip cannot grow the content. The second is the one that matters —
    /// had the strip been a real widget, a list that exactly filled the viewport
    /// would have pushed the content one widget past it and conjured a scrollbar.
    #[test]
    fn the_deselect_strip_does_not_disturb_the_scroll_content() {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut measured = (egui::Rect::ZERO, 0.0, 0.0);
        let mut output = ctx.run_ui(input, |ui| {
            let column =
                egui::Rect::from_min_max(egui::pos2(188.0, 12.0), egui::pos2(434.0, 212.0));
            let row_width = column.width();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for _ in 0..2 {
                        ui.allocate_exact_size(
                            egui::vec2(row_width, list_pane_row_height(OVERLAY_ROW_HEIGHT)),
                            egui::Sense::click(),
                        );
                    }
                    let viewport = ui.max_rect();
                    let rows_bottom = ui.cursor().min.y;
                    let strip = deselect_strip_rect(viewport, rows_bottom, row_width)
                        .expect("two rows leave room in a 200px viewport");
                    let cursor_before = ui.cursor().min.y;
                    let response =
                        ui.interact(strip, ui.id().with("deselect_strip"), egui::Sense::click());
                    measured = (viewport, cursor_before, response.rect.height());
                    assert_eq!(
                        ui.cursor().min.y,
                        cursor_before,
                        "registering the deselect strip moved the scroll content"
                    );
                });
        });
        output.textures_delta.clear();
        let (viewport, rows_bottom, strip_height) = measured;
        assert!(
            viewport.height() > rows_bottom,
            "the fixture is wrong: the rows filled the {viewport:?} viewport"
        );
        assert!(
            (strip_height - (viewport.bottom() - rows_bottom)).abs() < f32::EPSILON,
            "the strip was {strip_height}px tall but the leftover space is {}px",
            viewport.bottom() - rows_bottom
        );
    }

    /// Lays the detail pane out the way `show_detail_pane` does and reports the
    /// box it gave the content, the rect the footer ended up in, and the band the
    /// footer is supposed to live in.
    ///
    /// A mirror rather than a call: `show_detail_pane` is a method on `PingApp`,
    /// and standing one up needs a creation context, probes and a tray. The one
    /// thing that has to be reproduced faithfully is the *height bound*: in the
    /// real window the pane sits inside `ui.vertical` with its height set, so the
    /// space left over after the content is exactly the footer band. Without that
    /// bound `with_layout(Align::Center)` centres the button in the whole rest of
    /// the window and every number comes out wrong, which is how this helper
    /// briefly blamed the code for something the code was not doing.
    fn detail_pane_geometry(
        window: (f32, f32),
        selected: bool,
    ) -> (egui::Rect, egui::Rect, egui::Rect) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(window.0, window.1),
            )),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut measured = (egui::Rect::ZERO, egui::Rect::ZERO, egui::Rect::ZERO);
        let mut output = ctx.run_ui(input, |ui| {
            // The pane starts under the rail, the list pane and two dividers.
            let left = RAIL_WIDTH + PANE_GAP + SIDEBAR_WIDTH + PANE_GAP + PANE_MARGIN;
            let top = PANE_MARGIN;
            let height = window.1 - top - STATUS_BAR_HEIGHT;
            let content_height = (height - DETAIL_FOOTER_HEIGHT).max(120.0);
            let pane = egui::Rect::from_min_size(
                egui::pos2(left, top),
                egui::vec2(window.0 - left - PANE_MARGIN, height),
            );
            let content_rect =
                egui::Rect::from_min_size(pane.min, egui::vec2(pane.width(), content_height));

            // The bounds the real pane has: in the window the vertical only ever
            // sees the width left over after the rail, the list pane and the two
            // dividers, and its height is pinned to the pane. Handing it a
            // positioned child at the pane's rect reproduces both, which is what
            // keeps the footer's right edge on the pane's right edge.
            let mut pane_ui = ui.new_child(egui::UiBuilder::new().max_rect(pane));
            pane_ui.vertical(|ui| {
                ui.set_height(pane.height());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
                if selected {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut child, |ui| {
                            ui.label("an overlay's settings");
                        });
                } else {
                    empty_editor(&mut child);
                }
                measured.0 = child.min_rect();
                ui.advance_cursor_after_rect(content_rect);
                ui.add_space(4.0);
                measured.2 = egui::Rect::from_min_max(
                    egui::pos2(pane.left(), content_rect.bottom() + 4.0),
                    pane.max,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(DETAIL_FOOTER_BUTTON_WIDTH, DETAIL_FOOTER_BUTTON_HEIGHT),
                        egui::Sense::click(),
                    );
                    measured.1 = rect;
                });
            });
        });
        output.textures_delta.clear();
        measured
    }

    /// The detail pane's footer has to sit under the detail content whether or
    /// not anything is selected.
    ///
    /// It did not. The empty pane drew two labels inside `ui.centered_and_justified`,
    /// which is a one-widget builder, and with three widgets inside it the builder
    /// reported a min rect 22px *taller* than the box it had been given. The
    /// parent advanced its cursor by that inflated height, so Save and Discard
    /// landed 19.5px low: at y=793 in a 800px window, past the frame's own bottom
    /// margin and on top of the status bar. With an overlay selected the editor
    /// is a ScrollArea that reports its box exactly, and the footer was correct,
    /// which is why the bug only ever showed up empty.
    ///
    /// Both states are driven here, and the assertion is on the footer's position
    /// rather than on the message, so any future change to the empty state's
    /// content cannot quietly move the footer again.
    #[test]
    fn the_detail_footer_stays_under_the_detail_pane() {
        const WINDOW: (f32, f32) = (1000.0, 800.0);
        for (label, selected) in [("nothing selected", false), ("overlay selected", true)] {
            let (content_box, footer, band) = detail_pane_geometry(WINDOW, selected);
            // The footer is centred in the band, so the invariant is that it sits
            // in that band and not that it starts the instant the content ends.
            // The centring carries a 2px tolerance: egui rounds the centred region
            // to whole pixels and adds half the item spacing, and re-deriving that
            // arithmetic here would be the same mistake that produced this bug in
            // the first place. The bug was 19.5px, so 2px is an order of magnitude
            // tighter than what has to be caught, and the containment check below
            // is the assertion that actually pins it.
            let expected_top = band.top() + (band.height() - DETAIL_FOOTER_BUTTON_HEIGHT) / 2.0;
            assert!(
                (footer.top() - expected_top).abs() <= 2.0,
                "with {label} the footer sat at {footer:?} but the band is {band:?}, \
                 so it should have started at {expected_top}"
            );
            assert!(
                band.contains_rect(footer),
                "with {label} the footer {footer:?} escaped its band {band:?}"
            );
            assert!(
                footer.bottom() <= WINDOW.1 - PANE_MARGIN,
                "with {label} the footer ran to {footer:?}, past the {WINDOW:?} window"
            );
            // And the content must never be the thing that decides: whatever the
            // pane drew, the band starts a fixed gap below the box it was given.
            assert!(
                (band.top() - (content_box.bottom() + 4.0)).abs() < f32::EPSILON,
                "with {label} the content reported {content_box:?}, which moved the band"
            );
        }
    }

    /// The border preview needs the window open, the Overlays page showing,
    /// something selected and the animation enabled. The page went missing for
    /// a release, so all four are pinned here rather than only the window.
    #[test]
    fn the_border_preview_only_runs_on_the_overlays_page() {
        let cases = [
            (false, Page::Overlays, Some("overlay"), true, None),
            (false, Page::Profiles, Some("overlay"), true, None),
            (true, Page::Overlays, Some("overlay"), true, Some("overlay")),
            (true, Page::Profiles, Some("overlay"), true, None),
            (true, Page::Global, Some("overlay"), true, None),
            (true, Page::Overlays, None, true, None),
            (true, Page::Profiles, None, true, None),
            (true, Page::Global, None, true, None),
            // The global toggle: an otherwise perfect match animates nothing,
            // with or without an overlay selected.
            (true, Page::Overlays, Some("overlay"), false, None),
            (true, Page::Overlays, None, false, None),
        ];
        for (visible, page, selected, animation, expected) in cases {
            assert_eq!(
                selected_overlay_for_border(visible, page, selected, animation),
                expected,
                "with the window {} on {}, {:?} selected and the animation {} \
                 the border preview should be {:?}",
                if visible { "open" } else { "closed" },
                page.label(),
                selected,
                if animation { "on" } else { "off" },
                expected
            );
        }
    }

    /// Clicking the selected row clears the selection, which is the only way to
    /// stop the border and empty pane 3.
    #[test]
    fn clicking_the_selected_row_clears_it() {
        assert_eq!(toggled_selection(None, "a"), Some("a".to_string()));
        assert_eq!(toggled_selection(Some("a"), "a"), None);
        assert_eq!(toggled_selection(Some("b"), "a"), Some("a".to_string()));
        assert_eq!(toggled_selection(Some("a"), "b"), Some("b".to_string()));
    }

    /// The deselect strip is a hit target over the leftover viewport height. It
    /// must never reach below the pane, never appear when the rows already fill
    /// the viewport, and never be wider than the rows it sits under.
    #[test]
    fn the_deselect_strip_fills_only_the_space_the_rows_leave() {
        let viewport = egui::Rect::from_min_max(egui::pos2(188.0, 12.0), egui::pos2(434.0, 212.0));
        let row_width = 246.0;

        let strip = deselect_strip_rect(viewport, 100.0, row_width).expect("room to spare");
        assert_eq!(
            strip.top(),
            100.0,
            "the strip must start under the last row"
        );
        assert_eq!(
            strip.bottom(),
            viewport.bottom(),
            "the strip must reach the bottom"
        );
        assert_eq!(strip.left(), viewport.left());
        assert_eq!(
            strip.width(),
            row_width,
            "the strip must match the rows' width"
        );
        assert!(
            viewport.contains_rect(strip),
            "the strip {strip:?} escaped the list pane {viewport:?}"
        );

        // The rows exactly filling the viewport leaves no strip, so a list that
        // fits cannot acquire a scrollbar it did not deserve.
        assert!(
            deselect_strip_rect(viewport, viewport.bottom(), row_width).is_none(),
            "a full list must have no deselect strip"
        );
        assert!(
            deselect_strip_rect(viewport, viewport.bottom() + 40.0, row_width).is_none(),
            "a scrolling list must have no deselect strip"
        );
        assert!(
            deselect_strip_rect(viewport, 0.0, row_width).is_some(),
            "an empty list leaves the whole viewport as a strip"
        );
    }

    #[test]
    fn config_migration_messages_describe_cleanup() {
        assert_eq!(
            config_notice_status(&ConfigNotice::Migrated {
                legacy_directory_retained: false,
            }),
            "Config migrated to ~/.config/.PingLatencyOverlay."
        );
        assert!(config_notice_status(&ConfigNotice::Migrated {
            legacy_directory_retained: true,
        })
        .contains("legacy directory was retained"));
        assert!(config_notice_status(&ConfigNotice::MigrationFailed(
            "permission denied".to_string()
        ))
        .contains("Legacy config was kept"));
    }

    #[test]
    fn profile_notices_are_reported_in_the_status_bar() {
        let imported = config_notice_status(&ConfigNotice::ProfileImported);
        assert!(imported.contains("profiles/profile_default.json"));

        let skipped = config_notice_status(&ConfigNotice::ProfileImportSkipped);
        assert!(skipped.contains("config.json was kept"));

        let failed =
            config_notice_status(&ConfigNotice::ProfileImportFailed("bad json".to_string()));
        assert!(failed.contains("config.json was kept"));

        let fallback = config_notice_status(&ConfigNotice::ProfileFallback {
            profile: "work".to_string(),
            reason: "not found".to_string(),
        });
        assert!(fallback.contains("\"work\""));
        assert!(fallback.contains("not found"));
    }

    #[test]
    fn several_notices_are_joined_into_one_status_message() {
        let notices = [
            ConfigNotice::Migrated {
                legacy_directory_retained: false,
            },
            ConfigNotice::ProfileImported,
        ];
        let status = config_notices_status(&notices);
        assert!(status.contains("Config migrated"));
        assert!(status.contains("profile_default.json"));
        assert_eq!(config_notices_status(&[]), "");
    }

    #[test]
    fn unsaved_edits_block_switching_profiles() {
        assert!(!can_switch_profile(true));
        assert!(can_switch_profile(false));
    }

    #[test]
    fn the_window_title_names_the_active_profile() {
        assert_eq!(
            window_title("Home Net", false),
            "PingLatencyOverlay - Current Profile: Home Net"
        );
        assert_eq!(
            window_title("default", false),
            "PingLatencyOverlay - Current Profile: default",
            "a name that is not stored yet still shows the id"
        );
    }

    /// The version is a preference and it is off by default, because the title is
    /// already long and the About page is where the version belongs.
    #[test]
    fn the_window_title_can_carry_the_version() {
        assert_eq!(
            window_title("Home Net", true),
            format!(
                "PingLatencyOverlay - Current Profile: Home Net ({})",
                app_version()
            )
        );
    }

    #[test]
    fn added_profile_names_are_reported_once() {
        assert_eq!(
            config_notice_status(&ConfigNotice::ProfileNamesBackfilled { count: 1 }),
            "Added a display name to 1 existing profile."
        );
        assert!(
            config_notice_status(&ConfigNotice::ProfileNamesBackfilled { count: 3 })
                .contains("3 existing profiles")
        );
    }

    // --- Driving the window -------------------------------------------------
    //
    // Everything below runs the window's *real* per-frame path against a
    // sandbox: `PingApp::for_test` loads from a temp root and skips the tray and
    // renderer, and `frame_ui` is the exact body the eframe trait method calls.
    // A click here is the click a user makes, so a control that is unreachable,
    // mis-sized or wired to the wrong handler fails the test instead of needing
    // someone to look at a screenshot.

    /// A sandbox root for a driving test, emptied first so a stale run cannot
    /// decide what the window loads.
    fn driving_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// Run one real UI pass at the real window size and with the given events.
    ///
    /// The texture deltas are dropped because nothing here uploads them and a
    /// full delta list panics on drop, which is the same line the layout tests
    /// carry.
    fn drive(app: &mut PingApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.frame_ui(ui));
        output.textures_delta.clear();
        output
    }

    /// One pass at a taller window, for the sections below the fold: the detail
    /// pane's scroll area does not paint what it cannot show, and a test asking
    /// whether a control exists should not depend on the window's height.
    fn drive_tall(app: &mut PingApp, ctx: &egui::Context) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT + 600.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.frame_ui(ui));
        output.textures_delta.clear();
        output
    }

    /// Every piece of `text` a pass painted, wherever it came from.
    ///
    /// This is what makes a control addressable without coordinates. The rail
    /// rows, the theme tiles and the list rows are painted with `painter.text`
    /// rather than built as buttons, and a row or tile's hit target covers its
    /// label; a checkbox paints its label inside its own response. So finding
    /// the label is finding the control, even for the painted half of the UI.
    fn text_rects(output: &egui::FullOutput, text: &str) -> Vec<egui::Rect> {
        fn walk(shape: &egui::Shape, text: &str, found: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, text, found);
                    }
                }
                egui::Shape::Text(piece) if piece.galley.job.text == text => {
                    found.push(egui::Rect::from_min_size(piece.pos, piece.galley.size()));
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, text, &mut found);
        }
        found
    }

    /// Press and release the primary button at `pos`, across passes the way the
    /// host delivers them (move, press, release).
    fn click_at(app: &mut PingApp, ctx: &egui::Context, pos: egui::Pos2) {
        drive(app, ctx, vec![egui::Event::PointerMoved(pos)]);
        drive(
            app,
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        drive(
            app,
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    /// Click the single match for `text` that `keep` accepts.
    ///
    /// The uniqueness assertion is the point: a locator that silently clicked
    /// the first of two matches would let a test pass while clicking the wrong
    /// control. `keep` exists for the label that legitimately appears twice in
    /// one pass — an overlay's name on its list row and in the editor beside it.
    fn click_text_where(
        app: &mut PingApp,
        ctx: &egui::Context,
        text: &str,
        keep: impl Fn(egui::Rect) -> bool,
    ) {
        let output = drive(app, ctx, Vec::new());
        let rects: Vec<egui::Rect> = text_rects(&output, text)
            .into_iter()
            .filter(|rect| keep(*rect))
            .collect();
        assert_eq!(
            rects.len(),
            1,
            "\"{text}\" matched {} clickable places, not one: {rects:?}",
            rects.len()
        );
        click_at(app, ctx, rects[0].center());
    }

    /// Click the one place `text` appears in the current pass.
    fn click_text(app: &mut PingApp, ctx: &egui::Context, text: &str) {
        click_text_where(app, ctx, text, |_| true);
    }

    /// A tile click changes the theme live *and* stages it, and Save writes the
    /// sandbox file.
    ///
    /// Both halves matter for the same reason `choose_theme` writes both: the
    /// live value is what the next pass reads, and the draft is what Save
    /// writes. A test that read only the file would pass with the setting dead
    /// until Save.
    #[test]
    fn clicking_a_theme_tile_stages_and_saves() {
        let root = driving_root("plo-drive-theme");
        let ctx = egui::Context::default();
        let mut app = PingApp::for_test(&ctx, &root);
        assert_eq!(app.page, Page::Overlays, "the window opens on Overlays");

        click_text(&mut app, &ctx, "Global");
        assert_eq!(app.page, Page::Global, "the rail row did not switch pages");

        click_text(&mut app, &ctx, "Dark Theme");
        assert_eq!(
            app.prefs.ui.theme,
            ThemeMode::Dark,
            "the live theme did not follow the tile, so the next pass would undo it"
        );
        assert_eq!(
            app.prefs_draft.ui.theme,
            ThemeMode::Dark,
            "the tile did not stage the preference"
        );
        assert!(
            app.prefs_dirty,
            "the tile did not mark the preferences dirty"
        );

        click_text(&mut app, &ctx, "Save");
        assert!(!app.prefs_dirty, "Save left the preference staged");
        assert_eq!(
            config::Store::new(root.clone())
                .read_global_prefs()
                .ui
                .theme,
            ThemeMode::Dark,
            "Save did not write the theme"
        );
    }

    /// A rail row switches pages, from wherever the window currently is.
    #[test]
    fn clicking_the_rail_switches_pages() {
        let root = driving_root("plo-drive-rail");
        let ctx = egui::Context::default();
        let mut app = PingApp::for_test(&ctx, &root);

        click_text(&mut app, &ctx, "Profiles");
        assert_eq!(app.page, Page::Profiles);

        click_text(&mut app, &ctx, "About");
        assert_eq!(app.page, Page::About);

        click_text(&mut app, &ctx, "Overlays");
        assert_eq!(app.page, Page::Overlays);
    }

    /// The selection-border checkbox is staged and live at once; Discard puts
    /// the live value back and writes nothing, Save writes the sandbox file.
    #[test]
    fn the_selection_border_checkbox_stages_live_and_discards() {
        let root = driving_root("plo-drive-checkbox");
        let ctx = egui::Context::default();
        let mut app = PingApp::for_test(&ctx, &root);
        click_text(&mut app, &ctx, "Global");

        click_text(&mut app, &ctx, "Animate the selected overlay's border");
        assert!(
            !app.prefs.ui.selection_border_animation,
            "the live value did not follow the box"
        );
        assert!(
            !app.prefs_draft.ui.selection_border_animation,
            "the box did not stage the preference"
        );
        assert!(
            app.prefs_dirty,
            "the box did not mark the preferences dirty"
        );

        click_text(&mut app, &ctx, "Discard");
        assert!(
            app.prefs.ui.selection_border_animation,
            "Discard did not restore the live value"
        );
        assert!(
            app.prefs_draft.ui.selection_border_animation,
            "Discard left the draft behind"
        );
        assert!(!app.prefs_dirty, "Discard left the preferences dirty");
        assert!(
            config::Store::new(root.clone())
                .read_global_prefs()
                .ui
                .selection_border_animation,
            "Discard wrote the abandoned draft to the sandbox"
        );

        click_text(&mut app, &ctx, "Animate the selected overlay's border");
        click_text(&mut app, &ctx, "Save");
        assert!(!app.prefs_dirty, "Save left the preferences dirty");
        assert!(
            !config::Store::new(root.clone())
                .read_global_prefs()
                .ui
                .selection_border_animation,
            "Save did not write the preference"
        );
    }

    /// A list row selects its overlay, and clicking the selected row again
    /// clears it — the one deliberate way to stop the border preview.
    #[test]
    fn clicking_an_overlay_row_selects_and_reselecting_clears() {
        let root = driving_root("plo-drive-select");
        let store = config::Store::new(root.clone());
        let mut config = Config::default();
        config.overlays.push(OverlayConfig::new());
        let overlay_id = config.overlays[0].id.clone();
        store
            .save_profile("default", &config)
            .expect("seed the profile");
        store
            .set_active_profile("default")
            .expect("seed the active profile");

        let ctx = egui::Context::default();
        let mut app = PingApp::for_test(&ctx, &root);
        assert_eq!(app.page, Page::Overlays);
        assert_eq!(
            app.selected_id, None,
            "nothing may select an overlay just because the window opened"
        );

        // The row label is in the list pane, left of the detail pane. The
        // second click is why the filter exists: with the editor open, the
        // overlay's name is painted there too.
        let list_pane_right = RAIL_WIDTH + PANE_GAP + SIDEBAR_WIDTH;
        click_text_where(&mut app, &ctx, "New overlay", |rect| {
            rect.center().x < list_pane_right
        });
        assert_eq!(
            app.selected_id.as_deref(),
            Some(overlay_id.as_str()),
            "the row did not select its overlay"
        );

        click_text_where(&mut app, &ctx, "New overlay", |rect| {
            rect.center().x < list_pane_right
        });
        assert_eq!(
            app.selected_id, None,
            "clicking the selected row did not clear the selection"
        );
    }

    /// The three sticky boxes are three conditions, in one fixed order, and
    /// they cannot disagree with the matcher about what they mean.
    #[test]
    fn the_sticky_boxes_read_and_write_the_same_conditions() {
        let mut target = None;
        assert_eq!(sticky_value(target.as_ref(), Part::ProcessName), "");

        set_sticky_value(&mut target, Part::ProcessName, "chrome.exe".to_string());
        set_sticky_value(&mut target, Part::Title, "github".to_string());
        set_sticky_value(
            &mut target,
            Part::ClassName,
            "Chrome_WidgetWin_1".to_string(),
        );
        let target = target.expect("three boxes are a target");
        assert_eq!(
            target.when,
            vec![
                Condition {
                    part: Part::ProcessName,
                    matcher: MatchMode::Exact,
                    value: "chrome.exe".to_string(),
                },
                Condition {
                    part: Part::Title,
                    matcher: MatchMode::Contains,
                    value: "github".to_string(),
                },
                Condition {
                    part: Part::ClassName,
                    matcher: MatchMode::Exact,
                    value: "Chrome_WidgetWin_1".to_string(),
                },
            ]
        );
        assert_eq!(sticky_value(Some(&target), Part::Title), "github");

        // Clearing one box drops its condition; clearing the last one is back
        // to "no target", not an empty matcher.
        let mut target = Some(target);
        set_sticky_value(&mut target, Part::Title, String::new());
        let target = target.expect("two boxes are still a target");
        assert!(target
            .when
            .iter()
            .all(|condition| condition.part != Part::Title));
        let mut target = Some(target);
        set_sticky_value(&mut target, Part::ProcessName, String::new());
        set_sticky_value(&mut target, Part::ClassName, String::new());
        assert!(target.is_none(), "no boxes left is no target");
    }

    /// Typing in one box leaves the other conditions alone, modes included: a
    /// hand-edited regex title survives editing the process box.
    #[test]
    fn writing_a_sticky_box_leaves_the_other_conditions_alone() {
        let regex = Condition {
            part: Part::Title,
            matcher: MatchMode::Regex,
            value: "(?i)github".to_string(),
        };
        let mut target = Some(config::StickyTarget {
            when: vec![regex.clone()],
        });
        set_sticky_value(&mut target, Part::ProcessName, "chrome.exe".to_string());
        let target = target.expect("a target");
        assert!(
            target.when.contains(&regex),
            "editing the process box rewrote the regex title"
        );

        // The box for that part is what normalizes it, and only then.
        let mut target = Some(target);
        set_sticky_value(&mut target, Part::Title, "(?i)github".to_string());
        let target = target.expect("a target");
        assert!(target.when.iter().any(|condition| {
            condition.part == Part::Title && condition.matcher == MatchMode::Contains
        }));
    }

    /// A picked window fills the process and the class, and deliberately not
    /// the title: the picked title is true only for the moment it was read, and
    /// a stale one is what strands an overlay on a window right in front of the
    /// user (Windows 11's Notepad reopens its last document, a browser's title
    /// follows the page).
    #[test]
    fn a_picked_window_fills_process_and_class_but_not_the_title() {
        let window = WindowInfo {
            process: "notepad.exe".to_string(),
            title: "note.txt - Notepad".to_string(),
            class_name: "Notepad".to_string(),
        };
        let mut target = None;
        fill_sticky_target(&mut target, &window);
        let target = target.expect("a picked window is a target");
        assert_eq!(
            target.when,
            vec![
                Condition {
                    part: Part::ProcessName,
                    matcher: MatchMode::Exact,
                    value: "notepad.exe".to_string(),
                },
                Condition {
                    part: Part::ClassName,
                    matcher: MatchMode::Exact,
                    value: "Notepad".to_string(),
                },
            ],
            "the picked title must not become a condition"
        );
        assert_eq!(sticky_value(Some(&target), Part::Title), "");

        // Picking over a title the user typed on purpose starts a fresh target:
        // that title was true for the window they had, not the one they picked.
        let mut target = Some(target);
        set_sticky_value(&mut target, Part::Title, "note".to_string());
        fill_sticky_target(&mut target, &window);
        assert_eq!(sticky_value(target.as_ref(), Part::Title), "");
    }

    /// A pick commits on the release after its press, and on nothing else.
    ///
    /// This is the part of the crosshair that can be tested without a mouse: a
    /// pick must not fire on the click that starts it, must not fire while the
    /// button is held, and must fire exactly once on the release. The capture
    /// is deliberately not in this decision — winit drops it on every button-up.
    #[test]
    fn a_pick_commits_on_the_release_after_its_press() {
        let (mut armed, mut held) = (false, false);

        // The pass right after the button started the pick: the starting click
        // is already released, so the next press is the one that picks.
        assert!(
            !pick_step(&mut armed, &mut held, false),
            "nothing is picked before a press"
        );
        assert!(armed, "the picker never armed after the starting click");

        assert!(
            !pick_step(&mut armed, &mut held, true),
            "a press picks by itself"
        );
        assert!(held);
        assert!(
            !pick_step(&mut armed, &mut held, true),
            "a hold is not a click"
        );

        assert!(
            pick_step(&mut armed, &mut held, false),
            "the release did not commit the pick"
        );
        assert!(
            !pick_step(&mut armed, &mut held, false),
            "one release committed twice"
        );
    }

    /// The click that starts a pick is not the pick, even if its release is the
    /// first thing the picker sees.
    #[test]
    fn the_starting_click_is_never_the_pick() {
        let (mut armed, mut held) = (false, false);
        assert!(
            !pick_step(&mut armed, &mut held, true),
            "the starting press must not arm the picker"
        );
        assert!(!armed);
        assert!(
            !pick_step(&mut armed, &mut held, false),
            "the starting release must not commit"
        );
        assert!(
            armed,
            "the picker should be armed after the starting release"
        );
    }

    /// The pick glyph's parts stay inside the box the button gives it, the
    /// same check the theme glyphs get.
    ///
    /// A glyph whose parts come from a table can be centred and still poke out
    /// — the Global glyph shipped with a knob past its track — so the reach is
    /// summed rather than eyeballed.
    #[test]
    fn the_pick_glyph_stays_inside_its_box() {
        let reach = (PICK_ICON_TICK_OUTER + PICK_ICON_STROKE / 2.0)
            .max(PICK_ICON_RADIUS + PICK_ICON_STROKE / 2.0);
        assert!(
            reach <= 0.5,
            "the crosshair reaches {reach} of its box, past its edge"
        );
        let gap = PICK_ICON_TICK_INNER - (PICK_ICON_RADIUS + PICK_ICON_STROKE / 2.0);
        assert!(
            gap > 0.0,
            "the ticks have to start outside the ring, or they read as spokes"
        );
    }

    /// The cursor rule for a running pick, as a table.
    ///
    /// The displaced case is the one that matters: a window that has been moved
    /// out of the way has no rect for the pointer to leave, and the crosshair
    /// is the only sign a pick is still running.
    #[test]
    fn the_pick_cursor_follows_the_window() {
        let cases = [
            (false, true, egui::CursorIcon::Crosshair),
            (false, false, egui::CursorIcon::Default),
            (true, false, egui::CursorIcon::Crosshair),
            (true, true, egui::CursorIcon::Crosshair),
        ];
        for (displaced, inside, expected) in cases {
            assert_eq!(
                pick_cursor(displaced, inside),
                expected,
                "with displaced={displaced} and inside={inside}"
            );
        }
    }

    /// The Display Mode radios stage the mode through the real frame, and
    /// choosing Sticky puts the target's controls on screen.
    #[test]
    fn choosing_sticky_mode_stages_it_and_opens_the_target_boxes() {
        let root = driving_root("plo-drive-display-mode");
        let store = config::Store::new(root.clone());
        let mut config = Config::default();
        config.overlays.push(OverlayConfig::new());
        store
            .save_profile("default", &config)
            .expect("seed the profile");
        store
            .set_active_profile("default")
            .expect("seed the active profile");

        let ctx = egui::Context::default();
        let mut app = PingApp::for_test(&ctx, &root);

        let list_pane_right = RAIL_WIDTH + PANE_GAP + SIDEBAR_WIDTH;
        click_text_where(&mut app, &ctx, "New overlay", |rect| {
            rect.center().x < list_pane_right
        });
        assert!(app.selected_id.is_some(), "the editor did not open");

        assert_eq!(
            app.config.overlays[0].display_mode,
            config::DisplayMode::Global
        );
        click_text(&mut app, &ctx, "Sticky overlay");
        assert_eq!(
            app.config.overlays[0].display_mode,
            config::DisplayMode::Sticky,
            "the radio did not stage the mode"
        );
        assert!(app.dirty, "staging a mode did not mark the profile dirty");

        let output = drive_tall(&mut app, &ctx);
        assert!(
            !text_rects(&output, "Process").is_empty(),
            "the sticky target's boxes did not appear"
        );
        assert!(
            !text_rects(&output, "Always above other windows").is_empty(),
            "the sticky z-order choice did not appear"
        );
    }
}
