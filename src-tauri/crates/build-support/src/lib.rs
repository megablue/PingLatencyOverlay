//! What the three build scripts share.
//!
//! A build script applies to its own package's binaries, so all three packages
//! that produce an executable need one. What they need from it is the same two
//! things: the product version, `MAJOR.MINOR.(commits since countBase)`, and the
//! Windows resource that puts the app icon and that version on the exe. Both
//! used to live in the window's build script alone, which is why the tray and
//! the renderer had no icon and no version information to show.
//!
//! One implementation rather than three is the point: the version rule has two
//! other readers already (`scripts/build-nsis.ps1` and the About page), and a
//! fourth copy is how the app ends up reporting one number while Explorer
//! reports another.
//!
//! Paths here resolve from THIS crate's manifest directory, not the caller's:
//! `../../icons/icon.ico` is `src-tauri/icons/icon.ico` whichever build script
//! is running. Environment variables read at runtime (`CARGO_MANIFEST_DIR` in
//! the rerun watcher) are the calling package's instead, which is what makes
//! the per-package watcher possible.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `src-tauri`, whichever package's build script is running.
///
/// This crate lives at `src-tauri/crates/build-support`, so two levels up from
/// its own manifest directory is the application's directory: the one that
/// holds the root manifest and the icons. Compile-time `CARGO_MANIFEST_DIR` is
/// this crate's; the runtime environment variable used elsewhere is not.
fn project_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/build-support has two parents")
        .to_path_buf()
}

/// Absolute path to the artwork the executables and the tray all show.
fn icon_path() -> PathBuf {
    project_dir().join("icons").join("icon.ico")
}

/// Absolute path to the root package's manifest: the one record of the version
/// number and of `countBase`.
fn root_manifest_path() -> PathBuf {
    project_dir().join("Cargo.toml")
}

/// The version the app reports: `MAJOR.MINOR.(commits since countBase)`.
///
/// When there is no Git, a shallow clone, or the count is at or below the base
/// -- the bump commit itself -- the plain base version is used, which is what a
/// developer building from a tarball should see.
pub fn product_version() -> String {
    if let Ok(version) = std::env::var("PING_LATENCY_BUILD_VERSION") {
        let version = version.trim();
        if !version.is_empty() && !version.contains(['\n', '\r']) {
            return version.to_string();
        }
    }

    let base = package_version().unwrap_or_else(|| "0.2.0".into());
    derived_version(&base, git_commit_count(), count_base())
}

/// The rule itself, as a pure function over inputs, so the fallbacks can be
/// tested without a Git repository or a filesystem.
fn derived_version(base: &str, count: Option<u32>, count_base: u32) -> String {
    let Some(patch) = count.and_then(|count| count.checked_sub(count_base)) else {
        return base.to_string();
    };
    if patch == 0 {
        return base.to_string();
    }

    let mut parts = base.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    format!("{major}.{minor}.{patch}")
}

/// The value of one `key = ...` line in the root manifest.
///
/// A line scan rather than a TOML parse: `version`, `countBase` and
/// `copyright` are the manifest's only single-line facts, and keeping the
/// scan here is what lets this crate depend on nothing but winresource. The
/// remainder must start with `=`, so a key that happens to be a prefix of a
/// longer identifier cannot match. Quoting is optional and dropped, because
/// `countBase` is a bare number while the strings around it are quoted.
fn manifest_value(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(root_manifest_path()).ok()?;
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(key)?;
        let value = rest.trim_start().strip_prefix('=')?.trim();
        Some(
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value)
                .to_string(),
        )
    })
}

/// The `[package] version` of the root manifest.
///
/// Read from the file rather than `CARGO_PKG_VERSION`, because the build script
/// that runs this may belong to the tray or the renderer, and the environment
/// carries that package's own version -- `crates/core` has a `0.1.0` that means
/// nothing to the product.
fn package_version() -> Option<String> {
    manifest_value("version")
}

