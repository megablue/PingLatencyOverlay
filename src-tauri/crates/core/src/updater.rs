//! Whether a newer release exists, and what happened when that was last looked
//! up.
//!
//! The app checks GitHub once a day and offers a link when something newer is
//! published. It does not download, verify or run anything: the check is a
//! single GET over TLS whose answer is either "up to date" or a URL, so the
//! worst a hostile answer can do is take the user to a page — which they can
//! read before clicking. That is the whole trade, and it is deliberate. A bad
//! release is a problem for GitHub to solve; a bad *installer* would be this
//! app's.
//!
//! The pieces, and why the seams are where they are:
//!
//! - [`Release`] and [`Version`] are the answer's shape, and the semantics of
//!   "newer" are a pure function of the two, so they are testable without a
//!   network.
//! - [`due`] is the cadence decision, and [`UpdateState`] is what persists it
//!   across restarts. Both take their inputs as arguments — a test does not
//!   need to fake a clock or write into the real config directory.
//! - [`check`] is the only function that touches the network, and [`decide`] is
//!   everything both callers do with the answer.
//! - The Win32 is a `#[cfg(windows)]` module at the bottom, like `adapters`
//!   and `probe`: FFI that compiles away, and no `windows-sys` dependency.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The repository whose releases are published, in the form `owner/name`.
pub const REPOSITORY: &str = "megablue/PingLatencyOverlay";

/// How often the tray looks for a newer release.
///
/// Once a day: a user who has just installed the app has the newest build by
/// definition, and an app that asks on every launch is an app whose users turn
/// it off.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// One published release, as much of one as this app needs.
///
/// A release is a tag and a URL. Not the notes, not the assets, not the
/// published date: every field the check does not use is a field the GitHub
/// answer can change underneath a version of this app that parsed it before.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// The tag as published — `v0.2.72`, `v0.3.0`.
    pub tag: String,
    /// Where to send a user who wants it.
    pub url: String,
}

/// A version parsed out of a tag, and the only comparison this app makes.
///
/// `1.2.10 > 1.2.9` is the whole job, and a `Version` exists rather than a
/// string compare because the strings disagree about that in two different
/// ways: `"1.2.10" < "1.2.9"` lexically, and `"v1.2.10" < "v1.2.9"` because the
/// `v` is not the same shape. Parsing once — here — means the comparison is one
/// `Ord` of numbers and cannot be got wrong twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

/// What the last check said, and when it was.
///
/// `checked_at` is `None` until a check has answered. `release` is `None` until
/// one has found something newer, and is *kept* after the user has seen it: the
/// tray's menu still names the update, and the tray window's status line still
/// points at it, until a newer answer replaces it. It survives restarts, so a
/// user who clicks "download" a day later is not looking at a menu item that
/// has forgotten why it was interesting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateState {
    /// Unix seconds of the last answered check, if there has been one.
    pub checked_at: Option<u64>,
    /// The newest release found so far that is newer than what is running.
    pub release: Option<Release>,
}

/// Whether a check is worth running, given the state and the current time.
///
/// Both are arguments so a test can hold the decision against any history,
/// and so a caller that has not read the state file does not have to invent a
/// fake one.
pub fn due(state: &UpdateState, now: u64) -> bool {
    match state.checked_at {
        Some(at) => now.saturating_sub(at) >= CHECK_INTERVAL_SECS,
        None => true,
    }
}

/// Where the update state lives: beside the config, like the rules and the log.
pub fn state_path() -> PathBuf {
    state_path_at(&crate::config::config_dir())
}

/// The name of the file, kept by `updates.json` so a user can look at it next to
/// the rules and the log.
const UPDATES_FILE: &str = "updates.json";

/// The state path under a rooted directory, which is the seam the tests and the
/// resolver both use: [`crate::config::resolve_config_dir`] names a sandbox, and
/// nothing here has to know whether that is the real configuration or a test.
pub fn state_path_at(root: &std::path::Path) -> PathBuf {
    root.join(UPDATES_FILE)
}

