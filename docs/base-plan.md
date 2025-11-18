# Tauri-DBClient — Full Architecture & Design (MVP → Enterprise)

> Deliverables: Full architecture, Full feature list, Database driver structure, UI/UX wireframes (descriptive)

---

## Project overview (elevator pitch)

Build a cross-platform, lightweight, secure, and extensible database client desktop app using **Tauri + React**. The product supports SQL and popular NoSQL engines, offers a powerful SQL editor with autocomplete and results visualization, and includes advanced features (connection management, SSH tunneling, ER diagrams, export/import, plugins). Design for an incremental roadmap: **MVP** → **Advanced** → **Enterprise**.

---

# 1. Architecture

## 1.1 High-level components

- **Frontend (Renderer)** — React + TypeScript app packaged by Tauri. Responsible for UI, editor, state management, theming, and offline storage for non-sensitive settings.
- **Backend (Tauri Core / Rust)** — Native layer exposing secure commands for DB drivers, SSH tunnels, file I/O, encrypted secrets storage, native dialogs, and heavy-lifting tasks. Runs in same app as Tauri core.
- **Drivers Layer** — Rust modules/crates that encapsulate connections for each supported DB (Postgres, MySQL/MariaDB, SQLite, SQL Server, MongoDB, etc.). Exposes a uniform, async driver interface to the Tauri commands.
- **IPC / Command API** — Tauri Commands (invoke) provide typed RPC between React and Rust. All driver operations (connect, query, cancel, schema metadata) go through this.
- **Persistence & Settings** — Local encrypted store for connection credentials (Tauri Secure Storage / OS keyring), app settings in an encrypted config file or OS-protected storage, and a separate local DB (SQLite) for history, query snippets, and usage telemetry.
- **Plugins / Extensions (Optional)** — A plugin system (JS-based or WASM) enabling community extensions (formatters, visualizers, connectors).
- **Updater / Auto-update** — Optional auto-update channel integrated with the chosen distribution flow (GitHub Releases, custom server).

## 1.2 Data flow (typical query)

1. User selects a connection profile in the UI.
2. Frontend calls `tauri.invoke('openConnection', {profileId})`.
3. Tauri core looks up encrypted credentials, creates (or reuses) a driver instance in the Drivers Layer. If an SSH tunnel is required, Tauri spawns and manages it before opening the DB tunnel.
4. Frontend sends SQL via `tauri.invoke('runQuery', {connectionId, sql, options})`.
5. Driver executes query asynchronously and streams results back via an event channel (`tauri.emit`) in chunks (for large results).
6. Frontend renders partial results (virtualized grid) and provides export/format options. If user cancels, frontend invokes `cancelQuery`.

## 1.3 Security model

- **Credentials**: Never stored in plaintext. Use OS keychain (keytar equivalent in Rust - e.g., `keyring` crate) or Tauri's secure APIs. Optionally encrypt with a master passphrase stored by the user.
- **Network**: Support TLS/SSL options, certificate pinning optional, and SSH tunneling for private networks.
- **Permissions**: Limit the renderer's direct FS/network access; all privileged operations happen in Rust commands.
- **Sandboxing**: Use Tauri best practices: avoid evaluating untrusted scripts in the renderer and validate plugin boundaries.

## 1.4 Scalability & Performance

- Use streaming of result sets from Rust to renderer to avoid blocking memory for large datasets.
- Use virtualization (in the UI) to render rows efficiently.
- Offload expensive tasks (ER layout, diff, import/exports) to background threads in Rust.

---

# 2. Full feature list

Features grouped by priority and release stage.

## 2.1 MVP (must-have)

- Cross-platform desktop app (Windows/macOS/Linux)
- Connectors: PostgreSQL, MySQL/MariaDB, SQLite
- Connection manager: saved profiles, test connection, grouped folders
- Secure credential storage (OS keyring / encrypted file)
- SQL editor: Monaco editor, syntax highlighting, basic autocomplete (keywords), SQL formatter, multiple tabs
- Query runner: execute, cancel, results streaming, execution time, row count
- Results viewer: virtualized table, sort, filter, copy cell/row, export CSV/JSON
- Schema explorer: list databases -> schemas -> tables -> columns
- Query history and saved snippets
- Import CSV into table (simple mapping)

