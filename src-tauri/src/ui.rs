use std::collections::HashMap;
use std::error::Error;
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, ComboBox, Context, Frame, Grid, Layout, RichText, Ui, ViewportBuilder,
};
use eframe::{App, CreationContext, NativeOptions};

use crate::config::{self, Anchor, BorderEffect, Config, OverlayConfig, ProbeConfig};
use crate::overlay::OverlayManager;
use crate::probes::ProbeManager;
use crate::tray::{self, TrayAction, TrayState};

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
/// The About page's repository, shown as a read-only row.
const ABOUT_REPOSITORY: &str = "https://github.com/megablue/PingLatencyOverlay";
/// The author's own page, linked from the About page.
const ABOUT_AUTHOR_URL: &str = "https://github.com/megablue";
/// Where the About page's licence name links to. The licence itself is not
/// bundled with the app, so the link goes to the canonical text online.
const ABOUT_LICENCE_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";
/// Space above the About page's centred column, so it does not sit hard against
/// the top of the pane.
const ABOUT_TOP_GAP: f32 = 12.0;
/// Extra air before the About page's link block, which reads as a group
/// separate from the name, the tagline and the version above it.
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
}

/// The text of every line on the About page, in order.
///
/// The page used to be a labelled table with accent section headers, which read
/// as settings rather than as an About box. It is one centred column now, and
/// the facts are unchanged: what this is, which version, where the source is,
/// who wrote it, under what licence. The copyright carries no year on purpose,
/// so nothing here can go stale between releases.
fn about_page_lines() -> Vec<AboutLine> {
    let body = ui_text_size();
    vec![
        AboutLine {
            text: "PingLatencyOverlay".to_string(),
            size: body + 12.0,
            kind: AboutKind::Text,
        },
        AboutLine {
            text: "A small overlay that shows live network latency.".to_string(),
            size: body,
            kind: AboutKind::Text,
        },
        AboutLine {
            text: app_version(),
            size: body + 4.0,
            kind: AboutKind::Text,
        },
        AboutLine {
            text: "github.com/megablue/PingLatencyOverlay".to_string(),
            size: body,
            kind: AboutKind::Link(ABOUT_REPOSITORY),
        },
        AboutLine {
            text: "github.com/megablue".to_string(),
            size: body,
            kind: AboutKind::Link(ABOUT_AUTHOR_URL),
        },
        AboutLine {
            text: "Copyright \u{a9} Evert Chin".to_string(),
            size: body,
            kind: AboutKind::Text,
        },
        AboutLine {
            text: "GPL-3.0-only".to_string(),
            size: body,
            kind: AboutKind::Link(ABOUT_LICENCE_URL),
        },
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
fn about_page_column(ui: &mut Ui) {
    ui.add_space(ABOUT_TOP_GAP);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        for (index, line) in about_page_lines().iter().enumerate() {
            if index == 3 {
                // A little more air before the links than between the rest, so
                // the block reads as name, what it is, version, then contact.
                ui.add_space(ABOUT_GROUP_GAP);
            } else if index > 0 {
                ui.add_space(2.0);
            }
            let font = egui::FontId::proportional(line.size);
            match line.kind {
                AboutKind::Text => {
                    let colour = if index == 1 {
                        UI_TEXT_SECONDARY
                    } else {
                        UI_TEXT
                    };
                    ui.label(RichText::new(&line.text).font(font).color(colour));
                }
                AboutKind::Link(url) => {
                    ui.add(egui::Hyperlink::from_label_and_url(
                        RichText::new(&line.text).font(font),
                        url,
                    ));
                }
            }
        }
    });
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

// Windows 11 Explorer-inspired dark palette.
const UI_BACKGROUND: Color32 = Color32::from_rgb(0x19, 0x19, 0x19);
const UI_SURFACE: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const UI_SURFACE_ALT: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const UI_SURFACE_HOVER: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
const UI_INPUT_BACKGROUND: Color32 = Color32::from_rgb(0x0f, 0x0f, 0x0f);
const UI_BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
const UI_TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const UI_TEXT_SECONDARY: Color32 = Color32::from_rgb(0xc5, 0xc5, 0xc5);
const UI_ACCENT: Color32 = Color32::from_rgb(0x60, 0xcd, 0xff);
const UI_ACCENT_STRONG: Color32 = Color32::from_rgb(0x2f, 0x6f, 0x9f);
const UI_SELECTION: Color32 = Color32::from_rgb(0x2d, 0x4f, 0x6d);
const UI_SCROLLBAR: Color32 = Color32::from_rgb(0x23, 0x40, 0x56);
const UI_SCROLLBAR_HOVER: Color32 = Color32::from_rgb(0x2d, 0x4f, 0x6d);
const UI_DANGER: Color32 = Color32::from_rgb(0xf4, 0x87, 0x71);
const UI_DANGER_STRONG: Color32 = Color32::from_rgb(0x9e, 0x2b, 0x2b);

fn explorer_dark_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let border = egui::Stroke::new(1.0, UI_BORDER);
    let text = egui::Stroke::new(1.0, UI_TEXT);
    let accent_text = egui::Stroke::new(1.0, UI_ACCENT);
    let radius = egui::CornerRadius::same(4);

    visuals.override_text_color = Some(UI_TEXT);
    visuals.weak_text_color = Some(UI_TEXT_SECONDARY);
    visuals.panel_fill = UI_BACKGROUND;
    visuals.window_fill = UI_BACKGROUND;
    visuals.faint_bg_color = UI_SURFACE;
    visuals.extreme_bg_color = UI_INPUT_BACKGROUND;
    visuals.text_edit_bg_color = Some(UI_INPUT_BACKGROUND);
    visuals.hyperlink_color = UI_ACCENT;
    visuals.warn_fg_color = UI_DANGER;
    visuals.error_fg_color = UI_DANGER;
    visuals.selection.bg_fill = UI_SELECTION;
    visuals.selection.stroke = accent_text;
    visuals.window_stroke = border;
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.menu_corner_radius = radius;
    visuals.text_cursor.stroke = accent_text;
    visuals.button_frame = true;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.disabled_alpha = 0.45;

    visuals.widgets.noninteractive.bg_fill = UI_SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = UI_SURFACE;
    visuals.widgets.noninteractive.bg_stroke = border;
    visuals.widgets.noninteractive.fg_stroke = text;
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = UI_SCROLLBAR;
    visuals.widgets.inactive.weak_bg_fill = UI_SURFACE_ALT;
    visuals.widgets.inactive.bg_stroke = border;
    visuals.widgets.inactive.fg_stroke = text;
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = UI_SCROLLBAR_HOVER;
    visuals.widgets.hovered.weak_bg_fill = UI_SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = border;
    visuals.widgets.hovered.fg_stroke = text;
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = UI_SELECTION;
    visuals.widgets.active.weak_bg_fill = UI_SELECTION;
    visuals.widgets.active.bg_stroke = accent_text;
    visuals.widgets.active.fg_stroke = text;
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.bg_fill = UI_SURFACE_HOVER;
    visuals.widgets.open.weak_bg_fill = UI_SURFACE_HOVER;
    visuals.widgets.open.bg_stroke = accent_text;
    visuals.widgets.open.fg_stroke = text;
    visuals.widgets.open.corner_radius = radius;

    visuals
}

