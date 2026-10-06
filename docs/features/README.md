# PingLatencyOverlay — features map

Read this before touching code: find the area, then open only the one or two
feature docs it points at.

Read order: `HANDOFF.md` (current state) → this index → the feature doc(s) for
what you are about to change. `docs/SPEC.md` is user-visible behavior;
`AGENTS.md` is process and hard rules; the docs below are the map, the
mechanism and the traps.

**Keep this map true in the same commit as the change.** Before staging a
commit, update the feature doc(s) for whatever you changed, and add a row here
if the doc is new. User-visible behavior also updates `docs/SPEC.md` in the
same commit; a docs-only follow-up commit is the one that gets skipped. Pure
refactors with no behavior, mechanism, config or test change need no edit.

| Feature | Doc | Read when |
| --- | --- | --- |
| Runtime & pipe | [runtime.md](runtime.md) | processes, launch matrix, `transport.rs`, supervision |
| Tray | planned | tray icon and menu, resident loop, auto profile switching |
| Config — window | [window.md](config/window.md) | the egui window: `ui.rs`, panes, drafts, pickers, test seams |
| Config — storage | [storage.md](config/storage.md) | `config.rs`: schema, profiles, globals, `rules.json`, migrations |
| Probes | planned | `probe.rs` / `probes.rs`: ICMP and TCP, the address cache, cadence |
| Overlays — rendering | [rendering.md](overlays/rendering.md) | `overlay.rs` / `render.rs`: layered windows, draw order, reserves |
| Overlays — smooth rendering | [smooth-rendering.md](overlays/smooth-rendering.md) | the reveal hold and the smooth x mapping |
| Overlays — glow | [glow.md](overlays/glow.md) | the underglow: sweep, reserve, per-overlay settings |
| Overlays — sample cursor | [sample-cursor.md](overlays/sample-cursor.md) | cursor geometry, easing, its reserves |
| Overlays — display modes | planned | global / sticky / wallpaper, monitor pinning, follow |
| Theming | planned | `theme.rs`, palettes, theme files |
| Diagnostics | planned | the log file, `PLO_LOG`, fatal boxes |
| Packaging | planned | version rule, icons, NSIS, executable names |

Docs land one area at a time; a row's Doc cell becomes a link when its page
does. Until then, `AGENTS.md` still carries that area's working knowledge.

## Doc template

Every feature doc uses these headings, in this order, so a section can be
found by grep:

    # <Feature> — <one-line summary>
    Status: shipped (vX.Y) · Read when: <trigger>

    ## What it does
    3–6 bullets; link `docs/SPEC.md` for the user-visible detail.

    ## Map
    Files, types, functions and test names — the anti-hunting payload.
    Names, never line numbers (they rot).

    ## How & why
    The mechanism, the invariants and the traps: cause and effect, not a
    retelling of the bug that produced the rule.

    ## Config keys / UI
    Dense table: key, default, range or clamp, and the control that sets it.

    ## Tests that pin it
    Test names — run these first when touching the area.

    ## Related
    Links to the other feature docs.

Keep a doc 60–200 lines and split past ~250. No sentence lives in two docs:
`docs/SPEC.md` is behavior, a feature doc is map/mechanism/traps, `AGENTS.md`
is process.