/// The commit count the patch number restarts from.
///
/// Read out of the root manifest rather than hardcoded, because
/// `scripts/build-nsis.ps1` has to subtract exactly the same number and a
/// constant in each file is a constant that eventually drifts.
fn count_base() -> u32 {
    manifest_value("countBase")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// The copyright notice every exe carries and the About page shows.
///
/// A missing key is a build error rather than a blank: a blank would reach
/// users and look deliberate, and the notice exists so the build cannot
/// silently ship without one. It carries no year on purpose -- a year in a
/// shipped resource is a fact that goes stale.
pub fn copyright_notice() -> String {
    manifest_value("copyright")
        .expect("package.metadata.copyright must be set in src-tauri/Cargo.toml")
}

fn git_commit_count() -> Option<u32> {
    let output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(project_dir())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

/// Tell cargo everything the resource depends on beyond the caller's own files.
///
/// The per-package watcher (`build.rs`) is the caller's business, because only
/// the caller knows which package it is. The rest is shared: the version rule
/// reads the root manifest, the artwork lives beside it, and both the commit
/// count and the branch can change without any file in the calling package
/// moving, so Git's HEAD and the branch ref are watched by path.
pub fn watch_build_inputs() {
    println!("cargo:rerun-if-env-changed=PING_LATENCY_BUILD_VERSION");
    println!("cargo:rerun-if-changed={}", root_manifest_path().display());
    println!("cargo:rerun-if-changed={}", icon_path().display());
    emit_git_change_watchers();
}

fn emit_git_change_watchers() {
    let root = project_dir();
    if git_path(&root, &["rev-parse", "--git-dir"]).is_none() {
        return;
    }

    if let Some(head_path) = git_path(&root, &["rev-parse", "--git-path", "HEAD"]) {
        println!(
            "cargo:rerun-if-changed={}",
            resolve_path(&root, &head_path).display()
        );
    }

    if let Some(reference) = git_path(&root, &["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(reference_path) =
            git_path(&root, &["rev-parse", "--git-path", reference.trim()])
        {
            println!(
                "cargo:rerun-if-changed={}",
                resolve_path(&root, &reference_path).display()
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

/// Put the icon and the version on the executable being built.
///
/// `original_filename` is per executable because that is the field saying which
/// of the three this is; the description is what Task Manager shows as the
/// process name, so each process gets its own there too.
pub fn embed_windows_resources(original_filename: &str, file_description: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = product_version();
    let copyright = copyright_notice();
    // rc.exe accepts forward slashes, and winresource escapes backslashes on
    // the way into the generated resource file; one separator style avoids
    // having to care which escaping was intended.
    let icon = icon_path().to_string_lossy().replace('\\', "/");

    winresource::WindowsResource::new()
        .set_icon(&icon)
        .set("FileDescription", file_description)
        .set("ProductName", "PingLatencyOverlay")
        .set("InternalName", "ping-latency-overlay")
        .set("LegalCopyright", &copyright)
        .set("OriginalFilename", original_filename)
        .set("FileVersion", &version)
        .set("ProductVersion", &version)
        .set_version_info(
            winresource::VersionInfo::FILEVERSION,
            version_code(&version),
        )
        .set_version_info(
            winresource::VersionInfo::PRODUCTVERSION,
            version_code(&version),
        )
        .compile()
        .expect("failed to embed Windows resources");
}

/// `major << 48 | minor << 32 | patch << 16`: the packing the version block has
/// always used, which is what winresource derives from a package version.
fn version_code(version: &str) -> u64 {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    (major << 48) | (minor << 32) | (patch << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every branch of the rule: derived, at the base, below the base, no Git.
    #[test]
    fn the_version_derives_from_the_count_and_the_base() {
        assert_eq!(derived_version("0.2.0", Some(95), 76), "0.2.19");
        assert_eq!(derived_version("0.2.0", Some(76), 76), "0.2.0");
        assert_eq!(derived_version("0.2.0", Some(50), 76), "0.2.0");
        assert_eq!(derived_version("0.2.0", None, 76), "0.2.0");
        assert_eq!(derived_version("1.3.0", Some(95), 88), "1.3.7");
    }

    /// The base is read from the one line the packaging script also reads; a
    /// constant here instead would be free to drift from it.
    #[test]
    fn the_count_base_comes_from_the_root_manifest() {
        assert!(
            count_base() > 0,
            "countBase must parse from src-tauri/Cargo.toml"
        );
    }

    /// The numeric version is the packing Windows expects, and a string that
    /// is not three numbers must yield zeros rather than a panic.
    #[test]
    fn the_version_code_packs_major_minor_and_patch() {
        assert_eq!(version_code("0.2.19"), (2 << 32) | (19 << 16));
        assert_eq!(version_code("1.0.0"), 1 << 48);
        assert_eq!(version_code("nonsense"), 0);
    }

    /// What the exes will show is a plain three-part number.
    #[test]
    fn the_product_version_is_a_plain_version() {
        let version = product_version();
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {version}");
        assert!(
            parts.iter().all(|part| part.parse::<u32>().is_ok()),
            "expected three numbers, got {version}"
        );
    }

    /// The notice exists, and carries no year: a dated notice in a shipped
    /// resource goes stale between releases, which is the same reason the
    /// About page's line has never carried one.
    #[test]
    fn the_copyright_notice_has_no_year() {
        let notice = copyright_notice();
        assert!(notice.starts_with("Copyright"), "got {notice:?}");
        assert!(
            !notice.chars().any(|c| c.is_ascii_digit()),
            "the notice carries no year on purpose: {notice}"
        );
    }

    /// The build scripts point at real ICO artwork. A missing file would
    /// otherwise be a build error with no explanation, and a wrong file would
    /// embed nothing while looking fine.
    #[test]
    fn the_icon_exists_and_is_an_ico() {
        let bytes = std::fs::read(icon_path()).expect("icons/icon.ico must exist");
        assert!(bytes.len() > 22, "an ICO header plus at least one entry");
        assert_eq!(
            &bytes[..4],
            &[0, 0, 1, 0],
            "the file must start with ICO magic"
        );
    }
}
