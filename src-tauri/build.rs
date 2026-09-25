use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-env-changed=PING_LATENCY_BUILD_VERSION");

    emit_git_change_watchers();
    println!("cargo:rustc-env=APP_BUILD_VERSION={}", build_version());

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

fn build_version() -> String {
    if let Ok(version) = std::env::var("PING_LATENCY_BUILD_VERSION") {
        let version = version.trim();
        if !version.is_empty() && !version.contains(['\n', '\r']) {
            return version.to_string();
        }
    }

    let base_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
    let Some(commit_count) = git_commit_count() else {
        return base_version;
    };

    let mut parts = base_version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("1");
    if commit_count == 0 {
        base_version
    } else {
        format!("{major}.{minor}.{commit_count}")
    }
}

fn git_commit_count() -> Option<u32> {
    let output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn emit_git_change_watchers() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if git_path(&manifest_dir, &["rev-parse", "--git-dir"]).is_none() {
        return;
    }

    if let Some(head_path) = git_path(&manifest_dir, &["rev-parse", "--git-path", "HEAD"]) {
        println!(
            "cargo:rerun-if-changed={}",
            resolve_path(&manifest_dir, &head_path).display()
        );
    }

    if let Some(reference) = git_path(&manifest_dir, &["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(reference_path) = git_path(
            &manifest_dir,
            &["rev-parse", "--git-path", reference.trim()],
        ) {
            println!(
                "cargo:rerun-if-changed={}",
                resolve_path(&manifest_dir, &reference_path).display()
            );
        }
    }
}

fn git_path(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

fn resolve_path(root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}
