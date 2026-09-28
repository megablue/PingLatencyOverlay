//! The renderer process: probes, graph rendering and the layered overlay
//! windows, and nothing else.
//!
//! It exists as a separate process so that the latency graph never shares an
//! address space with the tray icon and the configuration window. Two reasons,
//! in order of weight. A GPU context costs tens of megabytes of driver memory,
//! and the configuration window needs one while the renderer never does, so
//! keeping them apart means the graph pays nothing for the window's existence.
//! And the game module that will eventually draw into these same windows can be
//! added here without the configuration window's GUI stack anywhere in reach.
//!
//! Everything drawn to the screen is a native `WS_EX_LAYERED` window updated
//! with `UpdateLayeredWindow`; there is no egui, no GPU and no event loop here.
//! See `overlay.rs` for why.
//!
//! The process takes commands over a named pipe rather than being configured
//! through a second channel, so `transport.rs` owns the wire format and both
//! ends of it are compiled from this same crate and cannot disagree.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::error::Error;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ping_latency_overlay_core::config::{self, smooth_frame_interval, Config};
use ping_latency_overlay_core::overlay::{
    enable_dpi_awareness, pump_messages, quit_requested, OverlayManager,
};
use ping_latency_overlay_core::probes::ProbeManager;
use ping_latency_overlay_core::transport::{serve, Message, Role, SingleInstance};

/// How often to repaint when nothing in the configuration asks for less.
const REPAINT_INTERVAL: Duration = Duration::from_millis(100);

/// The longest this thread will wait before answering Windows again.
///
/// A thread that owns windows has to service their messages often enough that
/// Windows does not decide it has hung, which it concludes after a few seconds
/// of silence and reports with a spinning cursor. Sixteen milliseconds is well
/// inside that, and costs a peek at an empty queue sixty times a second. It is
/// deliberately independent of `REPAINT_INTERVAL`: the repaint cadence is a
/// drawing decision and this is a liveness one, and tying them together would
/// mean a slow repaint is also an unresponsive process.
const MESSAGE_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// How long to block waiting for work before the thread must answer Windows.
///
/// Two rules pull against each other and both have to hold at once, which is
/// why this is a function with a test rather than an expression in the loop: a
/// wait that runs past the repaint deadline delays the next frame, and a wait
/// that ignores the cap is how a thread ends up not pumping its messages. A
/// deadline already in the past waits zero, so a burst of messages does not
/// push the repaint further away.
fn wait_before(next_repaint: Instant, now: Instant) -> Duration {
    next_repaint
        .saturating_duration_since(now)
        .min(MESSAGE_POLL_INTERVAL)
}

/// What the overlays are currently being drawn from.
struct State {
    config: Config,
    running: bool,
    /// The overlay whose startup border should animate, or `None` when the
    /// configuration window is not looking at any of them.
    border_preview: Option<String>,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Two renderers would fight over both the pipe and the overlay windows. The
    // kernel drops a named mutex when the last handle closes, so a renderer
    // that dies never leaves a lock behind for the next attempt to trip over.
    let Some(_lock) = SingleInstance::acquire(Role::Renderer)? else {
        // One is already running and owns the pipe. The shell attaches to that
        // one rather than starting this process, so exiting quietly is right.
        return Ok(());
    };

    // Before any window exists. This is a process-wide setting, so the shell
    // sets it too, for its own window.
    enable_dpi_awareness();

    // Probe tasks and the pipe reader run on the runtime's threads. The repaint
    // loop below deliberately does not: Win32 wants every window created and
    // destroyed on the thread that made it, so the thread that builds the
    // overlays has to be the thread that tears them down and updates them.
    let runtime = tokio::runtime::Runtime::new()?;
    let (tx, rx) = mpsc::channel();
    runtime.spawn(async move {
        // Nothing is listening on stderr in a windowed build, so a failure here
        // would be silent. It is logged, because a renderer whose pipe never
        // opened is a renderer nobody can reach: it runs, it draws nothing, and
        // from the outside it is indistinguishable from a healthy one. `create`
        // with `first_pipe_instance` fails outright if anything still holds the
        // name, which is exactly the state a force-killed predecessor can leave
        // behind, so this line is what separates "the renderer is up" from "the
        // renderer is up and reachable".
        if let Err(error) = serve(tx).await {
            ping_latency_overlay_core::diagnostics::log_line(
                "renderer",
                &format!("the pipe could not be served at all: {error}"),
            );
        }
    });

    // The shell pushes the same configuration again as soon as it connects, so
    // this read is the cold-start path rather than a competing source of truth.
    let loaded = config::load();
    let mut probes = ProbeManager::new(runtime.handle().clone());
    probes.apply_config(&loaded.config);
    let mut overlays = OverlayManager::new()?;
    let mut state = State {
        config: loaded.config,
        running: probes.is_running(),
        border_preview: None,
    };