/// Read the persisted state, with a missing or broken file as "never checked".
///
/// A broken file is not an error worth reporting: the state is a cache, and a
/// cache that will not parse is one that starts over. The alternative — routing
/// an `io::Error` into the tray's one check a day — scores a log line and a
/// stack trace for something whose only consequence is checking sooner.
pub fn load_state() -> UpdateState {
    load_state_at(&crate::config::config_dir())
}

/// [`load_state`] against a rooted directory.
pub fn load_state_at(root: &std::path::Path) -> UpdateState {
    let Ok(raw) = std::fs::read_to_string(state_path_at(root)) else {
        return UpdateState::default();
    };
    let stripped = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    serde_json::from_str(stripped).unwrap_or_default()
}

/// Remember the state, atomically, so a crash mid-write leaves nothing half.
///
/// The same temporary-file dance the profile saves and the rules file use, and
/// for the same reason: this is the one write the app does not want a reader to
/// catch in the middle of.
pub fn save_state(state: &UpdateState) -> std::io::Result<()> {
    save_state_at(&crate::config::config_dir(), state)
}

/// [`save_state`] against a rooted directory.
pub fn save_state_at(root: &std::path::Path, state: &UpdateState) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    crate::config::write_atomic(&state_path_at(root), &format!("{json}\n"))
}

/// Whether the tray's menu item and the About page should offer a download.
impl Release {
    /// The label shown for a release, e.g. `v0.2.72 is available`.
    pub fn label(&self) -> String {
        format!("{} is available", self.tag)
    }
}

/// Read a tag into the numbers worth comparing.
///
/// A leading `v` is accepted and dropped, because that is how the project tags
/// releases. Anything else is a number; a tag that is not is not a version.
pub fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim().strip_prefix(['v', 'V']).unwrap_or(text.trim());
    let mut parts = text.split('.');
    let version = Version {
        major: parts.next()?.parse().ok()?,
        minor: parts.next()?.parse().ok()?,
        patch: parts.next()?.parse().ok()?,
    };
    if parts.next().is_some() {
        return None; // A fourth number is not a version this app publishes.
    }
    Some(version)
}

/// What a check concluded.
///
/// `Unknown` is an answer, not an error: a request that timed out, a page that
/// was not a release, a tag with no number in it. The caller treats all the
/// same, because the user-visible consequence is the same one — nothing is
/// offered — and the tray logs which it was rather than branching on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Running is at or ahead of the latest published release.
    UpToDate,
    /// A newer release is published, and this is it.
    Available(Release),
    /// The check ran and could not decide. The message is for the log.
    Unknown(String),
}

impl Outcome {
    /// The release, when there is one.
    pub fn release(&self) -> Option<&Release> {
        match self {
            Outcome::Available(release) => Some(release),
            _ => None,
        }
    }

    /// The message, when the check could not decide.
    pub fn error(&self) -> Option<&str> {
        match self {
            Outcome::Unknown(message) => Some(message),
            _ => None,
        }
    }

    /// Whether this outcome carries something the user should be shown.
    pub fn answered(&self) -> bool {
        self.release().is_some()
    }
}

/// The whole decision, as a pure function of what came back.
///
/// `current` is the running version, `tag` the one in the answer, and the two
/// together are all there is to it: an answer that names a version this app has
/// never heard of is not an update, and an answer that names an *older* one is
/// not a downgrade.
pub fn decide(current: &str, tag: &str, url: &str) -> Outcome {
    let (Some(current), Some(found)) = (parse_version(current), parse_version(tag)) else {
        return Outcome::Unknown(format!("no version in {current:?} or {tag:?}"));
    };
    if found > current {
        Outcome::Available(Release {
            tag: tag.to_owned(),
            url: url.to_owned(),
        })
    } else {
        Outcome::UpToDate
    }
}

/// What the redirect lands on, with the version it is compared to.
///
/// Split from [`check`] because this half is the one worth testing: the fetch
/// is a call into WinHTTP that no unit test can run offline, but the answer
/// shape — a tag at the end of a URL, a page to send a user to — is pure data.
fn release_from_url(current: &str, page: &str) -> Outcome {
    // The page is `https://github.com/{repo}/releases/tag/{tag}`, and the tag
    // is the answer. Nothing strips a `v` here: `parse_version` does that, so
    // a tag this app does not publish is refused rather than guessed at.
    let Some(tag) = page.rsplit_once("/releases/tag/").map(|(_, tag)| tag) else {
        return Outcome::Unknown(format!("the answer named no release: {page}"));
    };
    decide(current, tag, page)
}