## 2.2 Advanced (next releases)

- Add connectors: SQL Server, MongoDB (CRUD + aggregation), Oracle (optional)
- Full autocomplete using DB metadata (tables, columns, functions, types)
- SSH tunneling and SSL configuration per-connection
- Table editor (edit rows inline, commit, rollback)
- Transaction controls and preview of generated SQL for edits
- ER Diagram generator (read-only with export PNG/SVG)
- Query plan / explain visualizer for supported DBs
- Export: Excel (.xlsx), SQL dump, JSON lines
- Theme support and layout customization (panels, docks)
- Multi-resultset handling and result panes

## 2.3 Enterprise (long term)

- Plugins/Extensions architecture (WASM/JS sandboxed plugins)
- Workspace sync / cloud profiles (optional end-to-end encryption)
- Visual query builder (drag & drop)
- Schema compare & migration scripts (diff & generate SQL)
- Role-based access control for shared deployments
- Collaboration features: share queries, comments, live editing (optional)
- Built-in AI assistant for natural language -> SQL, query explanation, optimization tips
- Connection templates and managed credential vault integration (HashiCorp Vault, AWS Secrets Manager)

## 2.4 Developer / QA features

- Debug logging and telemetry toggle (opt-in)
- Diagnostic export (for support)
- Self-hosted backend option (for plugin orchestration)

---

# 3. Database driver structure (technical)

Goal: keep driver implementations isolated, consistent, and testable. Use Rust async crates and a thin adapter layer to normalize behavior across DBs.

## 3.1 Design goals

- Uniform `Driver` trait in Rust that each adapter implements.
- All driver operations are async and return serializable payloads (for IPC to frontend).
- Support streaming results (paginated/chunked) and cancellation tokens.
- Provide metadata introspection functions (list_databases, list_schemas, list_tables, describe_table, list_indexes, list_functions).
- Provide a safe execution environment for user queries (timeouts, resource limits).

## 3.2 Suggested Rust types & interfaces (conceptual)

```rust
// conceptual (not compile-ready) pseudo-code
#[derive(Serialize, Deserialize)]
pub struct ConnectionOptions { /* host, port, user, passwordRef, database, ssl, ssh */ }

#[async_trait]
pub trait Driver: Send + Sync {
    async fn connect(opts: ConnectionOptions) -> Result<Box<dyn ConnectionHandle>>;
}

#[async_trait]
pub trait ConnectionHandle {
    async fn run_query(&self, sql: String, options: QueryOptions) -> Result<QueryStream>;
    async fn cancel_query(&self, query_id: QueryId) -> Result<()>;
    async fn list_databases(&self) -> Result<Vec<DbName>>;
    async fn list_schemas(&self, db: Option<String>) -> Result<Vec<SchemaInfo>>;
    async fn describe_table(&self, table: TableRef) -> Result<TableSchema>;
    async fn close(&self) -> Result<()>;
}
```

## 3.3 Driver adapter per DB (examples)

- **Postgres adapter**

  - Use `tokio-postgres` or `sqlx` (with runtime features), support `COPY` for fast import/export, `pg_catalog` introspection queries, support for `EXPLAIN (ANALYZE, FORMAT JSON)`.

- **MySQL/MariaDB adapter**

  - Use `mysql_async` or `sqlx-mysql`, support `INFORMATION_SCHEMA` for introspection, `mysqldump`-style exports.

- **SQLite adapter**

  - Use `rusqlite` or `sqlx` with SQLite feature. Direct file access, support attaching DBs, PRAGMA introspection.

- **MongoDB adapter**

  - Use official `mongodb` crate. Provide mapping of collections -> documents, aggregation runner, explain.

- **SQL Server adapter**

  - Use `tiberius` (async SQL Server driver) or `odbc` via `odbc-api` crate for larger compatibility.

## 3.4 Query streaming & cancellation

- Each `run_query` spawns a Rust async task that writes row batches to a `tokio::sync::mpsc` channel.
- The Tauri command returns a `query_id` and the frontend listens for `tauri` events like `query_result_batch::<query_id>`.
- For cancellation: maintain a `CancellationToken` mapped by query_id; cancel signals will abort the task and close the channel.

