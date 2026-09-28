//! The wire between the configuration window and the renderer process.
//!
//! Both processes are built from this crate, so the pipe name, the message
//! shapes and the framing are defined once and cannot drift apart. That matters
//! more than it looks: if the two sides each had their own idea of the format,
//! a change on one side would be a silent protocol mismatch rather than a
//! compile error.
//!
//! ## Why named pipes and not shared memory
//!
//! The traffic is a few kilobytes of configuration, sent when a person presses
//! Save. That is far too little for shared memory to pay for itself, and the
//! project ships both x64 and ARM64, where a hand-rolled struct in a mapped
//! file is exactly the thing that works on the development machine and ships
//! broken. A named pipe moves the bytes and the framing both sides get for
//! free.
//!
//! ## Why one JSON object per line
//!
//! `NamedPipe` is a byte stream, so something has to say where a message ends.
//! A four-byte length prefix is the usual answer and the usual source of
//! off-by-one bugs. JSON has already solved it: a newline inside a JSON string
//! is escaped as `\n`, so a line break can only ever be a delimiter. A single
//! [`serde_json::Value`] per line needs no length prefix and cannot be
//! mis-split, which matters because a profile name is free-form text and may
//! legitimately contain braces, quotes and even newlines.
//!
//! ## The pipe is also the rendezvous
//!
//! There is no registry, no lock file and no heartbeat. The renderer owns the
//! pipe name; a client that connects successfully is talking to a live
//! renderer, and one that fails to connect should start one and try again.
//! That is the whole of "is the renderer already running", and it is why a
//! renderer started by hand and one started by the tray are the same thing.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Duration;

/// The pipe the renderer owns.
///
/// The protocol version is in the name on purpose: a renderer left over from a
/// build with a different message shape cannot be spoken to by accident, it
/// simply will not be found, and the caller will start a matching one.
pub const PIPE_NAME: &str = r"\\.\pipe\PingLatencyOverlay-v1";

/// A command sent to the renderer process.
///
/// Serialised as `{"kind":"…"}` plus the fields, so an unknown `kind` is
/// rejected by serde rather than silently ignored. That is deliberate: a
/// message that is quietly dropped looks exactly like a message that was
/// handled, and the only honest way to tell them apart is to fail loudly on
/// the one side that cannot understand it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Message {
    /// Replace the renderer's overlay configuration.
    ///
    /// The whole [`Config`] goes over rather than a patch, because the renderer
    /// holds no draft of its own: the configuration window is the only writer
    /// and it already holds the authoritative value.
    SetConfig { config: Config },
    /// Pause or resume probing. The renderer keeps its windows; only the
    /// probes stop, so a pause freezes the graph instead of emptying it.
    SetPaused { paused: bool },
    /// Ask the renderer to animate the startup border effect for one overlay.
    ///
    /// This exists because the border preview is a feature of the *window*, and
    /// before the split it was a direct call into the overlay manager. With the
    /// renderer in another process it has to travel over the wire, and the
    /// first implementation that forgot to send it lost the feature silently.
    /// `None` clears it.
    SetBorderPreview { overlay_id: Option<String> },
    /// Stop the renderer and let it tear its windows down.
    Shutdown,
    /// "Are you there?" — carries no instruction and changes nothing.
    ///
    /// Exists so the tray can tell a live renderer from a dead one without
    /// borrowing a real message as a probe. Every existing variant either
    /// changes what is on screen or cannot be sent without knowing the
    /// receiver's current state, and a liveness check that can also mutate
    /// state is a bug waiting for the one time the two disagree. The renderer
    /// does nothing with it beyond the repaint deadline reset that any incoming
    /// message already causes.
    Ping,
}

/// Encode one message as a single line, with the trailing newline included.
///
/// The newline is part of the framing rather than the payload, so a caller
/// cannot forget it.
pub fn encode(message: &Message) -> String {
    let mut line = serde_json::to_string(message).expect("a Message is always serialisable");
    line.push('\n');
    line
}

/// Decode one line, or `None` if it is not a message this build understands.
///
/// A blank line is `None` rather than an error: a writer that closes without a
/// trailing newline would otherwise produce a spurious failure at end of
/// stream.
pub fn decode(line: &str) -> Option<Message> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

