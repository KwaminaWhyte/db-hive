# The Slint app

DB Hive is one native binary on Slint 1.18. This file records what exists, how
it is verified, and what is still open. UI rules are in `docs/ui-guidelines.md`.

## Shape

```
crates/dbhive-core      drivers, DDL, SSH, credentials, migrations, AI,
                        plugins, AppState, storage, service_* operations
crates/dbhive-ui-model  SQL builders, validation, layout, formatting, panel
                        state machines. No Slint types; unit tested
crates/dbhive-slint     the app: ui/kit (the only parts a screen uses),
                        ui/screens, src/ controllers, tests/
```

- **One engine, many windows.** Windows share `AppState`, the Tokio runtime
  and the connection pool; each has its own controller, screen, tabs and
  toasts, because Slint keeps globals per component instance. Async results
  carry the window id back (`runtime::spawn`), timers tick their own window.
- **Geometry is remembered.** `window-state.json` holds each window's size,
  position and maximized state, keyed `main`, `window-2`, and so on.
  `layout.json` holds the schema sidebar width and editor height.
- **Nothing blocks the UI.** Work runs on Tokio and lands through
  `invoke_from_event_loop`; the `AppState` lock is never held across an await.

## Verification

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace                      # 171 warnings today; add none
cargo test --workspace -- --skip credentials::tests   # 1783 pass
scripts/check-ui-tokens.sh                    # ratchet, 5 known literals
```

- Controller tests build the real window with `i_slint_backend_testing` and
  drive callbacks, pointer and key events against real SQLite files; live
  PostgreSQL and MySQL tests are `#[ignore]` and use scratch databases.
- Snapshots come from `examples/snapshot.rs` (dark and light, 1280x800 and
  1000x640) and are looked at, per the guidelines.
- `scripts/window-shot.sh` captures only the real GPU window.
  `cargo run -p dbhive-slint --example smoke -- "Local Post"` runs the core
  paths end to end without a UI.

## What is built

Connections (folders, favourites, SSH tunnels, colour tags, password prompt,
keychain, connection-lost recovery), multi-window, command palette with live
rebindable shortcuts, SQL editor with autocomplete and Stop, query tabs with
context menu and remembered layout, results grid with filter and sort, cell
selection and copy, CSV and JSON export, text and JSON views, charts, inline
row editing with a reviewed atomic commit, table inspector with structure,
schema tree with schemas, views, routines, context menus, Import Data, Create
Database and Drop Table behind a typed confirmation, routine execution with
arguments, query builder, table create/edit dialogs, ER diagram, schema
designer, import wizard (CSV/TSV/TXT/XLSX/XLS and `.sql` files), SQL dump
export, backup and restore, migrations, activity monitor, settings, history,
snippets with folders, query templates, plugins with a schema-driven UI, AI
assistant with free-form chat, About with the Slint credit, update checks.
Light, dark and system themes.

## Live database coverage

SQLite is covered by controller tests for every screen. Live tests cover, on
PostgreSQL: transactions, DDL, inspector, filters, sort, typed row edits,
rejected commits, SQL dump round trips, import, activity monitor, cross-schema
foreign keys and drops, database switching and creation, routine execution.
On MySQL: transactions, DDL, the same inspector and row-edit scenario, dump
round trips and import. Backup and restore ran on both with the real
`pg_dump`, `mysqldump`, `psql` and `mysql`. Not run against a server: SQL
Server, MongoDB, Redis and Turso; the query builder and ER diagram on a
non-default schema.

## Known limits and open work

- No Redis key browser and detail view; no EXPLAIN plan tree; no keyset
  pagination past the 100,000-offset guard.
- Update checks report a newer release and link to it; they do not download
  or install.
- Foreign keys in the schema designer are set with dropdowns, not by dragging
  between handles.
- Charts: one series, Bar or Line, no PNG export.
- MySQL commits implicitly before DDL, so a migration's rollback there covers
  only the statements after its last DDL statement (the server's rule).
- Foreign key targets in generated DDL are written unqualified, so a
  non-default PostgreSQL schema relies on the search path.
- Two windows editing Settings at once are last-writer-wins on the file.
- The native menu bar and focus rings are verified headlessly; a real-window
  click-through should be repeated after any menu change.

## Development aids

- `DBHIVE_PLUGIN_SOURCES`: extra folders searched for bundled plugins.
- `DBHIVE_DATA_DIR`: point every store at a scratch folder (tests do this).
