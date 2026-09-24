use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::{Config, OverlayConfig};
use crate::probe;
use crate::state::AppState;

/// Fixed probe cadence: one ping per second, so one tick == one second.
const TICK: Duration = Duration::from_millis(1000);

#[derive(Clone, Serialize)]
struct Sample {
    latency: Option<u32>,
}

/// Owns the per-overlay probe tasks so they can be restarted when the config
/// changes.
#[derive(Default)]
pub struct ProbeManager {
    tasks: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl ProbeManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Abort every running probe and start one task per enabled overlay.
    pub fn restart(&self, app: &AppHandle, config: &Config) {
        self.stop_all();
        let mut tasks = self.tasks.lock().unwrap();
        for overlay in config.overlays.iter().filter(|o| o.enabled) {
            let app = app.clone();
            let id = overlay.id.clone();
            let overlay = overlay.clone();
            let handle = tauri::async_runtime::spawn(async move { probe_loop(app, overlay).await });
            tasks.insert(id, handle);
        }
    }

    pub fn stop_all(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        for (_, handle) in tasks.drain() {
            handle.abort();
        }
    }
}

async fn probe_loop(app: AppHandle, overlay: OverlayConfig) {
    let event = format!("latency://{}", overlay.id);
    loop {
        if app.state::<AppState>().is_running() {
            let latency = probe::measure(&overlay.probe, overlay.timeout_ms).await;
            let _ = app.emit(&event, Sample { latency });
        }
        tokio::time::sleep(TICK).await;
    }
}