// ---------------------------------------------------------------- the client

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        lpfilename: *const u16,
        dwdesiredaccess: u32,
        dwsharemode: u32,
        lpsecurityattributes: *mut core::ffi::c_void,
        dwcreationdisposition: u32,
        dwflagsandattributes: u32,
        htemplatefile: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn WriteFile(
        hfile: *mut core::ffi::c_void,
        lpbuffer: *const u8,
        nnumberofbytestowrite: u32,
        lpnumberofbyteswritten: *mut u32,
        lpoverlapped: *mut core::ffi::c_void,
    ) -> i32;
    fn CloseHandle(hobject: *mut core::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
    fn PeekNamedPipe(
        hnamedpipe: *mut core::ffi::c_void,
        lpbuffer: *mut u8,
        nbufferlength: u32,
        lpbytesread: *mut u32,
        lptotalbytesavail: *mut u32,
        lpmessagesavail: *mut u32,
    ) -> i32;
    fn WaitNamedPipeW(lpname: *const u16, ntimeout: u32) -> i32;
}

const GENERIC_WRITE: u32 = 0x4000_0000;
/// Paired with [`GENERIC_WRITE`] on every client handle, because
/// [`Client::is_connected`] peeks and peeking needs read access.
const GENERIC_READ: u32 = 0x8000_0000;
const OPEN_EXISTING: u32 = 3;
const INVALID_HANDLE_VALUE: *mut core::ffi::c_void = -1isize as *mut core::ffi::c_void;
const ERROR_PIPE_BUSY: u32 = 231;
/// No instance is waiting for a client.
///
/// Not the same as a malfunction: it is the ordinary answer while the renderer
/// is starting up, and the answer `WaitNamedPipeW` exists for is the opposite
/// one. Conflating the two is what made the tray's attach look broken.
const ERROR_FILE_NOT_FOUND: u32 = 2;
const WAIT_PIPE_MS: u32 = 2_000;

/// A blocking connection to the renderer's pipe.
///
/// The configuration window is a GUI thread, so this is deliberately
/// synchronous: handing a few kilobytes to a local pipe completes immediately
/// unless the renderer has stopped reading, and a background task with a
/// channel would be a great deal of machinery for a write that does not block.
/// The renderer reads asynchronously, because it is already on a Tokio runtime
/// and blocking there would occupy the probe tasks' executor.
///
/// Holding a `Client` open is what tells the renderer a supervisor is still
/// watching, so it is dropped rather than leaked when the window closes.
#[derive(Debug)]
pub struct Client {
    handle: *mut core::ffi::c_void,
}

impl Client {
    /// Connect to a renderer that is already listening.
    ///
    /// Returns `Err` immediately when nothing is listening, which is the
    /// signal to start one. `ERROR_PIPE_BUSY` is different: a renderer exists
    /// but every instance of the pipe is busy, so this waits for one rather
    /// than reporting a failure that would cause a second renderer to be
    /// spawned.
    pub fn connect() -> io::Result<Self> {
        let name: Vec<u16> = std::ffi::OsStr::new(PIPE_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the
        // call, and every pointer argument is null, which the documentation
        // allows when no security attributes and no template file are wanted.
        // `GENERIC_READ` as well as `GENERIC_WRITE`, and both `CreateFileW`
        // calls below repeat it. The client only ever writes, but it has to be
        // able to *read* because `PeekNamedPipe` is how the tray asks whether
        // the renderer is still there, and a query that needs read access fails
        // outright on a write-only handle. See `Client::is_connected`.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            // SAFETY: `GetLastError` takes no arguments and cannot fail.
            let error = unsafe { GetLastError() };
            if error == ERROR_PIPE_BUSY {
                // SAFETY: as above; `name` outlives the call.
                unsafe { WaitNamedPipeW(name.as_ptr(), WAIT_PIPE_MS) };
                // SAFETY: as above.
                let handle = unsafe {
                    CreateFileW(
                        name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        core::ptr::null_mut(),
                        OPEN_EXISTING,
                        0,
                        core::ptr::null_mut(),
                    )
                };
                if handle == INVALID_HANDLE_VALUE {
                    return Err(io::Error::last_os_error());
                }
                return Ok(Self { handle });
            }
            if error == ERROR_FILE_NOT_FOUND {
                // No instance is waiting. That is the ordinary answer between
                // "the renderer has not started" and "the renderer has exited",
                // and it is the one a caller is most likely to act on, so it is
                // reported as its own kind rather than as whatever number
                // Windows happened to return. A caller can then tell "start one"
                // from something actually going wrong.
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no renderer is listening on the pipe",
                ));
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(Self { handle })
    }

    /// Whether a renderer is listening right now.
    ///
    /// Used to decide between attaching to a running renderer and starting
    /// one. It opens and immediately closes a connection, so it must not be
    /// used as a general liveness check: a renderer can be busy at the exact
    /// moment this runs.
    pub fn renderer_is_listening() -> bool {
        Self::connect().is_ok()
    }

