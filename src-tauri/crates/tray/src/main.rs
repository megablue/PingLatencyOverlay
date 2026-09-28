//! The tray icon, and the process that owns it.
//!
//! This is the application the user launches. It has no `eframe`, no GPU
//! context and no event loop: it shows a tray icon, makes sure a renderer is
//! running, and relays menu choices to that renderer down the pipe. The
//! configuration window is a *separate* process, started on demand, because a
//! window that creates an OpenGL context costs tens of megabytes of driver
//! memory for as long as it exists â€” memory this process used to pay for the
//! whole session, including every session where the window was never opened.
//!
//! So the two things a user can lose by this process exiting are handled
//! separately, and that is what the two exit menu items are for: **Exit**
//! stops the renderer and takes the overlays off the screen, while **Close
//! tray, keep overlays running** leaves the renderer alone. A renderer
//! somebody else launched is never stopped either way, because this process
//! only ever stops a child it started itself.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::Child;
use std::time::{Duration, Instant};

use ping_latency_overlay_core::diagnostics;
use ping_latency_overlay_core::transport::{
    self, Client, Message, Role, SingleInstance, CONFIG_EXE,
};
use ping_latency_overlay_tray::{TrayAction, TrayState};

/// How often the loop wakes.
///
/// The tray has nothing to redraw, so this is only a ceiling on how long a
/// menu click or a renderer death goes unnoticed. It is far longer than the
/// Config window needs because this process has no drawing to do at all.
const TICK: Duration = Duration::from_millis(200);

/// How often the tray's thread looks at its own message queue.
///
/// Sixteen milliseconds, so a click is acted on almost immediately. It is
/// separate from `TICK` because the two answer different questions: how often a
/// user should feel a click, and how often the renderer should be looked at.
const PUMP_INTERVAL: Duration = Duration::from_millis(16);

/// How long a renderer is given to answer its pipe after being started.
///
/// Generous, because the cost of waiting is nothing — the tray is idle either
/// way — and the cost of giving up too early is a crash counted that never
/// happened. Past this the attempt is abandoned and the ordinary crash path
/// takes over, which is the only place a restart is charged.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Restarts allowed inside [`RESTART_WINDOW`].
///
/// Five in a minute. A crash loop is worse than a stopped app: it burns a core
/// and buries the one message that would explain it, so past this the tray
/// stops trying and says so instead.
const RESTART_LIMIT: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// How a restart history expired.
///
/// Split out of the decision so a test can hold "an old attempt is forgotten"
/// as a statement about the list rather than inferring it from a boolean.
fn forget_expired(recent: &mut Vec<Instant>, now: Instant) {
    recent.retain(|at| now.duration_since(*at) < RESTART_WINDOW);
}

fn main() {
    let lock = match SingleInstance::acquire(Role::Shell) {
        Ok(Some(lock)) => lock,
        // Running the app twice is the ordinary answer, not a failure worth a
        // message box: the second tray would fight the first over the pipe.
        Ok(None) => return,
        Err(error) => {
            // A dialog, not a log line. The tray is the one process in this app
            // that never draws a window, so without this the failure is
            // completely invisible: the user clicks the shortcut and nothing
            // happens, ever, with nothing to look at.
            diagnostics::fatal(
                "tray",
                &format!(
                    "The tray could not claim its mutex: {error}\n\n\
                     Nothing is running. The log is in your config folder."
                ),
            );
            return;
        }
    };

    // Before any window or icon exists, so the icon is not scaled for the
    // wrong monitor.
    ping_latency_overlay_core::overlay::enable_dpi_awareness();

    let mut app = match App::new() {
        Ok(app) => app,
        Err(error) => {
            // The other startup failure a dialog is for: the tray could not
            // create its own icon, so there is nothing on screen to click.
            diagnostics::fatal(
                "tray",
                &format!("The tray icon could not be created: {error}"),
            );
            return;
        }
    };

    loop {
        // The tray icon is a window, and it belongs to THIS thread. Windows only
        // hands a window message to a thread that pumps its queue, so without
        // this call nothing this tray does can ever happen: the icon appears,
        // every click is dispatched to a thread that never reads its queue, and
        // the tray looks perfect and is completely dead. tray-icon documents the
        // requirement at the top of its own crate: "an event loop must be
        // running on the thread." It spawns no pump on Windows, so the caller
        // has to, and the sleep afterwards is the only thing keeping this loop
        // from spinning.
        ping_latency_overlay_core::overlay::pump_messages();
        if !app.step() {
            break;
        }
    }
    // Held for the process's lifetime; the point is that a second tray cannot
    // get past this line while this one is alive.
    drop(lock);
}

