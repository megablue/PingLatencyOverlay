use ping_latency_overlay_build_support as build_support;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_support::watch_build_inputs();
    // The package-wide script stamps both binaries; `plo-tray-probe.exe` is a
    // diagnostic that is deliberately not shipped, so sharing the tray's
    // description is a detail of no consequence.
    build_support::embed_windows_resources("plo-tray.exe", "PingLatencyOverlay — system tray");
}
