# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. The native implementation uses **Rust + egui/eframe** for the single
configuration window and native Win32 layered windows for overlays. Probes run
on Tokio tasks and the system tray uses `tray-icon`.

Two documents, and keeping them apart is the point:

- **`docs/SPEC.md` is product behavior** — what the app does and what a user
  sees, readable without knowing the code. Behavior belongs there even when
  the code is where the reason for it lives.
- **This file is working knowledge** — the shape of the crates, the traps, and
  why a rule exists. A rule here is one line of cause and effect plus the
  identifier and the test that pins it, not a retelling of the bug.

Where a rule has a behavior half and a mechanism half, the behavior half is a
pointer to `docs/SPEC.md` rather than a second copy: two copies of a default
value is one more thing that can be wrong.

## Layout
- `src-tauri/` — the Cargo workspace; run Cargo commands here. The root
  manifest is both the workspace and the application package; `crates/core`,
  `crates/tray` and `crates/build-support` are the other members.
  - `src/main.rs` — the entry point of `plo-config`, the Config window process,
    and the only thing in the project allowed to depend on eframe.
  - `src/ui.rs` — the egui configuration editor. It was "tray-mode" when the
    tray shared this process; it is now its own process that exits on close.
  - `src/theme.rs` — the Config window's colours, loaded from a file rather
    than compiled in. In the shell, not in `core`: the renderer never reads a
    theme, and `core` is "only what the renderer needs".
  - `crates/tray/` — the `ping-latency-overlay-tray` crate, the tray icon, the
    menu and the bundled artwork. The resident process, and it has no eframe.
    It is the only crate allowed to depend on `tray-icon`.
    - `src/main.rs` — the `plo-tray` entry point: the pipe client and the
      supervision loop that watches the renderer through it.
    - `src/lib.rs` — the icon, the native menu, and the icon-event loop that
      turns a left-click into Config and a right-click into the menu.
    - `src/bin/tray_probe.rs` — a diagnostic binary that is the icon and menu
      path and nothing else. It logs every step and deliberately omits the pipe
      attachment, the storm guard and the Config-window spawn, so a menu that
      never appears can be traced to `tray-icon` or the plumbing rather than to
      any of those. It is not part of the product, and it is behind the
      `tray-probe` feature, so an ordinary build never produces a fourth
      executable; `cargo build -p ping-latency-overlay-tray --features
      tray-probe` builds it when needed.
  - `crates/core/` — the `ping-latency-overlay-core` crate. Everything the
    renderer needs and **no GUI dependency at all**:
    - `src/lib.rs` — module wiring; the entries below are what it exposes.
    - `src/config.rs` — config schema, profiles, persistence, and directory
      migration.
    - `src/probe.rs` — one-shot ICMP or TCP latency measurement.
    - `src/probes.rs` — long-lived Tokio probe tasks and bounded sample buffers.
    - `src/overlay.rs` — native layered HWND creation, DPI/work-area layout, and
      per-window alpha compositing with `UpdateLayeredWindow`.
    - `src/render.rs` — software graph rendering into premultiplied RGBA.
    - `src/border.rs` — runtime border-effect state and software RGB border
      drawing.
    - `src/monitors.rs` — display enumeration and the rule that picks a
      display. The two are separate on purpose: `enumerate` is the only Win32
      in it, and `resolve`/`placement` are pure functions over a list, so the
      multi-monitor behaviour is testable on the one-monitor machine most of
      this is written on.
    - `src/transport.rs` — the pipe: wire format, client, server and the
      single-instance mutexes. Both ends compile from this module, so the two
      processes cannot disagree about the protocol.
    - `src/diagnostics.rs` — the log file and the startup `MessageBox`, which
      together are the only output a `windows_subsystem` process has.
    - `src/bin/renderer.rs` — the `plo-renderer` entry point: the overlay
      windows' message pump, the redraw loop, and the pipe server that answers
      a config, a pause or a shutdown.
- `scripts/gen-icons.mjs` — generates native artwork with no dependencies.
- `crates/build-support/` — the shared half of the three build scripts. It
  owns the `MAJOR.MINOR.(commits since countBase)` derivation and the Windows
  resource that puts the app icon, the version and the copyright notice on each
  executable, so the icon path, the version rule and the notice exist once
  rather than three times. The notice itself is `package.metadata.copyright`
  in the root manifest, which the About page reads through `APP_COPYRIGHT`. A
  build dependency runs on the host at build time and contributes a resource,
  not runtime code; `no_gui_dependencies.rs` still finds no GUI crate under it.
- `packaging/nsis/` and `scripts/build-nsis.ps1` — native installer packaging.
- `docs/GAME.md` — design document for the game module, which is **not
  built**. Nothing described there exists in the code. Read it before
  planning anything that draws something other than a latency graph.
- The pre-egui Tauri/React implementation remains available on `main`. This
  file describes the native branch.

## Commands
Native app (run from `src-tauri/`):
- `cargo run` — development build. Starts the Config window, which brings up
  the tray and renderer if they are not already running.
- `cargo build --release` — optimized standalone executables
- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`

`--all-features` is deliberate: it is what keeps the feature-gated
`plo-tray-probe` target compiling. `cargo build --release` stays feature-free
on purpose, because it is the command that must produce exactly the three
executables the installer ships.

Installer (run from the repository root):
- `npm run icons`
- `npm run bundle` — build the x64 NSIS installer from the existing release exe
- `.\scripts\build-nsis.ps1 -Arch arm64` — build the ARM64 installer after
  building the ARM64 release exe

Capture (run from the repository root):
- `.\scripts\capture-app.ps1` — run the app against a sandboxed config and
  photograph the Config window and every overlay; see *Workflow*

## Workflow
- **Measure layout, don't deduce it.** Reading the code and checking arithmetic
  against constants shipped three layout bugs in a row. Lay the real thing out
  headlessly with `Context::run_ui` and print the rects; the numbers name the
  bug immediately. Assert both the good and the broken case so each test
  carries its own failure mode.
- **Gate on clippy, not just `cargo test`.** `assertions_on_constants` and
  friends are clippy-only, and `cargo test` compiles and passes with them
  present. The gate is `cargo fmt`, `cargo clippy --all-targets --all-features
  -- -D warnings`, `cargo test --all-features`, `cargo build --release`.
- **The capture script is how the app gets looked at without a person at the
  screen.** `scripts/capture-app.ps1` stops the three exes, runs a session of
  its own (`plo-config` brings up the tray and renderer), photographs the
  Config window with `PrintWindow` and each overlay by `BitBlt`-ing the desktop
  DC with `CAPTUREBLT` (an `UpdateLayeredWindow` window presents nothing to
  `PrintWindow`), and writes PNGs plus `manifest.txt` into `-OutDir`. It points
  `PLO_CONFIG_DIR` at a sandbox copy of the live config by default, so a
  session can never rewrite the user's settings or log.
  `SetProcessDpiAwarenessContext(-4)` comes first so the window rects are
  physical pixels. Two traps are paid for in it: `Graphics.CopyFromScreen`
  returns an all-black image in this environment while the GDI call does not,
  and winit keeps an 18x18 visible helper window beside the real Config window,
  so the script picks that window by title and size rather than by "first
  visible". A `PrintWindow` that comes back flat falls back to the screen copy,
  because a black photograph is worse than an occluded one. Screenshots taken
  this way are worth reading, but nothing can script a real click into the
  running app, so a visual change still gets handed over as a build.
- **The driving tests click through the window's real frame path.**
  `PingApp::for_test` builds an app against a temp root with the tray and
  renderer spawns skipped (`attach: false` in the shared `build` body), and
  `frame_ui` is exactly what the eframe trait method calls, so the `drive` and
  `click_text` helpers run the pass the host would. `text_rects` finds painted
  labels by their galley text — the rail rows and the theme tiles are
  `painter.text` and their hit targets cover the label — so a click is
  addressed by label rather than by coordinates, and `click_text` asserts the
  label is unique so a test cannot silently click the wrong control. Tests
  drive `frame_ui` and deliberately not `frame_logic`: reconnection and the
  `sync_*` methods talk to the machine. The tests module's `use super::{...}`
  is an explicit list, so a new helper or constant a test touches must be added
  to it. **None of this reaches a release binary**: `for_test` is
  `#[cfg(test)]`, so is the tests module, and production only ever calls
  `build` with `attach: true` — the one release-visible piece of the seam is
  that parameter. (The capture script is not shipped either; the installer
  packs the three exes and the LICENSE.)
- **A manifest that is both a workspace root and a package narrows plain
  `cargo test` to that package alone.** The split into `crates/core` made
  `cargo test` report 35 passed against a 94-test suite and exit 0, silently
  skipping every test in `core`. `default-members` in the root manifest is what
  makes the ordinary command cover the workspace, and it is load-bearing: it
  lists `.`, `crates/core`, `crates/tray` and `crates/build-support`, and
  dropping any of them quietly narrows the suite. If the test count ever drops
  without a deletion, suspect this before suspecting a filter. The same numbers
  appear in that manifest's comment; they are historical, so if you correct
  one, correct both.
