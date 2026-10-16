# Updates — checking GitHub for a newer release, and saying so

Status: shipped (v0.2.61) · Read when: touching `crates/core/src/updater.rs`,
the tray's update menu item, or the About page's update row.

## What it does

- The tray asks GitHub once a day whether a newer release exists, and names it
  in its menu (`Update available: v0.2.72`) until a newer answer replaces it.
  The check is silent otherwise — no popup, no download, no installer spawn.
- A found update opens its release page in the user's default browser, through
  the same `ShellExecuteW` path every other link in the app uses.
- The Config window checks for itself: opening the About page starts a check
  and the page's status row says what came back, with a button to ask again.
- It is switched off with one preference, `ui.checkForUpdates`, which is **on
  by default**. Switching it off stops the tray's daily ask; the tray's menu
  item and the About page's button keep working, because a click is a user
  asking rather than a background phone-home.
- The last answer is kept in `updates.json` beside the config, so a restart
  does not wait a full day for the menu to be right again. See
  [docs/SPEC.md](../../SPEC.md) for the user-visible wording.

## Map

- `crates/core/src/updater.rs` — the whole feature's pure half and its one
  impure call:
  - `Version`, `parse_version` — a tag (`v1.2.3`) read into numbers, because
    the strings disagree about `1.2.10 > 1.2.9` twice over.
  - `Release { tag, url }`, `label()` — the tag as published and the page to
    send a user to. `label()` is a standalone caption (`v0.2.72 is available`)
    and is **not** a version string.
  - `Outcome { UpToDate, Available(Release), Unknown(String) }` with
    `release()`, `error()` and `answered()`.
  - `decide(current, tag, url)` — the pure decision: remote newer than local
    only, so a local build never reads "downgrade available".
  - `check(current)` — the blocking fetch. Its doc says callers spawn a thread.
  - `release_from_url(current, page)` — what the redirect landed on: the tag
    is the path segment after `/releases/tag/`, and the URL itself is the page.
    A page that is not a release says so rather than reading as "up to date".
  - `UpdateState { checked_at, release }`, `due(state, now)`,
    `state_path`/`load_state`/`save_state` (`updates.json`), `now_secs`.
  - A local `#[link(name = "winhttp")] extern "system"` block — host
    `github.com`, path `/{slug}/releases/latest`, port 443,
    `WINHTTP_FLAG_SECURE`, `WINHTTP_ACCESS_TYPE_DEFAULT_PROXY`, 5s timeouts.
    No new dependency, matching the project's own-FFI style.
- `crates/tray/src/lib.rs` — the menu: `TrayState.update`
  (`Mutex<Option<Release>>`), `set_known_update`, the `Update available` /
  `Check for updates` caption, the menu item and its dispatch,
  `TrayAction::CheckUpdate`, `open_url`.
- `crates/tray/src/main.rs` — the resident half: the 24h tick on the existing
  one-second loop, the thread + channel so the blocking check never freezes the
  loop, `reload_prefs` (the same stamp guard `reload_rules_if_changed` uses),
  and the one call to `save_state`. **The tray is the only writer of
  `updates.json`.**
- `crates/tray/build.rs` — sets `APP_BUILD_VERSION` for `env!`, because the
  tray's own manifest says `0.2.0` and would otherwise compare against that.
- `src-tauri/src/ui.rs` — the window: `update_answer`,
  `update_in_flight`, `update_wanted`, `sync_update_check`,
  `update_status_line`, `update_row`, and `set_check_for_updates`.
- `crates/core/src/config.rs` — `UiPrefs.check_for_updates`
  (serde `checkForUpdates`, defaults on by a hand-written `Default`).

## How & why

- **A blocking check can never run on the frame path.** `updater::check` takes
  seconds on a bad connection. In the tray it runs on a thread that sends the
  `Outcome` back over an `mpsc` channel and the loop polls it; in the Config
  window the click only sets `update_wanted` and `frame_logic` starts the
  thread. That split is also why the tests can click the button without
  reaching GitHub.
- **`sync_update_check` must run before `sync_profiles`.** The About page's
  arrival signal is the `last_page` transition, and `sync_profiles` is the
  pass that advances `last_page` — read it afterwards and the arrival is
  already consumed.
- **The 24h cadence is stored, not counted from launch.** A
  `since_update: Duration` from process start resets on every restart, so a
  user who opens the tray 30s after a check would wait a full day for the
  remaining ~23h. `updater::due` reads `checked_at` from disk instead. The flag
  guard lives in the caller, not in the thread, so a manual click always works.
- **The preference stops the timer, never the click.** `checkForUpdates = false`
  gates the About page's arrival check and the tray's 24h tick; both manual
  paths stay live. This is the one place the user's "stop asking me" must not
  remove their ability to ask.
- **The window never writes `updates.json`.** Two writers of one state is how
  the two ends come to disagree about what the last check said, so the tray is
  the single writer and the window holds its answer in memory only. That also
  means the window's state is never stale-on-disk: a window that checked and
  closed has written nothing the tray would believe.
