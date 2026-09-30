use ping_latency_overlay_build_support as build_support;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_support::watch_build_inputs();
    // When the optional `tray-probe` feature is on, the package-wide script
    // stamps that diagnostic with the tray's resource too; it is not shipped.
    build_support::embed_windows_resources("plo-tray.exe", "PingLatencyOverlay — system tray");
}
