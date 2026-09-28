# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. The native implementation uses **Rust + egui/eframe** for the single
configuration window and native Win32 layered windows for overlays. Probes run
on Tokio tasks and the system tray uses `tray-icon`. See `docs/SPEC.md` for
product behavior.

## Layout
- `src-tauri/` — the Cargo workspace; run Cargo commands here. The root
  manifest is both the workspace and the application package; `crates/core` is
  the other member.
  - `src/lib.rs` — module wiring and the application entry point (the shell).
  - `src/ui.rs` — tray-mode egui configuration editor.
  - `src/tray.rs` — tray icon, menu, and bundled artwork.
  - `crates/core/` — the `ping-latency-overlay-core` crate. Everything the
    renderer needs and **no GUI dependency at all**:
    - `src/config.rs` — config schema, profiles, persistence, and directory
      migration.
    - `src/probe.rs` — one-shot ICMP or TCP latency measurement.
    - `src/probes.rs` — long-lived Tokio probe tasks and bounded sample buffers.
    - `src/overlay.rs` — native layered HWND creation, DPI/work-area layout, and
      per-window alpha compositing with `UpdateLayeredWindow`.
    - `src/render.rs` — software graph rendering into premultiplied RGBA.
    - `src/border.rs` — runtime border-effect state and software RGB border
      drawing.
- `scripts/gen-icons.mjs` — generates native artwork with no dependencies.
- `packaging/nsis/` and `scripts/build-nsis.ps1` — native installer packaging.
- The pre-egui Tauri/React implementation remains available on `main`.

## Commands
Native app (run from `src-tauri/`):
- `cargo run` — development build
- `cargo run -- --show-config` — development launch with Config visible
- `cargo build --release` — optimized standalone executable
- `cargo fmt`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

Installer (run from the repository root):
- `npm run icons`
- `npm run bundle` — build the x64 NSIS installer from the existing release exe
- `.\scripts\build-nsis.ps1 -Arch arm64` — build the ARM64 installer after
  building the ARM64 release exe

## Workflow
- **Measure layout, don't deduce it.** Reading the code and checking arithmetic
  against constants shipped three layout bugs in a row. Lay the real thing out
  headlessly with `Context::run_ui` and print the rects; the numbers name the
  bug immediately. Assert both the good and the broken case so each test
  carries its own failure mode.
- **Gate on clippy, not just `cargo test`.** `assertions_on_constants` and
  friends are clippy-only, and `cargo test` compiles and passes with them
  present. The gate is `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo build --release`.
- **A manifest that is both a workspace root and a package narrows plain
  `cargo test` to that package alone.** The split into `crates/core` made
  `cargo test` report 35 passed against a 94-test suite and exit 0, silently
  skipping all 59 tests in `core`. `default-members = [".", "crates/core"]` in
  the root manifest is what makes the ordinary command cover the workspace, and
  it is load-bearing. If the test count ever drops without a deletion, suspect
  this before suspecting a filter.
- **Commit messages go through a file.** Write the message to a scratch file
  (this session uses `%LOCALAPPDATA%\Temp\opencode\plo-commit-msg.txt`) and run
  `git commit -F <path>`; a PowerShell here-string gets its terminator mangled
  by the shell tool.
- **Bundle after committing.** The version is
  `MAJOR.MINOR.(commits since countBase)`, not the raw commit count, so a new
  minor restarts at `.1`. `countBase` is in `[package.metadata.build]` in
  `src-tauri/Cargo.toml` and **both** readers take it from there — `build.rs`
  (which sets what the app reports) and `scripts/build-nsis.ps1` (which names
  the installer) — so a constant in each file cannot drift from the other.
  Raise the minor and the base together, in the same commit.
  `the_installer_and_the_app_agree_on_the_version` runs the script and
  compares it with the version compiled into the binary, because an app that
  says `0.2.3` inside `0.1.77-setup.exe` is the kind of drift nobody notices
  until a user reports it. So **bundle after committing**; bundling first
  yields the previous commit's number.
- **Do not commit before the user has looked at it.** Hand visual changes over
  as a build and wait for confirmation.

## Config storage
- Everything lives in `~/.config/.PingLatencyOverlay/`: `profiles/` holds one
  `profile_<id>.json` per profile and is the only place overlays are saved;
  `globalconfig.json` holds app-wide preferences and is created as `{}`. All of
  this is `Store` in `config.rs`, which takes its root directory so tests can
  point it at a temp folder.
- Migration is from `~/.PingLatencyOverlay/`: `config.json` is copied to
  `profiles/profile_default.json` and deleted only after the copy succeeds, and
  the legacy directory is removed only when that file was its sole entry.
