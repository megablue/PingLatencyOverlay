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

/// Crates whose presence anywhere in a GUI-free package's graph would mean
/// that process can initialise a GPU context or run an event loop. That is the
/// property being protected: none of these three processes may own one.
const NO_EVENT_LOOP: &[&str] = &[
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
    "orbtop",
];

/// Crates that put an icon in the notification area. A notification-area icon
/// is a Win32 shell object with no GPU context and no event loop, so the tray
/// process is allowed one and is little else. The renderer is not: it has no
/// tray, because the tray is what starts and watches it.
///
/// This list does two jobs. Above it is the extra deny-list for the core
/// package; below it is the set `foreign_tray_crates` searches a tree for, so
/// one constant answers both "may this package hold a notification-area icon"
/// and "did some other package quietly keep one".
const TRAY_ONLY: &[&str] = &["tray-icon", "muda"];

/// Every package that must never reach a GPU context or an event loop, with
/// the extra crates that are forbidden in it beyond [`NO_EVENT_LOOP`].
///
/// `cargo tree` reports a package's dependencies across **every** target, so
/// this covers the renderer's binary as well as the library, and naming the
/// tray package is what covers the tray — a separate package, so a check that
/// only named the core one would not see it at all.
///
/// The second element exists because "no GUI stack" is too blunt a rule to
/// apply to the tray: `tray-icon` IS the tray. Applying the renderer's rule to
/// it would have forced the choice between dropping the tray and deleting the
/// test, and the first is the bug and the second is the loss of the check. The
/// thing actually worth protecting is narrower and is stated here instead.
const GUI_FREE_PACKAGES: &[(&str, &[&str])] = &[
    // The renderer: no GPU context, no event loop, and no tray.
    ("ping-latency-overlay-core", TRAY_ONLY),
    // The tray: no GPU context, no event loop, but a notification-area icon.
    ("ping-latency-overlay-tray", &[]),
];

/// Every package in the workspace. Naming them all is what makes a
/// workspace-wide rule checkable, the same way naming the tray above is what
/// covers a package the core-only rule would never see.
const WORKSPACE_PACKAGES: &[&str] = &[
    "ping-latency-overlay",
    "ping-latency-overlay-core",
    "ping-latency-overlay-tray",
    "ping-latency-overlay-build-support",
];

/// The package name from one `cargo tree --prefix none` line, which is
/// `name vX.Y.Z` with an optional ` (proc-macro)` or ` (*)` suffix.
fn package_name(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or_default()
}

fn is_forbidden(name: &str) -> bool {
    NO_EVENT_LOOP.contains(&name)
}

/// Whether `name` is forbidden in a package with the given extra deny-list.
fn forbidden_in(name: &str, extra: &[&str]) -> bool {
    is_forbidden(name) || extra.contains(&name)
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
fn gui_free_crates_report_a_clean_tree() {
    for (package, extra) in GUI_FREE_PACKAGES {
        // Use the cargo that is running this test, so a toolchain mismatch
        // cannot make the check describe a different graph than the one that
        // was built.
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let output = Command::new(&cargo)
            .args([
                "tree", "-p", package,
                // One package per line with no tree indentation, so the parse
                // above does not have to understand tree drawing.
                "--prefix", "none",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo tree must be runnable to check the dependency invariant");

        assert!(
            output.status.success(),
            "cargo tree failed for {package}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let tree = String::from_utf8_lossy(&output.stdout);
        assert!(
            !tree.trim().is_empty(),
            "cargo tree produced no output for {package}, so this test would \
             pass on a graph it could not read"
        );

        let found: Vec<&str> = tree
            .lines()
            .map(package_name)
            .filter(|name| !name.is_empty() && forbidden_in(name, extra))
            .collect();
        let mut found = found;
        found.sort_unstable();
        found.dedup();

        assert!(
            found.is_empty(),
            "{package} must not depend on a GUI stack, but its dependency tree \
             contains {found:?}.\n\
             These processes are built without one precisely so that they can \
             never create a GPU context or run an event loop. If a GUI crate is \
             genuinely needed here, the split has lost its reason to exist and \
             the architecture needs revisiting rather than the deny-list."
        );
    }
}

/// The notification-area crates a package is forbidden to carry, sorted and
/// deduplicated: the crates in `tree` that belong to the tray and nowhere else.
fn foreign_tray_crates(tree: &str) -> Vec<&str> {
    let mut found: Vec<&str> = tree
        .lines()
        .map(package_name)
        .filter(|name| TRAY_ONLY.contains(name))
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// A notification-area icon belongs to the tray crate and to nothing else.
///
/// This exists because of how the dead dependency got there. The application
/// shell depended on `tray-icon` for a release after the split moved the icon
/// into its own crate, with nothing in `src/` referencing it and its manifest
/// comment still saying the shell owned the tray icon. Nothing failed: a comment
/// claiming a dependency is not evidence of one, and an unused dependency is
/// invisible to the rule above, because the shell is *allowed* a GUI stack.
///
/// So this is stated positively — only the tray may hold one — rather than as
/// another deny-list entry. A deny-list only fires on the package it names, and
/// this one has to fire on the package that left it behind.
#[test]
fn only_the_tray_crate_carries_a_notification_area_icon() {
    for package in WORKSPACE_PACKAGES {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let output = Command::new(&cargo)
            .args(["tree", "-p", package, "--prefix", "none"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo tree must be runnable to check the dependency invariant");

        assert!(
            output.status.success(),
            "cargo tree failed for {package}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let tree = String::from_utf8_lossy(&output.stdout);
        assert!(
            !tree.trim().is_empty(),
            "cargo tree produced no output for {package}, so this test would \
             pass on a graph it could not read"
        );

        let found = foreign_tray_crates(&tree);
        if *package == "ping-latency-overlay-tray" {
            // The one package meant to have it, which is also what keeps the
            // rule above honest: a check that forbids nothing passes just as
            // quietly as one that is broken. `TRAY_ONLY` names the alternative
            // backends too and the tray does not link all of them, so the
            // positive claim is about the one crate it actually uses.
            assert!(
                found.contains(&"tray-icon"),
                "the tray crate is the only one meant to carry a notification-area \
                 icon, so this test is looking at the wrong tree if it finds none \
                 (found {found:?})"
            );
        } else {
            assert!(
                found.is_empty(),
                "{package} depends on {found:?}, but the tray icon and its menu \
                 belong to ping-latency-overlay-tray. The shell and the renderer \
                 must not link a notification-area icon: dead weight in binaries \
                 that never show one, and a manifest comment claiming otherwise \
                 is not evidence of a dependency."
            );
        }
    }
}