const POSITION_PICKER_SIZE: f32 = 220.0;
const POSITION_PICKER_DISPLAY_SIZE: f32 = 180.0;
const POSITION_PICKER_PADDING: f32 = 4.0;
const POSITION_PICKER_CELL_SIZE: f32 = 40.0;
const POSITION_PICKER_GAP: f32 = 46.0;

struct PositionPicker {
    default_texture: egui::TextureHandle,
    hover_texture: egui::TextureHandle,
    selected_texture: egui::TextureHandle,
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
        }
    }

    fn show(&self, ui: &mut Ui, current: Anchor) -> Option<Anchor> {
        let display_size = ui
            .available_width()
            .clamp(1.0, POSITION_PICKER_DISPLAY_SIZE);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(display_size, display_size), egui::Sense::click());
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter()
            .image(self.default_texture.id(), rect, full_uv, Color32::WHITE);

        let hovered = response
            .hover_pos()
            .and_then(|point| position_cell_at(rect, point));
        if let Some(index) = hovered {
            let cell_rect = position_cell_rect(rect, index);
            ui.painter().image(
                self.hover_texture.id(),
                cell_rect,
                position_cell_uv(index),
                Color32::WHITE,
            );
        }

        let selected = position_index(current);
        let selected_rect = position_cell_rect(rect, selected);
        ui.painter().image(
            self.selected_texture.id(),
            selected_rect,
            position_cell_uv(selected),
            Color32::WHITE,
        );
        ui.painter().rect_stroke(
            selected_rect,
            8.0,
            egui::Stroke::new(1.5, UI_ACCENT),
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
    Running,
    HideRequested,
    CloseRequested,
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
    /// Preferences as last written to `globalconfig.json`.
    prefs: config::GlobalPrefs,
    /// The staged copy the Global page edits, written on Save.
    prefs_draft: config::GlobalPrefs,
    prefs_dirty: bool,
    config_visible: bool,
    running: bool,
    status: String,
    dirty: bool,
    confirm_delete: Option<String>,
    probes: ProbeManager,
    overlays: OverlayManager,
    tray: TrayState,
    position_picker: PositionPicker,
    _runtime: Option<tokio::runtime::Runtime>,
    shutdown_state: ShutdownState,
}