- A profile has two names. The **id** is the file name and the only unique part
  (a lowercase slug, 48 characters at most via `sanitize_profile_name`); the
  free-form `profileName` inside the file is what the window title, the switcher
  and the profile list show, and it may repeat. `create_profile`,
  `rename_profile` and `duplicate_profile` return the `ProfileEntry` that
  resulted, and collisions are resolved by postfixing the id
  (`profile_work_2.json`) rather than refusing — `with_postfix` keeps the result
  inside the cap so `list_profiles` still accepts it.
- `Store::backfill_profile_names` writes a derived display name into existing
  files at startup and reports it as a `ConfigNotice`; unreadable files are
  never rewritten. A test that writes a profile file must include `profileName`
  or it will pick up a `ProfileNamesBackfilled` notice it did not expect.
- `globalconfig.json` is the only source for the active profile, as
  `activeProfile` plus `activeProfileFile`. `Store::load` builds a candidate
  list from the stored file name, then the stored id, then `default`, and never
  scans the directory for a substitute; a missing `default` on a first run is
  the fresh-config case, not a `ProfileFallback`. Both keys are removed for
  `default`, so a fresh install keeps the file literally `{}`.
- App-wide preferences live under a single `ui` object (`GlobalPrefs`/`UiPrefs`,
  every field `#[serde(default)]`). `write_global_prefs` replaces only the `ui`
  key, so the active-profile pointer and any unknown key survive. Never write
  that file as a whole object.
- `horizontalMarginPx` and `verticalMarginPx` are signed screen-axis offsets.
  Edge anchors measure inward from the work-area edge; centered axes measure
  from the center. Legacy `marginPx` is mapped per anchor during normalization.

## The Config window
- Three panes plus a status bar (`config_ui`): a navigation rail (`show_rail`,
  `Page`/`PAGES`, `rail_width`), a list pane (`show_list_pane`, dropped by
  `page_has_list_pane`) and a detail pane (`show_detail_pane`). Pane switching is
  never guarded; only profile switching is. `page_has_list_pane` is a function
  rather than a `page != Global` test at the call site so a new list-free page is
  one line in it, not an edit buried in the layout code that nothing points at.
- Each pane is a scrolling area above a fixed footer. **Save** and **Discard**
  are the detail pane's sticky footer (`show_detail_footer`,
  `DETAIL_FOOTER_HEIGHT`), right aligned, enabled only while `dirty`, which is
  also how unsaved edits are signalled. `page_has_detail_footer` decides which
  pages carry it: only a page that stages a draft, so Overlays and Global do and
  Profiles and About do not.
- The status bar (`show_status_bar`) shows only the transient operation message,
  so it takes `&self`. The version is on the About page instead, and optionally
  in the window title via `prefs.ui.show_version_in_title` — which
  `PingApp::window_title` reads from the **live** `prefs`, not `prefs_draft`, so
  ticking the box updates the title on the spot while the write stays staged.
  `sync_window_title` runs every frame from `logic()` and only sends on change,
  so reading a preference there costs nothing.
- `show_status_bar` is paint-only, so unlike the rest of the window layout no
  test can hold its shape. That is a known gap, not an oversight.
- Edits are staged per source. Detail-pane edits and Add overlay stage into the
  profile draft; list-pane enable/delete and Pause/Resume apply immediately.
  Delete uses an inline confirmation because native script dialogs are not used.
- **There are two independent draft flags: `dirty` (the profile) and
  `prefs_dirty` (app-wide preferences).** Anything asking "is there anything to
  save?" must ask about both, via `has_pending_edits` / the free `pending_edits`.
  The footer's buttons were gated on `dirty` alone, so every Global preference
  was unsaveable for as long as the Global page existed - the rail-collapse
  preference has been in that state since it shipped. `save_edits` calls
  `persist_current` then `save_prefs` and reports "Saved." if either returned
  true, so the save path was always right; only the enablement was wrong.
- **Both draft writers need the same guard.** `save_prefs` returns early when
  `!prefs_dirty`; `persist_current` did not, so once the footer was enabled for
  preferences an unguarded Save rewrote the profile file from the in-memory
  draft even when only `globalconfig.json` had changed - silently clobbering
  any edit made to that file from outside the app while it was running. The two
  writers being asymmetric was what gave the bug away. Both return `true` when
  there was nothing to write, so a preferences-only save still says "Saved."
- The Global page stages into `prefs_draft` and writes on Save (`save_prefs`),
  while the rail's collapsed state is applied to the live UI immediately so the
  user sees it change. `save_edits` writes whichever draft is dirty and
  `discard_edits` reloads both.
- Profile switching is refused while `dirty`. **Discard** reloads the active
  profile from disk, and Save/Discard are the only ways to resolve pending
  edits.
