//! The application shell: the tray icon and the one configuration window.
//!
//! Everything that draws a latency graph lives in the `ping_latency_overlay_core`
//! crate instead, so that a renderer process can be built from it without
//! inheriting a GUI stack. This crate is the only place in the project that is
//! allowed to depend on eframe, and it is the reason the split exists at all.

mod tray;
mod ui;

/// Start the native application.
///
/// Still one process doing everything, including the rendering. Splitting the
/// renderer out is the next step; this crate exists so that the core half can
/// be separated without the shell having to give up its window.
pub fn run() {
    let _ = env_logger::try_init();
    ui::run();
}