/// The tray's whole state: the icon, the renderer connection, and what this
/// process started and therefore owns.
struct App {
    tray: TrayState,
    client: Option<Client>,
    /// Only ever `Some` when **this** process started the renderer.
    renderer: Option<Child>,
    /// Only ever `Some` when **this** process started the Config window.
    config: Option<Child>,
    running: bool,
    restarts: Vec<Instant>,
    gave_up: bool,
    /// Loop passes so far, for the heartbeat. Not behaviour: it only decides
    /// how often the tray says it is still alive.
    passes: u32,
    /// How long it has been since the renderer was last looked at.
    ///
    /// The loop runs at `PUMP_INTERVAL` because a click must feel immediate,
    /// and the renderer does not need checking sixty times a second. Splitting
    /// the two is the point: one sleep cannot be right for both, and using the
    /// long one for the pump is what made every click in this tray do nothing.
    since_supervise: Duration,
    /// Set while a renderer is being brought up, with the time it started.
    ///
    /// This exists because "the renderer is not answering yet" and "the
    /// renderer crashed" were the same check, and a fresh renderer is briefly
    /// the first thing. So one slow start reported itself as a crash every
    /// 200ms, burned the whole storm guard in about a second, and left a tray
    /// that had quietly decided never to try again. Bringing one up is a
    /// separate state with its own deadline, and it does not count.
    starting: Option<Instant>,
}

impl App {
    fn new() -> Result<Self, String> {
        let tray = ping_latency_overlay_tray::create()
            .map_err(|error| format!("could not create the tray icon: {error}"))?;
        let (client, renderer, failure) = transport::start_or_attach_renderer();
        if let Some(message) = failure {
            // A tray with no renderer is still worth showing: the user can
            // open the window and read this, rather than finding an app that
            // silently does nothing.
            diagnostics::log_line("tray", &message);
        }
        Ok(Self {
            tray,
            client,
            renderer,
            config: None,
            running: true,
            restarts: Vec::new(),
            gave_up: false,
            since_supervise: Duration::ZERO,
            starting: None,
            passes: 0,
        })
    }

    /// One pass. Returns false when the process should end.
    fn step(&mut self) -> bool {
        // Once every few hundred passes, say that the loop turned at all. This
        // exists because the loop can wedge -- it used to, on a blocking write
        // in the liveness probe -- and the symptom was a tray that ignored
        // every click, with the log going silent and no way to tell that apart
        // from a feature that had not been written. A heartbeat makes "the
        // loop stopped" a fact in the log rather than an inference.
        self.passes += 1;
        if self.passes.is_multiple_of(250) {
            diagnostics::log_line("tray", "still running");
        }
        for action in self.tray.poll() {
            if !self.on_action(action) {
                return false;
            }
        }
        self.since_supervise += PUMP_INTERVAL;
        // Kept current every pass rather than only at the points it changes, so
        // the menu can never be a step behind the pipe. Cheap: one atomic load
        // and, only when it differs, one atomic store.
        let connected = self.client.is_some();
        if connected != self.tray.renderer_connected() {
            self.tray.set_renderer_connected(connected);
        }
        if self.since_supervise >= TICK {
            self.since_supervise = Duration::ZERO;
            self.supervise();
        }
        // 16ms rather than TICK: `pump_messages` peeks rather than waits, so on
        // its own this loop would spin a core. Sixteen milliseconds keeps a click
        // feeling instant while costing one `PeekMessage` on an empty queue sixty
        // times a second, which is nothing.
        std::thread::sleep(PUMP_INTERVAL);
        true
    }

