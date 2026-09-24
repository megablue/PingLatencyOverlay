use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::state::AppState;

/// Show and focus the config window.
fn show_config(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("config") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Build the system tray icon and its menu.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let toggle_i = MenuItem::with_id(app, "toggle", "Pause", true, None::<&str>)?;
    let config_i = MenuItem::with_id(app, "config", "Config", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_i, &config_i, &quit_i])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("PingLatencyOverlay")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => {
                let state = app.state::<AppState>();
                let now_running = !state.is_running();
                state.set_running(now_running);
                let _ = toggle_i.set_text(if now_running { "Pause" } else { "Resume" });
            }
            "config" => show_config(app),
            "quit" => {
                app.state::<AppState>().begin_quit();
                app.exit(0);
            }
            _ => {}
        })
        // Single left click opens the config window; the menu stays on right click.
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_config(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