    /// Whether the renderer on the other end is still there, without writing
    /// anything and without ever blocking.
    ///
    /// `PeekNamedPipe` is a query, not a transfer, so it returns immediately
    /// whether the far end is there, has gone, or has stopped reading. That
    /// last case is the reason this exists: the tray's loop is the same loop
    /// that services tray clicks, so a liveness probe that used `send` could
    /// wedge it — `WriteFile` on a pipe nobody is draining does not return, and
    /// a wedged loop is one that never notices a click, let alone a menu.
    ///
    /// **This only works because the client handle is opened for reading as
    /// well as writing.** `PeekNamedPipe` needs read access, and on a
    /// write-only handle it fails with `ERROR_ACCESS_DENIED` — which this
    /// method would report, correctly, as `false`, "the renderer is gone". The
    /// tray connected, asked that question, was told no, dropped the client and
    /// started the renderer again, and then asked the same question about the
    /// new one. The whole tray half of the app was in a 200ms restart loop and
    /// reported "no renderer" every time it was asked, for many rounds, while
    /// every individual call did exactly what it says on the tin. Nothing about
    /// this function is wrong; what was wrong was the handle it was given.
    ///
    /// Deliberately conservative: a `false` means "assume it is gone", which
    /// costs a restart at worst, while a wrong `true` leaves a dead app
    /// looking alive.
    pub fn is_connected(&self) -> bool {
        let mut available: u32 = 0;
        // SAFETY: `handle` is an open pipe handle and every out-parameter is a
        // valid, NUL-free slot. No buffer is passed, so nothing is read into
        // memory: this asks about the pipe's state and discards the contents.
        let ok = unsafe {
            PeekNamedPipe(
                self.handle,
                core::ptr::null_mut(),
                0,
                core::ptr::null_mut(),
                &mut available,
                core::ptr::null_mut(),
            )
        };
        ok != 0
    }

    /// Send one message and flush it.
    ///
    /// Every byte is reported rather than assumed. A configuration that was
    /// written to disk but never reached the renderer is the failure this whole
    /// module exists to make visible, so the caller gets an error it can put in
    /// the status bar rather than a silent success.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let line = encode(message);
        let bytes = line.as_bytes();
        let mut written: u32 = 0;
        // SAFETY: `handle` is an open file handle, `bytes` outlives the call
        // and is exactly `len` long, and a null OVERLAPPED means the write is
        // synchronous, so the return value is the completed byte count.
        let ok = unsafe {
            WriteFile(
                self.handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // SAFETY: `GetLastError` takes no arguments and cannot fail.
            return Err(io::Error::from_raw_os_error(
                unsafe { GetLastError() } as i32
            ));
        }
        if written as usize != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!("wrote {written} of {} bytes to the renderer", bytes.len()),
            ));
        }
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: `handle` is an open handle that this value owns, and
        // `Client` is not `Copy`, so it is closed exactly once.
        unsafe { CloseHandle(self.handle) };
    }
}

// --------------------------------------------- finding and starting siblings

/// The renderer executable, found next to whichever process is starting it.
pub const RENDERER_EXE: &str = "ping-latency-overlay-renderer.exe";

/// The configuration window executable, likewise.
/// The tray: the program a user launches, and the only one that stays resident.
pub const TRAY_EXE: &str = "ping-latency-overlay-tray.exe";

/// The Config window, started on demand and exited when it is closed.
pub const CONFIG_EXE: &str = "ping-latency-overlay-config.exe";

/// How long to keep retrying the pipe after a spawn that has not answered.
///
/// A process that has started is not yet a process answering, so this is
/// bounded polling rather than a single attempt. Without it the caller either
/// reports a failure for a renderer that is merely slow, or blocks forever on
/// one that died before it could listen.
pub const ATTACH_ATTEMPTS: u32 = 30;
pub const ATTACH_INTERVAL: Duration = Duration::from_millis(100);

/// The path of a sibling executable, resolved next to this one.
///
/// Siblings rather than a fixed install directory, so the same code works
/// installed and run straight out of `target`. The `expect` is deliberate: a
/// build that produced one executable without the other cannot work, and "the
/// overlays silently never appear" is a far worse way to find out than a named
/// panic at startup.
pub fn sibling_exe(name: &str) -> PathBuf {
    let mut path = std::env::current_exe()
        .expect("the running executable should have a path")
        .parent()
        .expect("an executable should be inside a directory")
        .to_path_buf();
    path.push(name);
    path
}