    fn on_action(&mut self, action: TrayAction) -> bool {
        match action {
            TrayAction::Config => self.open_config(),
            TrayAction::ToggleRunning => {
                self.running = !self.running;
                // Both halves are logged, because a menu item that appears to
                // do nothing has two completely different causes — the click
                // never arrived, or the command never left — and they look
                // identical on screen. The old code discarded the send error, so
                // the second was indistinguishable from a paused graph.
                let paused = !self.running;
                diagnostics::log_line("tray", &format!("pause/resume: sending paused={paused}"));
                match self.send(&Message::SetPaused { paused }) {
                    Ok(()) => diagnostics::log_line("tray", "pause/resume: sent"),
                    Err(error) => {
                        // A failed send is the strongest evidence there is that
                        // the pipe is gone, so record it now rather than waiting
                        // for the next probe, and put `running` back: the menu
                        // then says "no renderer" and shows the real state on the
                        // very next right-click, which is the user being told
                        // the answer instead of being left to guess at it.
                        self.tray.set_renderer_connected(false);
                        self.running = !self.running;
                        diagnostics::log_line(
                            "tray",
                            &format!("pause/resume: could not reach the renderer: {error}"),
                        );
                    }
                }
                self.tray.set_running(self.running);
            }
            // Close the tray, leave the overlays on the screen. This is the
            // minimum-footprint mode: the renderer keeps running with nobody
            // supervising it, and re-running the app later brings a new tray
            // that attaches to it rather than starting a second.
            TrayAction::Detach => return false,
            TrayAction::Exit => {
                let _ = self.send(&Message::Shutdown);
                // Only a Config window this process started. One the user
                // launched by hand is left alone: the overlays stopping is
                // the message, and silently killing a window they opened is
                // not something an Exit menu item should also do.
                if let Some(mut child) = self.config.take() {
                    let _ = child.kill();
                }
                return false;
            }
        }
        true
    }

    fn open_config(&mut self) {
        // A window this tray started is cheap to check: its handle is right
        // here. A window the user launched by hand is not, and that is the case
        // that used to look broken — clicking the tray icon spawned a second
        // Config process, which exited at once on the mutex the first one holds,
        // and from where the user is sitting that is indistinguishable from the
        // click doing nothing. So ask the mutex, which is the same thing the
        // window itself uses to decide it is a duplicate.
        if let Some(child) = &mut self.config {
            if matches!(child.try_wait(), Ok(None)) {
                return; // The one we started is still open.
            }
            self.config = None;
        }
        if transport::role_is_running(transport::Role::Config) {
            diagnostics::log_line(
                "tray",
                "the window is already open; it was launched by something other than this tray, \
                 so it is left alone rather than started a second time",
            );
            return;
        }
        match transport::spawn_sibling(CONFIG_EXE) {
            Ok(child) => self.config = Some(child),
            Err(error) => diagnostics::log_line(
                "tray",
                &format!(
                    "could not open the window ({}): {error}",
                    transport::sibling_exe(CONFIG_EXE).display()
                ),
            ),
        }
    }

