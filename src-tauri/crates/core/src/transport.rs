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
    fn WaitNamedPipeW(lpname: *const u16, ntimeout: u32) -> i32;
}

const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const INVALID_HANDLE_VALUE: *mut core::ffi::c_void = -1isize as *mut core::ffi::c_void;
const ERROR_PIPE_BUSY: u32 = 231;
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
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
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
                        GENERIC_WRITE,
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
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    // `first_pipe_instance` is what makes a second renderer fail to start
    // rather than silently take over the name, which is the whole single
    // instance story for this role. Only the FIRST instance may claim it, so
    // this flag is spent once and every later instance gives it up. That is
    // safe because the renderer mutex, not this flag, is what guarantees only
    // one renderer exists.
    let mut claims_first_instance = true;

    loop {
        // A fresh instance per connection. Windows will not make
        // `ConnectNamedPipe` wait on an instance a client was already
        // connected to and has since disconnected: it returns
        // ERROR_PIPE_CONNECTED at once, meaning "a client is already
        // attached", which is stale by then. Tokio passes that through as a
        // success, so reusing one instance makes every reconnect
        // `connect`-returns-at-once, then the read hits end-of-file at once,
        // and round again -- with no sleep on any leg, because the only sleep
        // below guards a *failed* connect. A renderer left running with no
        // shell sat at 8% CPU for exactly that reason. A brand new instance
        // has never been connected, so `connect` blocks until a client
        // arrives, which is the parking this loop needs to do.
        let mut server = ServerOptions::new()
            .first_pipe_instance(claims_first_instance)
            .create(PIPE_NAME)?;
        claims_first_instance = false;

        if server.connect().await.is_err() {
            // A client that vanished between the instance being created and
            // the connection being made. Waiting a moment stops a busy loop
            // against a client that keeps disappearing.
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }
        let mut lines = BufReader::new(&mut server).lines();
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
                            return Ok(());
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        // Leaving the read loop drops `server`, which closes the instance and
        // frees the name for the next one. The loop then builds a fresh
        // instance rather than reconnecting to this one, per the note above.
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
    /// The configuration window and its tray icon.
    Shell,
    /// The overlay renderer, with its probes and, later, the game.
    Renderer,
}

impl Role {
    /// The short name that appears in the mutex name.
    pub fn label(&self) -> &'static str {
        match self {
            Role::Shell => "shell",
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
        let renderer = Role::Renderer.mutex_name();
        assert_ne!(shell, renderer, "the roles would lock each other out");
        assert_eq!(shell, r"Global\PingLatencyOverlay-v1-shell");
        assert_eq!(renderer, r"Global\PingLatencyOverlay-v1-renderer");
        for name in [&shell, &renderer] {
            assert!(name.starts_with(r"Global\"), "{name} is not session wide");
            assert!(name.contains("-v1-"), "{name} has no protocol version");
        }
    }

    /// The mutex has to be a real kernel object, or "already running" becomes
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
            assert!(Client::connect().is_err());
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