impl PingApp {
    pub fn new(cc: &CreationContext<'_>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        cc.egui_ctx.set_visuals(explorer_dark_visuals());
        cc.egui_ctx.global_style_mut(|style| {
            style.spacing.scroll.foreground_color = false;
        });
        let position_picker = PositionPicker::new(&cc.egui_ctx);
        let loaded = config::load();
        let config = loaded.config;
        let status = config_notices_status(&loaded.notices);
        let active_profile = loaded.active_profile;
        // The one place the rail's collapsed state survives a restart.
        let prefs = loaded.prefs;
        let rail_collapsed = prefs.ui.rail_collapsed;
        let profiles = loaded.profiles;
        // Nothing is selected on the Overlays page until the user picks an
        // overlay. Selecting the first one automatically meant a border was
        // animating the moment the window opened, which read as the app doing
        // something nobody asked for. The selection is view-only: staged edits
        // live in `config.overlays`, so emptying pane 3 cannot lose any.
        let show_config = std::env::args_os().any(|arg| arg == "--show-config");

        // The runtime is kept alive for the lifetime of the one egui window.
        // Probe tasks are long-lived and read their current settings from a
        // shared map; saving a config never aborts them.
        let runtime = tokio::runtime::Runtime::new()?;
        let mut probes = ProbeManager::new(runtime.handle().clone());
        probes.apply_config(&config);
        let tray = tray::create()?;
        let overlays = OverlayManager::new()?;
        let running = probes.is_running();
        tray.set_running(running);

        cc.egui_ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Visible(show_config),
        );
        // The static title in `run` cannot know the profile, so the real one
        // is sent as soon as the app state exists.
        let mut app = Self {
            page: Page::Overlays,
            // Matching the initial page, so the first pass is not read as an
            // arrival and the startup is not spent reading every profile file.
            last_page: Page::Overlays,
            rail_collapsed,
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
            selected_id: None,
            prefs_draft: prefs.clone(),
            prefs,
            prefs_dirty: false,
            config_visible: show_config,
            running,
            status,
            dirty: false,
            confirm_delete: None,
            probes,
            overlays,
            tray,
            position_picker,
            _runtime: Some(runtime),
            shutdown_state: ShutdownState::Running,
        };
        app.sync_window_title(&cc.egui_ctx);
        cc.egui_ctx.request_repaint();
        Ok(app)
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

    fn set_config_visible(&mut self, ctx: &Context, visible: bool) {
        if self.config_visible == visible {
            return;
        }
        self.config_visible = visible;
        // A selected overlay's RGB border belongs to the Config interaction.
        // Reconcile immediately so closing the window starts the normal fade
        // instead of leaving the border selected indefinitely.
        self.sync_overlays();
        ctx.request_repaint();
    }