- **Commit messages go through a file.** Write the message to a scratch file
  (this session uses `%LOCALAPPDATA%\Temp\opencode\plo-commit-msg.txt`) and run
  `git commit -F <path>`; a PowerShell here-string gets its terminator mangled
  by the shell tool.
- **No AI attribution in a commit message, ever, without being asked.** No
  `Co-Authored-By` trailer, no `Generated with` footer, nothing that reads as
  crediting a model or tool. These commits are the user's. This is enforced, not
  merely preferred: a machine-wide `commit-msg` hook
  (`core.hooksPath` = `C:/Users/mega/.githooks`) rejects the commit outright,
  and it blocks human co-authors too, so the only ways past it are `--no-verify`
  or `ALLOW_COAUTHOR=1`. **Use neither without the user asking in that
  conversation.** If the hook blocks something, report it and stop; do not
  reword the message to slip a trailer past the pattern.
- **Bundle after committing.** The version is
  `MAJOR.MINOR.(commits since countBase)`, not the raw commit count, so a new
  minor restarts at `.1`. `countBase` is in `[package.metadata.build]` in
  `src-tauri/Cargo.toml` and **both** readers take it from there —
  `crates/build-support`, which the three build scripts call (it sets what the
  app reports and what each exe's version resource says), and
  `scripts/build-nsis.ps1` (which names the installer) — so a constant in each
  file cannot drift from the other.
  Raise the minor and the base together, in the same commit.
  `the_installer_and_the_app_agree_on_the_version` runs the script and
  compares it with the version compiled into the binary, because an app that
  says `0.2.3` inside `0.1.77-setup.exe` is the kind of drift nobody notices
  until a user reports it. So **bundle after committing**; bundling first
  yields the previous commit's number.
- **Do not commit before the user has looked at it.** Hand visual changes over
  as a build and wait for confirmation.

## Config storage
Behavior — what a profile file means, how the active profile is chosen, what
migrates from where — is in `docs/SPEC.md`. What belongs here is why the code
is shaped this way:
- It is all `Store` in `config.rs`, which takes its root directory so a test
  can point it at a temp folder instead of the user's real config.
- A profile has two names. The **id** is the file name and the only unique part
  (a lowercase slug, 48 characters at most via `sanitize_profile_name`); the
  free-form `profileName` inside the file is display only and may repeat.
  Collisions postfix the id (`profile_work_2.json`) rather than refusing, and
  `with_postfix` keeps the postfixed id inside the cap so `list_profiles` still
  accepts it. `create_profile`, `rename_profile` and `duplicate_profile` all
  return the `ProfileEntry` that resulted, for the same reason.
- `Store::backfill_profile_names` writes a derived display name into existing
  files at startup and reports it as a `ConfigNotice`; unreadable files are
  never rewritten. **A test that writes a profile file must include
  `profileName`** or it will pick up a `ProfileNamesBackfilled` notice it did
  not expect.
- App-wide preferences are `GlobalPrefs` / `UiPrefs` in `config.rs`, every field
  `#[serde(default)]`, which is what makes adding a preference a non-breaking
  change to a file written by an older build. `write_global_prefs` replaces only
  the `ui` key, so the active-profile pointer and any key a future version adds
  survive. Never write that file as a whole object.
- `Store::load` builds the candidate list from the stored file name, then the
  stored id, then `default`, and never scans the directory for a substitute. A
  missing `default` on a first run is the fresh-config case, not a
  `ProfileFallback`, which is why the notice is the thing to test for and not
  the absence of a profile.
- Legacy `marginPx` is mapped per anchor during normalization, so a migrated
  overlay keeps its screen position.

## Auto profile switching
Behavior — the rule shape, the matching modes, what the user sees — is in
`docs/SPEC.md` under *Auto profile switching*. This is why the code is shaped
this way.
- **The tray owns it, and only the tray.** It is the resident process, so it is
  the only one running while the Config window is closed, which is when a
  switch matters. A renderer running with no tray means no switching; that is
  the same deal as Pause and Exit. No new process and no new pipe message: a
  switch is the `SetConfig` a profile load already sends. `crates/core/src/rules.rs`
  is the pure half and both the tray and the window use it, so a switch could
  be relocated later without rewriting the matching.
- **`rules.json` is its own file, and the tray re-reads it on `(mtime, len)`.
  `Store::save_rules` goes through `write_atomic`, so the stamp can be trusted:
  noticing a change can never mean reading half a file. A parse failure keeps
  the rules already loaded — compiling "no rules" would strand the user on the
  fallback because of a typo — while deleting the file means off. The
  window→tray channel is this file; there is no reverse pipe.
- **The tray's engine pauses while the Config window is open**
  (`role_is_running(Role::Config)`), and `Engine::resume()` runs as it closes,
  so one push always goes out afterwards. That pause is not a gap in switching
  any more: while the window is open it runs an `Engine` of its own, on the same
  one-second cadence, on **every page**, deciding from the **saved** rules — so
  the window is the only writer of `activeProfile` in that time, which is
  exactly why the tray standing down is safe. A settled switch is held while
  `dirty || rules_dirty` and the engine is simply not told it applied, so the
  hold is a retry rather than a dropped switch; it lands as soon as the drafts
  are saved or discarded. `switch_profile` returns whether the profile is active
  afterwards, and `mark_applied` is called only on `true`
  (`the_window_engine_holds_a_switch_until_the_drafts_are_resolved`). The tray
  applies straight away, because with the window closed there are no drafts to
  disturb.
- **An auto switch writes `activeProfile` too, and that is not bookkeeping.**
  The Config window loads that pointer at startup and pushes what it loaded, so
  a switch that did not record itself would be silently clobbered the moment
  the window opened. The write happens only after the renderer took the config:
  a failed send leaves the engine wanting to retry, which is why
  `Engine::mark_applied` is called on success only
  (`a_failed_apply_is_retried_until_it_lands`).
- **Profile files are never written and the pause state is never touched.** A
  switch is runtime state plus the pointer.
- **Background tracking keeps a departure's probes alive, and only the sender
  knows which departures are permanent.** `ui.backgroundTracking` (on by
  default) rides every `SetConfig` beside the config, along with `retire`, the
  removals a Save has committed. The renderer cannot tell "switched away from"
  from "host the user just deleted", and must not guess: the window diffs the
  last saved key set against the config being saved (`retired_targets`) and the
  tray never retires anything. `ProbeManager` therefore holds three facts —
  `active`, `background`, and `configs`, the merged map the probe loop reads —
  so a kept target keeps measuring while its overlay does not exist. Turning
  the setting off clears `background`; `a_departure_keeps_its_task_when_background_tracking_is_on`,
  `a_saved_removal_retires_the_kept_target`,
  `background_tracking_off_drops_what_was_kept` and
  `a_returning_target_reuses_its_kept_task` pin the three-way rule, and
  `a_save_retires_the_targets_it_removed` pins the diff.
- **The background-tracking checkbox is written live *and* to the draft, like
  the Appearance ones.** `sync_runtime_config` reads the live preference every
  pass, so a draft-only write left the kept probes running after the box was
  unticked — the setting looked dead until Save, which is the shape this class
  of bug always has (`the_background_tracking_toggle_is_staged_and_live`;
  `set_background_tracking` is the one place that writes both halves). A
  removal a failed send could not deliver waits in `pending_retire` and rides
  the next push, including the reconnection one
  (`a_pending_removal_keeps_the_push_due`).
- **A rule matches a window, not a set of independent facts.** Every condition
  is tested against the same candidate window
  (`every_condition_is_tested_against_the_same_window`): "cs2.exe **and** a
  title mentioning Counter-Strike" is about one window, not two. First match
  wins, and the fallback only applies when at least one rule can match —
  enabled with nothing usable is inert, so a half-typed rule cannot mass-apply
  the fallback (`enabled_with_no_usable_rules_is_inert_not_always_fallback`).
- **The debounce counts ticks; `Engine::step` has no clock.** Two consecutive
  evaluations of the same decision, which is what makes a whole flap history
  testable in microseconds — the same reason `should_restart` takes a `now`.
- **A decision is edge-triggered**: a settled decision equal to what is applied
  is `Tick::Idle`, so the renderer is not sent a full config every second. A
  failed send is retried, and the retry logs once per distinct failure because
  a line a second buries everything else in the file.
- **`winwatch` is the only impure part, and it is a module of its own** so the
  matching can be tested with synthetic window lists — `monitors::enumerate`
  vs `resolve` again. Titles come from `GetWindowTextW`, which Windows
  documents not to send `WM_GETTEXT` to another process's window and so cannot
  block on a hung game; process names come from a Toolhelp snapshot, which
  needs no handle into the target and therefore works for an elevated game
  where `OpenProcess` would fail. Tool windows (`WS_EX_TOOLWINDOW`) and
  invisible windows are not candidates.
- **The Config window's preview calls `rules::decide` too**, so the preview
  cannot drift from behaviour. It runs on a 1s clock and only on the Global
  page, and `sync_auto_preview_text` takes the enumeration as a parameter so a
  test can count the reads.
- **A `rules.json` that will not parse is reported at startup and never
  rewritten by an unrelated Save.** A rules Save is the explicit permission to
  replace it, and clearing `rules_error` is that statement.

## The Config window
Behavior — what each pane holds, what the buttons do, what blocks a switch — is
in `docs/SPEC.md` under *Config window layout*. This is the wiring behind it.
- Three panes plus a status bar (`config_ui`): a navigation rail (`show_rail`,
  `Page`/`PAGES`, `rail_width`), a list pane (`show_list_pane`, dropped by
  `page_has_list_pane`) and a detail pane (`show_detail_pane`). Pane switching
  is never guarded; only profile switching is. `page_has_list_pane` is a
  function rather than a `page != Global` test at the call site so a new
  list-free page is one line in it, not an edit buried in the layout code that
  nothing points at.
- Each pane is a scrolling area above a fixed footer. **Save** and **Discard**
  are the detail pane's sticky footer (`show_detail_footer`,
  `DETAIL_FOOTER_HEIGHT`), right aligned, and `page_has_detail_footer` decides
  which pages carry it: only a page that stages a draft.