## 3.5 Metadata SQL examples (normalized)

- Postgres: query `pg_catalog.pg_tables`, `information_schema.columns`.
- MySQL: query `information_schema.tables` and `columns`.
- SQLite: use `PRAGMA table_info(table_name)` and `sqlite_master`.

---

# 4. UI/UX wireframes (descriptive)

Below are concise wireframes and layout descriptions for screens and behaviors. These are intentionally implementation-friendly so you can hand them to a UI engineer or generate designs with Figma AI.

## 4.1 Main window (default layout)

Left column (sidebar, collapsible, 280px) — **Schema Explorer / Connections**

- Top: Connection selector + quick connect button
- Middle: Saved Profiles grouped in folders
- Bottom: Recent queries and snippets quick list

Center area — **Editor + Tabs**

- Tabbed editor (Monaco) with tabs for SQL files. Each tab shows connection name, DB, and unsaved marker.
- Editor toolbar: Run (▶), Run Selection (▶ selection), Explain, Format, Cancel, Save snippet, Toggle Results Bottom

Bottom pane — **Results / Messages** (resizable)

- Results grid (virtualized). Tabs for Grid / JSON / Raw / Explain Plan.
- Execution metadata: time, rows, returned columns, warnings.

Right column (optional, collapsible) — **Inspector**

- Column metadata, sample row, index info, foreign keys, quick actions (Open Table, Edit Rows, Generate SELECT \* SQL)

## 4.2 Connect modal / wizard

- Step 1: Select engine (Postgres/MySQL/SQLite/Mongo)
- Step 2: Connection details (host, port, user, database). Toggle advanced options (SSL, SSH tunnel, timeout)
- Step 3: Test connection (button) with result feedback. Option to save profile and folder.

## 4.3 Table editor view

- Grid showing rows with inline editing. Top bar: Add Row, Delete, Save Changes, Revert.
- Side: transaction log preview (SQL produced by changes) and commit mode (auto/manual).

## 4.4 ER Diagram view (read-only for MVP)

- Canvas area with nodes for tables and lines for foreign keys. Zoom controls, export PNG/SVG, auto-layout button.

## 4.5 Settings screen

- General (theme, auto-update, telemetry toggle)
- Cache & Storage (path, clear cache)
- Security (master passphrase, keychain settings)
- Plugins (enable/disable)

---

# 5. Folder & repo structure (recommended)

```
/ (repo root)
├─ apps/
│  ├─ tauri-app/         # Tauri + Rust core
│  └─ web/               # React renderer (TS)
├─ crates/               # Rust crates for drivers and shared libs
│  ├─ drivers_pg/
│  ├─ drivers_mysql/
│  ├─ drivers_sqlite/
│  └─ shared_types/
├─ packages/             # Optional: internal JS packages (ui components)
└─ infra/                # CI, release scripts, packaging
```

---

# 6. Roadmap & milestones (quick)

**Phase 0 (2 weeks)**: Project setup, CI, repo templates, basic Tauri + React app skeleton, Monaco integration.

**Phase 1 — MVP (8–12 weeks)**: Implement Postgres + SQLite drivers, connection manager, SQL editor, run queries, results viewer, basic exports, settings, and packaging.

**Phase 2 — Advanced (12–20 weeks)**: Add MySQL, SSH tunneling, autocomplete using metadata, table editor, ER diagrams, EXPLAIN visualizer.

**Phase 3 — Enterprise (ongoing)**: Plugins, NoSQL adapters, workspace sync, collaboration, RBAC.

---

# 7. Implementation tips & gotchas

- **Driver parity**: each DB exposes metadata differently — write normalization layer early.
- **Large result sets**: never pull everything into memory in the renderer; stream with backpressure.
- **SSH tunnels**: prefer managing tunnels in Rust rather than relying on external OS forwarders.
- **Editor & autocomplete**: fetch schema metadata in background and cache it with invalidation rules.
- **Distribution**: Tauri produces small binaries, but ensure CI handles code signing for macOS/Windows.

---

# 8. Deliverables checklist (what you asked for)

- ✅ Full architecture (this doc)
- ✅ Full feature list (MVP → Enterprise)
- ✅ Database driver structure (interfaces, patterns)
- ✅ UI/UX wireframes (descriptive)