- The profile switcher (`show_profile_switcher`) heads the Overlays list pane
  and anchors the profile menu. The menu is a **switcher only** and ends with
  **Manage profiles**, which opens the Profiles page; create, rename, duplicate
  and delete live there (`show_profile_detail`, `show_profile_dialog`).
- Selecting a row in the Profiles list only sets `selected_profile`; loading is
  the separate **Switch to this profile** action. Never make a list selection
  switch profiles, because switching is refused while `dirty`.
- `selected_id` (the overlay in pane 3) starts as `None` and is **view-only**:
  staged edits live in `self.config.overlays`, never in the selection, so it can
  be cleared freely. Do not re-add a startup auto-selection — it meant a border
  was animating as soon as the window opened. Two things set it:
  `toggled_selection` for a row click, which clears when the clicked row is
  already the selected one, and the blank-area hit target. `delete_overlay` and
  `switch_profile` still focus an overlay, because those are deliberate actions
  on a specific row or profile rather than a startup default.
- `selected_overlay_for_border` encodes **three** conditions, not one: the
  window is open, the Overlays page is showing, and something is selected. The
  page condition went missing for a release, leaving the last overlay's border
  animating on the Profiles and Global pages. It is also the documented
  exception to "test a trigger by driving it" below: here the predicate *is* the
  mechanism, because `sync_overlays` calls it every frame and hands the result
  straight to `overlays.apply`, so a table over the function is the whole test.
- The deselect strip must be registered with `ui.interact`, never laid out with
  `allocate_exact_size`. `interact` calls `create_widget` and touches no cursor,
  so the hit target adds nothing to the scroll area's content; a real widget of
  the leftover height pushed the content 725px past the viewport in
  `the_deselect_strip_does_not_disturb_the_scroll_content`, which would have
  conjured a scrollbar on any list that exactly fitted.
- `profile_overlay_counts` is a cache of overlay counts per profile, filled by
  `refresh_profiles` because that reads every profile file. It has exactly two
  kinds of trigger, and **both must exist**: the profile mutations
  (`create_profile`, `rename_profile`, `duplicate_profile`, `delete_profile`,
  `switch_profile`) and the switcher popup call it directly, and *arriving* on
  the Profiles page calls it through `sync_profiles`, which watches
  `last_page` from `logic()`. It shipped showing `0` on every row for a release
  because the cache started empty and only the mutations filled it, so arriving
  from the rail found it empty. A cache whose trigger is missing fails as a
  confident wrong number, not as an obvious gap, so **a count that is absent
  from the map must draw nothing** (`Option<usize>` all the way to
  `profile_row_contents` and `overlay_count_label`) — never `unwrap_or(0)`, and
  `refresh_profiles` filters unparseable profiles out rather than counting them
  as zero.
- A doc comment that describes a trigger the code does not have is worse than no
  comment: `refresh_profiles` claimed for a release that it ran on "entering the
  Profiles page", and that false claim is why the missing call above went
  unnoticed. When a comment names *when* something runs, check the call sites.
- **Test a trigger by driving it, not by testing its predicate.** Testing
  `arriving_page_needs_profiles` alone passed while the page never read
  anything, because the predicate was right and nothing acted on it. The body
  lives in the free `sync_profile_cache`, which takes the disk read as a
  parameter, so `arriving_on_the_profiles_page_re_reads_the_counts` can count
  the reads and assert the counts it published.
  `coming_back_to_the_profiles_page_reads_again` covers the other half, that
  `last_page` advances even on the frames that read nothing. A conditional
  action is exactly where a predicate-only test is worthless: write the action
  so it can be called without the object that owns it.
- The window title is `PingLatencyOverlay - Current Profile: <name>`, pushed
  with `ViewportCommand::Title` only when it changes (`sync_window_title`).
- The root egui viewport starts hidden. Tray **left-click** shows Config; the
  context menu is right-click. Config-window close hides the root viewport;
  only tray Exit closes the app.
- Rail rows, glyphs and the switcher are painted with `ui.painter()`, not
  buttons, so a label can sit beside an icon. A painted glyph whose parts come
  from a table needs a centring-and-containment check: the Global glyph shipped
  with its row offsets read as loop indices by `enumerate()`, so the tuple
  halves swapped jobs and a knob poked past its track. `GLOBAL_ICON_ROWS` plus
  `the_global_glyph_is_centred_and_stays_inside_its_box` pin it.

## egui layout traps
Every trap below shipped once. Each test named here fails on the old behaviour.
- **Centre a column with `ui.with_layout(..)`, never `ui.vertical_centered(..)`.**
  `with_layout` re-lays-out the *existing* `Ui` and creates no child, so there is
  no child min rect to overshoot and no 18px `interact_size.y` band.
  `vertical_centered(..)` is a `scope_builder` child, which is the exact shape
  that made the detail footer report 22px more than its box and slide into the
  status bar. `show_detail_footer` uses `with_layout` with two children and is
  correct, so the counter-example is right here in the file. Used by
  `about_page_column`. For `Layout::top_down` the main axis is vertical, so
  `Align::Center` is the cross axis and centres each line horizontally.
