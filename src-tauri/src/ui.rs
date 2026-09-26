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
const SIDEBAR_CONTROLS_HEIGHT: f32 = 188.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const PROFILE_ROW_HEIGHT: f32 = 26.0;
const PROFILE_ICON_SIZE: f32 = 24.0;
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

/// Inline editor or confirmation shown at the bottom of the profile popup.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProfileDialog {
    Create { name: String },
    Rename { from: String, name: String },
    Delete { name: String },
}

/// A profile request raised by the popup, applied after it closes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProfileAction {
    Switch(String),
    Create(String),
    Rename { from: String, to: String },
    Delete(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileIcon {
    Rename,
    Delete,
}

pub struct PingApp {
    config: Config,
    active_profile: String,
    profiles: Vec<String>,
    profile_menu_open: bool,
    profile_dialog: Option<ProfileDialog>,
    profile_name_focus: bool,
    selected_id: Option<String>,
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
        let profiles = loaded.profiles;
        let selected_id = config.overlays.first().map(|overlay| overlay.id.clone());
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
        cc.egui_ctx.request_repaint();

        Ok(Self {
            config,
            active_profile,
            profiles,
            profile_menu_open: false,
            profile_dialog: None,
            profile_name_focus: false,
            selected_id,
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
        })
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
        let selected_id =
            selected_overlay_for_border(self.config_visible, self.selected_id.as_deref());
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

    fn persist_current(&mut self) -> bool {
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

    fn save_edits(&mut self) {
        if self.persist_current() {
            self.status = "Saved.".to_string();
        }
    }

    /// Drop unsaved edits by reloading the active profile from disk.
    fn discard_edits(&mut self) {
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

    fn refresh_profiles(&mut self) {
        self.profiles = config::list_profiles();
    }

    /// Make another profile the active one and remember the choice.
    fn switch_profile(&mut self, name: &str) {
        self.profile_menu_open = false;
        if name == self.active_profile {
            return;
        }
        if !can_switch_profile(self.dirty) {
            self.status = "Save or discard your changes before switching profiles.".to_string();
            return;
        }
        match config::load_profile(name) {
            Ok(mut stored) => {
                stored.normalize();
                self.active_profile = name.to_string();
                if let Err(error) = config::set_active_profile(&self.active_profile) {
                    self.status = format!("Could not update globalconfig.json: {error}");
                }
                self.selected_id = stored.overlays.first().map(|overlay| overlay.id.clone());
                self.apply_saved_config(stored);
                self.dirty = false;
                self.status = format!("Loaded profile \"{name}\".");
            }
            Err(error) => {
                self.status = format!("Could not load profile \"{name}\": {error}");
            }
        }
        self.refresh_profiles();
    }

    /// Create a new empty profile, then load it. Returns false when the name
    /// was rejected.
    fn create_profile(&mut self, name: &str) -> bool {
        match config::create_profile(name) {
            Ok(slug) => {
                self.refresh_profiles();
                if can_switch_profile(self.dirty) {
                    self.switch_profile(&slug);
                } else {
                    self.status = format!(
                        "Created profile \"{slug}\". Save or discard your changes to load it."
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
    fn rename_profile(&mut self, from: &str, to: &str) -> bool {
        if to.trim() == from {
            return true;
        }
        match config::rename_profile(from, to) {
            Ok(slug) => {
                if self.active_profile == from {
                    self.active_profile = slug.clone();
                    if let Err(error) = config::set_active_profile(&self.active_profile) {
                        self.status = format!("Could not update globalconfig.json: {error}");
                    }
                }
                self.refresh_profiles();
                self.status = format!("Renamed profile \"{from}\" to \"{slug}\".");
                true
            }
            Err(error) => {
                self.status = format!("Could not rename the profile: {error}");
                false
            }
        }
    }

    fn delete_profile(&mut self, name: &str) {
        if self.profiles.len() <= 1 {
            self.status = "A profile cannot be deleted while it is the only one.".to_string();
            return;
        }
        if name == self.active_profile && !can_switch_profile(self.dirty) {
            self.status =
                "Save or discard your changes before deleting the active profile.".to_string();
            return;
        }
        if let Err(error) = config::delete_profile(name) {
            self.status = format!("Could not delete profile \"{name}\": {error}");
            return;
        }
        self.refresh_profiles();
        if name == self.active_profile {
            // Prefer the default profile, otherwise fall back to whatever is
            // left so the app always has a configuration.
            let fallback = if self
                .profiles
                .iter()
                .any(|profile| profile == config::DEFAULT_PROFILE)
            {
                config::DEFAULT_PROFILE.to_string()
            } else {
                self.profiles
                    .first()
                    .cloned()
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

    fn show_sidebar(&mut self, ui: &mut Ui) {
        let available = ui.available_size();
        let list_height = (available.y - SIDEBAR_CONTROLS_HEIGHT).max(100.0);
        let row_width = SIDEBAR_WIDTH - 20.0;

        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.allocate_ui(egui::vec2(row_width, list_height), |ui| {
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
                            let background = if active { UI_SELECTION } else { UI_SURFACE_ALT };
                            egui::Frame::group(ui.style())
                                .fill(background)
                                .inner_margin(egui::Margin::same(4))
                                .show(ui, |ui| {
                                    if self.confirm_delete.as_deref() == Some(id.as_str()) {
                                        ui.horizontal(|ui| {
                                            let label_response = ui.label(
                                                RichText::new(format!("Delete \"{name}\"?"))
                                                    .color(UI_DANGER),
                                            );
                                            let controls_width = 64.0;
                                            let gaps = ui.spacing().item_spacing.x * 3.0;
                                            let remaining_width = (row_width
                                                - label_response.rect.width()
                                                - controls_width
                                                - gaps)
                                                .max(0.0);
                                            let cancel_area = ui.allocate_response(
                                                egui::vec2(remaining_width, 26.0),
                                                egui::Sense::click(),
                                            );
                                            let ok_response = ui.add_sized(
                                                [36.0, 26.0],
                                                egui::Button::new(
                                                    RichText::new("OK").color(UI_TEXT),
                                                )
                                                .fill(UI_DANGER_STRONG),
                                            );
                                            let cancel_response =
                                                ui.add_sized([28.0, 26.0], egui::Button::new("X"));
                                            if ok_response.clicked() {
                                                self.confirm_delete = None;
                                                self.delete_overlay(&id);
                                            } else if label_response.clicked()
                                                || cancel_area.clicked()
                                                || cancel_response.clicked()
                                            {
                                                self.confirm_delete = None;
                                            }
                                        });
                                    } else {
                                        ui.horizontal(|ui| {
                                            let (tab_rect, mut tab_response) = ui
                                                .allocate_exact_size(
                                                    egui::vec2(row_width - 80.0, 28.0),
                                                    egui::Sense::click(),
                                                );
                                            let indicator_size = 22.0;
                                            let indicator_rect = egui::Rect::from_min_size(
                                                tab_rect.left_top(),
                                                egui::vec2(indicator_size, tab_rect.height()),
                                            );
                                            // Reuse the existing blue palette: muted
                                            // default, bright active selection.
                                            let indicator_color =
                                                if active { UI_ACCENT } else { UI_ACCENT_STRONG };
                                            draw_position_indicator(
                                                ui.painter(),
                                                indicator_rect,
                                                indicator,
                                                indicator_color,
                                            );
                                            let text_rect = egui::Rect::from_min_max(
                                                egui::pos2(
                                                    indicator_rect.right() + 6.0,
                                                    tab_rect.top(),
                                                ),
                                                tab_rect.right_bottom(),
                                            );
                                            let font_id =
                                                egui::TextStyle::Button.resolve(ui.style());
                                            let mut tab_text = egui::text::LayoutJob::default();
                                            tab_text.wrap.max_width = text_rect.width();
                                            tab_text.wrap.max_rows = 1;
                                            tab_text.wrap.break_anywhere = true;
                                            tab_text.append(
                                                &name,
                                                0.0,
                                                egui::TextFormat::simple(font_id, UI_TEXT),
                                            );
                                            let galley = ui.painter().layout_job(tab_text);
                                            let text_offset =
                                                (text_rect.height() - galley.size().y) / 2.0;
                                            ui.painter().galley(
                                                text_rect.left_top() + egui::vec2(0.0, text_offset),
                                                galley,
                                                UI_TEXT,
                                            );
                                            if tab_response.hovered() {
                                                tab_response = tab_response
                                                    .on_hover_text(position_name(indicator));
                                            }
                                            if tab_response.clicked() {
                                                self.selected_id = Some(id.clone());
                                            }
                                            if ui
                                                .add_sized(
                                                    [28.0, 28.0],
                                                    egui::Button::new(
                                                        RichText::new("X").color(UI_DANGER),
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                self.confirm_delete = Some(id.clone());
                                            }
                                            if ui
                                                .add_sized(
                                                    [28.0, 28.0],
                                                    egui::Button::new(if enabled {
                                                        "||"
                                                    } else {
                                                        ">"
                                                    }),
                                                )
                                                .clicked()
                                            {
                                                self.toggle_overlay(&id);
                                            }
                                        });
                                    }
                                });
                        }
                    });
            });

            ui.add_space(6.0);
            let profile_label = format!("Profile: {}", self.active_profile);
            let profile_button = ui.add_sized(
                [row_width, 32.0],
                egui::Button::new(RichText::new(profile_label).color(UI_TEXT)),
            );
            if profile_button.clicked() {
                self.profile_menu_open = !self.profile_menu_open;
                if self.profile_menu_open {
                    self.refresh_profiles();
                }
            }
            self.show_profile_popup(ui, &profile_button);
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
            let mut discard_clicked = false;
            ui.add_enabled_ui(self.dirty, |ui| {
                discard_clicked = ui
                    .add_sized([row_width, 34.0], egui::Button::new("Discard"))
                    .clicked();
            });
            if discard_clicked {
                self.discard_edits();
            }
            let save_button = egui::Button::new("Save")
                .fill(UI_ACCENT_STRONG)
                .min_size(egui::vec2(row_width, 34.0));
            let mut save_clicked = false;
            ui.add_enabled_ui(self.dirty, |ui| {
                save_clicked = ui.add_sized([row_width, 34.0], save_button).clicked();
            });
            if save_clicked {
                self.save_edits();
            }
        });
    }

    /// Profile picker shown under the profile button. Create, rename and
    /// delete live inside the menu so the sidebar keeps a single control.
    fn show_profile_popup(&mut self, ui: &mut Ui, anchor: &egui::Response) {
        let active = self.active_profile.clone();
        let profiles = self.profiles.clone();
        let dirty = self.dirty;
        let mut focus_field = std::mem::take(&mut self.profile_name_focus);
        // The dialog is taken out for the frame so the popup can edit it
        // without borrowing the app state, then the edited value is stored
        // back. Snapshotting it here would discard whatever was typed.
        let mut dialog = self.profile_dialog.take();
        let mut action: Option<ProfileAction> = None;

        // egui owns the open flag through `open_bool`, so it can close the popup
        // on a click outside or Escape without treating the click that opened
        // it as an outside click.
        let was_open = self.profile_menu_open;
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

                for name in &profiles {
                    ui.horizontal(|ui| {
                        let current = *name == active;
                        // Pending edits dim every other profile, because
                        // switching is refused until the draft is saved.
                        let color = if current {
                            UI_ACCENT
                        } else if dirty {
                            UI_TEXT_SECONDARY
                        } else {
                            UI_TEXT
                        };
                        // Name, rename and delete: three widgets, so two
                        // `item_spacing` gaps must be reserved. Reserving one
                        // gap made every row ask for more than the popup width.
                        let icons = PROFILE_ICON_SIZE * 2.0 + ui.spacing().item_spacing.x * 2.0;
                        let name_width = (ui.available_width() - icons).max(60.0);
                        let name_button = ui.add_sized(
                            [name_width, PROFILE_ROW_HEIGHT],
                            egui::Button::new(RichText::new(name.as_str()).color(color)),
                        );
                        if name_button.clicked() {
                            action = Some(ProfileAction::Switch(name.clone()));
                        }
                        name_button.on_hover_text(config::profile_file_name(name));
                        let rename =
                            profile_icon_button(ui, ProfileIcon::Rename, UI_TEXT_SECONDARY);
                        if rename.clicked() {
                            dialog = Some(ProfileDialog::Rename {
                                from: name.clone(),
                                name: name.clone(),
                            });
                            focus_field = true;
                        }
                        let delete = profile_icon_button(ui, ProfileIcon::Delete, UI_DANGER);
                        if delete.clicked() {
                            dialog = Some(ProfileDialog::Delete { name: name.clone() });
                        }
                    });
                }

                ui.separator();
                // The inline editor replaces the "+ New profile" row while a
                // dialog is open.
                let wants_new = match &dialog {
                    None => ui
                        .add_sized(
                            [ui.available_width(), PROFILE_ROW_HEIGHT],
                            egui::Button::new("+ New profile"),
                        )
                        .clicked(),
                    Some(_) => false,
                };
                // Assigned after the match, which borrows the dialog.
                let mut close_dialog = false;
                match &mut dialog {
                    Some(ProfileDialog::Create { name }) => {
                        ui.label(RichText::new("New profile").color(UI_TEXT));
                        let (submit, cancel) = profile_name_field(ui, name, "Create", focus_field);
                        if submit {
                            action = Some(ProfileAction::Create(name.clone()));
                        }
                        close_dialog = cancel;
                    }
                    Some(ProfileDialog::Rename { from, name }) => {
                        ui.label(RichText::new(format!("Rename \"{from}\"")).color(UI_TEXT));
                        let (submit, cancel) = profile_name_field(ui, name, "Rename", focus_field);
                        if submit {
                            action = Some(ProfileAction::Rename {
                                from: from.clone(),
                                to: name.clone(),
                            });
                        }
                        close_dialog = cancel;
                    }
                    Some(ProfileDialog::Delete { name }) => {
                        ui.label(RichText::new(format!("Delete \"{name}\"?")).color(UI_DANGER));
                        let mut confirm = false;
                        let mut cancel = false;
                        ui.horizontal(|ui| {
                            confirm = ui
                                .add_sized(
                                    [70.0, PROFILE_ROW_HEIGHT],
                                    egui::Button::new(RichText::new("Delete").color(UI_TEXT))
                                        .fill(UI_DANGER_STRONG),
                                )
                                .clicked();
                            cancel = ui
                                .add_sized([60.0, PROFILE_ROW_HEIGHT], egui::Button::new("Cancel"))
                                .clicked();
                        });
                        if confirm {
                            action = Some(ProfileAction::Delete(name.clone()));
                        }
                        close_dialog = cancel;
                    }
                    None => {}
                }
                if wants_new {
                    dialog = Some(ProfileDialog::Create {
                        name: String::new(),
                    });
                    focus_field = true;
                }
                if close_dialog {
                    dialog = None;
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

        // Closing the popup drops any inline editor it was showing.
        if was_open && !self.profile_menu_open {
            dialog = None;
            focus_field = false;
        }
        self.profile_name_focus = focus_field;
        self.profile_dialog = dialog;

        if let Some(action) = action {
            // A rejected name keeps the editor open so it can be corrected.
            if self.apply_profile_action(action) {
                self.profile_dialog = None;
            }
        }
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
            ProfileAction::Delete(name) => {
                self.delete_profile(&name);
                true
            }
        }
    }

    fn show_editor(&mut self, ui: &mut Ui) {
        let Some(selected_id) = self.selected_id.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label("Select an overlay, or add a new one.");
            });
            return;
        };
        let Some(index) = self
            .config
            .overlays
            .iter()
            .position(|overlay| overlay.id == selected_id)
        else {
            self.selected_id = self
                .config
                .overlays
                .first()
                .map(|overlay| overlay.id.clone());
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

    fn config_ui(&mut self, ui: &mut Ui) {
        let frame = Frame::central_panel(ui.style())
            .fill(UI_BACKGROUND)
            .inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 12,
                bottom: 0,
            });
        frame.show(ui, |ui| {
            ui.vertical(|ui| {
                let content_height = (ui.available_height() - STATUS_BAR_HEIGHT).max(160.0);
                ui.horizontal_top(|ui| {
                    ui.set_height(content_height);
                    ui.allocate_ui(egui::vec2(SIDEBAR_WIDTH, content_height), |ui| {
                        self.show_sidebar(ui);
                    });
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.set_height(content_height);
                        self.show_editor(ui);
                    });
                });
                self.show_status_bar(ui);
            });
        });
    }

    fn show_status_bar(&mut self, ui: &mut Ui) {
        let message = self.status.as_str();
        let version = format!("v{}", env!("APP_BUILD_VERSION"));
        ui.allocate_ui(egui::vec2(ui.available_width(), STATUS_BAR_HEIGHT), |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(egui::Label::new(message).truncate());
                if !message.is_empty() {
                    response.on_hover_text(message);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(version).color(UI_TEXT_SECONDARY));
                });
            });
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

/// A square button with a painted, font-independent profile icon.
fn profile_icon_button(ui: &mut Ui, icon: ProfileIcon, color: Color32) -> egui::Response {
    let response = ui.add_sized(
        [PROFILE_ICON_SIZE, PROFILE_ICON_SIZE],
        egui::Button::new(RichText::new("")),
    );
    draw_profile_icon(ui.painter(), response.rect, icon, color);
    response
}

/// Vector icons for the profile menu so they never depend on font glyphs.
fn draw_profile_icon(painter: &egui::Painter, rect: egui::Rect, icon: ProfileIcon, color: Color32) {
    let center = rect.center();
    let stroke = egui::Stroke::new(1.4, color);
    let arm = rect.width().min(rect.height()) * 0.24;
    match icon {
        ProfileIcon::Delete => {
            painter.line_segment(
                [
                    center + egui::vec2(-arm, -arm),
                    center + egui::vec2(arm, arm),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(arm, -arm),
                    center + egui::vec2(-arm, arm),
                ],
                stroke,
            );
        }
        ProfileIcon::Rename => {
            // A slanted pencil: barrel edges, ferrule, then the nib.
            let along = egui::vec2(1.0, -1.0).normalized();
            let across = egui::vec2(-along.y, along.x);
            let base = center + along * arm;
            let neck = center - along * arm * 0.1;
            let nib = center - along * arm * 0.95;
            painter.line_segment([base, neck + across * arm * 0.45], stroke);
            painter.line_segment([base, neck - across * arm * 0.45], stroke);
            painter.line_segment(
                [neck + across * arm * 0.45, neck - across * arm * 0.45],
                stroke,
            );
            painter.line_segment([neck, nib], stroke);
        }
    }
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

fn selected_overlay_for_border(config_visible: bool, selected_id: Option<&str>) -> Option<&str> {
    if config_visible {
        selected_id
    } else {
        None
    }
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

pub fn run() {
    crate::overlay::enable_dpi_awareness();
    let native_options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id("ping-latency-overlay")
            .with_title("PingLatencyOverlay - Config")
            .with_inner_size(egui::vec2(640.0, 640.0))
            .with_min_inner_size(egui::vec2(640.0, 480.0))
            .with_max_inner_size(egui::vec2(640.0, 8192.0))
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
        can_switch_profile, config_notice_status, config_notices_status,
        selected_overlay_for_border,
    };
    use crate::config::ConfigNotice;

    #[test]
    fn hidden_config_does_not_keep_overlay_selected_for_border() {
        let selected = Some("overlay");
        assert_eq!(selected_overlay_for_border(false, selected), None);
        assert_eq!(selected_overlay_for_border(true, selected), Some("overlay"));
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
}