- Edits are staged per source. Detail-pane edits and Add overlay stage into the
  profile draft; list-pane enable/delete and Pause/Resume apply immediately.
  Delete uses an inline confirmation because native script dialogs are not used.
- **There are three independent draft flags: `dirty` (the profile),
  `prefs_dirty` (app-wide preferences) and `rules_dirty` (the auto switching
  rules).** Anything asking "is there anything to save?" must ask about all
  three, via `has_pending_edits` / the free `pending_edits`. The two-flag
  version gated the footer on `dirty` alone, so every Global preference was
  unsaveable from the day the Global page shipped. `save_edits` already called
  `persist_current` then `save_prefs` and reported "Saved." if either returned
  true, so only the enablement was ever wrong.
  `the_footer_follows_every_draft` holds the three-flag rule.
- **Every draft writer needs the same guard.** `save_prefs` returns early when
  `!prefs_dirty`; `persist_current` did not, so an unguarded Save rewrote the
  profile file from the in-memory draft when only `globalconfig.json` had
  changed, clobbering any edit made to that file from outside the app.
  `persist_rules` follows `save_prefs`, for the same reason one file over: a
  Save another draft triggered must not rewrite a `rules.json` the user may
  have hand-edited. All of them return `true` when there was nothing to write,
  so a single-draft save still says "Saved."
- The Global page stages into `prefs_draft` and writes on Save (`save_prefs`),
  while the rail's collapsed state and `show_version_in_title` apply to the
  live UI immediately, so the user watches the change. `PingApp::window_title`
  reads the **live** `prefs` for exactly that reason, never `prefs_draft`.
  `discard_edits` reloads both.
- The profile functions are named so the rules in `docs/SPEC.md` have a pointer
  into the code: `show_profile_switcher` heads the Overlays list pane and
  anchors the menu, `show_profile_detail` and `show_profile_dialog` are the
  create/rename/duplicate/delete editors on the Profiles page, and a list row
  sets only `selected_profile`. The load is `switch_profile`, separate from the
  selection precisely because switching is refused while `dirty`.
- **`selected_target` is a SECOND view-only selection, and it has exactly the
  rules `selected_id` has.** It starts as `None`, nothing auto-selects it, and
  clearing it never touches the draft — staged edits live in
  `config.overlays[..].targets`, never in the selection. Two independent
  selections because the two are nested (which overlay, which of its hosts), and
  **it is cleared wherever `selected_id` changes**: a host id belongs to the
  overlay it was selected in, and an id carried across would either find nothing
  (the editor draws nothing, which is confusing) or — if ids ever collided —
  edit a host the user is not looking at. The editor resolves it with
  `selected_target_in(overlay, selected)`, which finds by id and returns `None`
  for anything that is not one of this overlay's hosts, so a stale selection
  shows **no** editor rather than host 1's fields under host 2's highlight.
  That second outcome is the worse one and the reason the lookup is by id rather
  than by index.
- `selected_id` (the overlay in pane 3) starts as `None` and is **view-only**:
  staged edits live in `self.config.overlays`, never in the selection, so it can
  be cleared freely. **Do not re-add a startup auto-selection** — it meant a
  border was animating as soon as the window opened. Two things set it:
  `toggled_selection` for a row click, which clears when the clicked row is
  already selected, and the blank-area hit target. `delete_overlay` and
  `switch_profile` still focus an overlay, being deliberate actions on a
  specific row or profile rather than a startup default.
- `selected_overlay_for_border` encodes **four** conditions, not one: the
  window is open, the Overlays page is showing, something is selected, and the
  selection border animation is enabled. The page condition went missing for a
  release, leaving the last overlay's border animating on the Profiles and
  Global pages. It is the documented exception to "test a trigger by driving
  it" below: here the predicate *is* the mechanism, because
  `sync_border_preview` calls it every frame and sends only what changed, so a
  table over the function is the whole test
  (`the_border_preview_only_runs_on_the_overlays_page`).
- **The selection-border toggle is written to `prefs` and `prefs_draft` by the
  one helper `set_selection_border_animation`, because `sync_border_preview`
  reads the live value every pass.** A draft-only write leaves the live value
  unchanged, so the border keeps animating and the control looks dead
  (`the_selection_border_toggle_is_staged_and_live`). It gates only the
  selection preview — the per-overlay `startup_border_effect` is a separate
  setting and this preference does not touch it.
- The deselect strip must be registered with `ui.interact`, never laid out with
  `allocate_exact_size`. `interact` calls `create_widget` and touches no cursor,
  so the hit target adds nothing to the scroll area's content; a real widget of
  the leftover height pushed the content 725px past the viewport in
  `the_deselect_strip_does_not_disturb_the_scroll_content`, conjuring a
  scrollbar on any list that exactly fitted.
- `profile_overlay_counts` is a cache of overlay counts per profile, filled by
  `refresh_profiles` because that reads every profile file. It has exactly two
  kinds of trigger and **both must exist**: the profile mutations
  (`create_profile`, `rename_profile`, `duplicate_profile`, `delete_profile`,
  `switch_profile`) and the switcher popup call it directly, and *arriving* on
  the Profiles page calls it through `sync_profiles`, which watches `last_page`
  from `logic()`. It shipped showing `0` on every row for a release because only
  the mutations filled it. A cache whose trigger is missing fails as a
  confident wrong number, not as an obvious gap, so **a count absent from the map
  draws nothing** (`Option<usize>` all the way to `profile_row_contents` and
  `overlay_count_label`) — never `unwrap_or(0)` — and `refresh_profiles` filters
  unparseable profiles out rather than counting them as zero.
- **A doc comment that describes a trigger the code does not have is worse than
  no comment.** `refresh_profiles` claimed for a release that it ran on
  "entering the Profiles page", and that false claim is why the missing call
  above went unnoticed. When a comment names *when* something runs, check the
  call sites.
- **Test a trigger by driving it, not by testing its predicate.** Testing
  `arriving_page_needs_profiles` alone passed while the page read nothing,
  because the predicate was right and nothing acted on it. The body lives in the
  free `sync_profile_cache`, which takes the disk read as a parameter, so
  `arriving_on_the_profiles_page_re_reads_the_counts` can count the reads and
  assert the counts it published. `coming_back_to_the_profiles_page_reads_again`
  covers the other half: `last_page` advances even on the frames that read
  nothing. Write a conditional action so it can be called without the object
  that owns it.
- `sync_window_title` runs every frame from `logic()` and pushes
  `ViewportCommand::Title` only on change, so reading a preference there costs
  nothing. The root egui viewport starts hidden; Config-window close hides it
  and only tray Exit closes the app.
- Rail rows, glyphs and the switcher are painted with `ui.painter()`, not
  buttons, so a label can sit beside an icon. A painted glyph whose parts come
  from a table needs a centring-and-containment check: the Global glyph shipped
  with its row offsets read as loop indices by `enumerate()`, so the tuple
  halves swapped jobs and a knob poked past its track. `GLOBAL_ICON_ROWS` plus
  `the_global_glyph_is_centred_and_stays_inside_its_box` pin it.
- `show_status_bar` takes `&self` and is paint-only, so unlike the rest of the
  window layout no test can hold its shape. That is a known gap, not an
  oversight.