- **eframe's NATIVE runner ignores `egui::OutputCommand::OpenUrl`,** so a
  `Hyperlink` on a native window is inert. egui emits the command and leaves it
  to the host; only eframe's *web* runner acts on it — in eframe 0.36.2
  `src/native/*.rs` contains no `OutputCommand` handling at all. The About
  page's link shipped a release claiming it opened a browser while doing
  nothing, because that half of the chain was verified and the half that
  actually runs was not. So `open_requested_urls` drains the command at the END
  of the UI pass with `Context::output_mut` (not in `logic`, which runs before
  the panes are drawn and would open the link a frame late) and
  `open_url_in_browser` calls `ShellExecuteW` itself, reporting either outcome
  in the status bar so a click is never silently ignored. `requested_url` is
  the pure mapping `a_link_click_becomes_a_url_to_open` holds. **A link that
  reads as obviously working is not evidence that it works** — check the host
  the app actually runs on, all the way down.
- **A child that does not fill its box can report a min rect LARGER than the box
  it was given**, and the parent advances its cursor by whatever the child
  reports. `show_editor`'s empty state sat in `ui.centered_and_justified`, whose
  doc says only one widget may be added, and three small widgets (label,
  `add_space`, label) made it report 22px taller than its box, pushing the
  detail footer 19.5px down into the status bar. The symptom only appeared with
  nothing selected, because the selected path is a `ScrollArea`, which reports
  its box exactly. A child that reports more than its box moves **everything**
  laid out after it. Use one widget - here one label with an embedded newline,
  via `empty_editor`. Pinned by `the_detail_footer_stays_under_the_detail_pane`.
  Note this is NOT "never put two widgets in a builder":
  `show_detail_footer` uses `ui.with_layout(..)` with two `add_enabled_ui`
  children and has rendered correctly through every release. The rule is about
  the child failing to fill its box, not about child count, and the footer is
  the standing counter-example.
- **A mirror test must reproduce the real bounds, width AND height, or its
  numbers are meaningless.** `detail_pane_geometry` stands in for
  `show_detail_pane`, which needs a live `PingApp`. Without wrapping its body in
  a positioned child the mirror's vertical saw the whole window, which produced
  three different false results in a row: the footer 17.5px low, then its right
  edge at x=1000 instead of the pane's 988, and only then the real figures. A
  mirror that does not reproduce the bounds blames the code for things the code
  is not doing.
- **Rows are painted, never `egui::Frame::group`,** because a group frame draws
  a 1px border and the rows must match the rail's borderless look. Allocate the
  row with `allocate_exact_size`, `rect_filled` the background yourself, then
  `ui.new_child(egui::UiBuilder::new().max_rect(..))` for the contents. Fill
  only when selected or hovered; an untouched row is transparent.
- **Allocate a painted row at `list_pane_row_height(contents)`,** never at the
  content height. `egui::Frame` sizes *itself* to its contents plus its margin
  and a `rect_filled` does not, so the child `Ui` was handed less room than it
  asked for and every button ended up flush against the fill.
  `a_painted_row_carries_its_margin` pins it.
- **`ui.horizontal(..)` is not vertically transparent.** It hard-sizes its
  child to `ui.spacing().interact_size.y` (18px by default) and allocates that
  through the parent layout, which centres it, so a taller widget inside it
  lands low and spills out the bottom. Inside a row of known height use a
  positioned child (`ui.new_child(..)`), which honours the absolute rect.
  `a_list_pane_row_puts_its_contents_inside_its_own_fill` pins it.
- **Both halves of a row come from one rect.** `row_inner(row)` gives the
  contents area and `right_anchored(inner, width)` places every right-hand
  control, so the name on the left and the buttons on the right cannot disagree
  about where the row ends. A row that reserves a gutter must lay it out
  horizontally and subtract `ui.spacing().item_spacing.x`; a `Frame` lays its
  content out top down, so adding the gutter to the same `Ui` drops it onto the
  next line and leaves an empty column.
  `a_profile_row_fits_its_pane` and `an_overlay_row_fits_its_pane` keep that
  arithmetic honest.
- **Everything in the list pane derives its width from `list_pane_column`,**
  which returns the one width shared by the header, the rows, the scroll area
  and the footer, plus the inset that centres it in the pane. An inset has to
  move a widget's **position**, not only come off its width, and a reserve may
  only be subtracted once — `list_pane_row_width_for` once did both, so all
  32px of slack landed on the right and the two boundaries read 4px and 40px.
  `the_list_pane_column_is_centred_in_its_pane` pins it.
