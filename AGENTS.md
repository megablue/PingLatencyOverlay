# AGENTS.md

PingLatencyOverlay is a Windows-only desktop overlay that shows live network
latency. The native implementation uses **Rust + egui/eframe** for the single
configuration window and native Win32 layered windows for overlays. Probes run
on Tokio tasks and the system tray uses `tray-icon`. See `docs/SPEC.md` for
product behavior.

## Layout
- `src-tauri/` — the Cargo project; run Cargo commands here.
  - `src/lib.rs` — module wiring and the application entry point.
  - `src/config.rs` — config schema, profiles, persistence, and directory
    migration.
  - `src/probe.rs` — one-shot ICMP or TCP latency measurement.
  - `src/probes.rs` — long-lived Tokio probe tasks and bounded sample buffers.
  - `src/overlay.rs` — native layered HWND creation, DPI/work-area layout, and
    per-window alpha compositing with `UpdateLayeredWindow`.
  - `src/render.rs` — software graph rendering into premultiplied RGBA.
  - `src/border.rs` — runtime border-effect state and software RGB border drawing.
  - `src/ui.rs` — tray-mode egui configuration editor.
  - `src/tray.rs` — tray icon, menu, and bundled artwork.
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
- **Commit messages go through a file.** Write the message to a scratch file
  (this session uses `%LOCALAPPDATA%\Temp\opencode\plo-commit-msg.txt`) and run
  `git commit -F <path>`; a PowerShell here-string gets its terminator mangled
  by the shell tool.
- **Bundle after committing.** The version is `MAJOR.MINOR.<git-commit-count>`,
  so bundling first yields the previous version.
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
  `Page`/`PAGES`, `rail_width`), a list pane (`show_list_pane`, dropped on the
  Global page) and a detail pane (`show_detail_pane`). Pane switching is never
  guarded; only profile switching is.
- Each pane is a scrolling area above a fixed footer. **Save** and **Discard**
  are the detail pane's sticky footer (`show_detail_footer`,
  `DETAIL_FOOTER_HEIGHT`), right aligned, enabled only while `dirty`, which is
  also how unsaved edits are signalled. `page_has_detail_footer` decides which
  pages carry it: only a page that stages a draft, so Overlays and Global do and
  Profiles does not.
- The status bar (`show_status_bar`) shows only the transient operation message
  and the right-aligned version, so it takes `&self`.
- Edits are staged per source. Detail-pane edits and Add overlay stage into the
  profile draft; list-pane enable/delete and Pause/Resume apply immediately.
  Delete uses an inline confirmation because native script dialogs are not used.
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
- The README is for users. Implementation detail belongs in `docs/SPEC.md` for
  behavior and here for working knowledge, and the user must be consulted before
  technical detail is added to the README.
