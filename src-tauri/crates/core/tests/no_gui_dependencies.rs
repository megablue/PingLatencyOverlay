//! The invariant that makes this crate worth having.
//!
//! Splitting the renderer into its own process is only worth doing if the
//! renderer genuinely cannot create a GPU context or open a window. That is a
//! property of the dependency graph, not of anyone's discipline, and a
//! property of a graph decays silently: one day someone needs a colour type
//! and reaches for `egui`, and the renderer quietly grows 13MB of GUI code and
//! the memory argument for the whole split evaporates without anybody noticing.
//!
//! So it is checked here. This test fails the moment a GUI crate appears
//! anywhere beneath `ping-latency-overlay-core`, whether it was added to
//! `Cargo.toml` directly or pulled in transitively by something else.
//!
//! This is a build-system test rather than a behavioural one, which is worth
//! being explicit about: it inspects the dependency graph, not the running
//! program. That is the only way to check the property, and the alternative —
//! trusting a comment in `Cargo.toml` — is how the Global page's preferences
//! went unsaveable for the page's entire life.

use std::process::Command;

/// Crates whose presence anywhere in this crate's graph would mean the
/// renderer process can initialise a GPU context, run an event loop, or own a
/// system tray. All three are things the renderer must never do.
const FORBIDDEN: &[&str] = &[
    // The egui/eframe stack, including the renderers and platform bindings
    // that come with it.
    "eframe",
    "egui",
    "egui_glow",
    "egui_winit",
    "egui_extras",
    "glow",
    "winit",
    "accesskit",
    "accesskit_winit",
    // A GPU rasteriser, which is the thing being avoided.
    "wgpu",
    // The tray. A renderer process has no tray; the shell owns it.
    "tray-icon",
    "orbtop",
    "muda",
];

/// The package name from one `cargo tree --prefix none` line, which is
/// `name vX.Y.Z` with an optional ` (proc-macro)` or ` (*)` suffix.
fn package_name(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or_default()
}

fn is_forbidden(name: &str) -> bool {
    FORBIDDEN.contains(&name)
}

/// The matcher itself, pinned.
///
/// A test that reads a dependency tree and asserts "nothing in it is bad" is
/// worthless if the parsing quietly stops working: it would pass on a tree it
/// could not read. So this checks that a real GUI crate is recognised and a
/// real non-GUI one is not, which is the failure this test is most able to
/// have without anybody noticing.
#[test]
fn a_gui_crate_in_the_tree_is_recognised() {
    assert_eq!(package_name("egui_glow v0.36.2"), "egui_glow");
    assert_eq!(package_name("tiny-skia v0.11.4 (proc-macro)"), "tiny-skia");
    assert_eq!(package_name("serde v1.0.219 (*)"), "serde");
    assert!(is_forbidden("egui_glow"), "egui_glow must be forbidden");
    assert!(is_forbidden("eframe"), "eframe must be forbidden");
    assert!(
        !is_forbidden("serde"),
        "serde is a core dependency and must not be forbidden"
    );
    assert!(
        !is_forbidden("tiny-skia"),
        "tiny-skia is the software renderer and is exactly what core should use"
    );
    // A crate whose name merely contains a forbidden name is still fine; the
    // match is exact so a future innocent crate is not caught by accident.
    assert!(!is_forbidden("my_egui_helpers"));
}

/// The invariant itself.
#[test]
fn core_cannot_reach_a_gui_stack() {
    // Use the cargo that is running this test, so a toolchain mismatch cannot
    // make the check describe a different graph than the one that was built.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args([
            "tree",
            "-p",
            "ping-latency-overlay-core",
            // One package per line with no tree indentation, so the parse
            // above does not have to understand tree drawing.
            "--prefix",
            "none",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree must be runnable to check the dependency invariant");

    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    assert!(
        !tree.trim().is_empty(),
        "cargo tree produced no output, so this test would pass on a graph it \
         could not read"
    );

    let found: Vec<&str> = tree
        .lines()
        .map(package_name)
        .filter(|name| !name.is_empty() && is_forbidden(name))
        .collect();
    let mut found = found;
    found.sort_unstable();
    found.dedup();

    assert!(
        found.is_empty(),
        "ping-latency-overlay-core must not depend on a GUI stack, but its \
         dependency tree contains {found:?}.\n\
         The renderer process is built from this crate precisely so that it can \
         never create a GPU context or run an event loop. If a GUI crate is \
         genuinely needed here, the split has lost its reason to exist and the \
         architecture needs revisiting rather than the deny-list."
    );
}
