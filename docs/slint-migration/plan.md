# Slint migration

Replace the egui shell (`crates/dbhive-app`) with Slint (`crates/dbhive-slint`).
`dbhive-core` is untouched. Branch: `slint-ui`, off `native-rust-migration`.

## Decisions

| | |
| --- | --- |
| Toolkit | Slint `=1.18.0`, pinned exactly; upgrades are deliberate commits |
| License | Royalty-free 2.0. Requires `AboutSlint` in the About dialog, reachable from the top menu. Plugins must never expose Slint APIs |
| Look | Custom honey-brand components on Slint primitives, not a stock style |
| Renderer | GPU (FemtoVG). The software renderer cannot carry the diagram |

## Phase 0: spikes (done)

| Spike | Result |
| --- | --- |
| SQL editor | Pass. Coloured runs under a transparent `TextInput`, placed by column x measured advance. Pixel-exact by debug-tint snapshot; typing verified with real key events; caret and selection verified by snapshot. Limit: wide glyphs misalign the colour layer |
| 50k-row grid | Pass. `ListView` + lazy `Model`. First frame 16 ms, jump to row 25,000 in 6 ms (software renderer, upper bound) |
| Diagram canvas | Pass on GPU: 120 fps dragging among 60 tables / 88 edges. Paths must be sized to their bounding box |
| Memory | On par with egui: 106 MB empty, 211 MB with the 60-table diagram (GPU). Not a memory win |

Tools this produced, kept for the rest of the migration:

- `examples/snapshot.rs`: render any component to an image headlessly
- `tests/*`: drive components with real key and pointer events
- `examples/grid_perf.rs`, `examples/diagram_perf.rs`, `examples/diagram_live.rs`

## Phases

1. **Lift logic.** New `crates/dbhive-ui-model`, no UI dependencies. Pure logic and its tests move out of the egui screens; egui keeps compiling against it.
2. **Shell.** Theme global generated from Rust tokens, SVG icons, async bridge, toasts, navigation, command palette, `MenuBar`, About with `AboutSlint`.
3. **Daily flow.** Connections, connection editor, password prompt, workspace, history.
4. **Data tools.** Table inspector, table editor, procedures, import/export, backup, migrations, settings.
5. **Canvas and extras.** ER diagram, schema designer, query builder, activity monitor charts, AI assistant.
6. **Plugins, multi-window, updater.**
7. **Cut over.** Interaction tests, smoke tests, memory, a human walkthrough of every screen, then delete `dbhive-app` and update docs, CI and packaging.

## Parity (measured 2026-10-20)

Yardstick: the 167-item flat checklist in `docs/rust-ui-migration/01-frontend-inventory.md`
section 4. Method: read the Slint code and snapshots, item by item. Done = built, wired and
working. Partial = counts half. Nothing here was verified by clicking through the real window.

| | Done | Partial | Missing |
| --- | --- | --- | --- |
| Connections (21) | 15 | 4 | 2 |
| Titlebar / shell (10) | 1 | 5 | 4 |
| Cross-cutting (14) | 2 | 7 | 5 |
| SQL editor (12) | 4 | 3 | 5 |
| Results grid (16) | 0 | 4 | 12 |
| Schema browser (10) | 0 | 4 | 6 |
| Settings (6) | 0 | 1 | 5 |
| Multi-window, ER diagram, schema designer, table dialogs, query builder, migrations, backup, import/export, activity, plugins, AI, palette, shortcuts, history, snippets, templates (78) | 0 | 0 | 78 |
| **Total (167)** | **22** | **28** | **117** |

Re-measured after the SQL editor and query tabs landed (was 18 / 26 / 123).

- **Feature coverage: about 22%** ((22 + 28/2) / 167).
- **Ready for real work: about 25%, a judgement.** You can add a connection, connect, open table
  previews and query tabs, write SQL with highlighting, run it (Cmd/Ctrl+Enter), get a guard on
  destructive statements, and have each run recorded in history and the activity log. The
  controller path is covered by `tests/workspace_controller.rs`. You still cannot: edit data in the
  grid, autocomplete, export, browse history or snippets, open settings, or save tabs across
  restarts.
- **Dead controls (defects):** the gear button, the search/command box, and the File > Settings,
  File > New Window and View > Command Palette items have no Rust handler. They look live and do
  nothing.
- **What makes this faster than it looks:** `dbhive-ui-model` already holds the logic for most
  missing areas (phase 1). The ER diagram, SQL editor and grid exist as proven components. The
  missing work is mostly screens, not logic.
- **Order that gets to real work soonest:** SQL editor screen with tabs, run and history; then
  settings and the command palette (they unblock the dead controls); then table editing; then
  the rest.

## Status

- [x] Phase 0
- [x] Phase 1: every screen's logic lives in `dbhive-ui-model`; `dbhive-app` re-exports it and still compiles and passes its own tests unchanged
- [~] Phase 2: theme, icons, async bridge, toasts, navigation and `MenuBar` are in; About has `AboutSlint`. The command palette itself (only its trigger) and connection editor are not yet wired to a search/run flow
- [~] Phase 3: connections screen and the new/edit/duplicate/delete flow are done. Connecting opens a workspace screen: table list sidebar (loading/empty/error states), a results grid for the selected table (filter and sort by column), disconnect returns to Connections. Password prompt for a saved connection with no cached secret, the SQL editor and history are not started
  - Connection editor: SSH tunnel sub-form (host, port, user, password or key), pasted connection string ("Fill fields") and colour tag swatches are in. Verified by snapshots and unit tests only; not yet driven by real clicks or against a real SSH host. Still missing: SSH local port field, key passphrase, showing the colour tag on the connections list
  - Workspace gap: read-only preview only (`SELECT * ... LIMIT 200`, no editing, no pagination past 200 rows, no ER diagram/table inspector yet). `tests/workspace_flow.rs` proves connect → get_tables → execute_query → grid rows works end to end against a real SQLite file
  - Table inspector (phase 4, read-only): a table tab now has Data and Structure views. Data: First/Prev/Next with a page size and the 100,000-offset guard, structured column filters (the egui operators, applied with Apply), row count, sort by header click done in SQL, single-cell selection with a JSON side panel, copy cell / row (TSV) / column / row JSON. Structure: columns with PK, auto and FK badges, indexes, foreign keys in both directions; a foreign key opens its table's tab. Logic is `dbhive_ui_model::table_view` (unit tested); `tests/table_inspector_controller.rs` drives it against a real 130-row SQLite table. Not done: keyset paging (never ported, see `docs/rust-ui-migration/04-plan.md`), the editable grid, page loads are not written to history, only verified on SQLite, parity counts above not re-measured
  - Layout gotcha hit twice (connection editor and workspace sidebar): a `VerticalLayout`/`ScrollView` with no child holding `vertical-stretch` spreads leftover space as gaps between every row instead of below the last one. Fix is either `alignment: start` (no stretch child in that layout) or give exactly one child `vertical-stretch: 1` (a stretch child already present, e.g. a `ScrollView` list) — never both on the same layout, which clips the stretch child to its unstretched size
