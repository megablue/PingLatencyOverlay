use ping_latency_overlay_build_support as build_support;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_support::watch_build_inputs();
    println!(
        "cargo:rustc-env=APP_BUILD_VERSION={}",
        build_support::product_version()
    );
    println!(
        "cargo:rustc-env=APP_COPYRIGHT={}",
        build_support::copyright_notice()
    );
    build_support::embed_windows_resources(
        "plo-config.exe",
        "PingLatencyOverlay — live network latency overlay",
    );
}
