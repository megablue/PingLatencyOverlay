use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::config::Config;

/// Application-wide state shared between the tray, commands and probe tasks.
pub struct AppState {
    /// Last loaded/saved configuration.
    pub config: Mutex<Config>,
    /// Whether probing is currently running (toggled from the tray).
    pub running: AtomicBool,
    /// Set just before a real quit so the exit handler lets the app close.
    pub quitting: AtomicBool,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config: Mutex::new(config),
            running: AtomicBool::new(true),
            quitting: AtomicBool::new(false),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::Relaxed)
    }

    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::Relaxed);
    }
}
