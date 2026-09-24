fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("icons/icon.ico")
            .set(
                "FileDescription",
                "PingLatencyOverlay — live network latency overlay",
            )
            .set("ProductName", "PingLatencyOverlay")
            .set("InternalName", "ping-latency-overlay")
            .set("OriginalFilename", "ping-latency-overlay.exe")
            .compile()
            .expect("failed to embed Windows resources");
    }
}
