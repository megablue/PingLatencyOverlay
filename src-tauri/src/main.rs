// Keep both debug and release launches as a Windows GUI application. This
// prevents `cargo run`/Explorer launches from flashing a console window.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    ping_latency_overlay_lib::run()
}