- **The comparison is by number, never by string.** `"1.2.10" < "1.2.9"`
  lexically and `"v1.2.10" < "v1.2.9"` because the `v` is not the same shape,
  so the parse happens once and the compare is one `Ord` of numbers.
- **`label()` is a caption, so it is not reused in a sentence.** The About
  row's wording is built from `tag`, not `label()`, because `label()` carries
  "is available" and would read `Update available: v9.9.9 is available.`.
- **The session opens with `DEFAULT_PROXY`, and must never reopen with
  `AUTOMATIC_PROXY`.** Automatic runs WPAD auto-discovery before the first
  request — DHCP option 252, then a DNS lookup for `wpad` — and on a machine
  with virtual adapters, a tunnel and no DNS suffix that discovery stalls past
  the 5s timeouts, so the request dies with `ERROR_WINHTTP_TIMEOUT` (12002)
  before a byte is sent. Measured on this machine, access type 4 timed out
  after ~8s twice while types 0 and 1 answered in the same breath.
  `DEFAULT_PROXY` honors a statically configured proxy and otherwise goes
  direct, which is the honest shape for a once-a-day ask of a public page.
- **The endpoint is `github.com`, not `api.github.com`.** The API's
  unauthenticated quota is sixty an hour *per address*, and an address behind
  a shared NAT or a VPN spends somebody else's budget — which is what this
  user's WARP egress hit, while `github.com` answered normally in the same
  minute. `/releases/latest` answers a redirect, and following it leaves the
  tag and the page to open in one string: no JSON, no quota spent.
- **A failure is words before it is a code.** Rust formats a raw OS error as
  `OS Error 12002 (FormatMessageW() returned error 317)`, because the message
  table for a winhttp.dll code is not the one it can read. `describe()` maps
  the four codes this request can produce into a sentence; anything it did not
  keep the code, so a person can still search for it.
- **A wrong modifier bit is a different query, not a wrong answer.** The status
  query asks for a number with
  `WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER`. The flag was
  declared `0x0002`, and `19 | 2` is 21 — `WINHTTP_QUERY_RAW_HEADERS` — so the
  call asked for every response header as NUL-separated text into a 4-byte
  buffer and WinHTTP correctly answered `ERROR_INSUFFICIENT_BUFFER` (122). The
  modifier flags are the **high** bits (`0x2000_0000`); a low bit silently
  becomes another attribute, which is exactly why the constant carries its trap
  in a comment. No observable symptom pointed at the query — the stall it was
  found beside was the WPAD one — so this is worth re-reading before touching
  the headers call.

## Config keys / UI

| Key | Default | Set by |
| --- | --- | --- |
| `ui.checkForUpdates` | `true` | Global page — **Check for updates in the background** |
| `updates.json`: `checkedAt` | absent (never checked) | the tray, after each answered check |
| `updates.json`: `release` | `null` | the tray, when a newer release is found |

A missing or unparseable `updates.json` reads as "never checked" — the daily
check is due immediately — and is never rewritten or deleted by the reader, so
a file the user hand-edited is left as it is until a check answers.

## Tests that pin it

- `crates/core/src/updater.rs`: `a_tag_reads_the_same_with_or_without_the_v`,
  `the_version_compares_by_number_not_by_string`,
  `a_tag_that_is_not_three_numbers_is_refused`,
  `an_answer_with_no_readable_version_explains_itself`,
  `a_check_is_due_once_a_day`, `the_state_survives_a_restart`,
  `a_broken_state_file_means_never_checked`,
  `a_missing_state_file_means_never_checked`, `a_release_labels_itself`,
  `an_outcome_answers_only_when_it_offers_something`,
  `an_answer_names_the_release_and_its_page`,
  `an_answer_that_names_no_release_explains_itself`,
  `an_answer_naming_the_running_version_is_up_to_date`,
  `a_failure_comes_back_in_words_a_user_can_read`.
- `crates/core/src/config.rs`:
  `the_check_for_updates_preference_defaults_on_and_round_trips`.
- `src-tauri/src/ui.rs`: `the_update_row_says_what_it_found` (the wording, all
  five states, with no network), `the_about_page_button_only_asks_for_a_check`
  (the click sets a flag and spawns no thread), and
  `the_update_check_toggle_is_staged_and_live` (both halves, Discard, Save).

**The tray's own update wiring has no test of its own** — the loop, the thread
and the menu caption are not covered by the tray crate's suite. Its pure half
(`updater.rs`) is, and the About page's half is; what is missing is the glue in
`crates/tray/src/main.rs`.

## Related

- [tray.md](tray.md) — the resident loop the 24h tick runs on.
- [runtime.md](runtime.md) — the three processes, `windows_subsystem`, and why
  a tray error path must log or `fatal`.
- [diagnostics.md](diagnostics.md) — the log file the check reports into.
- [config/window.md](config/window.md) — the About page and its row.
- [config/storage.md](config/storage.md) — the global prefs and the files
  beside the config.