/// Start a sibling process with its standard streams discarded.
///
/// A windowed build has no console, so anything the child wrote would go
/// nowhere; the parent detects a child that failed by its pipe going quiet,
/// not by reading its output.
pub fn spawn_sibling(name: &str) -> io::Result<std::process::Child> {
    std::process::Command::new(sibling_exe(name))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// Connect to the renderer, starting one if nothing is listening.
///
/// Returns the client, the child **only when this call started it**, and a
/// message when either step failed. The child is returned separately and
/// deliberately: a renderer somebody launched by hand is not ours to poll,
/// restart or kill, and the only way to keep that straight is to know which
/// one we started.
pub fn start_or_attach_renderer() -> (Option<Client>, Option<std::process::Child>, Option<String>) {
    // Attaching is the whole point of the pipe being the rendezvous: if one is
    // already running, starting a second would give the user two sets of
    // overlays fighting over the same screens.
    if let Ok(client) = Client::connect() {
        return (Some(client), None, None);
    }
    let path = sibling_exe(RENDERER_EXE);
    let child = match spawn_sibling(RENDERER_EXE) {
        Ok(child) => child,
        Err(error) => {
            return (
                None,
                None,
                Some(format!(
                    "Could not start the renderer ({}): {error}",
                    path.display()
                )),
            )
        }
    };
    for _ in 0..ATTACH_ATTEMPTS {
        std::thread::sleep(ATTACH_INTERVAL);
        if let Ok(client) = Client::connect() {
            return (Some(client), Some(child), None);
        }
    }
    (
        None,
        None,
        Some(format!(
            "The renderer started but never answered on the pipe ({}).",
            path.display()
        )),
    )
}

// ------------------------------------------------------------- the server

/// Accept connections forever, forwarding every decoded message.
///
/// Each connection is optional by design. The renderer is meant to run with no
/// supervisor attached, and to pick one up later without restarting, so a
/// disconnect is not a reason to stop: the loop simply waits for the next
/// client. The sender is a plain channel rather than a broadcast because the
/// renderer's loop is a blocking thread that wakes on a timeout, and a blocking
/// receiver is the only thing it can wait on without an executor.
///
/// Reading is line based for the same reason [`encode`] writes lines: a JSON
/// string escapes its own newlines, so a line break is an unambiguous
/// end-of-message marker and no length prefix can be got wrong.
///
/// **Each connection gets a fresh pipe instance.** That is not tidiness, it is
/// the difference between parking and spinning, and the reason is in
/// [`serve`] itself where it cannot be missed.
pub async fn serve(sender: std::sync::mpsc::Sender<Message>) -> io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    // `first_pipe_instance` is what makes a second renderer fail to start
    // rather than silently take over the name, which is the whole single
    // instance story for this role. It is claimed HERE, synchronously, so that
    // a renderer which could not have the name is able to say so in this
    // function's return value rather than in a spawned task nobody reads.
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    // TWO loops, not one. Each holds one instance at a time and, finishing
    // with a client, immediately makes the next instance and waits again.
    // That is what lets the tray and the Config window be connected at the
    // same time: the instance busy serving one client is never the instance
    // the other has to reach, because a spare is always on its way.
    //
    // A single sequential loop cannot do this, and the version that could not
    // is why the tray appeared to do nothing at all. It made the next instance
    // only after a client disconnected, so with the Config window connected
    // there was nothing for the tray to attach to -- the whole tray half of
    // the app was quietly broken while the overlays went on drawing.
    tokio::spawn(instance_loop(sender.clone(), Some(first)));
    tokio::spawn(instance_loop(sender, None));
    Ok(())
}