- **`Ui::new_child` is invisible to the layout that created it.** It paints into
  the rect you give it and reports nothing back, and `allocate_ui` finishes by
  setting the parent's cursor from the child's `min_rect`
  (`advance_after_rects` advances from the *widget* rect, not the frame rect).
  A child that painted itself in a grandchild therefore has an empty `min_rect`,
  the cursor goes back to the pane's left edge, and the next pane is laid out on
  top of it. So `config_ui` follows `show_list_pane` with
  `ui.advance_cursor_after_rect(ui.max_rect())`, and any future pane that draws
  itself that way needs the same line.
  `a_pane_that_draws_itself_keeps_its_place` pins it.
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
- The app is **three processes**. The tray (`plo-tray.exe`) is what a user launches and the only one that stays resident; the Config window (`plo-config.exe`) starts on demand and **exits when it is closed**; the renderer (`plo-renderer.exe`, built from `crates/core`) draws the overlays. The three executables are named tersely on purpose — they sit side by side in the install folder and in Task Manager, and a name long enough to abbreviate is hard to tell from its neighbour at a glance. The **crates** keep their descriptive names; only the binaries are short. The product name, the installer file name, the Start Menu folder and the window title are all still PingLatencyOverlay. `LEGACY_EXE_NAMES` in `transport.rs` holds the pre-rename names, and the installer kills those before removing a previous install, because a file cannot be deleted while its process holds it open. Two processes carry no eframe at all, which is the entire point: the GL context exists only while the window is open. A hidden-but-alive window would hold it, which is why close means exit and not hide.
- **No package may reach a GPU context or an event loop.** That is the property the memory win depends on, and it is checked: `crates/core/tests/no_gui_dependencies.rs` runs `cargo tree` over `GUI_FREE_PACKAGES` and fails on `eframe`/`egui`/`glow`/`winit`/`accesskit`/`wgpu` for the core, and on `tray-icon`/`muda` for the core only. The tray is allowed `tray-icon` — it IS the tray — so the lists are two-tier and the rule is named for what it forbids rather than for the crates it happens to list. Applying the renderer's blanket list to the tray was tried and was wrong.
- The renderer is a `[[bin]]` in `crates/core` rather than its own package because it needs nothing the library does not already depend on, and `cargo tree` reports deps across **every** target — so the dependency test guards the renderer binary for free. The tray needs a package of its own, because a `[[bin]]` in the same crate as the window would link eframe into the resident process, which is the whole thing being avoided.
- `default-run` on the root package is set, because `cargo run` with two binaries is ambiguous and it is ambiguous exactly where a developer starts the app. The window is the default because it starts whatever it needs.
- **Every process here is `windows_subsystem = "windows"`, so `eprintln!` goes nowhere.** That is not a style note, it is why this app shipped a release with an unreachable Config window and a tray that did nothing on right-click, and why the only symptom was "it does not do anything". `crates/core/src/diagnostics.rs` is the answer: `log_line` appends to `pinglatencyoverlay.log` beside the config, and `fatal` puts up a `MessageBox` for the tray's two startup failures, because the tray is the one process here that never draws a window and so the only one that can talk to a user at all. **Any `return` on an error path in the tray needs one of the two** — a bare `return` is invisible forever. The tray also logs a heartbeat every few hundred passes, because a loop that has wedged and a feature that was never written look identical from the outside; the log turns "the loop stopped" into a fact.
- **The launch matrix is `should_start(me, missing)` and it is tested over all nine cells.** A process never starts itself; the renderer starts nothing; the tray does not open the window on its own (that is a menu item, not a side effect); everything else that is missing gets started, because a user who launched one executable should end up with a working app rather than a piece of one. It is a function rather than a comment because all three of the bugs it fixes came from the rule living in three places and none of them agreeing.
- **The tray supervises the PIPE, not the child handle.** An older version returned early whenever it had not spawned the renderer, which is exactly the attach case, so a user who launched the Config window first got an app that never recovered from a crash. "Only ever stop a process you did not spawn" is about *killing* something with state worth protecting; a dead process has none, and not restarting it leaves the user with an app that silently does nothing. `PeekNamedPipe` is the probe (see below), and a lost pipe is a better signal than a child handle, because a hung renderer still holds its mutex while answering nothing.
- **Bringing a renderer up and noticing a crash are two different states, and only the second one costs an attempt.** `App::starting` is an `Option<Instant>`; while it is set, `finish_starting` retries the pipe and the config push and nothing is counted against the storm guard. It was one check, and a fresh renderer is briefly indistinguishable from a dead one, so one slow start reported itself as a crash every 200ms, burned all five attempts in about a second, and left a tray that had quietly decided never to try again. The storm guard's job is to stop a **crash loop**; a renderer that has not answered yet is not crashing. `START_TIMEOUT` (10s) is what ends the grace period and hands the attempt back to the crash path.
- **Never discard a send error.** Both fire-and-forget pushes — the config handed to a relaunched renderer, and `set_paused` from the tray menu — used `let _ = self.send(..)`, so a command that never left the process looked exactly like one that was obeyed. That is the same shape of quiet wrongness as the profile-count zero and the "Saved." that did not reach the renderer, and it is the reason a relaunched renderer could come back with no overlays and no explanation. `finish_starting` now retries the config push until it lands, and `toggle_running` logs both halves of the action.
- **The renderer serves SEVERAL clients at once, and it has to.** `serve` spawns two `instance_loop`s, each holding one pipe instance and making the next one when it finishes with a client, so while one instance is busy serving, another is already waiting. A single sequential loop — create, connect, read to EOF, create again — is what shipped and it is why the tray looked dead: you launched the Config window first, it took the only instance, the tray's `CreateFileW` got `ERROR_FILE_NOT_FOUND`, and `Client::connect` only special-cased `ERROR_PIPE_BUSY`, so the tray never attached and went round the restart guard while a healthy renderer drew the overlays. Both processes legitimately need a connection at the same time; a rendezvous designed for one caller and handed two is a defect, not a configuration.
- **A liveness check must not be able to block, and must be given a handle that can answer it.** The tray asks with `PeekNamedPipe` — a query, not a transfer, which returns immediately whether the far end is there, has gone, or has stopped reading. The previous version probed with `send`, and `WriteFile` on a pipe nobody drains does not return; that probe ran in `step`, the same loop that services tray clicks, so a wedged probe meant *no click did anything at all* — the menu looked broken and it was the loop. Anything in a loop that also has to stay responsive to the user is a place a blocking call is a bug. The subtler half cost many more rounds: **`PeekNamedPipe` needs read access**, and the client handle was opened `GENERIC_WRITE` only, so every query failed with `ERROR_ACCESS_DENIED` and reported "the renderer is gone" the instant it was asked. The tray connected, asked, was told no, dropped the client and restarted the renderer, and repeated every 200ms — reporting "no renderer" forever while every individual call did exactly what it said. Nothing was wrong with `is_connected`; the handle it was given could not answer the question. **Every client handle is opened `GENERIC_READ | GENERIC_WRITE`**, and if a query is ever added here, the first question is whether the handle can answer it.
- **"No renderer is listening" is an answer, not a fault.** `Client::connect` maps `ERROR_FILE_NOT_FOUND` to `io::ErrorKind::NotFound`, distinct from every other failure, because it is the result that says *start one*. `connecting_with_nothing_listening_fails_quickly` asserts the kind, not merely that it failed: lumping the ordinary answer in with a real fault is what made a working system look broken.
- `crates/core/src/transport.rs` owns the wire format and **both ends are compiled from the same crate**, so the two processes cannot disagree about the protocol without a compile error. Messages are `set_config`, `set_paused`, `set_border_preview`, `ping` and `shutdown`, tagged with `#[serde(tag = "kind")]` so an **unknown kind is rejected rather than silently ignored** — a silently dropped command is indistinguishable from one that was obeyed. `ping` exists to be a no-op and nothing should ever act on it; borrowing `set_paused` for a probe would have quietly resumed the probes whenever the sender's mirror of the paused state was stale.
- **Framing is one JSON object per line, and the newline is not optional.** A named pipe is a byte stream, so something must delimit messages. A length prefix is the usual source of off-by-one bugs; JSON escapes newlines inside strings, so a line break can only ever be a delimiter. `a_name_with_quotes_braces_and_newlines_survives` exists to hold that.
- **The pipe is the rendezvous.** `Client::connect()` succeeding means a renderer is already running and the caller attaches; failing means spawn one and retry. There is no registry, no lock file and no heartbeat, which is what makes "run without the tray, then bring one later" free. `SingleInstance::acquire(Role)` uses a per-role named mutex (`Global\PingLatencyOverlay-v1-shell` / `-config` / `-renderer`) — **per role, not per app**: the tray must be able to exit while the window stays open, the window must be openable twice without a second one fighting for the pipe, and a crashed renderer must be replaceable. Three roles means three pairs, and `each_role_gets_its_own_versioned_mutex` checks all of them rather than one, because a test that compares only two lets the third silently share one. `None` from `acquire` means another process holds it, which is the ordinary "user ran it twice" answer and not an error.
- **`renderer_process` is set only when THIS process started the renderer.** Never stop, poll or restart a process you did not spawn: a renderer somebody launched by hand is the tray-less case, and a blanket kill takes an overlay set the user still wants off their screen. The same rule governs the `Shutdown` on `Drop` and the tray's `Exit`, which kills the Config window only if the tray started it. `Child` is returned from `start_or_attach_renderer` only when the call actually spawned, which is the single place that rule is decided.
- **The storm guard's clock is a parameter, not the system clock.** `RestartGuard::should_restart(now)` takes an `Instant` so a test can drive a whole crash history in microseconds, and `forget_expired` is split out so "an old attempt is forgotten" is a statement about the list rather than something inferred from a boolean. A crash loop is worse than a stopped app: it burns a core and buries the one message that would explain it. The tray has its own copy as a free fn over `&mut Vec<Instant>`, and it is the tray that supervises — a renderer with no tray is not restarted at all.
- **Neither the tray nor the window has a `ProbeManager` or an `OverlayManager` any more.** Neither has a tokio runtime, and only the window has eframe. Anything the renderer needs to know — including the animated border preview, which used to be a direct call and is now `set_border_preview` — has to travel over the pipe, and an omitted one loses a feature silently. The preview is sent **only when it changes**; per frame is a pipe write per frame, and the window clears it as it closes (`release_border_preview`) so a border cannot outlive the window that asked for it.
- **Say what actually happened.** Writing the profile file and telling the renderer are two things that can fail separately and only the first is under the window's control, so `persist_current` reports "Saved, but the overlays were not updated" rather than a confident "Saved." A profile that reached disk while the renderer was never told is the same shape of quiet wrongness this project has shipped before.
- The renderer is found as a **sibling of the caller's own executable** (`sibling_exe`, which is `current_exe().pop().push(name)`), so the same code works installed and under `cargo run`. All three must therefore be installed into the same directory, and `build-nsis.ps1` checks all three paths exist before packaging. `the_renderer_is_a_sibling_exe` holds that, because a wrong directory yields an app that starts cleanly and shows no overlays with nothing at runtime to say why.
- **A client is not a supervisor, and the restart half must not travel with the start half.** The Config window starts the renderer once at launch if nothing else has, and the tray supervises it from then on. When the window also gained a restart path the two processes fought: the tray's Exit stopped the renderer and the window brought it straight back, and neither could win. Factoring a supervisor into a function the client shares is how that happened — `start-if-missing` is a courtesy at launch, `restart-on-death` is ownership, and only the tray owns it. The window's `reconnect_renderer` reconnects and never spawns, and carries no storm guard: a storm guard stops a *crash loop*, and this is not the process that would be looping.
- **Letting the default handler run can be worse than ignoring the message.** `WM_CLOSE` reached `DefWindowProcW`, which *destroys the window*; the renderer's loop then saw a missing handle and rebuilt it on the next pass. So Task Manager's "End task" closed the windows, waited, found the process alive with brand-new windows, and concluded its graceful close had succeeded — so it never escalated to a kill and the process simply never ended. `overlay_wnd_proc` now handles `WM_CLOSE | WM_QUERYENDSESSION` by setting a flag and returning `0` **without** calling `DefWindowProcW`, and the loop checks `quit_requested()` immediately after `pump_messages` — the only place a request can arrive, because the pump is what dispatches it. The flag is a static because those windows belong to that one loop thread, and a window procedure cannot stop the loop that called it. Any future `WM_*` we choose not to handle has to be checked against this: the default is not a safe place to land.
- **Any thread in ANY of our processes that creates a window must pump that thread's messages.** The overlay windows are ordinary Win32 windows, so Windows posts cursor, hover-tracking and repaint messages to the thread that created them, and a **tray icon is a window too**. Windows only ever delivers a window message to a thread that pumps its queue, so a thread that sleeps or blocks instead does not merely behave oddly: it receives *nothing*, and the symptom is a window or an icon that looks completely correct and is completely dead. `tray-icon` says this at the top of its own crate documentation — "an event loop must be running on the thread" — and spawns no pump on Windows, so the caller has to; `pump_messages()` in `overlay.rs` is ours and both the renderer and the tray call it. **This exact bug shipped twice**, once per process, and cost four rounds of bisection the second time: the rule was already written down here for the renderer in v0.1.76, and the tray was written afterwards without it. A rule that names one caller is a rule the next caller will not find — so this bullet names the class, and `grep` for `windows_subsystem = "windows"` before assuming a new process is exempt. A second symptom of the same cause was Windows declaring the renderer "Not responding" with a black background, which is what the pump fixed.
- **The repaint cap and the liveness cap are different decisions.** `MESSAGE_POLL_INTERVAL` (16ms) is deliberately independent of `REPAINT_INTERVAL` (100ms): tying them together would mean a slow repaint is also an unresponsive process. The wait is `min(until the repaint deadline, MESSAGE_POLL_INTERVAL)` via `wait_before`, so a far-off deadline still services messages and a near one still repaints on time. A command from the pipe resets the deadline, so a config change shows immediately instead of waiting out the interval. `the_wait_is_bounded_by_both_the_cap_and_the_deadline` holds the arithmetic — but only the user can confirm Windows stops calling the process hung.
- **The pipe server builds a FRESH instance per connection.** Reusing one instance spins. Windows will not make `ConnectNamedPipe` wait on an instance a client was already connected to and has since disconnected: it returns `ERROR_PIPE_CONNECTED` at once, meaning "a client is already attached", which is stale by then. Tokio passes that through as a **success** (it special-cases only `ERROR_PIPE_BUSY`), so one instance makes every reconnect `connect`-returns-at-once, then the read hits end-of-file at once, and round again — with no sleep on any leg, because the loop's only sleep guards a *failed* connect. A renderer left running with no shell sat at **8% CPU** for exactly that reason. A brand new instance has never been connected, so `connect` blocks. Only the first instance may claim `first_pipe_instance`, which is safe because the renderer mutex, not that flag, is what guarantees one renderer. **No test can catch this one** — the spin is Windows returning success instead of blocking, and only the OS in front of you shows it. It is also the kind of change that gets "tidied" back into the old shape by someone who does not know why the instance is rebuilt, which is why the reason is a comment on the line rather than a commit message.

