use ping_latency_overlay_build_support as build_support;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_support::watch_build_inputs();
    // The tray needs the same version the shell bakes, so that the update check
    // can compare the running build against what GitHub has published. It is
    // read through `env!` rather than from the exe's resource because the
    // resource is a string this process has no reason to parse.
    println!(
        "cargo:rustc-env=APP_BUILD_VERSION={}",
        build_support::product_version()
    );
    // When the optional `tray-probe` feature is on, the package-wide script
    // stamps that diagnostic with the tray's resource too; it is not shipped.
    build_support::embed_windows_resources("plo-tray.exe", "PingLatencyOverlay — system tray");
}
