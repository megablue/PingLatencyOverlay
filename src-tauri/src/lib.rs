mod config;
mod overlay;
mod probe;
mod probes;
mod state;
mod tray;

use tauri::{AppHandle, Emitter, Manager};

use config::Config;
use probes::ProbeManager;
use state::AppState;

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    mut config: Config,
) -> Result<(), String> {
    config.normalize();
    config::save(&config).map_err(|e| e.to_string())?;
    *state.config.lock().unwrap() = config.clone();

    // Creating windows must happen off the main thread on Windows, hence this
    // command is `async`. A single failure must not abort the rest.
    if let Err(err) = overlay::reconcile(&app, &config) {
        log::error!("failed to reconcile overlay windows: {err}");
    }
    app.state::<ProbeManager>().restart(&app, &config);

    // Push the new config to live overlay windows so style changes (colors,
    // orientation, height, ...) apply immediately without recreating windows.
    let _ = app.emit("config://updated", &config);
    Ok(())
}

#[tauri::command]
fn get_running(state: tauri::State<AppState>) -> bool {
    state.is_running()
}

#[tauri::command]
fn set_running(state: tauri::State<AppState>, running: bool) {
    state.set_running(running);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::try_init();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = config::load();

            app.manage(AppState::new(cfg.clone()));
            app.manage(ProbeManager::new());

            tray::create(&handle)?;
            overlay::reconcile(&handle, &cfg)?;
            handle.state::<ProbeManager>().restart(&handle, &cfg);

            // Keep overlays above the taskbar, which is topmost too and otherwise
            // wins the z-order when it auto-shows.
            let topmost_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_millis(1000));
                loop {
                    ticker.tick().await;
                    overlay::reassert_topmost(&topmost_handle);
                }
            });

            // The app starts in tray mode; make sure the config window stays hidden.
            if let Some(config_window) = app.get_webview_window("config") {
                let _ = config_window.hide();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_running,
            set_running
        ])
        // Tray app: closing the config window hides it instead of destroying it.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "config" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // Keep running in the tray when all windows are closed; only the tray
        // "Exit" item (which sets `quitting`) actually terminates the app.
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !app.state::<AppState>().is_quitting() {
                    api.prevent_exit();
                }
            }
        });
}
