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

## Parity (measured 2026-10-21, after the second wave)

Yardstick: the 167-item flat checklist in `docs/rust-ui-migration/01-frontend-inventory.md`
section 4. Method: read the Slint code, tests and snapshots, item by item. Done = built, wired
and tested. Partial = counts half. Not verified by clicking through the real GPU window; the
controller tests drive the real window headlessly instead.

| | Done | Partial | Missing |
| --- | --- | --- | --- |
| Connections (21) | 15 | 4 | 2 |
| Titlebar / shell (10) | 2 | 5 | 3 |
| Cross-cutting (14) | 2 | 8 | 4 |
| SQL editor (12) | 5 | 4 | 3 |
| Results grid (16) | 5 | 7 | 4 |
| Schema browser (10) | 0 | 4 | 6 |
| Settings (6) | 2 | 4 | 0 |
| Command palette (3) | 2 | 1 | 0 |
| Keyboard shortcuts (3) | 2 | 1 | 0 |
| History (3) | 3 | 0 | 0 |
| Snippets (3) | 2 | 0 | 1 |
| Query templates (5) | 4 | 1 | 0 |
| ER diagram (7) | 6 | 1 | 0 |
| Visual schema designer (6) | 5 | 1 | 0 |
| Table create / edit dialogs (4) | 4 | 0 | 0 |
| Visual query builder (8) | 8 | 0 | 0 |
| Migrations (5) | 5 | 0 | 0 |
| Backup / restore (5) | 5 | 0 | 0 |
| Import / export (3) | 3 | 0 | 0 |
| Activity monitor (7) | 7 | 0 | 0 |
| Plugins (6) | 5 | 1 | 0 |
| AI assistant (5) | 4 | 1 | 0 |
| Multi-window (5) | 0 | 0 | 5 |
| **Total (167)** | **96** | **43** | **28** |

History of this number: 18 / 26 / 123 (2026-10-20), 22 / 28 / 117 after the SQL editor,
40 / 35 / 92 after wave one (history, inspector, palette, settings), 83 / 37 / 47 after wave two
(ER diagram and designer, import/export and backup, activity and migrations, query builder and
table dialogs), 96 / 43 / 28 after wave three (autocomplete, the editable grid, plugins, the AI
assistant). The wave-three tally is mine, counted against the inventory checklist; it was not
re-audited independently.

What moved in wave three: SQL editor items 4 (metadata refresh) done and 5 (suggestions) partial
(no database/schema suggestions); results grid 7, 8 and 12 done (inline edit, pending highlight,
atomic commit) and 9, 10 and 11 partial (single-row select only, no client-side UUID/timestamp
defaults, one review list rather than separate diff and raw views); plugins 5 done and install
partial (core installs only bundled plugins); AI 4 done and generate/explain/optimize/fix partial
(only the OpenAI wire format was run end to end).

- **Feature coverage: about 70%** ((96 + 43/2) / 167).
- **Ready for real work: about 72%, a judgement.** You can manage connections, browse and
  inspect tables, write and run SQL in tabs, build queries visually, create and alter tables,
  view and design schemas as diagrams, diff and migrate schemas, import and export data, back up
  and restore, watch activity, edit rows in the grid with a reviewed atomic commit, autocomplete
  SQL, manage plugins, use the AI assistant, and use settings, history, snippets and templates.
  You cannot yet: open a second window, multi-select rows, export or chart results, or browse
  schemas as a tree.
- **What ran against a real server.** SQLite is covered by the headless controller tests for every
  screen. Beyond that, `#[ignore]` live tests (run with `-- --ignored`, each in its own scratch
  database, dropped afterwards) cover, on PostgreSQL: transactions, the DDL generator, the
  inspector, filter, sort, typed row edits, add, delete and a rejected commit (serial, numeric,
  boolean, timestamptz, uuid, jsonb, array columns), SQL dump export and replay, data import, and
  the activity monitor's server tab. On MySQL: transactions, the DDL generator, the same inspector
  and row-edit scenario (auto_increment, decimal, tinyint(1), datetime, json), SQL dump export and
  replay, and data import. Backup and restore ran on both (real `pg_dump`, `mysqldump`, `psql`,
  `mysql`). String and identifier quoting round trips run on SQLite, PostgreSQL and MySQL. The
  live runs found four real bugs, all fixed: boolean words refused by MySQL `TINYINT(1)` on
  import; a MySQL backup that the same GTID server refused to restore; a PostgreSQL restore with
  "drop and recreate" that failed because the app itself held the database open; and `psql`
  reporting a failed restore as success. Not run against a server: the query builder and the ER
  diagram on a non-default schema, SQL Server, MongoDB, Redis and Turso. None of it has been clicked through in the real GPU window.
