//! Telling a silent failure from a feature that does not exist.
//!
//! Every one of the three processes is a `windows_subsystem = "windows"`
//! binary, which means **stdout and stderr go nowhere**. That is the whole
//! reason a broken tray menu was indistinguishable from a working one: the
//! code had messages for exactly these failure modes, and not one of them could
//! reach anybody. The third release of the split shipped with an unreachable
//! Config window and a tray that did nothing when clicked, and the only symptom
//! was "it does not do anything".
//!
//! So there are two ways out, and a process with no window of its own needs
//! both. [`log_line`] writes to a file next to the config, which is where
//! everything that is merely wrong goes. [`fatal`] puts up a `MessageBox`,
//! which is the only way the tray can tell a user anything, because the tray is
//! the one process in this app that never draws a window.
//!
//! Nothing here is required for the app to work, and a failure to write the log
//! is swallowed: a diagnostics path that can itself take the app down would be
//! worse than having none.
//!
//! The log is a development and support tool, not a product feature: a release
//! build writes nothing unless `PLO_LOG` is set, so an ordinary install does not
//! accumulate a file forever.

use std::io::Write;

/// Whether anything should reach the log file.
///
/// A debug build always logs; a release build logs only when `PLO_LOG` names
/// something. The log exists so a silent failure can be told from a feature
/// that was never written, which is a problem during development and a support
/// case — not something an ordinary install should accumulate a file for.
pub fn enabled() -> bool {
    log_enabled(
        cfg!(debug_assertions),
        std::env::var_os("PLO_LOG").as_deref(),
    )
}

/// The decision behind [`enabled`], with both inputs passed in.
///
/// Split out so a test can drive every combination without mutating the process
/// environment, the same reason `resolve_config_dir` is.
pub fn log_enabled(debug: bool, override_value: Option<&std::ffi::OsStr>) -> bool {
    debug || override_value.is_some_and(|value| !value.is_empty())
}

/// Append one line to the shared log, tagged with the process and the time.
///
/// A failure here is ignored on purpose. The log is a courtesy, and a user
/// whose config directory is read-only still needs the app to start.
pub fn log_line(process: &str, message: &str) {
    if !enabled() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    else {
        return;
    };
    let _ = writeln!(file, "[{stamp}] {process}: {message}", stamp = timestamp());
}

/// Where the log lives: beside the config, in a file the user can be told to
/// look at. The config directory already exists by the time anything is worth
/// logging, and a log in `%TEMP%` would be gone before anyone could read it.
pub fn log_path() -> std::path::PathBuf {
    crate::config::config_dir().join("pinglatencyoverlay.log")
}

/// Show a `MessageBox` and carry on.
///
/// The tray is the only process here that never draws a window, so without a
/// dialog a fatal mistake in it is completely invisible. A non-fatal
/// consequence of that is that this is unusable from a build with no attached
/// console, which is exactly the build that needs it.
pub fn fatal(process: &str, message: &str) {
    log_line(process, &format!("FATAL: {message}"));
    let _ = message_box("PingLatencyOverlay", message);
}

/// Seconds since the Unix epoch, as a plain number.
///
/// Not a formatted clock, and that is a deliberate trade: this crate has no
/// date library and adding one to format a log line is not worth the
/// dependency. The number is monotonic and the file's own timestamps are there
/// when a human wants them.
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(windows)]
// The same allow the `win` modules in `overlay.rs` and the tray already carry:
// these names are Win32's, not ours, and renaming them to satisfy a lint would
// make them harder to look up than they are to tolerate.
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win {
    use core::ffi::c_void;

    pub type HWND = *mut c_void;
    pub const MB_OK: u32 = 0x0000_0000;
    pub const MB_ICONERROR: u32 = 0x0000_0010;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn MessageBoxW(
            hWnd: HWND,
            lpText: *const u16,
            lpCaption: *const u16,
            uType: u32,
        ) -> i32;
    }
}

#[cfg(windows)]
fn message_box(caption: &str, message: &str) -> Result<(), ()> {
    use std::os::windows::ffi::OsStrExt;

    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call, and
    // a null HWND means "no owner window", which is what a process with no
    // window wants. A failed MessageBox is reported by its zero return rather
    // than by `GetLastError`, which is all this caller can do about it.
    unsafe {
        let wide_caption: Vec<u16> = std::ffi::OsStr::new(caption)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_message: Vec<u16> = std::ffi::OsStr::new(message)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        win::MessageBoxW(
            std::ptr::null_mut(),
            wide_message.as_ptr(),
            wide_caption.as_ptr(),
            win::MB_OK | win::MB_ICONERROR,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn message_box(_caption: &str, message: &str) -> Result<(), ()> {
    eprintln!("{message}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log has to land beside the config, because that is the one path a
    /// user can be told to look at and the one directory that is guaranteed to
    /// exist by the time anything is worth logging.
    #[test]
    fn the_log_sits_beside_the_config() {
        let path = log_path();
        assert!(path.starts_with(crate::config::config_dir()), "{path:?}");
        // `Path::ends_with` compares whole components, so `.ends_with(".log")`
        // is always false and a string suffix has to be asked for explicitly.
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("log"));
    }

    /// Writing must not be able to take the app down, and that includes the
    /// case where the log cannot be opened at all. A diagnostics path that
    /// panics is worse than one that is missing.
    #[test]
    fn logging_never_fails_the_caller() {
        log_line("test", "a line written on purpose");
        log_line("", "");
    }

    /// The log is off for an ordinary release install and on for a development
    /// build or a support session.
    ///
    /// Both inputs are parameters so this test never touches the process
    /// environment: another test thread reading `PLO_LOG` at the same time would
    /// make it flaky, the same reason `resolve_config_dir` is split out.
    #[test]
    fn the_log_is_off_in_a_release_build_unless_asked() {
        use std::ffi::OsStr;

        assert!(log_enabled(true, None), "a debug build logs");
        assert!(
            log_enabled(true, Some(OsStr::new(""))),
            "an empty value changes nothing in a debug build"
        );
        assert!(
            log_enabled(false, Some(OsStr::new("1"))),
            "PLO_LOG brings the log back in a release build"
        );
        assert!(
            !log_enabled(false, None),
            "a release build is silent by default"
        );
        assert!(
            !log_enabled(false, Some(OsStr::new(""))),
            "an empty PLO_LOG is not a request"
        );
    }
}
