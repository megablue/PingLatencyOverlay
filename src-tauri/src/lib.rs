mod config;
mod overlay;
mod probe;
mod probes;
mod render;
mod tray;
mod ui;

/// Start the native application.
pub fn run() {
    let _ = env_logger::try_init();
    ui::run();
}
