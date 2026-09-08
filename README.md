# DB Hive

> A modern, cross-platform database client. Native Rust, no WebView.

![Version](https://img.shields.io/badge/version-0.21.0--beta-blue)
![License](https://img.shields.io/badge/license-MIT-green)

## Overview

DB Hive is a professional database client designed to compete with tools like Beekeeper Studio and DbGate. Built with security, performance, and developer experience in mind: fast, native, and lightweight.

## Features

### ✅ Implemented

- **Multi-Database Support**
  - PostgreSQL with full metadata support
  - Supabase (PostgreSQL-compatible, TLS by default)
  - Neon (PostgreSQL-compatible serverless, TLS required)
  - MySQL / MariaDB with connection handling
  - SQLite with local database management

- **Connection Management**
  - Save and manage multiple connection profiles with folder organisation
  - Test connections before saving
  - Password persistence in the OS keyring
  - SSH tunnelling with password and private key (OpenSSH/PEM) authentication

- **SQL Editor**
  - Monaco Editor with SQL syntax highlighting and autocomplete
  - Execute single or multiple SQL statements (Ctrl+Enter)
  - Multi-statement support with result tabs per statement
  - Query history with automatic saving and search
  - Snippet management for reusable queries

- **Schema Browser**
  - Browse databases, schemas, and tables
  - View table structure: columns, types, indexes, foreign keys
  - Switch databases from an inline dropdown
  - Table row counts in the sidebar

- **Table Inspector & Data Editing**
  - Live data grid with pagination and sortable columns
  - Inline cell editing via double-click; primary keys are read-only
  - Right-click context menu: Copy Cell, Copy Row as JSON, Edit Cell, Set to NULL, Delete Row
  - Bulk row selection with checkboxes and floating action bar (Copy JSON, Delete, Deselect All)
  - Pending Changes Panel: Beekeeper Studio-inspired Visual/SQL diff view before committing
  - Column type badges inline in headers (`#`, `T`, `B`, `{}`, `D`, `U`, `0x`, `[]`)

- **Results Viewer**
  - Grid, JSON, and Raw view modes
  - Sortable columns with visual indicators
  - NULL value indicators
  - Export results to CSV or JSON

- **Import / Export**
  - SQL dump import with cancellable, progress-aware dialog
  - Handles large `mysqldump` files including multi-row INSERT batches and BLOB-heavy rows
  - Automatic splitting of INSERT statements that exceed server `max_allowed_packet`
  - Continue-on-error mode; skips unsplittable oversized rows gracefully
  - Full error log written next to the source file; open directly from the dialog
  - SQL export with configurable options (DROP, CREATE, INSERT, table filter)
  - CSV and JSON export for query results

- **ER Diagram Generator**
  - Interactive entity-relationship diagram with automatic dagre layout
  - Foreign key relationship mapping (PostgreSQL, MySQL, SQLite)
  - Junction table detection with smart colour coding
  - Zoom (0.05×–1.5×), pan, drag, and minimap controls
  - SVG export

- **Command Palette**
  - VS Code / Raycast-style palette via `Cmd+K` / `Ctrl+K`
  - Grouped commands: Navigation, Theme, Window, Actions
  - Fuzzy search with keyboard navigation and shortcut hints

- **UI / UX**
  - Dark / Light / System theme
  - Collapsible sidebar (`Cmd+B` / `Ctrl+B`)
  - Resizable panels, loading skeletons, and error boundaries
  - Toast notifications (Sonner)
  - Browser text selection disabled app-wide (inputs and editors remain selectable)
  - In-app update banner with download progress and restart button

- **Settings**
  - General, Appearance, Query Execution, Keyboard Shortcuts sections
  - Persistent JSON storage in the platform app-data directory
  - Query timeout, max rows, auto-commit, theme mode

### 🚧 Planned

- MongoDB support
- SQL Server support
- Advanced SQL autocomplete (schema-aware)
- Query plan visualiser
- Plugin system

## Technology Stack

### Frontend

- egui / eframe for the native UI
- wgpu for rendering, AccessKit for screen readers
- Monaco Editor for SQL editing
- TanStack Table v8 for virtualised data grids
- shadcn/ui + TailwindCSS for styling
- Zustand for state management
- Sonner for toast notifications

### Backend

- Rust with Tokio async runtime
- tokio-postgres for PostgreSQL
- mysql_async for MySQL / MariaDB
- rusqlite for SQLite
- tauri-plugin-store for persistence
- tauri-plugin-opener for opening files and URLs
- serde for serialisation

## Getting Started

### Prerequisites

- **Rust** 1.75+
- **Git**

No Node, no npm, no bundler. The UI is Rust.

### Installation

```bash
git clone https://github.com/KwaminaWhyte/db-hive.git
cd db-hive

cargo run -p dbhive-app          # debug
cargo run --release -p dbhive-app
```

### Building for Production

```bash
cargo build --release -p dbhive-app     # target/release/dbhive

# macOS: assemble and sign DB Hive.app
./packaging/build-macos-app.sh
./packaging/build-macos-app.sh --sign "Developer ID Application: ..."
```

Code signing is not only about distribution. Connection passwords live in
the OS keychain, and macOS grants keychain items to a stable code identity;
an unsigned binary gets a new identity on every rebuild and cannot read back
what it saved. The script ad-hoc signs by default so that works locally.
#   Windows → .msi / .exe
```

See [Code Signing Guide](docs/code-signing.md) for signing binaries on all platforms.

## Usage

### Connecting to a Database

1. Click **"New Connection"** on the welcome screen
2. Select the driver (PostgreSQL, MySQL, SQLite)
3. Fill in host, port, database, username, and password
4. Optionally configure an SSH tunnel
5. Click **"Test Connection"** then **"Save"**

### Writing and Executing Queries

1. Click a saved connection to open it
2. Write SQL in the Monaco editor
3. Press **Ctrl+Enter** (or **Cmd+Enter**) to execute
4. View results in Grid / JSON / Raw tabs

### Browsing and Editing Data

1. After connecting, the Schema Explorer appears on the left
2. Click a table to open the Table Inspector
3. Use the **Data** tab to view, sort, filter, and edit rows
4. Double-click a cell to edit; right-click for more options
5. Changes queue in the Pending Changes Panel; click **Commit All** to apply

### Importing a SQL Dump

1. From the toolbar or File menu, choose **Import SQL**
2. Select your `.sql` file
3. Choose options (continue-on-error, transaction mode)
4. Click **Import**; the dialog locks during the operation
5. Click **Stop** at any time to cancel gracefully
6. If any statements were skipped, click **Open error log** to see details

### Exporting Data

- **Query results**: CSV or JSON via the results toolbar
- **Database dump**: File → Export SQL, configure tables and options

## Project Structure

```
db-hive/
├── crates/dbhive-app/    # native egui shell
│   ├── components/       # UI components
│   ├── widgets/          # shared widgets (grid, editor, graph layout)
│   ├── store/            # Zustand stores
│   └── types/            # TypeScript type definitions
├── src-tauri/            # Rust backend
│   ├── src/
│   │   ├── service*.rs   # shell-agnostic operations
│   │   ├── drivers/      # Database drivers
│   │   ├── models/       # Data models
│   │   └── state/        # Application state
│   └── tauri.conf.json
└── docs/                 # Architecture and planning docs
```

## Development

```bash
cargo run -p dbhive-app     # run it
cargo check --workspace
cargo test --workspace
cargo fmt
```

Three `credentials::tests::*` failures are expected on macOS: they exercise
the real OS keyring, which the system denies to an unsigned test binary.

Development aids, all unset in normal use:

```bash
DBHIVE_AUTOCONNECT="Local MySQL"   # open a profile at startup
DBHIVE_AUTODB=mydb                 # switch database once connected
DBHIVE_AUTOQUERY="SELECT 1;"       # run a statement
DBHIVE_AUTOSCREEN=er               # open a screen
DBHIVE_AUTOWINDOWS=1               # open extra windows
DBHIVE_DEV_WINDOW="0,0,1300,820"   # pin window geometry
```

## Architecture

### Single process

```
crates/dbhive-core   drivers, DDL, SSH, credentials, migrations, AI,
                     AppState, storage, and the service layer. No UI.
crates/dbhive-app    the egui shell. screens/, widgets/, task, theme.
```

One process and one window server connection, rather than an app process
plus WebView, GPU and networking helpers. Idle memory is 111-130 MB against
roughly 400-650 MB for the previous Tauri build; see
`docs/rust-ui-migration/00-baseline-measurements.md` for the measurements.

The UI never blocks: database work is spawned on a Tokio runtime and polled
from the frame loop (`dbhive-app/src/task.rs`). See
`docs/rust-ui-migration/03-conventions.md` before adding a screen.

### Security Model

- Credentials stored in the OS keyring (`keyring` crate), never in the
  settings files
- No sensitive data in frontend state
- SQL injection prevention through parameterised queries
- SSH tunnelling for secure remote connections

## Troubleshooting

**"Connection refused"**: verify the server is running, check host/port, confirm firewall rules.

**"Authentication failed"**: double-check username and password; verify the user has access to the specified database.

**SQL import stuck or slow**: large files are normal; the dialog shows live progress. Click **Stop** to cancel.

**"Could not open log file"**: check that the source SQL file's directory is writable.

**App won't start**: check `rustc --version` (1.75+ needed) and run `cargo build -p dbhive-app` to see the real error.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests where applicable
4. Submit a pull request

## Roadmap

See [Implementation Roadmap](docs/implementation-roadmap.md) for the full plan.

**Current status**: active beta, `v0.21.0-beta`

## License

MIT License, see [LICENSE](LICENSE)

## Acknowledgments

Inspired by [Beekeeper Studio](https://www.beekeeperstudio.io/), [DbGate](https://dbgate.org/), and [DBeaver](https://dbeaver.io/).

Built with [egui](https://github.com/emilk/egui), [eframe](https://github.com/emilk/egui/tree/master/crates/eframe), [Tokio](https://tokio.rs/), and [syntect](https://github.com/trishume/syntect).

For issues and feature requests: [GitHub Issues](https://github.com/KwaminaWhyte/db-hive/issues)

---

**Built with ❤️ using Claude Code**
