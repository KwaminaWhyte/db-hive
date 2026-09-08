# Migration plan and status

Target: replace the Tauri + React shell with a standalone cross-platform
native Rust desktop app (eframe/egui), at full parity, then delete the Tauri
shell.

Decisions taken at the start, and why they constrain everything below:

| Decision | Choice |
| --- | --- |
| Toolkit | egui + eframe |
| Plugin UI | schema-driven, rendered natively; boa_engine scripting kept |
| SQL editor | solid basics: highlighting, schema completion, find/replace, line numbers |
| Parity scope | everything, including the three canvas tools; no phasing |

## Architecture

```
crates/dbhive-core     UI-agnostic. drivers, DDL, SSH, credentials,
                       migrations, AI, AppState, storage, service
crates/dbhive-app      egui shell. screens/, widgets/, task, theme
src-tauri              reference implementation, deleted at parity
```

Both shells depend on `dbhive-core`, so behaviour cannot drift while both
exist. Work that would previously have gone into a `#[tauri::command]` goes
into `dbhive_core::service` instead, where both shells can reach it.

Data is shared too: `JsonStore` reproduces the exact on-disk format and
location `tauri_plugin_store` used, so an existing installation keeps its
connections, history and snippets. This has been verified against a real
installation: the native shell lists the same five saved connections.

## Status

Done:

- [x] Extract 12,494 LOC of Tauri-free logic into `dbhive-core`
- [x] Replace `tauri_plugin_store` with `JsonStore`, move `AppState` into core
- [x] `service::{connect, disconnect, switch_database, load_persisted}`
- [x] Async bridge (`task::Job`), theme (OKLCH tokens as sRGB), toasts
- [x] Results grid (virtualised, sortable, filterable)
- [x] SQL editor (highlighting, completion, find/replace, gutter)
- [x] Connections list, connect/disconnect, password prompt
- [x] Workspace: schema sidebar, database and schema switching, query tabs
- [x] Query history recording and persistence
- [x] Settings (persisted, owns theme; accent colour actually wired)
- [x] Connection editor (all 9 drivers, SSH, validation, test connection)
- [x] Table inspector (data, columns, indexes, relations, paging, filters)
- [x] Command palette and shortcut registry
- [x] History / Snippets / Templates panel
- [x] Table editor (create table via the DDL generator)
- [x] Migrations (schema diff, generate, apply)
- [x] Backup / restore
- [x] AI assistant
- [x] Activity monitor, and feeding the ActivityLogger
- [x] ER diagram, and `graph_layout` as the dagre replacement
- [x] Menu bar, About

In progress (parallel):

- [ ] Visual schema designer, visual query builder, plugins

Not started:

- [ ] Multi-window (eframe multi-viewport, replacing `commands/window.rs`).
      Deliberately last: it reshapes `DbHiveApp` into per-window state and
      would collide with any screen being written at the same time.
- [ ] Import / export (agent reported; wiring pending)
- [ ] Updater
- [ ] File pickers. Several screens take a typed path because `rfd` is not
      a dependency yet. Adding it closes them all at once.
- [ ] Strip the remaining Tauri command wrappers
- [ ] Delete `src-tauri`, drop the npm toolchain, single-binary packaging

## Known risks

The ER diagram is done: `widgets::graph_layout` is a layered (Sugiyama)
layout with barycentre crossing reduction, standing in for dagre, and the
diagram itself is an egui painter with pan, zoom and drag-to-arrange. The
schema designer and query builder are being built against the same pieces,
so the risk that mattered here has largely been retired.

The plugin system is the other one. Plugins currently render React. The
chosen replacement is a declarative schema the host renders natively, which
means an API redesign and a migration path for any existing plugin.

## Unfinished business inherited from the web shell

Recorded so it is not silently lost in the rewrite:

- Idle WebContent memory was 310-565 MB and was never explained. If the cause
  was retained data rather than the framework, the same mistake can be ported
  straight into the native UI. Watch result-set and schema-metadata retention.
- The accent colour setting was a dead control: written by the picker, read by
  nothing. Now genuinely wired in the native shell.

## Parity items that cannot be met as specified

Recorded rather than quietly dropped:

- **Migration rollback and a migration list.** This codebase's migration
  model is connection-to-connection schema diffing. There is no persisted
  migration history anywhere to roll back to, and no migration files to
  list. The transaction toggle is the only undo, and it covers only the run
  in progress. Building real rollback means designing a migration history,
  which is new scope rather than parity.
- **Keyset pagination in the table inspector.** The Tauri shell had a
  `get_table_data_keyset` command; the native inspector uses offset paging
  with a primary-key ORDER BY and a 100,000-row deep-offset guard.
- **The editable data grid.** Inline cell editing, dirty tracking and
  generated INSERT/UPDATE/DELETE are not ported. This is the data-safety
  critical half of the React table inspector and needs its own pass.
- **Mongo backups** report size 0, because `mongodump` writes a directory
  rather than the single file the entry names. Inherited, not introduced.