## Overlay rendering
- Overlays are not egui child viewports. Each is a native `WS_EX_LAYERED` popup
  rendered with `UpdateLayeredWindow`, so it has true per-pixel alpha, no DWM
  frame, no taskbar button, no focus, and mouse passthrough.
- `render.rs` writes premultiplied RGBA; `overlay.rs` swaps R/B to
  premultiplied BGRA before copying it into a 32-bit DIB. `bgOpacity=0` leaves
  the alpha byte at zero; positive values are composited by Windows.
- Graph orientation rotates the whole graph **anticlockwise** (90 means time
  runs bottom-to-top); values above `maxYMs` clamp to the top. Timeout samples
  draw a full-height timeout-colored line and break the latency line. See
  `docs/SPEC.md`.
- The graph uses actual physical window dimensions. Do not assume
  `windowSeconds * scale` is the drawable size under Windows DPI/text scaling.
- Sample buffers are bounded and overlay HWNDs are reused by stable ID. Do not
  allocate one renderer or surface per overlay.
- `ProbeManager::apply_config` must not restart all tasks for a style-only Save.
  Existing tasks read shared settings each tick; only deleted/disabled overlays
  are stopped. This keeps Save from pausing the graph.

## Hard rules
- **`crates/core` must never gain a GUI dependency.** The renderer process is
  built from it precisely so that it can never create a GPU context or run an
  event loop; that is a property of the dependency graph, and graph properties
  decay silently. No `eframe`, `egui`, `glow`, `winit`, `accesskit`, `wgpu` or
  `tray-icon`, directly or transitively, and no `windows-sys` either (the
  layered-window code declares its own `extern "system"` blocks).
  `crates/core/tests/no_gui_dependencies.rs` runs `cargo tree` and fails the
  suite if one appears, so do not "fix" a deny-list entry instead of the
  dependency. The shell package is the only thing allowed a GUI stack.