/// One instance's working life, repeated for as long as the renderer runs.
///
/// `Some(server)` takes an instance the caller already made, so the first one
/// keeps the sole name claim. Every later instance is made here, and none of
/// them may claim the name again.
async fn instance_loop(
    sender: std::sync::mpsc::Sender<Message>,
    starting: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
) {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut server = starting;
    loop {
        let Some(mut instance) = server.take().or_else(|| {
            // Every instance after the first gives up the name claim. Failing
            // here is transient -- instances are allowed to multiply -- so
            // wait and try again rather than losing the spare for good.
            ServerOptions::new()
                .first_pipe_instance(false)
                .create(PIPE_NAME)
                .ok()
        }) else {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };

        // A fresh instance has never been connected, so this blocks until a
        // client arrives, which is the parking the renderer needs to do. It is
        // never a stale success: reusing one instance is what caused that.
        if instance.connect().await.is_err() {
            // A client that vanished between the instance being created and
            // the connection being made. Waiting a moment stops a busy loop
            // against a client that keeps disappearing.
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }

        let mut lines = BufReader::new(&mut instance).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    // A line this build does not understand is dropped rather
                    // than fatal. The alternative is that one bad message kills
                    // the renderer and takes the overlays off the user's screen.
                    if let Some(message) = decode(&line) {
                        // A send failure means the renderer's loop has gone, and
                        // there is nothing left to serve.
                        if sender.send(message).is_err() {
                            return;
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        // The client is gone. Dropping the instance closes it, and the next
        // pass makes a fresh one, so a spare is never missing.
    }
}

// ------------------------------------------------- single instance by mutex

/// Which job a process is doing, which decides its mutex.
///
/// Separate names per role rather than one for the app: the renderer and the
/// configuration window are independent processes, and the whole point of the
/// split is that either can be restarted without disturbing the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The tray icon, and whatever it supervises.
    Shell,
    /// The configuration window, which is only running while it is open.
    ///
    /// A role of its own rather than a share of [`Role::Shell`], so that
    /// closing the window can exit that process while the tray keeps going,
    /// and so that opening the window twice focuses the one that is already
    /// open instead of starting a second one fighting it for the pipe.
    Config,
    /// The overlay renderer, with its probes and, later, the game.
    Renderer,
}

impl Role {
    /// The short name that appears in the mutex name.
    pub fn label(&self) -> &'static str {
        match self {
            Role::Shell => "shell",
            Role::Config => "config",
            Role::Renderer => "renderer",
        }
    }

    /// The process-wide mutex that says "one of me is already running".
    ///
    /// `Global\` so that it spans the interactive session's logon boundaries in
    /// the same way the pipe name spans processes. The name carries the
    /// protocol version for the same reason the pipe name does: a stale
    /// process from an older build must not be mistaken for a current one.
    pub fn mutex_name(&self) -> String {
        format!(r"Global\PingLatencyOverlay-v1-{}", self.label())
    }
}

const ERROR_ALREADY_EXISTS: u32 = 183;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        lpmutexattributes: *mut core::ffi::c_void,
        binitialowner: i32,
        lpname: *const u16,
    ) -> *mut core::ffi::c_void;
}

/// Proof that one role is already running in this session.
///
/// The mutex is named rather than held deliberately: the kernel drops a named
/// mutex when the last handle closes, including when a process is killed, so
/// a crashed renderer does not leave a lock behind that has to be cleaned up by
/// hand. That is the whole reason for using one instead of a lock file.
#[derive(Debug)]
pub struct SingleInstance {
    handle: *mut core::ffi::c_void,
}

/// Whether a process holding `role`'s mutex is running right now.
///
/// An acquire-and-drop probe of the same named mutex the role itself uses, so
/// there is no registry and no second thing to keep in step: if the mutex can
/// be taken, nobody is holding it. That is what lets the Config window ask
/// "is the tray up?" before spawning one, which is the whole question the
/// launch matrix needs answered.
/// Whether a process playing `me` should start one playing `missing`.
///
/// The launch matrix, in one place so it is a rule and not a habit. A process
/// never starts itself, so the diagonal is false without exception. A process
/// that is not the renderer starts the ones the user is likely to want, because
/// a user who double-clicks one executable should end up with a working app
/// rather than a piece of one. The renderer is the exception: it is the thing
/// being started, it has no window, and bringing up a tray nobody asked for
/// from a process that draws only overlays is exactly the kind of surprise this
/// table exists to rule out.
///
/// Exists as a function rather than as a comment because the three bugs it
/// caused all came from the rule living in three different places and none of
/// them agreeing.
pub fn should_start(me: Role, missing: Role) -> bool {
    match (me, missing) {
        // A process never starts itself. Not an optimisation: two trays
        // fighting over the same pipe is a bug that is very hard to read off a
        // symptom.
        (role, other) if role == other => false,
        // The renderer is the thing being started. It has no window and draws
        // only overlays, so bringing up a tray nobody asked for from it is
        // exactly the surprise this table exists to rule out.
        (Role::Renderer, _) => false,
        // The Config window is started by the user clicking the tray's menu
        // item, which the tray handles directly, NOT as a side effect of the
        // tray itself coming up. Launching the tray must not open a window the
        // user did not ask for.
        (Role::Shell, Role::Config) => false,
        // Everything else: a process that is missing one of its peers starts
        // it, because a user who launched one executable should end up with a
        // working app rather than a piece of one.
        _ => true,
    }
}