/// Why a request gave up, in words a user can read.
///
/// Rust formats a raw OS error as `OS Error 12002 (FormatMessageW() returned
/// error 317)`: winhttp.dll keeps its messages in a table the system one does
/// not have, so the code has to translate. These four are the ones this request
/// can actually produce — the same five-second timeout across resolve, connect,
/// send and receive — and the rest are passed through as they were, because an
/// unmapped code is better reported than guessed at.
fn describe(error: &std::io::Error) -> String {
    let Some(code) = error.raw_os_error() else {
        // A message this app built itself — the rate-limit line, the HTTP
        // status — is already a sentence.
        return error.to_string();
    };
    match code {
        // ERROR_WINHTTP_TIMEOUT: the one this app's users have met, on
        // networks where proxy discovery stalls before the request is even
        // sent.
        12002 => "the request timed out".to_owned(),
        // ERROR_WINHTTP_NAME_NOT_RESOLVED.
        12007 => "the address could not be resolved".to_owned(),
        // ERROR_WINHTTP_CANNOT_CONNECT.
        12029 => "the connection could not be made".to_owned(),
        // ERROR_WINHTTP_SECURE_FAILURE: TLS refused, or the certificate was
        // not one the system trusts.
        12175 => "the secure connection failed".to_owned(),
        _ => format!("the request failed: {error}"),
    }
}

/// Look for a newer release, on the calling thread.
///
/// Blocking, and for seconds at a time, which is why neither caller runs it
/// where a user is waiting: the tray spawns a thread and the Config window
/// runs the check from its own. Both end up holding an [`Outcome`].
pub fn check(current: &str) -> Outcome {
    let url = format!("https://github.com/{REPOSITORY}/releases/latest");
    fetch_latest_release(&url).map_or_else(
        |error| Outcome::Unknown(describe(&error)),
        |page| release_from_url(current, &page),
    )
}