- **The sticky crosshair holds the mouse capture instead of installing a hook.**
  `StickyPicker` calls `SetCapture` on the Config window when a pick starts and
  then polls `GetCursorPos`/`GetAsyncKeyState` every pass rather than waiting for
  its own mouse events, so it works over other processes' windows and the click
  that ends the pick is swallowed — the target app never sees it. A
  `WH_MOUSE_LL` hook would do the same job while looking like a keylogger to
  every scanner, for a control used once per overlay. The candidates are
  `winwatch::shapes()` minus this process's own windows, i.e. exactly what the
  renderer's matcher can see, so a window that cannot be picked could not have
  been followed either. The three boxes are conditions in a fixed order
  (process exact, title contains, class exact); writing one box normalizes only
  that box and leaves the others' conditions verbatim, so a hand-edited regex
  title survives editing a different field. **A pick fills the process and the
  class and leaves the title empty** (`fill_sticky_target`): the picked title is
  true only for the moment it was read — Windows 11's Notepad reopens its last
  document, a browser's title follows the page — and since the boxes are ANDed,
  a stale title hides the overlay and nothing matches, so it never comes back
  (`a_picked_window_fills_process_and_class_but_not_the_title`). The title box
  stays for narrowing a target on purpose. **A lost capture is not a cancel.**
  winit's Windows backend calls `ReleaseCapture()` on every mouse-button-up once
  its own capture count reaches zero, so the release that ends a pick always
  drops the capture before the next pass; `poll` therefore commits on the
  release edge first and re-takes the capture afterwards. Cancelling on
  `GetCapture() != capture` — which is what shipped first — meant the boxes
  could never fill, and the next click went to the window under the cursor
  instead. The edge tracking is the pure `pick_step`, so the click sequence is
  tested without a mouse (`a_pick_commits_on_the_release_after_its_press`), and
  the capture window is checked to be this process's own
  (`own_capture_window`), because capturing a foreign window would send the
  pick's clicks there instead of swallowing them. **The window is displaced,
  not hidden, while the pick runs.** Hiding it — the first version — costs the
  pick three things at once: Windows hands the foreground back to the previous
  window, the hidden window loses the capture, and a hidden window stops
  receiving frames, so the polling sleeps; the first click then only focuses
  the target and the second one picks. `displace`/`restore_displaced` move it
  to `(-32000, -32000)` and back instead. The crosshair cursor belongs to the
  window: `poll` sets it only while the pointer is inside the window's rect,
  and the ordinary arrow comes back the moment it leaves — except while the
  window is displaced, when the crosshair is the only sign a pick is still
  running (`pick_cursor` is that rule, as a table). The button is the Save
  button's size, fill and label colour (`PICK_BUTTON_WIDTH` is wider so the
  painted crosshair and the label both fit), and the glyph is painted like the
  rail's, not an asset (`draw_crosshair_icon`, `the_pick_glyph_stays_inside_its_box`).

## The Config window's theme
Behavior — what a theme may set, and where the files live — is in
`docs/SPEC.md`. What is here is the shape and the traps.
- **A theme is the window's palette and nothing else.** The overlay graph draws
  with the per-overlay `lineColor` / `bgColor` / `prefillLineColor` the user
  already chose, and a theme that could override those would be fighting a
  setting that already exists. The tray menu is a native `HMENU` that Windows
  paints, and the tray icon and About logo are brand artwork. So `theme.rs` is
  15 colours and the Config window's own pixels.
- **`Visuals::light()` and `Visuals::dark()` are not one palette with the ends
  swapped.** Much of egui's widget drawing — checkbox ticks, scrollbar grips,
  selection handles, shaded non-interactive text — is derived from that base and
  not from the fields being set. Repainting the fifteen colours onto the wrong
  base gives a light background sitting on dark internals, which is the classic
  half-themed window. `theme::visuals_for` switches the base *and then* applies
  the palette, and `the_mode_really_switches_the_egui_base` holds it.
- **A theme change has to reach egui's WIDGETS, and `set_visuals` alone does
  not do that.** egui keeps two styles, `Options::dark_style` and
  `Options::light_style`, and `Context::set_visuals` is literally
  `style_mut_of(self.theme(), ..)`: it writes into *whichever theme egui
  currently considers active*, and `self.theme()` comes from egui's own
  `theme_preference`, which defaults to `System` and is re-read from the OS on
  every pass. So one `set_visuals` fills one of two slots without choosing
  which, and when egui later switches slots the widgets read a slot the app
  never wrote. The window was briefly right, then the buttons and popups came
  back as egui's stock defaults: white text on white. `theme::apply` therefore
  fills **both** slots and then calls `set_theme`, so egui's detection has
  nothing left to decide. `a_theme_change_reaches_the_widgets_after_egui_switches_slots`
  drives `apply` and reads a real widget's fill; against the old one-liner it
  reports `#3C3C3C` where the light theme wanted `#C2DDF0`.
- **The painted half and the widget half use different mechanisms, which is why
  the failure looked partial.** Painted things read the thread-local palette
  (`UI_*()`), which `set_palette` updates immediately; widgets read egui
  `Visuals`. A theme bug can therefore be invisible in half the window, and a
  single screenshot of the background proves nothing. Check a *button*.
- **A test that reads a colour this app painted cannot see this class of bug.**
  Every assertion on the mode changing, or on `set_visuals` having been called,
  passed while the window was visibly broken.
- **The fifteen `UI_*` names are functions reading a thread-local palette, not
  constants.** There are 119 uses across `ui.rs`, most of them in free
  functions with no route to a `&PingApp`, so threading a `&Palette` through
  every one would bury the change under a refactor. The trade is real: the
  palette is global state, so a test needing a particular palette has to set it
  and a second UI thread would not see this one. The egui UI is single-threaded
  and the palette does not change within a frame.
- **"Immutable" means repaired, not overwritten.** The built-in theme's files are
  embedded with `include_str!` and written to disk only when the copy there is
  missing or unparseable. A file that parses is honoured, so editing one is a
  real thing to do. Always overwriting would destroy an edit without saying so,
  and a theme you cannot experiment with is not worth having on disk.
- **A missing colour inherits the built-in for the mode being loaded**, which
  is why `ColorsFile` holds `Option<String>` rather than `#[serde(default)]`
  values: a `Default` impl cannot know the mode, so a partial *light* theme
  would get dark values for the keys it left out.
- **An asset name is a path, and the file saying so is user-writable.** Every
  name goes through `safe_asset_name`, which keeps only a bare file name; the
  worst case for anything else is a missing picture and a built-in fallback,
  rather than a theme reading a path out of the config directory.
- **A theme can be unreadable and still load perfectly**, so `contrast_report`
  runs on the way in and the built-ins are held to WCAG AA in a test. This is the
  "fails as a confident wrong thing" shape: no parse error, no gap in the UI,
  just a window nobody can read. It caught its own test fixture, which had
  asserted that an unreadable edit produced no notices.
- **A staged setting that something else re-reads every frame must be written
  LIVE as well as to the draft.** `sync_theme` resolves the mode from
  `prefs.ui.theme` — the *saved* value — every pass. A control that only wrote
  `prefs_draft` would have its pick applied and then silently undone on the next
  frame, which is the "briefly right, then it changes back" shape this app has
  now produced twice. `choose_theme` writes both, which is also what makes
  Discard work: `discard_prefs` restores `prefs`, and the same pass puts the
  theme back. `choosing_a_theme_changes_it_and_survives_the_next_pass` drives
  both and asserts the staging-only path does *not* change the mode.
- **The mode tiles are painted, and the moon's bite is only correct over its
  own tile.** The row is three `allocate_exact_size` squares; their fill, stroke
  and rounding come from `Style::interact_selectable`, the same `WidgetVisuals`
  a `selectable_label` uses, so they follow whatever a theme does to buttons.
  The glyphs are painted for the same reason the rail's are — no artwork, no
  light and dark variants — and the moon is a disc with an offset bite painted
  in the tile's fill colour, because egui has no subtractive clip. That is exact
  only because the tile paints itself and hands the same colour to
  `draw_theme_icon`; do not reuse the glyph over another surface.
  `the_theme_glyphs_stay_inside_their_boxes` holds the geometry (the sun's rays
  are a table, summed for balance, like `GLOBAL_ICON_ROWS`) and
  `the_theme_tiles_fit_their_pane` measures the tiles and their labels against
  the pane. Painted tiles take no keyboard focus, the same trade the rail rows
  make.
- **A test must not depend on the machine's Windows theme.** `System` resolves
  from the developer's own setting, so a test written against it passes on a
  light desktop and fails on a dark one. Pin an explicit `Light`/`Dark`
  preference in tests that need a known mode.
- **The mode is re-read on a clock, not on a notification.** `System` is the
  default, Windows changes its app theme with no message this process gets, and
  the registry read is cheap, so `sync_theme` looks every pass the way
  `sync_monitors` does. The comparison is on the *mode* and not on the theme,
  because the mode is the only thing that can change without the user touching
  anything — a theme file edited on disk is picked up at the next launch.

## egui layout traps
Every trap below shipped once. Each test named here fails on the old behaviour.
- **Centre a column with `ui.with_layout(..)`, never `ui.vertical_centered(..)`.**
  `with_layout` re-lays-out the *existing* `Ui` and creates no child, so there
  is no child min rect to overshoot and no 18px `interact_size.y` band.
  `vertical_centered(..)` is a `scope_builder` child, which is the exact shape
  that made the detail footer report 22px more than its box and slide into the
  status bar. For `Layout::top_down` the main axis is vertical, so `Align::Center`
  is the cross axis and centres each line horizontally. Used by
  `about_page_column`. `show_detail_footer` is the standing counter-example: it
  uses `with_layout` with two `add_enabled_ui` children and has rendered
  correctly through every release, so the rule is about the child failing to
  fill its box, not about child count.
