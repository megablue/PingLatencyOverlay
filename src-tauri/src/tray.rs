use std::error::Error;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ToggleRunning,
    Config,
    Exit,
}

/// Owns the tray icon and its mutable Start/Pause item.
pub struct TrayState {
    _tray: TrayIcon,
    toggle_item: MenuItem,
}

impl TrayState {
    pub fn set_running(&self, running: bool) {
        self.toggle_item
            .set_text(if running { "Pause" } else { "Resume" });
    }
}

/// Load the bundled artwork for the native window and taskbar.
pub fn app_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(include_bytes!("../icons/icon.png"))
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

pub fn create() -> Result<TrayState, Box<dyn Error + Send + Sync>> {
    let menu = Menu::new();
    let toggle_item = MenuItem::with_id("toggle", "Pause", true, None);
    let config_item = MenuItem::with_id("config", "Config", true, None);
    let exit_item = MenuItem::with_id("exit", "Exit", true, None);
    menu.append(&toggle_item)?;
    menu.append(&config_item)?;
    menu.append(&exit_item)?;

    let image = image::load_from_memory(include_bytes!("../icons/icon.png"))?.to_rgba8();
    let width = image.width();
    let height = image.height();
    let icon = Icon::from_rgba(image.into_raw(), width, height)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(true)
        .with_tooltip("PingLatencyOverlay")
        .with_icon(icon)
        .build()?;

    Ok(TrayState {
        _tray: tray,
        toggle_item,
    })
}

/// Drain tray menu and icon events without blocking the egui event loop.
pub fn poll() -> Vec<TrayAction> {
    let mut actions = Vec::new();

    while let Ok(event) = MenuEvent::receiver().try_recv() {
        match event.id().as_ref() {
            "toggle" => actions.push(TrayAction::ToggleRunning),
            "config" => actions.push(TrayAction::Config),
            "exit" => actions.push(TrayAction::Exit),
            _ => {}
        }
    }

    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            actions.push(TrayAction::Config);
        }
    }

    actions
}