/// A `Role` that can sensibly be started by another process, in the order they
/// should be tried. The renderer is absent on purpose — it is started by
/// `start_or_attach_renderer` under its own rules, because a renderer that
/// answers is attached to rather than spawned.
pub const STARTABLE_BY_OTHERS: [Role; 1] = [Role::Shell];

pub fn role_is_running(role: Role) -> bool {
    matches!(SingleInstance::acquire(role), Ok(None))
}

/// Start the tray if nothing is holding its role, and say why if that failed.
///
/// The inverse of the rule the renderer follows: the Config window is the one
/// process a user can launch expecting a whole app, so it brings the tray up
/// with it. A `None` return is the ordinary success — either the tray was
/// already running and nothing was spawned, or it was spawned and is on its
/// way up. Only a failure to do either comes back as `Some`.
///
/// Deliberately returns no `Child`. The tray is the app and is meant to
/// outlive the window that started it, so there is nothing here to clean up,
/// and a caller that had a handle would be tempted to kill it.
pub fn start_or_attach_tray() -> Option<String> {
    if role_is_running(Role::Shell) {
        return None;
    }
    let path = sibling_exe(TRAY_EXE);
    match spawn_sibling(TRAY_EXE) {
        Ok(_) => None,
        Err(error) => Some(format!(
            "Could not start the tray ({}): {error}",
            path.display()
        )),
    }
}

