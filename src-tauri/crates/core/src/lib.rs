//! Everything the renderer needs, and nothing it does not.
//!
//! This crate holds latency probing, the software graph renderer, the runtime
//! border effect, and the native layered windows the graph is drawn into. It
//! has no GUI dependency at all, which is the property that lets a renderer
//! process be built from it without dragging along a GPU context and an event
//! loop it would never use.
//!
//! That property is only worth something if it is enforced, so
//! `tests/no_gui_dependencies.rs` runs `cargo tree` over this package and fails
//! if a GUI crate has appeared anywhere beneath it. Adding an `eframe` line to
//! `Cargo.toml` here fails the suite.
//!
//! The modules marked "internal" are not consumed from outside this crate
//! today; they are public because their siblings' public interfaces mention
//! their types, and hiding them would be a lie about what the renderer
//! process can reach.

// Internal to this crate: the software rasteriser and the RGB border effect
// that rides on top of it, and the one-shot ICMP/TCP measurement.
pub mod border;
pub mod render;

// Internal to this crate: the one-shot measurement a probe task calls.
pub mod probe;

// The storage layer: profiles, preferences and directory migration. Also the
// only thing that writes to disk, so that stays in one process by design.
pub mod config;

// The native `WS_EX_LAYERED` overlay windows. `UpdateLayeredWindow` is called
// directly with a 32-bit DIB; the Win32 entry points are declared as
// `extern "system"` blocks in the module rather than pulled in from a binding
// crate, which is part of why this crate needs no Windows SDK dependency.
pub mod overlay;

// The long-lived probe tasks and their bounded sample buffers.
pub mod probes;

// Which displays are attached, and which one an overlay belongs on. Enumeration
// is Win32, but the rule that picks a display is a pure function over a list, so
// the multi-monitor behaviour is testable on a machine with one monitor — which
// is the only kind of machine most of this is developed on. Both the renderer
// and the configuration window call in here; neither sends the other anything.
pub mod monitors;

// The wire between this process and the configuration window: the pipe name,
// the message shapes and the line framing. It lives here rather than in the
// shell because both ends are built from this crate, so the two cannot
// disagree about the format without a compile error.
pub mod diagnostics;
pub mod transport;

/// The bundled artwork, as the PNG bytes it is stored as.
///
/// The tray and the configuration window each want this icon and neither may
/// depend on the other, so the single `include_bytes!` lives here and both read
/// it from this function. Decoding is deliberately left to the caller: the
/// tray wants RGBA for `tray-icon` and the window wants an `egui::IconData`, so
/// the two decode differently, and pulling in an image decoder here would put
/// one in the renderer process for a file it never draws.
pub fn icon_png() -> &'static [u8] {
    include_bytes!("../../../icons/icon.png")
}
