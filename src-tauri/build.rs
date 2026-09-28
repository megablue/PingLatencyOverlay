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
            .set("OriginalFilename", "plo-config.exe")
            .compile()
            .expect("failed to embed Windows resources");
    }
}

/// The version the app reports: `MAJOR.MINOR.(commits since countBase)`.
///
/// `countBase` is read from the same `Cargo.toml` line the packaging script
/// reads, so the number in the About page and the number in the installer's
/// filename come out of one fact rather than two implementations agreeing by
/// luck. When the count is at or below the base — no Git, a shallow clone, or
/// the bump commit itself — the plain base version is used, which is what a
/// developer building from a tarball should see.
fn build_version() -> String {
    if let Ok(version) = std::env::var("PING_LATENCY_BUILD_VERSION") {
        let version = version.trim();
        if !version.is_empty() && !version.contains(['\n', '\r']) {
            return version.to_string();
        }
    }

    let base_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.2.0".into());
    let Some(patch) = git_commit_count().and_then(|count| count.checked_sub(count_base())) else {
        return base_version;
    };
    if patch == 0 {
        return base_version;
    }

    let mut parts = base_version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    format!("{major}.{minor}.{patch}")
}

/// The commit count that the patch number restarts from.
///
/// Read out of `Cargo.toml` rather than hardcoded, because
/// `scripts/build-nsis.ps1` has to subtract exactly the same number and a
/// constant in each file is a constant that eventually drifts.
fn count_base() -> u32 {
    // SAFETY: `CARGO_MANIFEST_DIR` is set by cargo for every build script.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(manifest) else {
        return 0;
    };
    text.lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("countBase")?;
            rest.trim_start().strip_prefix('=')?.trim().parse().ok()
        })
        .unwrap_or(0)
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