- **Known defects and risks:**
  - Settings stored but not consumed: language, startup, update checks, editor font family,
    minimap, word wrap. Shortcut rebinding saves but is not dispatched. (Max rows now applies to
    query results.)
  - Table-inspector page loads are not written to history or the activity log.
  - MySQL commits implicitly before DDL, so a migration's rollback there covers only the
    statements since the last DDL statement (the server's rule, now noted in core).
  - The ER canvas node rendering was rewritten; its 120 fps figure has not been re-measured.
  - Foreign key targets in generated DDL are unqualified, so a non-default Postgres schema
    relies on the search path.
- **Remaining work, in order:** multi-window; then the partial items (schema browser tree and
  context menus, results export and charts, update checks, connection folders, a Connection Lost
  screen), then clicking everything through in the real GPU
  window, which has never been done.

## Status

- [x] Phase 0
- [x] Phase 1: every screen's logic lives in `dbhive-ui-model`; `dbhive-app` re-exports it and still compiles and passes its own tests unchanged
- [~] Phase 2: theme, icons, async bridge, toasts, navigation and `MenuBar` are in; About has `AboutSlint`. The command palette itself (only its trigger) and connection editor are not yet wired to a search/run flow
- [~] Phase 3: connections screen and the new/edit/duplicate/delete flow are done. Connecting opens a workspace screen: table list sidebar (loading/empty/error states), a results grid for the selected table (filter and sort by column), disconnect returns to Connections. Password prompt for a saved connection with no cached secret, the SQL editor and history are not started
  - Connection editor: SSH tunnel sub-form (host, port, user, password or key), pasted connection string ("Fill fields") and colour tag swatches are in. Verified by snapshots and unit tests only; not yet driven by real clicks or against a real SSH host. Still missing: SSH local port field, key passphrase, showing the colour tag on the connections list
  - Workspace gap: read-only preview only (`SELECT * ... LIMIT 200`, no editing, no pagination past 200 rows, no ER diagram/table inspector yet). `tests/workspace_flow.rs` proves connect → get_tables → execute_query → grid rows works end to end against a real SQLite file
  - Table inspector (phase 4, read-only): a table tab now has Data and Structure views. Data: First/Prev/Next with a page size and the 100,000-offset guard, structured column filters (the egui operators, applied with Apply), row count, sort by header click done in SQL, single-cell selection with a JSON side panel, copy cell / row (TSV) / column / row JSON. Structure: columns with PK, auto and FK badges, indexes, foreign keys in both directions; a foreign key opens its table's tab. Logic is `dbhive_ui_model::table_view` (unit tested); `tests/table_inspector_controller.rs` drives it against a real 130-row SQLite table. Not done: keyset paging (never ported, see `docs/rust-ui-migration/04-plan.md`), the editable grid, page loads are not written to history, only verified on SQLite, parity counts above not re-measured
  - Visual query builder and table create/edit dialogs (phase 4/5): each is a state global plus an overlay (`ui/screens/query_builder.slint`, `table_dialog.slint`), driven by one `act(action, row, choice, text)` callback into `dbhive_ui_model::builder_editor` and `table_dialog` (unit tested). The builder adds tables from the open connection (columns read from the driver), suggests joins from foreign keys, builds WHERE trees to 3 levels, GROUP BY with live validation, HAVING, ORDER BY, LIMIT/OFFSET, and Run or Open in tab goes through the normal run path. The create wizard has 4 steps (basics, columns with 9 presets, unique and foreign keys with live pickers, SQL preview); the edit dialog soft-deletes and restores columns, refuses to drop a primary key column, counts pending operations, asks before a column drop, and reports each statement's outcome. `tests/query_builder_controller.rs` and `tests/table_dialog_controller.rs` drive both against a real SQLite file. Not done: schema picker (current schema only), changing a live column's type or default, check constraints and indexes, drop table, create database, a confirmation before removing a table from the builder, wrapping long lines in the SQL preview. Foreign key targets are written unqualified (the DDL layer quotes the name as one identifier), so on a non-default Postgres schema the server resolves them through its search path. Only verified on SQLite; the Postgres and MySQL statements are unit tested, not run. Parity counts above not re-measured
  - Layout gotcha hit twice (connection editor and workspace sidebar): a `VerticalLayout`/`ScrollView` with no child holding `vertical-stretch` spreads leftover space as gaps between every row instead of below the last one. Fix is either `alignment: start` (no stretch child in that layout) or give exactly one child `vertical-stretch: 1` (a stretch child already present, e.g. a `ScrollView` list) — never both on the same layout, which clips the stretch child to its unstretched size
- [~] Phase 4, import/export and backup (own overlays, own state globals, no new window properties): View menu items and the `go-import-export` / `go-backup` palette commands open them over the workspace. Import: CSV/TSV/TXT/XLSX/XLS wizard (file, preview with sheet pick, auto-mapping with per-column type/default/skip, header/delimiter/skip rows/batch size/truncate options, a reviewed plan before any write, a typed table name to confirm a truncate, running with Stop, result with counts and a truncated error list), run a `.sql` file (transaction and continue-on-error, Stop, error log path), export a table to CSV/JSON/Excel, write a SQL dump (DROP/CREATE/INSERT toggles, table scope). Backup: list with filter, folder shown and openable, create (schema/data toggles, note, tables, output folder), restore with a warning and optional drop-and-recreate, delete behind a confirmation, stage progress. There is no file picker, so paths are typed with a date-stamped Downloads default. Logic is `dbhive_ui_model::{import_wizard, data_forms, backup_manager}`; `tests/data_tools_controller.rs` drives it on a real SQLite file. Not done: a native file dialog, a cancel for backup and restore (the dump tools cannot be interrupted safely), row-level import progress (the engine reports none), verified only on SQLite (Postgres and MySQL dump tools are untested here)
  - ER diagram and schema designer (phase 5): both open over the workspace (header button, View menu, palette). ER: 7 of 7 inventory items built; designer: 5 done, 1 partial (foreign keys are set with dropdowns, not by dragging between handles). Verified by snapshots (dark and light, 1280x800 and 1000x640), unit tests, and `tests/erd_controller.rs`, `tests/erd_pointer.rs`, `tests/designer_controller.rs` against real SQLite. Not run in the GPU window. Not done: loading an existing schema into the designer, keyboard selection of tables on the canvas, a prompt before leaving the workspace with an unapplied design. Parity table above not re-measured.
  - Plugins (phase 6, the plugin manager): `ui/screens/plugins_panel.slint` is a state global (`PluginsState`) plus an overlay, opened from the View menu, the `go-plugins` palette command (now live, from either screen) and a plug button in the connections and workspace headers. Installed tab: list with enable/disable (backend load and unload), detail (author, licence, category, type, install date, executions, errors, permissions with sensitive ones marked, keywords, links), Open interface, Settings (a form generated from `configSchema`, saved through the plugin store), Uninstall behind a confirmation. Marketplace tab: search, category, sort, verified only, per-card Install / Installing / Installed / Failed with the reason. A plugin's interface and its toolbar buttons are drawn by the host from data (`dbhive_core::plugins::ui`), never by the plugin; the first three enabled plugins' toolbar buttons sit in the screen headers. Logic is `dbhive_ui_model::plugins_panel` (unit tested); `tests/plugins_controller.rs` drives the real window, engine and a temp plugin on disk (install, disable, enable, settings, interface, button toast, toolbar toast, refused permission, uninstall). Not done: opening a link (no URL opener, so links are shown as text), tooltips on header buttons (no tooltip in the kit; the plugin name is in the accessible description), a real download (core installs only plugins bundled with the app, and the catalogue is the static three-entry list), plugin menu items (`menu` in the descriptor is parsed but not shown), text inputs notify the plugin on Enter rather than on every key. `DBHIVE_PLUGIN_SOURCES` adds directories to search for bundled plugins (a development aid, used by the test). Parity table above not re-measured.
  - AI assistant (phase 5): a panel docked at the right of the workspace (header "AI" button, View menu, and the `go-ai-assistant` palette command, now live and workspace-scoped); state is `AiState` plus `ai_assistant.slint`, session logic is `dbhive_ui_model::ai_panel` (unit tested), controller is `app/ai_glue.rs`. Provider picker (Ollama, OpenAI, Claude, Gemini, OpenRouter), live status badge, API key saved straight to the OS keyring (never in `ai.json`, a view, a toast or a log; the key field is cleared the moment it is taken), base URL, model picker from the fetched list (the pick is remembered), guided states for a missing key, Ollama not running, an unreachable endpoint, and a failed model list. Generate, Explain, Optimize and Fix error run through `service_ai`; an answer's statement goes into the editor on request (Replace, Add below, New tab) and is never run for you. Schema context is built from the open connection, shows what each button sends, can be switched off, and is viewable as sent; SQLite sends the file name, never the path, and no request carries row data. `service_ai` gained an `ApiKeyStore` trait (keyring or in-memory) with `*_with` variants so tests never touch the keychain. `tests/ai_controller.rs` drives the whole flow against a stub provider on a local socket, including a free-form chat: a question box in the conversation area, the schema and the earlier turns sent with it, the answer's statement offered to the editor on request and never run. The Claude (Anthropic Messages) and Ollama wire formats have their own stub-server tests in `dbhive_core::service_ai::wire_tests`. Not done: streaming, the Gemini and OpenRouter wire formats were not run end to end (they use the same `service_ai` path), nothing was clicked in the real GPU window. Parity table above not re-measured.
  - Results export, view modes, error state and charts (phase 4): `ui/screens/results_tools.slint` is a state global (`ResultsState`) plus the pieces the query tab's results area uses, built from new kit parts (`ResultViewText`, `ResultViewError`, `ExportDialog`, `ChartPlot`); logic is `dbhive_ui_model::{result_export, result_chart}` (unit tested, including nasty CSV and JSON values), controller is `app/results.rs`. Export writes CSV or JSON from a typed, date-stamped Downloads path, writes through a temp file, flags an existing file before replacing it, offers Reveal in folder afterwards, and says which rows it covers (a query tab: every row the grid shows after its filter, up to the driver's 50,000 cap; a table tab: the current page only). The query tab gets Table / JSON / Text / Chart: JSON is highlighted and Text is aligned columns (Copy gives real TSV), both capped at 2,000 rows with a notice; Chart is one Bar or Line series, X and Y pickers, sums per group, 100 group cap, no PNG. A failed query shows a headline, the driver message, a code when there is one, Copy and Retry, and the query behind a disclosure; a zero-row result and a filter that hides every row have their own states. `tests/results_controller.rs` drives all of it against a real SQLite file. Not done: view modes and the structured error on table tabs (that grid area is being changed elsewhere), Area, Pie and Scatter charts, multi-series Y, chart PNG export, a native save dialog, opening the revealed folder was not run (the command is unit tested, not spawned), only verified on SQLite, nothing clicked in the real GPU window. Parity table above not re-measured.
  - Schema browser (phase 4): the flat table list is a tree. `ui/screens/schema_tree.slint` (state `TreeState`, panel, right-click menu, drop and create-database dialogs, routines panel), logic in `dbhive_ui_model::{schema_tree, routine_list}` (unit tested: rows, filter, keyboard rules, menus, the typed-name drop confirmation), SQL generation, the dependency check and drop in `dbhive_core::service_schema`, controller in `app/schema_tree.rs`. Databases are rows (the connected one is marked, double click or Enter on another switches by reconnecting and closes open table tabs), a Postgres database's schemas are nodes that load their tables on first open (a search loads the ones it has not seen), tables and views have different icons and row counts, a click highlights and a double click or Enter opens. A table in a schema other than the current one opens in its own tab pinned to that schema (`Tab::schema`); acting on a schema (Create Table, Edit Table) makes it the current one. Right click: schema (Create Table, Refresh), table (View Data, Generate SELECT and INSERT into a new editor tab and never run, Copy Name, Edit Table, Refresh, Drop Table), database (Switch, Refresh, Create Database). Drop Table asks for the exact name, lists the tables with foreign keys to it (found across schemas) and offers CASCADE only on Postgres and only after that list. Keyboard: arrows, Home/End, Left/Right, Enter, Space, menu key, with a focus ring. Also built: Create Database (Postgres and MySQL) and a Procedures & Functions panel (list, search, definition, copy, open in editor; no execute). `tests/schema_tree_controller.rs` drives it on a real SQLite file with real pointer and key events; `tests/schema_tree_postgres.rs` starts its own throwaway Postgres cluster and covers schemas, cross-schema pinning and drops, CASCADE, database switching and creation, and routines, including running a function and a procedure with arguments (it skips when no `initdb` is found). That run found two core defects, both fixed: the Postgres `get_foreign_keys` missed every foreign key between two schemas, and `execute_query` reported any server refusal as just "db error". The context menus carry Import Data (the import wizard prefilled with the table); Create Database covers SQL Server too; dragging a table row into the SQL editor inserts its quoted name at the caret (pointer-tracked inside the window: Slint 1.18's DragArea/DropArea carry opaque host data, so the tree reports the pointer and the drop lands at the editor's last caret, not at the pixel); and the Procedures & Functions panel runs a routine with one typed box per argument, a confirmation, and the result in a new query tab. Not done: Redis key browsing, MySQL and SQL Server were not run (their statements are unit tested), nothing was clicked in the real GPU window. Parity table above not re-measured.