    // The next repaint is a deadline, not the length of the wait. The wait is
    // capped so this thread can answer Windows often enough not to be declared
    // hung, and the deadline is what keeps a smooth-rendered graph at its own
    // cadence instead of spinning at whatever the cap happens to be.
    let mut next_repaint = Instant::now();
    loop {
        // Before waiting, not instead of waiting: the windows this thread owns
        // post to it, and a thread that never pumps them is one Windows will
        // eventually report as unresponsive.
        pump_messages();
        // Checked here, immediately after the pump, because that is the only
        // place a request can arrive: Task Manager's "End task" and a logoff
        // both arrive as `WM_CLOSE`, and the pump above is what dispatches it.
        // The flag is set by the window procedure because a window procedure
        // cannot stop the loop that called it — it runs inside `pump_messages`.
        if quit_requested() {
            ping_latency_overlay_core::diagnostics::log_line(
                "renderer",
                "asked to close; shutting down",
            );
            break;
        }
        let now = Instant::now();
        if now >= next_repaint {
            overlays.apply(
                &state.config,
                probes.samples(),
                state.running,
                state.border_preview.as_deref(),
            );
            next_repaint = now + repaint_interval(&state, &overlays);
        }
        // Whatever is left until the next repaint, but never longer than the
        // responsiveness cap.
        let wait = wait_before(next_repaint, Instant::now());
        match rx.recv_timeout(wait) {
            Ok(message) => {
                if !apply(message, &mut state, &mut probes) {
                    break;
                }
                // A command has just changed what should be on screen, so do
                // not make the user wait out a whole repaint interval to see it.
                next_repaint = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The pipe server is gone, so nothing can reach this process to stop
            // it or to change it. Sitting here would leave overlays on screen
            // that the user has no way to switch off, so exit and let the shell
            // start a fresh renderer.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    probes.stop_all();
    // A probe's ICMP or DNS call may be blocked, and its worker is detached from
    // process shutdown, so waiting for the runtime would hang the exit.
    runtime.shutdown_background();
    Ok(())
}

/// Act on one command, returning whether the renderer should keep running.
fn apply(message: Message, state: &mut State, probes: &mut ProbeManager) -> bool {
    match message {
        Message::SetConfig { config } => {
            state.config = config;
            // Reuses the tasks that are already running, so a change to a colour
            // or a line width does not interrupt a measurement in progress.
            probes.apply_config(&state.config);
        }
        Message::SetPaused { paused } => {
            state.running = !paused;
            probes.set_running(state.running);
        }
        Message::SetBorderPreview { overlay_id } => {
            state.border_preview = overlay_id;
        }
        Message::Shutdown => return false,
        // Nothing to do, and deliberately so: the tray uses this to tell a live
        // renderer from a dead one, so anything it changed would be a bug.
        Message::Ping => {}
    }
    true
}

/// The shortest of the repaint cadences anything in the configuration asks for.
///
/// Lives here rather than in the configuration window because it reads the
/// overlay manager's prefill and border timers, and those windows live in this
/// process now.
fn repaint_interval(state: &State, overlays: &OverlayManager) -> Duration {
    let smooth_interval = if state.running {
        state
            .config
            .overlays
            .iter()
            .filter(|overlay| overlay.enabled && overlay.smooth_rendering)
            .map(|overlay| smooth_frame_interval(overlay.smooth_fps))
            .min()
    } else {
        None
    };
    smooth_interval
        .into_iter()
        .chain(overlays.prefill_repaint_interval())
        .chain(overlays.border_repaint_interval())
        .min()
        .unwrap_or(REPAINT_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wait must be bounded by the liveness cap and by the repaint
    /// deadline, whichever is nearer, and neither bound may be missed.
    ///
    /// The two failures this guards are both silent. A wait of the full repaint
    /// interval leaves the thread mute for a tenth of a second at a time, which
    /// is what Windows calls hung; a wait that ignores the deadline drifts the
    /// graph's cadence later on every pass. Driving it with a real clock is not
    /// possible, so the instants are synthetic and the whole table is exact.
    #[test]
    fn the_wait_is_bounded_by_both_the_cap_and_the_deadline() {
        let now = Instant::now();
        let cases = [
            // (deadline offset from now, expected wait, what it is checking)
            (
                Duration::from_millis(1000),
                MESSAGE_POLL_INTERVAL,
                "a repaint far away still waits only the cap, or this thread stops pumping",
            ),
            (
                Duration::from_millis(5),
                Duration::from_millis(5),
                "a repaint nearer than the cap waits exactly the remainder",
            ),
            (
                MESSAGE_POLL_INTERVAL,
                MESSAGE_POLL_INTERVAL,
                "a repaint exactly at the cap is the cap",
            ),
            (
                Duration::ZERO,
                Duration::ZERO,
                "a deadline already passed waits zero rather than underflowing",
            ),
        ];
        for (deadline, expected, what) in cases {
            assert_eq!(wait_before(now + deadline, now), expected, "{what}");
        }
    }

    /// The cap has to be short enough that Windows never sees a silent thread.
    ///
    /// Windows decides a window is hung after a few seconds of a thread not
    /// answering its messages, so a cap measured in hundreds of milliseconds
    /// would be fine in practice and still be a liveness bug in principle. The
    /// floor is stated as a bound on the constant rather than a comparison of
    /// two constants, which clippy would reject as an assertion with no
    /// possible failure.
    #[test]
    fn the_cap_leaves_windows_room_to_be_answered() {
        assert!(
            MESSAGE_POLL_INTERVAL >= Duration::from_millis(1),
            "a cap of {:?} is not a cadence at all",
            MESSAGE_POLL_INTERVAL
        );
        assert!(
            MESSAGE_POLL_INTERVAL <= REPAINT_INTERVAL,
            "the cap {:?} is longer than the idle repaint {:?}, so the repaint cadence \
             is no longer capped by anything",
            MESSAGE_POLL_INTERVAL,
            REPAINT_INTERVAL
        );
    }
}