- Windows-only: MSVC toolchain (`x86_64-pc-windows-msvc` or
  `aarch64-pc-windows-msvc`) + MSVC Build Tools. No WebView2 runtime is needed.
- ICMP uses the `ping-rs` crate (Win32 `IcmpSendEcho2`) and does not require
  Administrator. `src/probe.rs` resolves ICMP targets to IPv4; TCP supports
  hostname resolution through Tokio.
- Do not replace `UpdateLayeredWindow` with egui/GPU child viewports. The old
  multi-viewport renderer was the source of the white-background and excessive
  memory problems.
- `src-tauri/Cargo.toml` uses eframe with the `glow` renderer for the one config
  window and `tiny-skia` only for software overlay pixels. Do not add WebView2
  or Tauri back into the native branch.
- The installer is hand-written NSIS (`packaging/nsis/installer.nsi`, driven by
  `scripts/build-nsis.ps1`). MUI2 gives the finish page exactly **two** checkbox
  slots, and there is no way to add a third. `MUI_FINISHPAGE_SHOWREADME` is
  reusable as a generic second one: set `MUI_FINISHPAGE_SHOWREADME_TEXT` and
  `MUI_FINISHPAGE_SHOWREADME_FUNCTION`, and MUI calls the function instead of
  `ExecShell open`ing a file. That is how the desktop shortcut is offered, and
  the name is misleading, so the script says so at the definition.
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
- The README is for users. Implementation detail belongs in `docs/SPEC.md` for
  behavior and here for working knowledge, and the user must be consulted before
  technical detail is added to the README.
