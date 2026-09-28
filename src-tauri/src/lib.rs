//! The application shell: the tray icon and the one configuration window.
//!
//! Everything that draws a latency graph lives in the `ping_latency_overlay_core`
//! crate instead, and runs in a separate `ping-latency-overlay-renderer`
//! process that this crate starts and talks to over a named pipe. This crate is
//! the only place in the project allowed to depend on eframe, which is what
//! keeps a GPU context out of the renderer.

mod tray;
mod ui;

/// Start the native application.
///
/// One shell at a time: a second copy would fight the first over the pipe, and
/// the one that lost would sit there with a working renderer and no idea which
/// process it belongs to. The renderer has its own separate guard, so a crashed
/// renderer can still be replaced while this window is open.
pub fn run() {
    let _ = env_logger::try_init();
    match ping_latency_overlay_core::transport::SingleInstance::acquire(
        ping_latency_overlay_core::transport::Role::Shell,
    ) {
        Ok(Some(_lock)) => ui::run(),
        // Another shell is already running, which is the ordinary answer to
        // running the app twice. Not an error worth a message box.
        Ok(None) => {}
        Err(error) => eprintln!("Could not check whether another copy is running: {error}"),
    }
}
