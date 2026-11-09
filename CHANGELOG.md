# Changelog

## 0.21.0

DB Hive is a native Rust desktop app on Slint. One binary, one window server
connection, no WebView and no bundler.

- PostgreSQL, MySQL, SQLite, MongoDB, Redis, SQL Server and Turso drivers
  over one shared core.
- Saved connections in folders, SSH tunnels, OS keyring for every secret,
  password prompt when none is cached, and a Connection Lost screen that can
  reconnect without losing open tabs.
- SQL editor with syntax highlighting, autocomplete across schemas, Stop and
  Clear, query tabs with a right-click menu and a remembered pane layout.
- Results grid with filters, sort, inline edits reviewed before commit, copy
  cell/row/column, CSV and JSON export, text and JSON views, and a chart.
- Schema tree with schemas, tables, views, routines, context menus, Create
  Database and Drop Table behind a typed confirmation.
- Visual query builder, table create/edit dialogs, ER diagram and schema
  designer.
- Import CSV, TSV, TXT, XLSX and XLS, run a `.sql` file, export a SQL dump,
  and back up and restore with `pg_dump`, `mysqldump` and `psql`.
- Activity monitor, schema migrations, plugins with a native UI schema, and
  an AI assistant (Ollama, OpenAI, Claude, Gemini, OpenRouter) that never
  runs a generated statement for you.
- Multiple windows over one engine, each with its own screen, tabs and saved
  geometry, sharing the same connections and credential store.
- Light, dark and system themes; live shortcut rebinding; command palette.