    fn handle_root_close(&mut self, ctx: &Context) {
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && self.shutdown_state == ShutdownState::Running {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::CancelClose);
            self.set_config_visible(ctx, false);
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Visible(false),
            );
        }
    }

    fn process_tray_events(&mut self, ctx: &Context) {
        if self.shutdown_state != ShutdownState::Running {
            return;
        }
        for action in self.tray.poll() {
            match action {
                TrayAction::ToggleRunning => {
                    self.running = !self.running;
                    self.probes.set_running(self.running);
                    self.tray.set_running(self.running);
                }
                TrayAction::Config => {
                    self.set_config_visible(ctx, true);
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::ROOT,
                        egui::ViewportCommand::Visible(true),
                    );
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::ROOT,
                        egui::ViewportCommand::Minimized(false),
                    );
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                }
                TrayAction::Exit => {
                    self.config_visible = false;
                    // eframe applies viewport commands after the current frame.
                    // Defer Close until a later logic-only pass so a visible
                    // Config window is hidden before the viewport is destroyed.
                    self.shutdown_state = ShutdownState::HideRequested;
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::ROOT,
                        egui::ViewportCommand::Visible(false),
                    );
                    self.probes.stop_all();
                    ctx.request_repaint();
                    return;
                }
            }
        }
    }

    fn sync_overlays(&mut self) {
        let selected_id = selected_overlay_for_border(
            self.config_visible,
            self.page,
            self.selected_id.as_deref(),
        );
        self.overlays.apply(
            &self.config,
            self.probes.samples(),
            self.running,
            selected_id,
        );
    }

    fn repaint_interval(&self) -> Duration {
        let smooth_interval = if self.running {
            self.config
                .overlays
                .iter()
                .filter(|overlay| overlay.enabled && overlay.smooth_rendering)
                .map(|overlay| config::smooth_frame_interval(overlay.smooth_fps))
                .min()
        } else {
            None
        };
        smooth_interval
            .into_iter()
            .chain(self.overlays.prefill_repaint_interval())
            .chain(self.overlays.border_repaint_interval())
            .min()
            .unwrap_or(REPAINT_INTERVAL)
    }

    fn apply_runtime_config(&mut self) {
        // Apply task and HWND changes immediately without writing the draft
        // configuration to disk. Existing probe tasks are reused by
        // ProbeManager::apply_config.
        self.probes.apply_config(&self.config);
        self.sync_overlays();
    }

    fn apply_saved_config(&mut self, next: Config) {
        self.config = next;
        self.apply_runtime_config();
    }

    /// Whether either draft has something to write.
    ///
    /// The profile draft and the preferences draft are independent, and pages
    /// stage into one or the other, so the footer's buttons must follow both.
    fn has_pending_edits(&self) -> bool {
        pending_edits(self.dirty, self.prefs_dirty)
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
    fn persist_current(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        let mut next = self.config.clone();
        next.normalize();
        // Profiles are the only configuration storage now; the previous
        // single-file config.json was moved into the profiles directory.
        match config::save_profile(&self.active_profile, &next) {
            Ok(()) => {
                self.apply_saved_config(next);
                self.dirty = false;
                self.status.clear();
                true
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
            self.apply_runtime_config();
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
    /// The profile draft and the preferences draft are independent: the Global
    /// page stages its own copy, so one Save can carry both.
    fn save_edits(&mut self) {
        let profile_saved = self.persist_current();
        let prefs_saved = self.save_prefs();
        if profile_saved || prefs_saved {
            self.status = "Saved.".to_string();
        }
    }

    /// Store the preferences draft, leaving the active profile pointer alone.
    fn save_prefs(&mut self) -> bool {
        if !self.prefs_dirty {
            return true;
        }
        match config::write_global_prefs(&self.prefs_draft) {
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

    /// Drop unsaved edits by reloading the active profile and the preferences
    /// from disk.
    fn discard_edits(&mut self) {
        self.discard_prefs();
        match config::load_profile(&self.active_profile) {
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
        self.prefs = config::read_global_prefs();
        self.prefs_draft = self.prefs.clone();
        self.rail_collapsed = self.prefs.ui.rail_collapsed;
        self.prefs_dirty = false;
    }

    /// Re-read the profile list, and with it the overlay count every profile
    /// row shows.
    ///
    /// The counts cost one file read per profile, so this only runs on a user
    /// action. Arriving on the Profiles page goes through `sync_profiles`;
    /// everything else that can change a count â€” creating, renaming,
    /// duplicating, deleting and switching a profile, and opening the switcher â€”
    /// calls this directly. Never per frame.
    ///
    /// A profile whose file will not parse is left out of the count map rather
    /// than counted as zero, so it draws no number instead of a wrong one.
    fn refresh_profiles(&mut self) {
        let snapshot = read_profile_snapshot();
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
        sync_profile_cache(last_page, page, profiles, counts, read_profile_snapshot);
    }

    /// Make another profile the active one and remember the choice.
    fn switch_profile(&mut self, id: &str) {
        self.profile_menu_open = false;
        self.selected_profile = Some(id.to_string());
        if id == self.active_profile {
            return;
        }
        if !can_switch_profile(self.dirty) {
            self.status = "Save or discard your changes before switching profiles.".to_string();
            return;
        }
        match config::load_profile(id) {
            Ok(mut stored) => {
                stored.normalize();
                self.active_profile = id.to_string();
                if let Err(error) = config::set_active_profile(&self.active_profile) {
                    self.status = format!("Could not update globalconfig.json: {error}");
                }
                self.selected_id = stored.overlays.first().map(|overlay| overlay.id.clone());
                self.apply_saved_config(stored);
                self.dirty = false;
                // Read after the load, so a name that is only stored in the
                // file is the one reported.
                self.refresh_profiles();
                self.status = format!("Loaded profile \"{}\".", self.active_profile_name());
            }
            Err(error) => {
                self.status = format!("Could not load profile \"{id}\": {error}");
            }
        }
    }

    /// Create a new empty profile, then load it. Returns false when the name
    /// was rejected.
    fn create_profile(&mut self, display_name: &str) -> bool {
        match config::create_profile(display_name) {
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
        match config::rename_profile(from, display_name) {
            Ok(renamed) => {
                if self.active_profile == from {
                    self.active_profile = renamed.id.clone();
                    // Keep the draft in step with the file, so the next Save
                    // cannot write the previous name back into it.
                    self.config.profile_name = renamed.name.clone();
                    if let Err(error) = config::set_active_profile(&self.active_profile) {
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
        match config::duplicate_profile(from, display_name) {
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
        if let Err(error) = config::delete_profile(id) {
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
        self.probes.set_running(self.running);
        self.tray.set_running(self.running);
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
                                    .color(UI_TEXT_SECONDARY),
                            );
                        }
                        let rows: Vec<(String, String, Anchor, bool)> = self
                            .config
                            .overlays
                            .iter()
                            .map(|overlay| {
                                (
                                    overlay.id.clone(),
                                    if overlay.name.is_empty() {
                                        "(unnamed)".to_string()
                                    } else {
                                        overlay.name.clone()
                                    },
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
                                Some(UI_SELECTION)
                            } else if row_response.hovered() {
                                Some(UI_SURFACE_ALT)
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
                                    .color(UI_TEXT_SECONDARY),
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
                                Some(UI_SELECTION)
                            } else if row_response.hovered() {
                                Some(UI_SURFACE_ALT)
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
                RichText::new("Select a profile, or create a new one.").color(UI_TEXT_SECONDARY),
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
                    .color(UI_TEXT),
                );
                ui.add_space(2.0);
                // Names may repeat, so the file is what identifies the profile.
                ui.label(
                    RichText::new(config::profile_file_name(&entry.id))
                        .small()
                        .color(UI_TEXT_SECONDARY),
                );
                // Nothing is said when the count is unknown, rather than a zero
                // that would be a confident lie.
                let count_label = overlay_count_label(count);
                if !count_label.is_empty() {
                    ui.label(RichText::new(count_label).small().color(UI_TEXT_SECONDARY));
                }
                ui.add_space(8.0);

                if is_active {
                    ui.label(RichText::new("This is the active profile.").color(UI_TEXT_SECONDARY));
                } else {
                    let mut switch_clicked = false;
                    ui.add_enabled_ui(can_switch_profile(dirty), |ui| {
                        switch_clicked = ui
                            .add_sized(
                                [PROFILE_ACTION_WIDTH, 32.0],
                                egui::Button::new(
                                    RichText::new("Switch to this profile").color(UI_TEXT),
                                )
                                .fill(UI_ACCENT_STRONG),
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
                        .color(UI_TEXT_SECONDARY),
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
                            egui::Button::new(RichText::new("Delete").color(UI_DANGER))
                                .fill(UI_SURFACE_ALT),
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
            UI_SELECTION
        } else if response.hovered() {
            UI_SURFACE_HOVER
        } else {
            UI_SURFACE_ALT
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
            egui::TextFormat::simple(egui::TextStyle::Button.resolve(ui.style()), UI_TEXT),
        );
        let galley = ui.painter().layout_job(text);
        let text_offset = (rect.height() - galley.size().y) / 2.0;
        ui.painter().galley(
            rect.left_top() + egui::vec2(SWITCHER_TEXT_LEFT_PAD, text_offset),
            galley,
            UI_TEXT,
        );
        draw_dropdown_arrow(ui.painter(), arrow, UI_TEXT_SECONDARY);
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
                let color = if active { UI_TEXT } else { UI_TEXT_SECONDARY };
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(row_width, RAIL_ROW_HEIGHT),
                    egui::Sense::click(),
                );
                if active {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SELECTION);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SURFACE_ALT);
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
                    if active { UI_ACCENT } else { UI_TEXT_SECONDARY },
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
                        draw_active_dot(ui.painter(), row_corner_dot_slot(rect), UI_ACCENT);
                    }
                    hint.push_str(" â€” unsaved changes");
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
                        .rect_filled(rect, egui::CornerRadius::same(4), UI_SURFACE_ALT);
                }
                let icon_rect =
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(RAIL_ICON_SIZE));
                draw_collapse_chevron(ui.painter(), icon_rect, collapsed, UI_TEXT_SECONDARY);
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
                ui.label(RichText::new("Global").heading().color(UI_TEXT));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("These settings apply to the whole app, not to a profile.")
                        .color(UI_TEXT_SECONDARY),
                );

                ui.add_space(12.0);
                ui.label(RichText::new("APPEARANCE").color(UI_ACCENT));
                ui.separator();
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

                ui.add_space(12.0);
                ui.label(RichText::new("STORAGE").color(UI_ACCENT));
                ui.separator();
                for (label, path) in [
                    (
                        "Config folder",
                        config::config_dir().to_string_lossy().to_string(),
                    ),
                    (
                        "Profiles",
                        config::profiles_dir().to_string_lossy().to_string(),
                    ),
                    (
                        "Global config",
                        config::global_config_path().to_string_lossy().to_string(),
                    ),
                ] {
                    ui.horizontal(|ui| {
                        let label_width = 96.0;
                        let gap = ui.spacing().item_spacing.x;
                        let path_width =
                            (ui.available_width() - label_width - gap - STORAGE_PATH_MIN).max(60.0);
                        ui.add_sized(
                            [label_width, 26.0],
                            egui::Label::new(RichText::new(label).color(UI_TEXT_SECONDARY)),
                        );
                        ui.add_sized(
                            [path_width, 26.0],
                            egui::Label::new(RichText::new(&path).color(UI_TEXT)).truncate(),
                        )
                        .on_hover_text(path.clone());
                    });
                }
            });
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
                about_page_column(ui);
            });
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
            .frame(Frame::popup(ui.style()).fill(UI_SURFACE))
            .show(|ui| {
                ui.set_width(PROFILE_POPUP_WIDTH);
                let header = ui.label(RichText::new("Profiles").strong().color(UI_TEXT));
                header.on_hover_text(config::profiles_dir().display().to_string());
                ui.separator();

                for profile in &profiles {
                    let current = profile.id == active;
                    // Pending edits dim every other profile, because
                    // switching is refused until the draft is saved.
                    let color = if current {
                        UI_ACCENT
                    } else if dirty {
                        UI_TEXT_SECONDARY
                    } else {
                        UI_TEXT
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
                            .color(UI_TEXT_SECONDARY),
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
                ui.label(RichText::new("New profile").heading().color(UI_TEXT));
                ui.label(
                    RichText::new("A new profile starts empty and gets its own file.")
                        .small()
                        .color(UI_TEXT_SECONDARY),
                );
                let (submit, cancel) = profile_name_field(ui, name, "Create", focus_field);
                if submit {
                    action = Some(ProfileAction::Create(name.clone()));
                }
                close_dialog = cancel;
            }
            Some(ProfileDialog::Rename { from, name }) => {
                ui.add_space(4.0);
                ui.label(RichText::new("Rename profile").heading().color(UI_TEXT));
                // Names may repeat, so the file is shown: a taken name gives the
                // new profile a postfixed file instead of an error.
                ui.label(
                    RichText::new(format!(
                        "{} will be written as a new file if the name is taken.",
                        config::profile_file_name(from)
                    ))
                    .small()
                    .color(UI_TEXT_SECONDARY),
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
                ui.label(RichText::new("Duplicate profile").heading().color(UI_TEXT));
                ui.label(
                    RichText::new(format!(
                        "The overlays of {} are copied into a new profile.",
                        config::profile_file_name(from)
                    ))
                    .small()
                    .color(UI_TEXT_SECONDARY),
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
                        .color(UI_DANGER),
                );
                // Names may repeat, so the file confirms what goes.
                ui.label(
                    RichText::new(config::profile_file_name(id))
                        .small()
                        .color(UI_TEXT_SECONDARY),
                );
                if id == &self.active_profile {
                    ui.label(
                        RichText::new("This is the active profile, so another one is loaded next.")
                            .small()
                            .color(UI_TEXT_SECONDARY),
                    );
                }
                let mut confirm = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    confirm = ui
                        .add_sized(
                            [96.0, DETAIL_FOOTER_BUTTON_HEIGHT],
                            egui::Button::new(RichText::new("Delete").color(UI_TEXT))
                                .fill(UI_DANGER_STRONG),
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
                    .color(UI_TEXT),
                );
                ui.add_space(8.0);
                let mut changed = false;
                edit_overlay(
                    ui,
                    &mut self.config.overlays[index],
                    &mut changed,
                    &self.position_picker,
                );
                if changed {
                    self.dirty = true;
                    self.status.clear();
                }
            });
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
                                egui::Button::new(RichText::new("Save").color(UI_TEXT))
                                    .fill(UI_ACCENT_STRONG),
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
            .fill(UI_BACKGROUND)
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

/// Whether either draft has something to write.
///
/// There are two independent drafts: the profile draft (`dirty`) and the
/// app-wide preferences draft (`prefs_dirty`). A page stages into one of them,
/// so anything asking "is there anything to save?" has to ask about both. Free
/// function so a test can drive the rule rather than assert a predicate nothing
/// acts on, which is how this shipped broken.
fn pending_edits(dirty: bool, prefs_dirty: bool) -> bool {
    dirty || prefs_dirty
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
        egui::Stroke::new(1.0, UI_BORDER),
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
        let label = ui.label(RichText::new(format!("Delete \"{name}\"?")).color(UI_DANGER));
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
                egui::Button::new(RichText::new("OK").color(UI_TEXT)).fill(UI_DANGER_STRONG),
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
    let indicator_color = if active { UI_ACCENT } else { UI_ACCENT_STRONG };
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
    tab_text.append(name, 0.0, egui::TextFormat::simple(font_id, UI_TEXT));
    let galley = ui.painter().layout_job(tab_text);
    let text_offset = (text_rect.height() - galley.size().y) / 2.0;
    ui.painter().galley(
        text_rect.left_top() + egui::vec2(0.0, text_offset),
        galley,
        UI_TEXT,
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
            egui::Button::new(RichText::new("X").color(UI_DANGER)),
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
        UI_ACCENT
    } else if dirty {
        UI_TEXT_SECONDARY
    } else {
        UI_TEXT
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
            UI_TEXT_SECONDARY,
        );
    }
    if is_active {
        // A dot, because the accent name alone is a weak cue in a long list.
        draw_active_dot(painter, dot_slot, UI_ACCENT);
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
        ui.label(RichText::new(empty_editor_message(ui.style())).color(UI_TEXT_SECONDARY));
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
fn read_profile_snapshot() -> ProfileSnapshot {
    ProfileSnapshot {
        profiles: config::list_profiles_detailed(),
        counts: config::profile_overlay_counts(),
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
/// Three things have to line up, and the page was the one that was missing: the
/// Config window has to be open, the Overlays page has to be the one on screen,
/// and an overlay has to be selected. Leaving the Overlays page used to leave
/// the last overlay's border animating, because this only asked whether the
/// window was visible.
///
/// Unlike the profile-count trigger in `sync_profile_cache`, a table over this
/// function is the whole mechanism rather than a predicate standing in for one:
/// `sync_overlays` calls it every frame and hands the answer straight to
/// `overlays.apply`, so nothing has to act on the result for the test to mean
/// anything.
fn selected_overlay_for_border(
    config_visible: bool,
    page: Page,
    selected_id: Option<&str>,
) -> Option<&str> {
    if config_visible && page == Page::Overlays {
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

impl App for PingApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let was_shutting_down = self.shutdown_state != ShutdownState::Running;
        self.process_tray_events(ctx);

        if self.shutdown_state != ShutdownState::Running {
            if was_shutting_down && self.shutdown_state == ShutdownState::HideRequested {
                self.shutdown_state = ShutdownState::CloseRequested;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                // Close is delivered as a viewport event, so request the
                // following logic-only pass to observe it.
                ctx.request_repaint();
            }
            return;
        }

        self.sync_overlays();
        self.sync_profiles();
        self.sync_window_title(ctx);
        ctx.request_repaint_after(self.repaint_interval());
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.handle_root_close(ui.ctx());
        if self.shutdown_state != ShutdownState::Running {
            return;
        }
        self.config_ui(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        UI_BACKGROUND.to_normalized_gamma_f32()
    }
}

impl Drop for PingApp {
    fn drop(&mut self) {
        self.probes.stop_all();
        if let Some(runtime) = self._runtime.take() {
            // Do not make the tray Exit action wait for an in-flight
            // spawn_blocking ICMP/DNS operation. Its worker is detached from
            // process shutdown and cannot hold up the tray application.
            runtime.shutdown_background();
        }
    }
}

fn edit_overlay(
    ui: &mut Ui,
    overlay: &mut OverlayConfig,
    changed: &mut bool,
    position_picker: &PositionPicker,
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

                ui.label("Protocol");
                let mut protocol = match overlay.probe {
                    ProbeConfig::Icmp { .. } => "icmp",
                    ProbeConfig::Tcp { .. } => "tcp",
                };
                ComboBox::from_id_salt("protocol")
                    .selected_text(if protocol == "icmp" {
                        "ICMP echo"
                    } else {
                        "TCP connect"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut protocol, "icmp", "ICMP echo");
                        ui.selectable_value(&mut protocol, "tcp", "TCP connect");
                    });
                if protocol == "tcp" && matches!(overlay.probe, ProbeConfig::Icmp { .. }) {
                    let host = overlay.probe.host().to_string();
                    overlay.probe = ProbeConfig::Tcp { host, port: 80 };
                    *changed = true;
                } else if protocol == "icmp" && matches!(overlay.probe, ProbeConfig::Tcp { .. }) {
                    let host = overlay.probe.host().to_string();
                    overlay.probe = ProbeConfig::Icmp { host };
                    *changed = true;
                }
                ui.end_row();

                ui.label("Target host / IP");
                let mut host = overlay.probe.host().to_string();
                if ui
                    .add(egui::TextEdit::singleline(&mut host).desired_width(f32::INFINITY))
                    .changed()
                {
                    set_probe_host(&mut overlay.probe, host);
                    *changed = true;
                }
                ui.end_row();

                ui.label("Port (TCP only)");
                let mut port = overlay.probe.port().max(1) as i64;
                if matches!(overlay.probe, ProbeConfig::Icmp { .. }) {
                    ui.add_enabled(false, egui::DragValue::new(&mut port).range(1..=65535));
                } else if ui
                    .add(egui::DragValue::new(&mut port).range(1..=65535))
                    .changed()
                {
                    if let ProbeConfig::Tcp { port: value, .. } = &mut overlay.probe {
                        *value = port.clamp(1, 65535) as u16;
                    }
                    *changed = true;
                }
                ui.end_row();

                ui.label("Timeout (ms)");
                let mut timeout = overlay.timeout_ms.max(1) as i64;
                if ui
                    .add(egui::DragValue::new(&mut timeout).range(1..=600_000))
                    .changed()
                {
                    overlay.timeout_ms = timeout.clamp(1, 600_000) as u32;
                    *changed = true;
                }
                ui.end_row();
            });
    });

    section(ui, "Position", |ui| {
        if let Some(anchor) = position_picker.show(ui, overlay.position) {
            if anchor != overlay.position {
                overlay.position = anchor;
                *changed = true;
            }
        }
        Grid::new("position-offsets-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Horizontal margin (px)").on_hover_text(
                    "Positive shifts right on centered anchors or inward from a left/right edge; negative shifts the opposite way.",
                );
                let mut horizontal_margin = overlay.horizontal_margin_px as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut horizontal_margin).range(
                            config::MIN_MARGIN_OFFSET_PX as i64
                                ..=config::MAX_MARGIN_OFFSET_PX as i64,
                        ),
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

                ui.label("Vertical margin (px)").on_hover_text(
                    "Positive shifts down on centered anchors or inward from a top/bottom edge; negative shifts the opposite way.",
                );
                let mut vertical_margin = overlay.vertical_margin_px as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut vertical_margin).range(
                            config::MIN_MARGIN_OFFSET_PX as i64
                                ..=config::MAX_MARGIN_OFFSET_PX as i64,
                        ),
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

    section(ui, "Graph", |ui| {
        Grid::new("graph-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Sampling (sec)");
                let mut window_seconds = overlay.window_seconds as i64;
                if ui
                    .add(egui::DragValue::new(&mut window_seconds).range(30..=86_400))
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
                    .add(egui::DragValue::new(&mut graph_height).range(10..=10_000))
                    .changed()
                {
                    overlay.graph_height_px = graph_height.clamp(10, 10_000) as u32;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Latency Ceiling");
                let mut max_y = overlay.max_y_ms.max(1) as i64;
                if ui
                    .add(egui::DragValue::new(&mut max_y).range(1..=1_000_000))
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
                color_field(ui, "Line color", &mut overlay.line_color, changed);
                ui.end_row();
                color_field(ui, "Timeout color", &mut overlay.timeout_color, changed);
                ui.end_row();
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
            });
    });

    section(ui, "Startup Behaviors", |ui| {
        Grid::new("startup-behaviors-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Cosmetic Startup Prefill")
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

                ui.label("Prefill animation (sec)");
                let mut prefill_animation = overlay.prefill_animation_sec as i64;
                if ui
                    .add_enabled(
                        overlay.cosmetic_startup_prefill,
                        egui::DragValue::new(&mut prefill_animation)
                            .range(
                                config::MIN_PREFILL_ANIMATION_SEC as i64
                                    ..=config::MAX_PREFILL_ANIMATION_SEC as i64,
                            )
                            .suffix(" sec"),
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
                            "RGB Noise",
                        );
                        ui.selectable_value(&mut border_effect, BorderEffect::Disabled, "Disabled");
                    });
                if border_effect != overlay.startup_border_effect {
                    overlay.startup_border_effect = border_effect;
                    *changed = true;
                }
                ui.end_row();

                ui.label("Border animation (sec)");
                let mut border_animation = overlay.border_animation_sec as i64;
                if ui
                    .add_enabled(
                        overlay.startup_border_effect != BorderEffect::Disabled,
                        egui::DragValue::new(&mut border_animation)
                            .range(
                                config::MIN_BORDER_ANIMATION_SEC as i64
                                    ..=config::MAX_BORDER_ANIMATION_SEC as i64,
                            )
                            .suffix(" sec"),
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

                ui.label("Border fade out (sec)");
                let mut border_fade = overlay.border_fade_sec as i64;
                if ui
                    .add_enabled(
                        overlay.startup_border_effect != BorderEffect::Disabled,
                        egui::DragValue::new(&mut border_fade)
                            .range(0..=config::MAX_BORDER_FADE_SEC as i64)
                            .suffix(" sec"),
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
            .color(UI_ACCENT),
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
        BorderEffect::RgbNoise => "RGB Noise",
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
    crate::overlay::enable_dpi_awareness();
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id("ping-latency-overlay")
            .with_title("PingLatencyOverlay - Config")
            .with_inner_size(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT))
            .with_min_inner_size(egui::vec2(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT))
            .with_max_inner_size(egui::vec2(WINDOW_MAX_WIDTH, WINDOW_MAX_HEIGHT))
            .with_resizable(true)
            .with_visible(false)
            .with_icon(tray::app_icon()),
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
        about_page_lines, app_version, can_switch_profile, config_notice_status,
        config_notices_status, deselect_strip_rect, draw_pane_divider, empty_editor,
        list_pane_column, list_pane_row_height, list_pane_row_width_for, overlay_count_label,
        overlay_name_width, overlay_row_contents, page_has_detail_footer, page_has_list_pane,
        pending_edits, profile_name_width, profile_row_contents, profile_row_label, rail_width,
        row_inner, selected_overlay_for_border, sync_profile_cache, toggled_selection,
        ui_text_size, window_title, AboutKind, Frame, Page, ProfileSnapshot, ABOUT_AUTHOR_URL,
        ABOUT_ICON_DOT_RADIUS, ABOUT_ICON_ROWS, ABOUT_LICENCE_URL, ABOUT_REPOSITORY,
        DETAIL_FOOTER_BUTTON_HEIGHT, DETAIL_FOOTER_BUTTON_WIDTH, DETAIL_FOOTER_HEIGHT,
        GLOBAL_ICON_KNOB_RADIUS, GLOBAL_ICON_ROWS, GLOBAL_ICON_TRACK_HALF, LIST_PANE_INSET,
        OVERLAY_ROW_HEIGHT, PAGES, PANE_GAP, PANE_MARGIN, PROFILE_ROW_HEIGHT, PROFILE_ROW_TRAILING,
        RAIL_ROW_HEIGHT, RAIL_WIDTH, ROW_MARGIN, SCROLL_BAR_RESERVE, SIDEBAR_WIDTH,
        STATUS_BAR_HEIGHT, UI_BACKGROUND, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH,
    };
    use crate::config::{Anchor, ConfigNotice, ProfileEntry};
    use eframe::egui;
    use std::collections::HashMap;

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
                .fill(UI_BACKGROUND)
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

    /// Save and Discard follow *either* draft.
    ///
    /// The profile draft and the preferences draft are separate flags and pages
    /// stage into one or the other, so gating the footer on the profile flag
    /// alone left every Global preference unsaveable. The rail-collapse
    /// preference has been in that state since it shipped.
    #[test]
    fn the_footer_follows_either_draft() {
        let cases = [
            (false, false, false),
            (true, false, true),
            (false, true, true),
            (true, true, true),
        ];
        for (dirty, prefs_dirty, expected) in cases {
            assert_eq!(
                pending_edits(dirty, prefs_dirty),
                expected,
                "with the profile draft {dirty} and the preferences draft {prefs_dirty} \
                 Save and Discard should be {}",
                if expected { "enabled" } else { "disabled" }
            );
        }
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

    /// Every clickable About line opens a real web address.
    ///
    /// The display text drops the `https://` prefix, so a typo in a URL would
    /// not be visible on the page at all; only the click would fail, and then in
    /// the user's browser rather than in this app. Checking the constants here
    /// catches that, and also catches a line quietly losing its link.
    #[test]
    fn about_page_links_open_a_web_address() {
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
            vec![ABOUT_REPOSITORY, ABOUT_AUTHOR_URL, ABOUT_LICENCE_URL],
            "the About page's links changed, so check the text says what each one opens"
        );
        for url in linked {
            assert!(
                url.starts_with("https://"),
                "{url} is shown on the About page and would not open"
            );
        }
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
    /// the strip cannot grow the content. The second is the one that matters â€”
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

    /// The border preview needs the window open, the Overlays page showing and
    /// something selected. The page went missing for a release, so all three are
    /// pinned here rather than only the window.
    #[test]
    fn the_border_preview_only_runs_on_the_overlays_page() {
        let cases = [
            (false, Page::Overlays, Some("overlay"), None),
            (false, Page::Profiles, Some("overlay"), None),
            (true, Page::Overlays, Some("overlay"), Some("overlay")),
            (true, Page::Profiles, Some("overlay"), None),
            (true, Page::Global, Some("overlay"), None),
            (true, Page::Overlays, None, None),
            (true, Page::Profiles, None, None),
            (true, Page::Global, None, None),
        ];
        for (visible, page, selected, expected) in cases {
            assert_eq!(
                selected_overlay_for_border(visible, page, selected),
                expected,
                "with the window {} on {} and {:?} selected the border preview should be {:?}",
                if visible { "open" } else { "closed" },
                page.label(),
                selected,
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
}