    fn send(&mut self, message: &Message) -> std::io::Result<()> {
        match self.client.as_mut() {
            Some(client) => client.send(message),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "no renderer is running",
            )),
        }
    }

    /// Whether the renderer has stopped answering.
    ///
    /// Asked with `PeekNamedPipe` rather than by writing. The write version was
    /// a probe that could block forever: `WriteFile` on a pipe nobody is
    /// draining does not come back, and this runs in `step`, which is the same
    /// loop that services tray clicks — so a wedged probe meant no click did
    /// anything at all, with no log line to say why. A query always returns.
    fn renderer_is_gone(&mut self) -> bool {
        if self.gave_up {
            return false;
        }
        if self.client.is_none() {
            // No client at all: either it was never established or the last one
            // died. Either way the renderer is not answering, and the storm
            // guard is what bounds how often this retries.
            return true;
        }
        if !self.client.as_ref().expect("checked").is_connected() {
            diagnostics::log_line("tray", "the renderer stopped answering: the pipe is gone");
            self.client = None;
            return true;
        }
        false
    }

    /// Put the renderer back if it is gone, whether or not this process started
    /// it.
    ///
    /// This watches the **pipe**, not the child handle, and that is the whole
    /// fix. The old version returned early whenever `renderer` was `None`, and
    /// that is exactly the attach case: a user who launched the Config window
    /// first leaves a renderer running that the tray did not spawn, so the tray
    /// refused to watch it and the app sat there with overlays frozen and no
    /// way back. "Only ever stop a process you did not spawn" is about *killing*
    /// something that has state you might destroy. A dead process has no state,
    /// and not restarting it leaves the user with an app that silently does
    /// nothing â€” which is worse than any rule this is protecting.
    ///
    /// Losing the pipe is the signal, and it is a better one than a child
    /// handle anyway: a renderer that hung without exiting still holds the
    /// mutex but stops answering, and a `None` client is what that looks like
    /// from here.
    fn supervise(&mut self) {
        // Phase one: a renderer is on its way up. Keep trying, and do not count
        // any of it against the storm guard, because a renderer that has not
        // answered yet is not a renderer that has crashed.
        if self.starting.is_some() {
            self.finish_starting();
            return;
        }

        if !self.renderer_is_gone() {
            return;
        }
        // A child we spawned is reaped here rather than left as a zombie. Not
        // killing it: it is already gone, this is only collecting the status.
        if let Some(child) = &mut self.renderer {
            let _ = child.try_wait();
            self.renderer = None;
        }
        self.client = None;

        let now = Instant::now();
        forget_expired(&mut self.restarts, now);
        if self.restarts.len() >= RESTART_LIMIT {
            self.gave_up = true;
            // Logged, because setting this flag used to be completely silent and
            // the result was a tray that had quietly decided never to try again
            // with the user left believing the app had simply given up on its
            // own. A guard that stops must always say so.
            diagnostics::log_line(
                "tray",
                &format!(
                    "gave up restarting the renderer after {RESTART_LIMIT} attempts in {}s; \
                     it will not try again, use the tray's Exit and start the app again",
                    RESTART_WINDOW.as_secs()
                ),
            );
            return;
        }
        self.restarts.push(now);
        diagnostics::log_line(
            "tray",
            &format!(
                "restarting the renderer (attempt {} of {RESTART_LIMIT})",
                self.restarts.len()
            ),
        );
        self.starting = Some(now);
        let (client, renderer, failure) = transport::start_or_attach_renderer();
        self.client = client;
        self.renderer = renderer;
        if let Some(message) = failure {
            diagnostics::log_line("tray", &message);
        }
    }

    /// Second half of bringing a renderer up: wait for its pipe, then hand it
    /// the config, retrying both until they work.
    ///
    /// The config push used to be fire-and-forget with its error thrown away,
    /// which is how a relaunched renderer could come back with no overlays and
    /// no explanation: the tray believed it had configured it, and the error
    /// that would have said otherwise was discarded. A push that has not
    /// landed is not a push that worked, so this keeps trying and says so.
    fn finish_starting(&mut self) {
        if self.client.is_none() {
            match transport::Client::connect() {
                Ok(client) => {
                    diagnostics::log_line("tray", "the new renderer answered the pipe");
                    self.client = Some(client);
                }
                Err(_) => {
                    if self.starting.expect("checked").elapsed() > START_TIMEOUT {
                        diagnostics::log_line(
                            "tray",
                            "the new renderer never answered the pipe; trying again",
                        );
                        // Back to the crash path, which counts an attempt.
                        self.starting = None;
                    }
                    return;
                }
            }
        }
        let config = ping_latency_overlay_core::config::load().config;
        match self.send(&Message::SetConfig { config }) {
            Ok(()) => {
                diagnostics::log_line("tray", "the new renderer has its config");
                self.starting = None;
            }
            Err(error) => diagnostics::log_line(
                "tray",
                &format!("the new renderer has not taken its config yet: {error}"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crash loop must be stopped, and a rare failure must not be.
    ///
    /// The clock is a parameter and synthetic instants are used, so a whole
    /// restart history runs in microseconds instead of waiting a real minute.
    /// Both halves are needed: a test that only checks the refusal cannot tell
    /// a working window from one that has swallowed every attempt.
    #[test]
    fn a_crash_loop_is_stopped_and_a_rare_failure_is_not() {
        let start = Instant::now();
        let mut tight: Vec<Instant> = Vec::new();
        for n in 0..=RESTART_LIMIT {
            let at = start + Duration::from_millis(10 * n as u64);
            forget_expired(&mut tight, at);
            let refused = tight.len() >= RESTART_LIMIT;
            if !refused {
                tight.push(at);
            }
            if n < RESTART_LIMIT {
                assert!(!refused, "attempt {n} inside the window should be allowed");
            } else {
                assert!(refused, "attempt {n} should have been refused");
            }
        }
        assert!(
            tight.len() >= RESTART_LIMIT,
            "a refused attempt must still be remembered, or the guard never trips"
        );

        let mut rare: Vec<Instant> = Vec::new();
        for n in 0..(RESTART_LIMIT * 2) {
            let at = start + Duration::from_secs(3 * 3_600 * n as u64);
            forget_expired(&mut rare, at);
            let refused = rare.len() >= RESTART_LIMIT;
            if !refused {
                rare.push(at);
            }
            assert!(!refused, "one failure every three hours, #{n}");
        }
        // One whole attempt-step PAST the last one. Landing exactly on it would
        // be wrong: an attempt at `now` has age zero, which is inside the
        // window, so it is correctly still remembered. `now` before the last
        // attempt is worse than wrong, it is a subtraction that goes backwards.
        forget_expired(
            &mut rare,
            start + Duration::from_secs(3 * 3_600 * (RESTART_LIMIT as u64 * 2)),
        );
        assert!(
            rare.is_empty(),
            "every attempt more than a window old should be forgotten, {} left",
            rare.len()
        );
    }
}