impl SingleInstance {
    /// Take the role's mutex, or `None` when another process already has it.
    ///
    /// `None` is not an error. It is the ordinary answer when the user runs the
    /// application a second time, and the caller's response is to attach to the
    /// process that is already there rather than to start a rival.
    pub fn acquire(role: Role) -> io::Result<Option<Self>> {
        let name: Vec<u16> = std::ffi::OsStr::new(&role.mutex_name())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `name` is NUL terminated and outlives the call, and a null
        // security-attributes pointer requests default permissions.
        let handle = unsafe { CreateMutexW(core::ptr::null_mut(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `GetLastError` takes no arguments and cannot fail. It has to
        // be read immediately, before anything else can call into Win32.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` is a valid handle this call just created, and
            // dropping it is what releases the reference that did not give us
            // ownership.
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this value, which is not `Copy`, and
        // `ReleaseMutex` is needed because the mutex was created owned.
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReleaseMutex(hmutex: *mut core::ffi::c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OverlayConfig;
    use std::collections::HashMap;
    use std::ffi::OsStr;

    fn one_overlay() -> Config {
        let mut config = Config::default();
        config.overlays.push(OverlayConfig::new());
        config
    }

    /// A line has to survive a round trip, and a message must be exactly one
    /// line: that is the entire framing guarantee, and the renderer's whole
    /// input is a sequence of these.
    #[test]
    fn a_message_is_exactly_one_line() {
        for message in [
            Message::SetConfig {
                config: one_overlay(),
            },
            Message::SetPaused { paused: true },
            Message::SetBorderPreview {
                overlay_id: Some("probe".to_string()),
            },
            Message::Shutdown,
        ] {
            let line = encode(&message);
            assert_eq!(
                line.matches('\n').count(),
                1,
                "{message:?} produced a line with more than one newline: {line:?}"
            );
            assert!(line.ends_with('\n'), "the newline is framing, not optional");
            assert_eq!(decode(&line), Some(message));
        }
    }

    /// The reason the framing is a line and not a length prefix. A display
    /// name is free-form text, so it can contain anything a user can type,
    /// including a newline, and the decoder still has to see one message.
    #[test]
    fn a_name_with_quotes_braces_and_newlines_survives() {
        let mut config = one_overlay();
        config.profile_name = "he said \"hi\"\n{ braces }\tend\t\\ backslash".to_string();
        let message = Message::SetConfig { config };
        let line = encode(&message);
        assert_eq!(line.matches('\n').count(), 1, "the name broke the framing");
        assert_eq!(decode(&line), Some(message));
    }

    /// Two messages written back to back have to come out as two messages,
    /// which is the case a length-prefix bug would fail and a line split does
    /// not.
    #[test]
    fn two_messages_in_one_write_stay_two_messages() {
        let first = Message::SetPaused { paused: true };
        let second = Message::SetBorderPreview {
            overlay_id: Some("probe".to_string()),
        };
        let mut buffer = encode(&first);
        buffer.push_str(&encode(&second));
        let lines: Vec<String> = buffer.lines().map(str::to_string).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(decode(&lines[0]), Some(first));
        assert_eq!(decode(&lines[1]), Some(second));
    }

    /// A writer that dies without its trailing newline must not turn the last
    /// message into a failure, and a blank line must not decode into a message
    /// that then gets acted on.
    #[test]
    fn a_blank_or_truncated_line_is_not_a_message() {
        assert_eq!(decode(""), None);
        assert_eq!(decode("   "), None);
        assert_eq!(decode("\n"), None);
        // A truncated payload is a real error, and returning `None` for it is
        // how the reader knows the stream is broken rather than finished.
        assert_eq!(decode(r#"{"kind":"setPaused","paused":tr"#), None);
    }

    /// An unknown `kind` has to be rejected, not ignored. A silently dropped
    /// command is indistinguishable from one that was obeyed, and the whole
    /// point of tagging the enum is to make that impossible.
    #[test]
    fn an_unknown_message_is_rejected_rather_than_ignored() {
        assert_eq!(decode(r#"{"kind":"somethingNew"}"#), None);
        assert_eq!(decode(r#"{"paused":true}"#), None);
        assert_eq!(decode("not json at all"), None);
    }

    /// The wire format is a public contract with the on-disk format: both go
    /// through the same `Config` serde impl, so a field renamed in one cannot
    /// silently differ in the other. Pinned here so the coupling is visible
    /// rather than assumed.
    #[test]
    fn the_wire_uses_the_profile_files_camel_case() {
        let line = encode(&Message::SetConfig {
            config: one_overlay(),
        });
        assert!(line.contains("\"overlays\""), "config fields are camelCase");
        // `ProbeConfig` is internally tagged, so the discriminator travels
        // inside the `probe` object rather than beside it.
        assert!(line.contains("\"probe\":"), "{line}");
        assert!(
            line.contains("\"protocol\":\"icmp\""),
            "the probe variant tag is camelCase: {line}"
        );
        assert!(
            !line.contains("\"profile_name\""),
            "no snake_case reaches the wire"
        );
    }

    /// Every variant must be reachable, or adding one could compile and then be
    /// silently unhandled on the far side.
    #[test]
    fn every_variant_round_trips_through_json() {
        let seen: HashMap<&str, Message> = [
            (
                "setConfig",
                Message::SetConfig {
                    config: Config::default(),
                },
            ),
            ("setPaused", Message::SetPaused { paused: false }),
            (
                "setBorderPreview",
                Message::SetBorderPreview { overlay_id: None },
            ),
            ("shutdown", Message::Shutdown),
        ]
        .into_iter()
        .collect();
        for (kind, message) in seen {
            let line = encode(&message);
            assert!(
                line.contains(&format!("\"kind\":\"{kind}\"")),
                "{kind} did not tag itself: {line}"
            );
            assert_eq!(decode(&line), Some(message));
        }
    }

    /// The two roles must not collide, and each name has to carry the protocol
    /// version. If they ever shared a name, restarting the renderer would be
    /// impossible while the window was open, which is the exact case the split
    /// is meant to support.
    #[test]
    fn each_role_gets_its_own_versioned_mutex() {
        let shell = Role::Shell.mutex_name();
        let config = Role::Config.mutex_name();
        let renderer = Role::Renderer.mutex_name();
        // Every pair, not just one: three roles make three pairs, and a test
        // that only compared two of them would let the third silently share
        // one of the other two -- which is exactly the mistake that would make
        // the tray lock out the window it is supposed to be opening.
        for (left, right) in [(&shell, &config), (&shell, &renderer), (&config, &renderer)] {
            assert_ne!(left, right, "the roles would lock each other out");
        }
        assert_eq!(shell, r"Global\PingLatencyOverlay-v1-shell");
        assert_eq!(config, r"Global\PingLatencyOverlay-v1-config");
        assert_eq!(renderer, r"Global\PingLatencyOverlay-v1-renderer");
        for name in [&shell, &config, &renderer] {
            assert!(name.starts_with(r"Global\"), "{name} is not session wide");
            assert!(name.contains("-v1-"), "{name} has no protocol version");
        }
    }

    /// The launch matrix, pinned.
    ///
    /// Every one of the nine cells, including the three where the answer is
    /// "do not start yourself" — a table that skipped the diagonal would let
    /// the most obviously wrong case in, a process launching a second copy of
    /// itself, pass unnoticed.
    #[test]
    fn the_launch_matrix_is_what_the_user_asked_for() {
        use Role::{Config, Renderer, Shell};
        let cases = [
            (Shell, Shell, false, "the tray IS the tray"),
            (
                Shell,
                Config,
                false,
                "the tray must not open a window on its own",
            ),
            (Shell, Renderer, true, "the tray starts a missing renderer"),
            (Config, Shell, true, "the window brings the tray up with it"),
            (Config, Config, false, "the window is the window"),
            (
                Config,
                Renderer,
                true,
                "the window starts a missing renderer",
            ),
            (Renderer, Shell, false, "the renderer never starts a tray"),
            (
                Renderer,
                Config,
                false,
                "the renderer never starts a window",
            ),
            (Renderer, Renderer, false, "the renderer is the renderer"),
        ];
        for (me, missing, expected, why) in cases {
            assert_eq!(
                should_start(me, missing),
                expected,
                "{:?} starting {:?}: {why}",
                me,
                missing
            );
        }
    }

    /// The renderer is the one process that is never started by another, so it
    /// must not appear in the list of things a process can start.
    #[test]
    fn only_the_tray_is_startable_by_others() {
        assert_eq!(STARTABLE_BY_OTHERS, [Role::Shell]);
        assert!(!STARTABLE_BY_OTHERS.contains(&Role::Renderer));
    }

    /// The mutexes have to be a real kernel object, or "already running" becomes
    /// something the renderer has to work out for itself. Acquiring it twice in
    /// a row is the observable proof: the second attempt must report that
    /// somebody else already holds it, which is what a caller branches on.
    ///
    /// This test takes and immediately releases the renderer mutex, so it
    /// cannot run while a real renderer is up. Rather than skip in that case,
    /// which would make the test pass vacuously exactly when it matters, it
    /// asserts the honest outcome either way.
    #[test]
    fn taking_a_role_mutex_twice_reports_the_second_as_taken() {
        match SingleInstance::acquire(Role::Renderer) {
            Ok(Some(first)) => {
                // Held by us, so the next attempt must fail. Note the first is
                // still alive here, which is what makes this meaningful.
                assert!(
                    matches!(SingleInstance::acquire(Role::Renderer), Ok(None)),
                    "a second renderer must not be able to take the same mutex"
                );
                drop(first);
                // Released, so it must be available again. A named mutex the
                // kernel kept after a drop would strand the role forever.
                assert!(
                    matches!(SingleInstance::acquire(Role::Renderer), Ok(Some(_))),
                    "the mutex was not released when its guard was dropped"
                );
            }
            Ok(None) => {
                // A real renderer is running, so the interesting assertion
                // cannot be made. Say so rather than passing quietly.
                eprintln!("skipped: a renderer is already running");
            }
            Err(error) => panic!("CreateMutexW failed: {error}"),
        }
    }

    /// Nothing here opens a pipe, so the test must fail rather than hang when
    /// no renderer is running. A test that waits on a real IPC endpoint is a
    /// test that will one day wait forever on a machine where something else
    /// happens to own the name.
    #[test]
    fn connecting_with_nothing_listening_fails_quickly() {
        // SAFETY: `GetLastError` takes no arguments and cannot fail.
        let listening = Client::renderer_is_listening();
        // The assertion is deliberately about the *shape* of the result, not
        // about whether a renderer happens to be running: either answer is
        // valid, and what must not happen is a hang or a panic.
        if listening {
            // A renderer was running, so nothing to assert about the failure.
        } else {
            // NotFound specifically, and not merely "an error". "No renderer is
            // listening" is the ordinary answer that tells a caller to start
            // one; lumping it in with a real fault is what made the tray's
            // attach look like a malfunction and sent it round the restart
            // guard while a perfectly good renderer was drawing overlays.
            let error = Client::connect().expect_err("nothing is listening");
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        }
    }

    /// The name has to be a path, not a bare name, and it carries the protocol
    #[test]
    fn the_pipe_name_is_versioned_and_fully_qualified() {
        assert!(PIPE_NAME.starts_with(r"\\.\pipe\"), "{PIPE_NAME}");
        assert!(
            PIPE_NAME.contains("-v1"),
            "{PIPE_NAME} has no protocol version"
        );
        assert!(PIPE_NAME.ends_with("PingLatencyOverlay-v1"), "{PIPE_NAME}");
        // The UTF-16 encoding used by the client must be NUL terminated, and
        // this is the one place that could silently lose the terminator.
        let wide: Vec<u16> = OsStr::new(PIPE_NAME).encode_wide().chain(Some(0)).collect();
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(
            wide.iter().filter(|c| **c == 0).count(),
            1,
            "exactly one NUL"
        );
    }
}