- **eframe's NATIVE runner ignores `egui::OutputCommand::OpenUrl`,** so a
  `Hyperlink` on a native window is inert; only eframe's *web* runner acts on
  it (0.36.2's `src/native/*.rs` contains no `OutputCommand` handling at all).
  The About page's link shipped a release claiming it opened a browser while
  doing nothing, because half the chain was verified and the half that actually
  runs was not. So `open_requested_urls` drains the command at the **end** of the
  UI pass with `Context::output_mut` (not in `logic`, which runs before the panes
  are drawn and would open it a frame late) and `open_url_in_browser` calls
  `ShellExecuteW`, reporting either outcome in the status bar. `requested_url` is
  the pure mapping `a_link_click_becomes_a_url_to_open` holds. **A link that
  reads as obviously working is not evidence that it works** — check the host
  the app actually runs on, all the way down.
- **A child that does not fill its box can report a min rect LARGER than the box
  it was given**, and the parent advances its cursor by whatever the child
  reports, so it moves **everything** laid out after it. `show_editor`'s empty
  state sat in `ui.centered_and_justified`, whose doc says only one widget may
  be added, and three small widgets made it report 22px taller than its box,
  pushing the detail footer 19.5px down into the status bar. The symptom only
  appeared with nothing selected, because the selected path is a `ScrollArea`,
  which reports its box exactly. Use one widget — here one label with an
  embedded newline, via `empty_editor`. The three it replaced were a label, an
  `add_space` and a label. Pinned by
  `the_detail_footer_stays_under_the_detail_pane`.
- **A mirror test must reproduce the real bounds, width AND height.** Without
  wrapping its body in a positioned child, `detail_pane_geometry`'s vertical saw
  the whole window, which produced three false results in a row before the real
  figures. A mirror that does not reproduce the bounds blames the code for
  things the code is not doing.
- **Rows are painted, never `egui::Frame::group`,** because a group frame draws
  a 1px border and the rows must match the rail's borderless look. Allocate the
  row with `allocate_exact_size`, `rect_filled` the background yourself, then
  `ui.new_child(egui::UiBuilder::new().max_rect(..))` for the contents. Fill only
  when selected or hovered; an untouched row is transparent.
- **Allocate a painted row at `list_pane_row_height(contents)`,** never at the
  content height. `egui::Frame` sizes *itself* to its contents plus its margin
  and a `rect_filled` does not, so the child `Ui` was handed less room than it
  asked for and every button ended up flush against the fill. Pinned by
  `a_painted_row_carries_its_margin`.
- **`ui.horizontal(..)` is not vertically transparent.** It hard-sizes its child
  to `ui.spacing().interact_size.y` (18px by default) and allocates that through
  the parent layout, which centres it, so a taller widget inside it lands low
  and spills out the bottom. Inside a row of known height use a positioned child
  (`ui.new_child(..)`), which honours the absolute rect. Pinned by
  `a_list_pane_row_puts_its_contents_inside_its_own_fill`.
- **Both halves of a row come from one rect.** `row_inner(row)` gives the
  contents area and `right_anchored(inner, width)` places every right-hand
  control, so the name on the left and the buttons on the right cannot disagree
  about where the row ends. A row that reserves a gutter must lay it out
  horizontally and subtract `ui.spacing().item_spacing.x`; a `Frame` lays its
  content out top down, so adding the gutter to the same `Ui` drops it onto the
  next line and leaves an empty column. `a_profile_row_fits_its_pane` and
  `an_overlay_row_fits_its_pane` keep that arithmetic honest.
- **Everything in the list pane derives its width from `list_pane_column`,** the
  one width shared by the header, the rows, the scroll area and the footer, plus
  the inset that centres it. An inset has to move a widget's **position**, not
  only come off its width, and a reserve may only be subtracted once —
  `list_pane_row_width_for` once did both, so all 32px of slack landed on the
  right and the two boundaries read 4px and 40px. Pinned by
  `the_list_pane_column_is_centred_in_its_pane`.
- **`Ui::new_child` is invisible to the layout that created it.** It paints into
  the rect you give it and reports nothing back, and `allocate_ui` finishes by
  setting the parent's cursor from the child's `min_rect` (`advance_after_rects`
  advances from the *widget* rect, not the frame rect). A child that painted
  itself in a grandchild therefore has an empty `min_rect`, the cursor goes back
  to the pane's left edge, and the next pane is laid out on top of it. So
  `config_ui` follows `show_list_pane` with
  `ui.advance_cursor_after_rect(ui.max_rect())`, and any future pane that draws
  itself that way needs the same line. Pinned by
  `a_pane_that_draws_itself_keeps_its_place`.
- **Judge pane spacing by content to content** across a boundary, not hairline
  to nearest content. The rail and the detail pane sit flush to their pane edges
  while the list pane's column is inset, so the hairline metric reads 16px on
  one side and 24px on the other even when the spacing is even at 28px both
  sides.
- **The window has to fit.**
  `the_window_fits_the_rail_the_list_and_the_detail_pane` and
  `the_detail_pane_keeps_its_scrolling_area_above_the_footer` hold the
  minimum sizes honest.

## Processes and the pipe
### The three processes
- `plo-tray.exe` is what a user launches and the only one that stays resident;
  `plo-config.exe` starts on demand and **exits when closed**; `plo-renderer.exe`
  (a `[[bin]]` in `crates/core`) draws the overlays. Two of the three carry no
  eframe at all — that is the whole point, since the GL context exists only
  while the window is open, so close means exit and never hide.
- **No package may reach a GPU context or an event loop.** That is the property
  the memory win depends on, and `crates/core/tests/no_gui_dependencies.rs`
  checks it with `cargo tree`. The deny lists are two-tier because the tray
  *is* `tray-icon`; the rule is named for what it forbids, not for the crates it
  happens to list. `tray-icon` is a GUI crate and belongs to `crates/tray`
  alone — the application shell once depended on it and nothing in `src/` used
  it, which is how a dead dependency hides in a manifest for a release.
- The renderer needs no package of its own — it uses nothing the library does
  not already depend on, and `cargo tree` reports a package's deps across every
  target, so the test guards the binary for free. The tray does need one: a
  `[[bin]]` beside the window would link eframe into the resident process.
- The binaries are named tersely on purpose: they sit side by side in the
  install folder and Task Manager, and a name long enough to abbreviate is hard
  to tell from its neighbour. Crates keep descriptive names; the product name,
  installer, Start Menu folder and window title are all still
  PingLatencyOverlay. `LEGACY_EXE_NAMES` in `transport.rs` holds the pre-rename
  names and the installer kills them, because a file cannot be deleted while
  its process holds it open.
- `default-run` is set on the root package; `cargo run` with two binaries is
  ambiguous exactly where a developer starts the app. The window is the default
  because it starts whatever it needs.

### Launching
- **Every process is `windows_subsystem = "windows"`, so `eprintln!` goes
  nowhere.** `crates/core/src/diagnostics.rs` is the answer: `log_line` appends
  to `pinglatencyoverlay.log` beside the config, and `fatal` puts up a
  `MessageBox` for the tray's startup failures, the tray being the one process
  that never draws a window. **Any `return` on a tray error path needs one of
  the two** — a bare `return` is invisible forever. The tray also logs a
  heartbeat, because a wedged loop and a feature that was never written look
  identical from outside. **A release build writes nothing unless `PLO_LOG` is
  set** (`log_enabled`; a debug build logs as before), so the "one of the two"
  rule has a release reading: a log line is silent for an ordinary install, and
  anything the user must see has to be a `fatal`.
- The launch matrix is `should_start(me, missing)`, tested over all nine cells:
  a process never starts itself, the renderer starts nothing, the tray does not
  open the window on its own, and everything else missing gets started. It is a
  function because three bugs came from the rule living in three places and none
  agreeing.
- **The pipe is the rendezvous**, with no registry, lock file or heartbeat,
  which is what makes "run without the tray, bring one later" free.
  `SingleInstance::acquire(Role)` uses a per-**role** mutex so the tray can exit
  while the window stays open and a crashed renderer is replaceable; three roles
  means three pairs, and `each_role_gets_its_own_versioned_mutex` checks all of
  them, because comparing only two lets the third share one. `Ok(None)` means
  somebody else holds it — the ordinary "ran it twice" answer, not an error.

### The pipe
- `transport.rs` owns the wire format and **both ends are compiled from the same
  crate**, so the two processes cannot disagree without a compile error. Messages
  are `set_config`, `set_paused`, `set_border_preview`, `ping` and `shutdown`,
  tagged `#[serde(tag = "kind")]` so an unknown kind is **rejected rather than
  silently ignored**. `ping` is a no-op and nothing may act on it; borrowing
  `set_paused` for a probe would resume the probes whenever the sender's mirror
  was stale.
- **Framing is one JSON object per line, and the newline is not optional.** A
  pipe is a byte stream, a length prefix is the usual source of off-by-one bugs,
  and JSON escapes its own newlines.
- **The server serves several clients at once** — `serve` spawns two
  `instance_loop`s, each holding one instance and making the next when it
  finishes with a client, so a spare is always waiting. A rendezvous designed
  for one caller and handed two is a defect, not a configuration.
- **A fresh instance per connection.** Windows returns a stale
  `ERROR_PIPE_CONNECTED` (a success to tokio) rather than blocking on an
  instance a client has disconnected from, so reusing one spins at 8% CPU with
  no sleep on any leg. Only the first instance may claim `first_pipe_instance`;
  the renderer mutex, not that flag, is what guarantees one renderer. **No test
  can catch this** — only the OS shows you a spin.
- Each process finds the others as **siblings of its own executable**
  (`sibling_exe`), so all three must be installed into one directory and
  `build-nsis.ps1` checks all three exist before packaging.
- A client handle is opened `GENERIC_READ | GENERIC_WRITE`, and a liveness check
  must not be able to block. `PeekNamedPipe` queries rather than transfers, but
  it **needs read access**: a write-only handle answers every query with
  `ERROR_ACCESS_DENIED`, which reads as "the renderer is gone". Before adding a
  query here, check the handle can answer it.
- `ERROR_FILE_NOT_FOUND` maps to `io::ErrorKind::NotFound`, distinct from every
  other failure, because it is the answer that says *start one*. Lumping the
  ordinary answer in with a fault is what makes a working system look broken.

### Live edits
- **An overlay edit reaches the renderer without a Save.** Before the renderer
  was its own process, `logic()` called `sync_overlays()` on **every frame** and
  the in-process overlay manager re-read `self.config`, so any staged change was
  on screen immediately. `a2e0404` turned that per-frame call into a pipe
  message and put the message only on the save paths — so every appearance edit
  silently became save-only, and the only edit that stayed live was the list
  pane's enable toggle, because `ac58523` had added an explicit call for that
  one action. **The regression was silent**: the overlay still appeared on Save,
  so nothing failed and the app just stopped feeling live. If live updating ever
  looks broken, check `sync_runtime_config` is still called from `logic()`
  before suspecting the pipe.
- **`sync_runtime_config` normalizes the draft IN PLACE before comparing, and
  that order is load-bearing twice over.** Compared the other way — a normalized
  `last_pushed` against an un-normalized draft — the two never match, so the
  window sends the whole configuration on **every frame** for as long as it
  stays open. That is not slow enough to look wrong: it is a pipe write and a
  full config parse ten times a second, forever, with no symptom. It also makes
  the drag story right, since a clamped value is what Save would write anyway.
  `an_out_of_range_draft_settles_instead_of_resending` counts sends over 20
  frames, which is the only way that bug is visible — there is nothing to assert
  against except the count.
- **The comparison is the affordability argument, so do not "simplify" it into
  an unconditional push.** `Config` derives `PartialEq`, so an unchanged frame
  costs a field comparison and *no clone*; the clone only happens when there is
  something to send.
- **`last_pushed` must be updated by every path that sends a config**, and
  `persist_current` is the exception that proves it: it sends directly rather
  than through `push_config` because it has to tell "written to disk" from
  "renderer told" apart, and it updates the record **only on success** so a
  failed send leaves `sync_runtime_config` still wanting to try.

### Supervision
- **The tray supervises the pipe, not the child handle.** A lost pipe is a
  better signal than a child, because a hung renderer still holds its mutex
  while answering nothing.
- **Only stop a process you spawned.** `renderer_process` is set only when this
  process started the renderer, which also governs the `Shutdown` on `Drop` and
  the tray's `Exit`. A blanket kill takes an overlay set the user still wants
  off their screen.
- **A client is not a supervisor, and the restart half must not travel with the
  start half.** The window starts the renderer once at launch if nothing else
  has; the tray owns it from then on. When the window also gained a restart path
  they fought and neither could win. The window's `reconnect_renderer`
  reconnects and never spawns, and carries no storm guard — a storm guard stops
  a crash loop, and this is not the process that would be looping.
- **Bringing a renderer up and noticing a crash are two different states, and
  only the second costs an attempt.** `App::starting` is an `Option<Instant>`;
  while set, `finish_starting` retries the pipe and the config push and nothing
  counts against the guard. `START_TIMEOUT` (10s) ends the grace period.
- **Never discard a send error.** Both fire-and-forget pushes used
  `let _ = self.send(..)`, so a command that never left the process looked
  exactly like one that was obeyed. `finish_starting` retries the config push
  until it lands; `toggle_running` logs both halves.
- **The storm guard's clock is a parameter.** `should_restart(now)` takes an
  `Instant` so a test can drive a whole crash history in microseconds, and
  `forget_expired` is split out so forgetting is a statement about the list. The
  tray owns the guard; a renderer with no tray is not restarted.
- **Say what actually happened.** Writing the profile and telling the renderer
  can fail separately, so `persist_current` reports "Saved, but the overlays
  were not updated" rather than a confident "Saved."
- Neither the tray nor the window has a `ProbeManager` or `OverlayManager`, or a
  tokio runtime. Anything the renderer needs travels over the pipe —
  **including the animated border preview**, which used to be a direct call and
  is now `set_border_preview`, sent only when it changes, and cleared by
  `release_border_preview` on close.

### Windows
- **Any thread in ANY of our processes that creates a window must pump that
  thread's messages.** A tray icon is a window too. Windows delivers a window
  message only to a thread that pumps its queue, so a thread that sleeps
  receives *nothing* and the symptom is a window that looks perfect and is
  completely dead. `tray-icon` documents this ("an event loop must be running
  on the thread") and spawns no pump on Windows, so the caller must;
  `pump_messages()` is ours. **This shipped twice, once per process** — so grep
  for `windows_subsystem = "windows"` before assuming a new process is exempt. A
  rule that names one caller is a rule the next caller will not find.
- **The repaint cap and the liveness cap are different decisions.**
  `MESSAGE_POLL_INTERVAL` (16ms) is independent of `REPAINT_INTERVAL` (100ms);
  tying them together would mean a slow redraw is also an unresponsive process.
  The wait is `min(deadline, cap)` via `wait_before`, and a pipe command resets
  the deadline. Only a user can confirm Windows stops calling the process hung.
- **Letting the default handler run can be worse than ignoring the message.**
  `DefWindowProcW` on `WM_CLOSE` *destroys* the window and the loop rebuilds it,
  so Task Manager's "End task" concludes its graceful close worked and never
  escalates. `overlay_wnd_proc` handles `WM_CLOSE | WM_QUERYENDSESSION` by
  setting a flag and returning `0` without calling the default, and the loop
  checks `quit_requested()` right after `pump_messages`. The flag is a static
  because a window procedure cannot stop the loop that called it. Any `WM_*` we
  do not handle must be checked against this: the default is not a safe place
  to land.

## Overlay rendering
- Overlays are not egui child viewports. Each is a native `WS_EX_LAYERED` popup
  rendered with `UpdateLayeredWindow`, so it has true per-pixel alpha, no DWM
  frame, no taskbar button, no focus, and mouse passthrough.
- **Wallpaper mode parks the window above the desktop instead of embedding it,
  and that is a measured decision, not a simplification.** A layered child of
  the desktop presents nothing but a flash on the raised desktop (build 26100+
  and this machine's 26300: the compositor does not hold the content of a child
  under `Progman`), and the only recommended way to live in that layer is a GPU
  present path — which the renderer deliberately does not have. The overlay
  therefore stays an ordinary top-level layered window: created without
  `WS_EX_TOPMOST`, and placed with `SetWindowPos(hwnd, host, …)` so it sits
  directly above the shell's desktop window (`GetShellWindow`, i.e. Progman),
  which puts it above the wallpaper and the desktop icons and below every
  normal window and the taskbar. It is above the icons rather than below them —
  the icons show through wherever the overlay draws nothing — and it is what
  Rainmeter's on-desktop skins do. The re-assert that used to re-set topmost is
  checked rather than unconditional (`GetWindow(hwnd, GW_HWNDNEXT) == host`),
  because re-placing a window that is already in the right spot is churn the
  user sees as a flicker; if the shell window cannot be found the window is
  simply re-checked the next second. A wallpaper-mode window refuses
  `SC_MINIMIZE` in its window procedure, so Show Desktop cannot take it away —
  the absent topmost bit is also the mode's flag there.
- **Display mode is an enum, not a pair of flags, because the three placements
  are mutually exclusive by construction.** `DisplayMode { Global, Sticky,
  Wallpaper }` replaced `wallpaper_mode: bool`; the old key is still read —
  `legacy_wallpaper_mode` is `skip_serializing` and `normalize` folds
  `wallpaperMode: true` into `DisplayMode::Wallpaper` only when the file names
  no mode of its own, the same one-way pattern as `legacy_margin_px`. Two
  booleans could express "wallpaper and sticky", which has no meaning, and the
  monitor pin is kept rather than cleared so switching back to Global restores
  the display the user chose.
- **Sticky mode is followed by the renderer's own poll, not by a hook and not
  by the tray.** `OverlayManager::follow_sticky` runs at the top of every loop
  tick (the loop already wakes every `MESSAGE_POLL_INTERVAL`, 16 ms): per live
  target it reads `IsWindow`/`IsIconic`/`IsWindowVisible`/`GetClientRect`/
  `ClientToScreen`, all window-manager getters that cannot block on the target
  process, and only when there is no handle does it sweep `winwatch::shapes()`
  — at most once a second — and resolve. **A target is placeable only while it
  is neither minimized nor hidden**: Steam closing to the tray hides its window
  instead of destroying it, so `IsWindow` alone said the overlay should keep
  following a window nobody can see. It returns whether anything changed so the
  loop pulls its next layout pass forward, which is the whole follow latency.
  The tray would have
  had to poll the same getters and then ship coordinates down the pipe at up to
  display rate; the work is a handful of syscalls, so the hop would only add
  moving parts. `sticky::resolve` is pure over a `&[WindowShape]` (focused
  match first, else topmost; a minimized window is never a target), which is
  what makes the multi-window rules testable with synthetic lists — the same
  split as `monitors::enumerate` vs `monitors::resolve`.
- **The sticky matcher is `rules`' matcher.** `CompiledMatcher` compiles a
  `Condition` list with the same `compile_checks`/`checks_match` the auto-switch
  rules use, so "chrome.exe and a title mentioning GitHub" cannot mean one thing
  to a rule and another to a target; a matcher that cannot mean anything (empty,
  blank value, bad regex) matches nothing, exactly like an errored rule.
- **An owned overlay is the FollowWindow z-order's one sharp edge.** With
  `StickyZOrder::FollowWindow` the overlay drops `WS_EX_TOPMOST` and is owned by
  the target (`SetWindowLongPtrW(GWLP_HWNDPARENT, target)`), so Windows keeps it
  in the owner's z-order band — and destroys it when the owner is destroyed.
  That is expected: `apply` notices the dead window and the ordinary creation
  path rebuilds it. The once-a-second re-assert skips FollowWindow on purpose;
  re-setting topmost there would break the mode.
- **A hidden overlay now has two causes, and they are one flag on purpose.**
  `OverlayWindow::hidden` was "the pinned display is not attached"; it is also
  "the sticky target is minimized, closed, or matched by nothing". Both mean the
  same three things — do not repaint on the prefill/border clocks, do not
  re-assert topmost, keep the series so the graph comes back with its history —
  so the flag stayed one flag and its doc names both.
- `render.rs` writes premultiplied RGBA; `overlay.rs` swaps R/B to premultiplied
  BGRA before copying it into a 32-bit DIB. `bgOpacity=0` leaves the alpha byte
  at zero; positive values are composited by Windows.
- **The graph uses the actual physical window dimensions.** Do not assume
  `windowSeconds * scale` is the drawable size under Windows DPI/text scaling.
- **There is no `groups` array: a group is an overlay with N `targets`, and a
  single-target overlay is what every pre-grouping profile migrates into.** One
  code path rather than two is the whole reason; a grouped and an ungrouped path
  would be two things that could disagree about how a line is placed. The legacy
  `probe` / `timeoutMs` / `lineColor` / `timeoutColor` keys are
  `skip_serializing` fields consumed in `migrate_probe_into_targets`, exactly like
  `legacy_margin_px`. `normalize` guarantees `targets` is non-empty *unless the
  overlay had no legacy probe either*, and `validate` rejects that case —
  **inventing a host would put `1.1.1.1` on someone's screen they never asked to
  ping**, which is the "confident wrong thing" shape. `a_pre_targets_profile_becomes_one_target`
  and `an_overlay_with_no_hosts_is_refused_rather_than_given_one` pin both halves.
- **`parse_and_validate` normalizes BEFORE it validates, and the order is
  load-bearing.** Validating first rejects every existing user's file, because a
  pre-grouping profile has no `targets` key at all, and the error it produces
  ("overlay has no host to probe") describes the migration rather than anything
  the user did. It is also the natural order: normalize only fixes what it can
  and leaves a blank host blank and a TCP port of zero at zero, so nothing
  validation is there to catch gets repaired away.
- **Probe tasks are keyed by `(overlay_id, target_id)`, not by overlay id.**
  `SampleStore` is therefore `HashMap<overlay id, HashMap<target id, SampleBuffer>>`
  — nested rather than flat because the renderer walks all of one overlay's hosts
  on every frame, and a flat map would make the cost of a group grow with the very
  thing the lock protects. **One task per host, not one per overlay**: a probe
  measures for as long as its timeout allows, so a task probing four hosts in
  turn takes four timeouts per tick when they are all down. Separate tasks
  overlap. A target id is only unique within its overlay, which is why the pair is
  the key and why `validate` checks per overlay.
- **Every host's line is drawn from its own series state**, and `draw_series`
  resets the segment, the last point and the last Y per host. Carrying any of it
  across iterations draws a diagonal from one host's last sample to another's
  first — invisible with one line, so it shipped into the first draft of this.
  `a_host_is_drawn_where_it_would_be_drawn_alone` compares each host's drawn
  pixels with the group against them alone.
- **`draw_series` breaks the line on a data gap, and the threshold follows the
  target's timeout.** The probe loop measures, writes a sample — including a
  failed one — and then sleeps a whole tick, so the longest interval it can
  produce is `timeout_ms` plus `SAMPLE_INTERVAL`; `sample_gap_threshold` adds
  slack on top. A fixed threshold would either cut a slow-but-continuous line or
  stay silent through a real hole. Everything longer is time nothing was probing
  the target — a profile switched away, or Pause — and the segment ends and
  resumes at the last known value exactly as the timeout path does. The buffers
  deliberately survive a profile switch (that is what lets the graph come back
  with its history); this break is what keeps them honest. `SAMPLE_INTERVAL`
  lives in `render.rs` and `probes.rs` reads it, so the cadence samples are
  written at and the cadence a gap is measured against cannot drift.
  `a_data_gap_is_not_interpolated_in_smooth_mode`,
  `a_data_gap_is_not_interpolated_in_index_mode` and
  `a_one_second_cadence_is_not_a_gap` pin both halves.
- **A host's visible window is cropped per series, not once for the overlay.**
  A newly added host has a handful of samples and none of the history the others
  have; cropping them at one index drops them or pushes them to the left of where
  their timestamps put them. `a_newly_added_host_is_placed_by_its_own_timestamps`
  pins it.
- **`sync_series` is a free function taking `&mut Vec<WindowSeries>`,** not a
  method: a window owns an `HWND` and a `LayeredSurface`, so a method could only
  be tested with a real window and the tests would skip off Windows. It matches
  on target **id**, not position, so a reorder or a removal does not reset the
  lines below it, and it returns whether it changed anything — a host added while
  the samples and the rest of the config are both unchanged is invisible to every
  other trigger in `apply`.
- **The prefill is seeded by target id, not overlay id.** Seeding per overlay
  gives every host of a group the same fake latency curve, which looks like one
  host with a fat line — the exact thing grouping disambiguates.
- **The prefill's shape is deliberate: a drifting baseline with jitter and
  sparse spikes, not a wave, and its level is absolute milliseconds rather
  than a fraction of the axis.** Real latency is not periodic, and the summed
  sines this replaced read as a pacemaker; the fraction-of-the-axis version
  that followed rested at a fifth to a third of a one-second axis, which reads
  as a terrible connection on an overlay nobody is even probing yet, and said
  something different on every scale. `the_cosmetic_prefill_is_jagged_rather_than_a_curve`
  holds the shape (frequent direction changes, an attack larger than the old
  smooth step) and `the_cosmetic_prefill_rests_at_a_healthy_latency` the level.
- **`OverlayWindow.sample_generation` is the max across hosts, not one host's.**
  It answers "has the real graph started", and a group where one host answered
  has started; the others draw an empty line rather than a fake one.
- **Timeout markers are drawn in the host's own timeout colour, and the render
  loop skips an empty series.** The colour is per host because a full-height
  marker on a shared plot is otherwise indistinguishable from another host's.
  In the pixel tests, unpremultiply the alpha before comparing colours — a 1.5px
  antialiased stroke stores roughly half the colour at the edges, and matching
  the raw bytes finds only the fully covered middle, which is how an assertion
  meant for a timeout marker can be satisfied by another host's *line*.
- **A monitor is a device name, and a missing one hides the overlay.**
  `MonitorInfo` is a plain struct with no Win32 in it and `resolve` is pure,
  because the interesting states — a panel left of the primary, one at 150%
  next to one at 100%, a pinned display that is unplugged — are exactly the
  ones a one-monitor developer cannot produce. `GetDpiForSystem` is the
  *primary* monitor's DPI whatever the caller does, so `layout_for` takes a
  `&MonitorInfo` rather than an `f32`: a DPI number and a work area that can
  disagree are how a 150% panel ends up laid out at 100%.
- **A hidden overlay has to be excluded from three things that do not look like
  rendering.** `prefill_repaint_interval`, `border_repaint_interval` and the
  once-a-second `reassert_topmost` all read `self.windows`, and all three would
  otherwise work on a window that is not on screen: the first two spin the
  renderer at display rate for a frozen prefill, and the third passes
  `SWP_SHOWWINDOW`, which quietly puts the graph back. `OverlayWindow::hidden`
  exists so that is one flag and not three separate guesses.
- **The picker's list is not allowed to disagree with the profile.** A pinned
  display that is unplugged is still an entry, and still the selected one, so a
  hidden overlay has a visible reason to be hidden. An egui `ComboBox` whose
  entries do not contain the selected value falls back to its first row, so
  dropping the entry would report a *different* monitor as chosen and a save
  would re-pin it.
- **`EnumDisplayMonitorsW` is exported under the name `EnumDisplayMonitors`.**
  The header says `W`, the SDK's `user32.lib` has no such member (verified with
  `dumpbin /LINKERMEMBER`, which lists every other A/W pair separately), and
  there is no A/W split to make anyway — the call takes no string. It is
  declared with `#[link_name]`; spelling it `W` is a link error, not a wrong
  answer.
- Sample buffers are bounded and overlay HWNDs are reused by stable ID. Do not
  allocate one renderer or surface per overlay.
- `ProbeManager::apply_config` must not restart all tasks for a style-only Save.
  Existing tasks read shared settings each tick; only deleted/disabled overlays
  are stopped. This keeps Save from pausing the graph.
- **The underglow is cast in the graph's own frame, and one function owns its
  room.** `line_glow_reserve_px` is read both by `overlay::layout_in_rect`,
  which grows the window's short dimension, and by `render_graph_into_internal`,
  which insets `bottom` by the same amount, so the box and the drawing cannot
  disagree about the band past the zero line and the axis keeps the height
  `graphHeightPx` names. The cast direction is `glow_direction`, the image of
  the graph's +Y basis under the same rotation and mirror as `transform_point`
  (they share `rotation`), because screen-down would smear a 90°/270° overlay's
  cast along the time axis. Timeout markers end at the canvas edge rather than
  at `bottom`, so they run through that band. The draw order is every series'
  cast, then every marker, then every series' core: a glow over a marker would
  tint it into the glow, while a core still lands on top of a marker it crosses
  and no glow can tint an earlier host's line where two cross.
- **The stroke width is one read for every line, and the pad follows it.**
  `render_graph_into_internal` builds one `Stroke` from `lineStrokePx` and hands
  it to the lines and to the timeout markers alike. `stroke_pad` —
  `(width / 2 + 0.5).max(2)` — keeps the pixmap edge from slicing a thick line
  clamped to the ceiling or resting on the zero line; at the default 1.5px it is
  exactly the old 2px. The cast's layers already start at the core's half-width
  (`stroke.width / 2.0`), so a thicker line's glow starts at its edge and the
  reserve stays `radius + 1`.
- Graph orientation, timeout marks, the X and Y axes and the work-area
  positioning rules are behavior and live in `docs/SPEC.md`; do not restate them
  here.

## Installer and build
- The installer is hand-written NSIS (`packaging/nsis/installer.nsi`, driven by
  `scripts/build-nsis.ps1`). What it installs and which checkboxes it offers is
  in `docs/SPEC.md`; the traps are here.
- MUI2 gives the finish page exactly **two** checkbox slots, and there is no way
  to add a third. `MUI_FINISHPAGE_SHOWREADME` is reusable as a generic second
  one: set `MUI_FINISHPAGE_SHOWREADME_TEXT` and
  `MUI_FINISHPAGE_SHOWREADME_FUNCTION`, and MUI calls the function instead of
  `ExecShell open`ing a file. That is how the desktop shortcut is offered, and
  the name is misleading, so the script says so at the definition.
- **`ExecWait` of a console-subsystem program flashes a terminal, and there is
  no flag to stop it.** The installer is a windowed program and `taskkill` is a
  console one, so a plain `ExecWait` hands it a new console and the user sees
  one flash per call — seven of them. `SW_HIDE` is **not** the answer: it is an
  `ExecShell` flag, and makensis rejects it on `ExecWait` ("expects 1-2
  parameters"). Measured with a probe that calls `GetConsoleWindow` on itself —
  a bare `ExecWait` reports a *visible* console, `nsExec::ExecToStack` reports
  one that is allocated but never shown, because it uses `CREATE_NO_WINDOW`.
  `nsExec` ships with every NSIS distribution, so the build script needs no
  change. Keep the old-uninstaller `ExecWait` on `ExecWait`: that one is a
  windowed program and has no console to flash.
- **A gate needs both directions tested, and `$INSTDIR` in a test installer is
  not the directory you set.** `InstallDirRegKey` reads the *real*
  installation's `InstallLocation`, so a test that sets `InstallDir` to a temp
  directory still installs over the user's real one unless the uninstall key is
  repointed too. That is not hypothetical: a gate test that read 3 kills for an
  upgrade that should have made 7 was the harness lying, the macro was right.
  A count-only assertion would have called that a pass.
- Two NSIS traps, both of which cost time here. `MUI_PAGE_CUSTOMFUNCTION_PRE`,
  `_SHOW` and `_LEAVE` are **not page-scoped**: `Pages.nsh` `!undef`s them after
  the first page that reaches the insertion point, so they fire on the welcome
  page and can never reach the finish page. And NSIS has no `.onInstDone` — the
  callback is `.onInstSuccess`. A misspelled callback still compiles, but NSIS
  emits `warning 6010: ... not referenced - zeroing code out` and **deletes the
  body**, so the silent-install path quietly did nothing while the build looked
  green. Treat warning 6010 as a hard failure when packaging. Relatedly,
  `${Silent}` is a LogicLib *condition*, so it is `${If} ${Silent}`, never
  `${If} ${Silent} == 1`.
- The installer stops seven executable names before removing the old
  installation: the three current ones, the three pre-rename names in
  `LEGACY_EXE_NAMES`, and the single pre-split `ping-latency-overlay.exe`.
  The current three are killed unconditionally — a developer running the build
  out of `target\debug` has them running from a directory `$INSTDIR` knows
  nothing about. The four pre-0.2.0 names go through `KILL_IF_INSTALLED`, which
  checks `$INSTDIR` first, so a current install spawns three `taskkill`s
  instead of seven. The four names are hand-copied from `LEGACY_EXE_NAMES`
  because an NSIS script cannot import a Rust constant.
  Windows will not delete a file its process holds open, so an upgrade over a
  running app otherwise leaves the previous copy in place and says nothing. The
  uninstall section only ever deletes the binaries, the LICENSE, the two
  shortcuts and its own registry key; settings live outside `$INSTDIR` and are
  never touched.

## Hard rules
- **`crates/core` must never gain a GUI dependency.** The renderer process is
  built from it precisely so that it can never create a GPU context or run an
  event loop; that is a property of the dependency graph, and graph properties
  decay silently. No `eframe`, `egui`, `glow`, `winit`, `accesskit`, `wgpu` or
  `tray-icon`, directly or transitively, and no `windows-sys` either (the
  layered-window code declares its own `extern "system"` blocks).
  `crates/core/tests/no_gui_dependencies.rs` runs `cargo tree` and fails the
  suite if one appears, so do not "fix" a deny-list entry instead of the
  dependency. That tree includes build-dependencies, and they are allowed:
  `crates/build-support` runs on the host and contributes a resource to the
  exe, never runtime code, and its `winresource` subgraph is `toml`,
  `serde_core`, `winnow` and `version_check` — no GUI crate and no
  `windows-sys`. The application shell is the only thing allowed a GUI stack,
  and `tray-icon` belongs to `crates/tray` alone.
- Do not replace `UpdateLayeredWindow` with egui/GPU child viewports. The old
  multi-viewport renderer was the source of the white-background and excessive
  memory problems.
- `src-tauri/Cargo.toml` uses eframe with the `glow` renderer for the one config
  window and `tiny-skia` only for software overlay pixels. Do not add WebView2
  or Tauri back into the native branch.
- Windows-only: MSVC toolchain (`x86_64-pc-windows-msvc` or
  `aarch64-pc-windows-msvc`) + MSVC Build Tools. No WebView2 runtime is needed.
- ICMP uses the `ping-rs` crate (Win32 `IcmpSendEcho2`) and does not require
  Administrator. `src/probe.rs` resolves ICMP targets to IPv4; TCP supports
  hostname resolution through Tokio.
- The README is for users. Implementation detail belongs in `docs/SPEC.md` for
  behavior and here for working knowledge, and the user must be consulted before
  technical detail is added to the README.
