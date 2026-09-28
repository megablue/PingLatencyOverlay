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
