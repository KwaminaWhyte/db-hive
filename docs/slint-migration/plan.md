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

## Parity (measured 2026-10-21)

Yardstick: the 167-item flat checklist in `docs/rust-ui-migration/01-frontend-inventory.md`
section 4. Method: read the Slint code, tests and snapshots, item by item. Done = built, wired
and tested. Partial = counts half. Not verified by clicking through the real GPU window; the
controller tests drive the real window headlessly instead.

| | Done | Partial | Missing |
| --- | --- | --- | --- |
| Connections (21) | 15 | 4 | 2 |
| Titlebar / shell (10) | 2 | 5 | 3 |
| Cross-cutting (14) | 2 | 8 | 4 |
| SQL editor (12) | 4 | 3 | 5 |
| Results grid (16) | 2 | 4 | 10 |
| Schema browser (10) | 0 | 4 | 6 |
| Settings (6) | 2 | 4 | 0 |
| Command palette (3) | 2 | 1 | 0 |
| Keyboard shortcuts (3) | 2 | 1 | 0 |
| History (3) | 3 | 0 | 0 |
| Snippets (3) | 2 | 0 | 1 |
| Query templates (5) | 4 | 1 | 0 |
| Multi-window, ER diagram, schema designer, table dialogs, query builder, migrations, backup, import/export, activity, plugins, AI (61) | 0 | 0 | 61 |
| **Total (167)** | **40** | **35** | **92** |

History of this number: 18 / 26 / 123 (2026-10-20), 22 / 28 / 117 after the SQL editor, 40 / 35 / 92
after the history panel, table inspector, command palette and settings landed.

- **Feature coverage: about 34%** ((40 + 35/2) / 167).
- **Ready for real work: about 40%, a judgement.** You can add and edit connections, connect, browse
  tables with paging, sorting, filters and structure, write and run SQL in tabs with highlighting,
  get a guard on destructive statements, re-run from history, keep snippets and templates, use
  the command palette and shortcuts, and change settings. Every run is recorded. You cannot yet:
  edit data in the grid, autocomplete, export results, see an ER diagram, import data, run
  migrations or backups, or keep tabs across restarts.
- **Defects found in this pass, not yet fixed:**
  - Settings that are stored but not consumed: language, startup, update checks, max rows, editor
    font family, minimap, word wrap. The dialog says so beside each.
  - Shortcut rebinding saves, but the app still dispatches the fixed shortcuts.
  - Page loads in the table inspector are not written to history or the activity log.
  - SQLite `get_table_schema` does not escape quotes in a table name (core).
- **Dead controls:** fixed. The gear, the command box, File > Settings and View > Command Palette
  work. File > New Window was removed rather than left dead (one window per controller today).
- **What is left is mostly screens, not logic:** `dbhive-ui-model` already holds the logic for most
  missing areas, and the ER diagram canvas is a proven component.
- **Order from here:** table editing and export (data you can change and take out), autocomplete,
  then ER diagram, migrations, backup, import/export, activity monitor, query builder, plugins, AI.

## Status

- [x] Phase 0
- [x] Phase 1: every screen's logic lives in `dbhive-ui-model`; `dbhive-app` re-exports it and still compiles and passes its own tests unchanged
- [~] Phase 2: theme, icons, async bridge, toasts, navigation and `MenuBar` are in; About has `AboutSlint`. The command palette itself (only its trigger) and connection editor are not yet wired to a search/run flow
- [~] Phase 3: connections screen and the new/edit/duplicate/delete flow are done. Connecting opens a workspace screen: table list sidebar (loading/empty/error states), a results grid for the selected table (filter and sort by column), disconnect returns to Connections. Password prompt for a saved connection with no cached secret, the SQL editor and history are not started
  - Connection editor: SSH tunnel sub-form (host, port, user, password or key), pasted connection string ("Fill fields") and colour tag swatches are in. Verified by snapshots and unit tests only; not yet driven by real clicks or against a real SSH host. Still missing: SSH local port field, key passphrase, showing the colour tag on the connections list
  - Workspace gap: read-only preview only (`SELECT * ... LIMIT 200`, no editing, no pagination past 200 rows, no ER diagram/table inspector yet). `tests/workspace_flow.rs` proves connect → get_tables → execute_query → grid rows works end to end against a real SQLite file
  - Table inspector (phase 4, read-only): a table tab now has Data and Structure views. Data: First/Prev/Next with a page size and the 100,000-offset guard, structured column filters (the egui operators, applied with Apply), row count, sort by header click done in SQL, single-cell selection with a JSON side panel, copy cell / row (TSV) / column / row JSON. Structure: columns with PK, auto and FK badges, indexes, foreign keys in both directions; a foreign key opens its table's tab. Logic is `dbhive_ui_model::table_view` (unit tested); `tests/table_inspector_controller.rs` drives it against a real 130-row SQLite table. Not done: keyset paging (never ported, see `docs/rust-ui-migration/04-plan.md`), the editable grid, page loads are not written to history, only verified on SQLite, parity counts above not re-measured
  - Layout gotcha hit twice (connection editor and workspace sidebar): a `VerticalLayout`/`ScrollView` with no child holding `vertical-stretch` spreads leftover space as gaps between every row instead of below the last one. Fix is either `alignment: start` (no stretch child in that layout) or give exactly one child `vertical-stretch: 1` (a stretch child already present, e.g. a `ScrollView` list) — never both on the same layout, which clips the stretch child to its unstretched size
- [~] Phase 4, import/export and backup (own overlays, own state globals, no new window properties): View menu items and the `go-import-export` / `go-backup` palette commands open them over the workspace. Import: CSV/TSV/TXT/XLSX/XLS wizard (file, preview with sheet pick, auto-mapping with per-column type/default/skip, header/delimiter/skip rows/batch size/truncate options, a reviewed plan before any write, a typed table name to confirm a truncate, running with Stop, result with counts and a truncated error list), run a `.sql` file (transaction and continue-on-error, Stop, error log path), export a table to CSV/JSON/Excel, write a SQL dump (DROP/CREATE/INSERT toggles, table scope). Backup: list with filter, folder shown and openable, create (schema/data toggles, note, tables, output folder), restore with a warning and optional drop-and-recreate, delete behind a confirmation, stage progress. There is no file picker, so paths are typed with a date-stamped Downloads default. Logic is `dbhive_ui_model::{import_wizard, data_forms, backup_manager}`; `tests/data_tools_controller.rs` drives it on a real SQLite file. Not done: a native file dialog, a cancel for backup and restore (the dump tools cannot be interrupted safely), row-level import progress (the engine reports none), verified only on SQLite (Postgres and MySQL dump tools are untested here)