/// Seconds since the Unix epoch, and the clock this module compares against.
///
/// A broken clock reads as zero, which makes the last check look impossibly old
/// and a check due — the harmless direction for a once-a-day request.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// One GET over TLS, returning the page a `/releases/latest` link redirects
/// to, which is where the release's tag is read from.
///
/// The URL is a parameter rather than a built-in constant so this module holds
/// one concept — the request — and the caller holds the other, which link is
/// asked. It also keeps the FFI seam narrow: the URL is split here and nothing
/// else is parsed, because the host and the port are the only parts WinHTTP
/// wants as literals.
///
/// The answer is the final URL rather than the page's body. GitHub answers
/// `/releases/latest` with a redirect to the release's tag page, WinHTTP
/// follows it, and [`win::WinHttpQueryOption`] reads back where a browser
/// would have landed. That page's URL names the tag — so the tag and the page
/// to open are one string — and the site serving it is the one that still
/// answers when the API's sixty-an-hour quota for a shared address is spent.
#[cfg(windows)]
fn fetch_latest_release(url: &str) -> std::io::Result<String> {
    // A URL with no scheme is a configuration mistake, not something to guess
    // at: the one caller builds it from `https://github.com/...`, and a scheme
    // that is not that is refused rather than attempted over cleartext.
    let Some(authority) = url.strip_prefix("https://") else {
        return Err(std::io::Error::other("only https is used"));
    };
    let (host, path) = authority.split_once('/').unwrap_or((authority, ""));
    let host = wide(host);
    let path = wide(&format!("/{path}"));
    let agent = wide(concat!("PingLatencyOverlay/", env!("CARGO_PKG_VERSION")));
    let verb = wide("GET");

    // Each handle is closed on every path out, including the ones that give up
    // halfway: a session leaked per check is one leak per day per user, which is
    // the sort of thing nobody notices for a year.
    unsafe {
        // DEFAULT_PROXY, not AUTOMATIC_PROXY: the automatic mode spends
        // seconds on WPAD auto-discovery before the first request — DHCP
        // option 252, then a DNS lookup for `wpad` — and on a machine with
        // virtual adapters or a tunnel and no DNS suffix that discovery stalls
        // for longer than the timeouts below, so the request dies with
        // ERROR_WINHTTP_TIMEOUT before a byte is sent. A user behind a proxy
        // that mattered would have told us; a daily ask for a public page
        // should not cost a WPAD hunt.
        let session = win::WinHttpOpen(
            agent.as_ptr(),
            win::WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if session.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let connection = win::WinHttpConnect(session, host.as_ptr(), HTTPS_PORT, 0);
        if connection.is_null() {
            win::WinHttpCloseHandle(session);
            return Err(std::io::Error::last_os_error());
        }
        let request = win::WinHttpOpenRequest(
            connection,
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            win::WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            win::WinHttpCloseHandle(connection);
            win::WinHttpCloseHandle(session);
            return Err(std::io::Error::last_os_error());
        }

        // The caps are placed in a closure so the three handles below have one
        // exit each; every error path returns through here and drops nothing.
        let outcome = (|| {
            // No flags: a synchronous call is what this app wants, because a
            // callback needs a message loop this tray thread does not have.
            let ok = win::WinHttpSetTimeouts(
                request,
                TIMEOUT_MS as i32,
                TIMEOUT_MS as i32,
                TIMEOUT_MS as i32,
                TIMEOUT_MS as i32,
            );
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let ok =
                win::WinHttpSendRequest(request, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0);
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The call that waits: no body was sent, so this is where resolve,
            // connect and TLS all happen, and where the timeouts above bite.
            let ok = win::WinHttpReceiveResponse(request, std::ptr::null_mut());
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut status: u32 = 0;
            let mut length = std::mem::size_of::<u32>() as u32;
            let ok = win::WinHttpQueryHeaders(
                request,
                win::WINHTTP_QUERY_STATUS_CODE | win::WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                std::ptr::addr_of_mut!(status).cast(),
                std::ptr::addr_of_mut!(length),
                std::ptr::null_mut(),
            );
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            // An error page is a valid HTTP response, so a 403 or a 404 has to
            // be reported as one rather than as an unreadable answer. The
            // status is the only thing that distinguishes them, and the two
            // that mean the same thing to a user — 403 and 429 — get a
            // sentence of their own, because the unauthenticated quota is
            // sixty an hour per address and an address behind a shared NAT or
            // a VPN spends somebody else's budget.
            if !(200..300).contains(&status) {
                if matches!(status, 403 | 429) {
                    return Err(std::io::Error::other(
                        "GitHub is rate-limiting this network; try again later",
                    ));
                }
                return Err(std::io::Error::other(format!(
                    "the server answered HTTP {status}"
                )));
            }
            // The page the redirect landed on, which is the whole answer: its
            // URL ends in `/releases/tag/{tag}`, so the tag and the page a user
            // should be sent to are one string and nothing is parsed. WinHTTP
            // followed the redirect on the way in; this reads back where it
            // ended up. The body is not read — it is a page, not the answer.
            //
            // Two calls, because one is not enough: WinHTTP writes the length
            // it wants into the buffer-length argument, so the first call
            // passes no buffer at all and learns how many bytes it needs. A
            // "the data area passed to a system call is too small" (122) on a
            // single call is precisely the error that pattern removes, and it
            // removes any guess about how long a redirect chain can make a
            // URL.
            let mut length: u32 = 0;
            let ok = win::WinHttpQueryOption(
                request,
                win::WINHTTP_OPTION_URL,
                std::ptr::null_mut(),
                std::ptr::addr_of_mut!(length),
            );
            if ok == 0 {
                // ERROR_INSUFFICIENT_BUFFER (122) is this call succeedding at
                // its real job, which is saying how much it wants. Any other
                // failure is a real one.
                if std::io::Error::last_os_error().raw_os_error() != Some(122) {
                    return Err(std::io::Error::last_os_error());
                }
            }
            let mut buffer: Vec<u16> = vec![0; length as usize];
            let ok = win::WinHttpQueryOption(
                request,
                win::WINHTTP_OPTION_URL,
                buffer.as_mut_ptr().cast(),
                std::ptr::addr_of_mut!(length),
            );
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The length comes back in bytes, counting the NUL terminator, so
            // halve it and drop the terminator before reading the string out.
            let chars = (length as usize / 2).saturating_sub(1);
            Ok(String::from_utf16_lossy(&buffer[..chars]))
        })();

        win::WinHttpCloseHandle(request);
        win::WinHttpCloseHandle(connection);
        win::WinHttpCloseHandle(session);
        outcome
    }
}

/// How long each phase of the request is given.
///
/// Five seconds, split across resolve, connect, send and receive by WinHTTP
/// itself. Long enough for a phone hotspot on a bad day, and short enough that
/// the tray's thread does not sit there for a minute. This app's user is having
/// a slow day already, so an answer that takes this long is not worth having.
const TIMEOUT_MS: u32 = 5_000;

#[cfg(not(windows))]
fn fetch_latest_release(_url: &str) -> std::io::Result<String> {
    Err(std::io::Error::other("not supported on this platform"))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The HTTPS port: the only one this app talks to.
#[cfg(windows)]
const HTTPS_PORT: u16 = 443;

#[cfg(windows)]
// The same allow the other `extern` blocks in this crate carry: these names are
// Win32's, not ours, and renaming them to satisfy a lint would make them harder
// to look up than they are to tolerate.
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win {
    use core::ffi::c_void;

    pub type HINTERNET = *mut c_void;
    pub type LPCWSTR = *const u16;

    pub const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
    pub const WINHTTP_OPTION_URL: u32 = 34;
    pub const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
    pub const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
    /// The modifier that asks for a DWORD — and it lives in the **high** bits.
    ///
    /// A low bit here is a trap worth a comment: `STATUS_CODE | 2` is 21, which
    /// is `WINHTTP_QUERY_RAW_HEADERS`, so the call quietly asks for every
    /// response header as NUL-separated text into a 4-byte buffer and answers
    /// `ERROR_INSUFFICIENT_BUFFER` (122) — a correct answer to a question the
    /// caller did not mean to ask. `winhttp.h` puts all three modifier flags
    /// above the attribute values for exactly this reason.
    pub const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;

    #[link(name = "winhttp")]
    unsafe extern "system" {
        pub fn WinHttpOpen(
            pszAgentW: LPCWSTR,
            dwAccessType: u32,
            pszProxyW: LPCWSTR,
            pszProxyBypassW: LPCWSTR,
            dwFlags: u32,
        ) -> HINTERNET;
        pub fn WinHttpConnect(
            hSession: HINTERNET,
            pszServerName: LPCWSTR,
            nServerPort: u16,
            dwReserved: u32,
        ) -> HINTERNET;
        pub fn WinHttpOpenRequest(
            hConnect: HINTERNET,
            pwszVerb: LPCWSTR,
            pwszObjectName: LPCWSTR,
            pwszVersionName: LPCWSTR,
            pwszReferrer: LPCWSTR,
            ppwszAcceptTypes: *const LPCWSTR,
            dwFlags: u32,
        ) -> HINTERNET;
        pub fn WinHttpSetTimeouts(
            hInternet: HINTERNET,
            dwResolveTimeout: i32,
            dwConnectTimeout: i32,
            dwSendTimeout: i32,
            dwReceiveTimeout: i32,
        ) -> i32;
        pub fn WinHttpSendRequest(
            hRequest: HINTERNET,
            lpszHeaders: LPCWSTR,
            dwHeadersLength: u32,
            lpOptional: *const c_void,
            dwOptionalLength: u32,
            dwTotalLength: usize,
            dwContext: usize,
        ) -> i32;
        pub fn WinHttpReceiveResponse(hRequest: HINTERNET, lpReserved: *mut c_void) -> i32;
        pub fn WinHttpQueryHeaders(
            hRequest: HINTERNET,
            dwInfoLevel: u32,
            pwszName: LPCWSTR,
            lpBuffer: *mut c_void,
            lpdwBufferLength: *mut u32,
            lpdwIndex: *mut u32,
        ) -> i32;
        pub fn WinHttpQueryOption(
            hInternet: HINTERNET,
            dwOption: u32,
            lpBuffer: *mut c_void,
            lpdwBufferLength: *mut u32,
        ) -> i32;
        pub fn WinHttpCloseHandle(hInternet: HINTERNET) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag with a leading `v` and one without are the same version, because
    /// the project tags one way and the answer could arrive either way.
    #[test]
    fn a_tag_reads_the_same_with_or_without_the_v() {
        assert_eq!(parse_version("0.2.71"), parse_version("v0.2.71"));
        assert_eq!(
            parse_version("v1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
    }

    /// `1.2.10` is newer than `1.2.9`, and that is the whole reason the
    /// comparison is numeric: the strings say the opposite.
    #[test]
    fn the_version_compares_by_number_not_by_string() {
        let a = parse_version("v0.2.9").expect("parsed");
        let b = parse_version("v0.2.10").expect("parsed");
        assert!(b > a, "0.2.10 sorts after 0.2.9");
    }

    /// A tag that is not three numbers is not a version, and is refused rather
    /// than guessed at: a `v0.2.7-rc1` would otherwise be read as `0.2.7`.
    #[test]
    fn a_tag_that_is_not_three_numbers_is_refused() {
        for tag in [
            "",
            "v1",
            "v1.2",
            "v1.2.3.4",
            "v1.2.x",
            "vabc.1.2",
            "1.2.3-rc1",
        ] {
            assert_eq!(parse_version(tag), None, "{tag:?} should not parse");
        }
    }

    /// The one rule that keeps an app built ahead of the releases quiet.
    #[test]
    fn only_a_newer_release_offers_itself() {
        assert_eq!(
            decide("0.2.71", "v0.2.72", "https://example.com/one"),
            Outcome::Available(Release {
                tag: "v0.2.72".to_owned(),
                url: "https://example.com/one".to_owned()
            })
        );
        assert_eq!(
            decide("0.2.71", "v0.2.71", "https://example.com/same"),
            Outcome::UpToDate
        );
        assert_eq!(
            decide("0.2.72", "v0.2.71", "https://example.com/older"),
            Outcome::UpToDate,
            "a published release behind the running build is not a downgrade"
        );
    }

    /// A version that cannot be read is a reason, not a panic: the answer named
    /// something this app does not publish.
    #[test]
    fn an_answer_with_no_readable_version_explains_itself() {
        let outcome = decide("0.2.71", "latest", "https://example.com");
        let Some(reason) = outcome.error() else {
            panic!("expected Unknown, got {outcome:?}");
        };
        assert!(reason.contains("latest"), "{reason}");
    }

    /// The cadence, from both directions: a state that has never checked is due
    /// now, and a fresh one is not until the interval has passed.
    #[test]
    fn a_check_is_due_once_a_day() {
        let never = UpdateState::default();
        assert!(due(&never, 0), "never checked means due");

        let recent = UpdateState {
            checked_at: Some(1_000),
            release: None,
        };
        assert!(!due(&recent, 1_000 + CHECK_INTERVAL_SECS - 1));
        assert!(due(&recent, 1_000 + CHECK_INTERVAL_SECS));
        // A clock that has run backwards reads as "checked just now", because
        // `saturating_sub` floors the subtraction at zero instead of wrapping
        // to a number that would read as "checked in the future" and make a
        // request due on every launch — which is a rate-limit, not an update.
        assert!(!due(&recent, 500));
    }

    /// The state round-trips through the file, because that file is the only
    /// thing the menu has to be right about after a restart.
    #[test]
    fn the_state_survives_a_restart() {
        let root = TestDir::new("updater-state");
        let state = UpdateState {
            checked_at: Some(1_700_000_000),
            release: Some(Release {
                tag: "v0.2.72".to_owned(),
                url: "https://github.com/megablue/PingLatencyOverlay/releases/tag/v0.2.72"
                    .to_owned(),
            }),
        };
        save_state_at(root.path(), &state).expect("save");
        assert_eq!(load_state_at(root.path()), state);
    }

    /// A file that is not JSON is a cache that starts over, not an error the
    /// caller has to route.
    #[test]
    fn a_broken_state_file_means_never_checked() {
        let root = TestDir::new("updater-broken");
        std::fs::write(state_path_at(root.path()), "not json at all").expect("write");
        assert_eq!(load_state_at(root.path()), UpdateState::default());
    }

    /// A file that is not there is the same thing: every installation that has
    /// never checked looks exactly like this, so the first run cannot be told
    /// apart from a broken file on purpose.
    #[test]
    fn a_missing_state_file_means_never_checked() {
        let root = TestDir::new("updater-missing");
        assert_eq!(load_state_at(root.path()), UpdateState::default());
    }

    /// A release's label is the sentence the tray shows and the About page
    /// repeats.
    #[test]
    fn a_release_labels_itself() {
        let release = Release {
            tag: "v0.2.72".to_owned(),
            url: "https://example.com".to_owned(),
        };
        assert_eq!(release.label(), "v0.2.72 is available");
    }

    /// An outcome that offers nothing answers nothing, which is what keeps the
    /// two callers from having to look inside it.
    #[test]
    fn an_outcome_answers_only_when_it_offers_something() {
        assert!(Outcome::UpToDate.release().is_none());
        assert!(!Outcome::UpToDate.answered());
        let release = Release {
            tag: "v1.0.0".to_owned(),
            url: "https://example.com".to_owned(),
        };
        assert!(Outcome::Available(release.clone()).answered());
        assert_eq!(
            Outcome::Available(release).release(),
            Some(&Release {
                tag: "v1.0.0".to_owned(),
                url: "https://example.com".to_owned(),
            })
        );
        assert_eq!(
            Outcome::Unknown("offline".to_owned()).error(),
            Some("offline")
        );
    }

    /// The answer's shape: a tag at the end of a URL, and that URL as the page
    /// to send a user to. The redirect names both at once, which is the whole
    /// reason the body is not read.
    #[test]
    fn an_answer_names_the_release_and_its_page() {
        let page = "https://github.com/megablue/PingLatencyOverlay/releases/tag/v0.2.72";
        assert_eq!(
            release_from_url("0.2.71", page),
            Outcome::Available(Release {
                tag: "v0.2.72".to_owned(),
                url: page.to_owned(),
            })
        );
    }

    /// A URL that is not a release's page — the releases list, the repository
    /// itself, nothing at all — names no release, and says so rather than
    /// being read as "up to date": calling it "no update" would hide the
    /// reason nothing was offered.
    #[test]
    fn an_answer_that_names_no_release_explains_itself() {
        for page in [
            "https://github.com/megablue/PingLatencyOverlay/releases",
            "https://github.com/megablue/PingLatencyOverlay",
            "",
        ] {
            let outcome = release_from_url("0.2.71", page);
            let Some(reason) = outcome.error() else {
                panic!("expected Unknown for {page:?}");
            };
            assert!(reason.contains("no release"), "{reason}");
        }
    }

    /// An answer naming the running version reads as up to date, which is the
    /// sentence that covers almost every launch.
    #[test]
    fn an_answer_naming_the_running_version_is_up_to_date() {
        let page = "https://github.com/megablue/PingLatencyOverlay/releases/tag/v0.2.71";
        assert_eq!(release_from_url("0.2.71", page), Outcome::UpToDate);
    }

    /// A failure reaches the user in words rather than as an OS error code.
    ///
    /// A reader once met `OS Error 12002 (FormatMessageW() returned error
    /// 317)`: that is Rust's formatting failing to find message text for a
    /// winhttp.dll code, so the number is all the row ever had. A message this
    /// module built itself is already a sentence and passes through unchanged.
    #[test]
    fn a_failure_comes_back_in_words_a_user_can_read() {
        for (code, words) in [
            (12002, "the request timed out"),
            (12007, "the address could not be resolved"),
            (12029, "the connection could not be made"),
            (12175, "the secure connection failed"),
        ] {
            let error = std::io::Error::from_raw_os_error(code);
            assert_eq!(describe(&error), words, "{code}");
        }
        let reason = std::io::Error::other("GitHub is rate-limiting this network; try again later");
        assert_eq!(
            describe(&reason),
            "GitHub is rate-limiting this network; try again later"
        );
    }

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            use std::time::UNIX_EPOCH;

            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "ping-latency-overlay-{label}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
