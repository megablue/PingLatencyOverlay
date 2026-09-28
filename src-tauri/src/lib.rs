//! The configuration window, and only the configuration window.
//!
//! Everything that draws a latency graph lives in the `ping_latency_overlay_core`
//! crate instead, and runs in a separate `plo-renderer` process that this one
//! starts or attaches to over a named pipe. The tray
//! lives in a third process, `plo-tray`, which has no eframe at
//! all. This crate is the only place in the project allowed to depend on
//! eframe, and it is deliberately short-lived: a window that creates an OpenGL
//! context costs tens of megabytes of driver memory for as long as it exists, so
//! it is started on demand and exits when it is closed.

mod ui;

/// Start the configuration window.
///
/// One window at a time, under a mutex of its own rather than the tray's: the
/// tray is meant to keep running with no window open, and opening the window
/// twice should focus the one that is already open instead of starting a second
/// one fighting it for the pipe.
///
/// The renderer has a separate guard again, so a crashed renderer can still be
/// replaced while this window is open.
pub fn run() {
    let _ = env_logger::try_init();
    use ping_latency_overlay_core::transport::{Role, SingleInstance};
    match SingleInstance::acquire(Role::Config) {
        Ok(Some(_lock)) => ui::run(),
        // Another window is already open. The tray noticed this process's
        // shortcut and started it anyway, so the ordinary answer is to leave
        // rather than to raise an error the user cannot act on.
        Ok(None) => {}
        Err(error) => eprintln!("Could not check whether the window is already open: {error}"),
    }
}
