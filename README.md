# DB-Hive

> A modern, cross-platform database client built with Tauri 2.0 and React 19

![Version](https://img.shields.io/badge/version-0.19.0--beta-blue)
![License](https://img.shields.io/badge/license-MIT-green)

## Overview

DB-Hive is a professional database client designed to compete with tools like Beekeeper Studio and DbGate. Built with security, performance, and developer experience in mind — fast, native, and lightweight.

## Features

### ✅ Implemented

- **Multi-Database Support**
  - PostgreSQL with full metadata support
  - MySQL / MariaDB with connection handling
  - SQLite with local database management

- **Connection Management**
  - Save and manage multiple connection profiles with folder organisation
  - Test connections before saving
  - Password persistence using Tauri plugin-store
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
  - Pending Changes Panel — Beekeeper Studio-inspired Visual/SQL diff view before committing
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
  - Persistent storage via Tauri Store plugin
  - Query timeout, max rows, auto-commit, theme mode

### 🚧 Planned

- MongoDB support
- SQL Server support
- Advanced SQL autocomplete (schema-aware)
- Query plan visualiser
- Plugin system

## Technology Stack

### Frontend

- React 19 with TypeScript (strict mode)
- Tauri 2.0 for native functionality
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

- **Node.js** 18+ (Bun recommended)
- **Rust** 1.70+
- **Git**

### Installation

```bash
# Clone the repository
git clone https://github.com/KwaminaWhyte/db-hive.git
cd db-hive

# Install dependencies
bun install

# Run in development mode
npm run tauri dev
```

### Building for Production

```bash
bun run build          # Frontend only
npm run tauri build    # Full Tauri app

# Output in src-tauri/target/release/bundle/
#   macOS  → .dmg / .app
#   Linux  → .deb / .rpm / .AppImage
#   Windows → .msi / .exe
```

See [Code Signing Guide](docs/CODE_SIGNING.md) for signing binaries on all platforms.

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
5. Changes queue in the Pending Changes Panel — click **Commit All** to apply

### Importing a SQL Dump

1. From the toolbar or File menu, choose **Import SQL**
2. Select your `.sql` file
3. Choose options (continue-on-error, transaction mode)
4. Click **Import** — the dialog locks during the operation
5. Click **Stop** at any time to cancel gracefully
6. If any statements were skipped, click **Open error log** to see details

### Exporting Data

- **Query results**: CSV or JSON via the results toolbar
- **Database dump**: File → Export SQL, configure tables and options

## Project Structure

```
db-hive/
├── src/                  # React frontend
│   ├── components/       # UI components
│   ├── hooks/            # Custom React hooks
│   ├── store/            # Zustand stores
│   └── types/            # TypeScript type definitions
├── src-tauri/            # Rust backend
│   ├── src/
│   │   ├── commands/     # Tauri command handlers
│   │   ├── drivers/      # Database drivers
│   │   ├── models/       # Data models
│   │   └── state/        # Application state
│   ├── capabilities/     # Tauri permission config
│   └── tauri.conf.json
└── docs/                 # Architecture and planning docs
```

## Development

```bash
# Run dev server
npm run tauri dev

# Rust checks and tests
cd src-tauri
cargo check
cargo test
cargo fmt

# Frontend build
bun run build
```

## Architecture

### Multi-Process Design

- **Core Process (Rust)**: database connections, state, credentials, all I/O
- **WebView Process (React)**: UI rendering, user interactions
- **IPC**: Tauri Commands (frontend → backend), Events (backend → frontend)

### Security Model

- Credentials stored via Tauri plugin-store (encrypted local storage)
- No sensitive data in frontend state
- SQL injection prevention through parameterised queries
- SSH tunnelling for secure remote connections

## Troubleshooting

**"Connection refused"** — Verify the server is running, check host/port, confirm firewall rules.

**"Authentication failed"** — Double-check username and password; verify the user has access to the specified database.

**SQL import stuck or slow** — Large files are normal; the dialog shows live progress. Click **Stop** to cancel.

**"Could not open log file"** — Check that the source SQL file's directory is writable.

**App won't start** — Run `bun install` and `rustc --version` to verify dependencies.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests where applicable
4. Submit a pull request

## Roadmap

See [Implementation Roadmap](docs/implementation-roadmap.md) for the full plan.

**Current status**: Active beta — `v0.19.0-beta`

## License

MIT License — see [LICENSE](LICENSE)

## Acknowledgments

Inspired by [Beekeeper Studio](https://www.beekeeperstudio.io/), [DbGate](https://dbgate.org/), and [DBeaver](https://dbeaver.io/).

Built with [Tauri](https://tauri.app/), [React](https://react.dev/), [Monaco Editor](https://microsoft.github.io/monaco-editor/), [TanStack Table](https://tanstack.com/table/), and [shadcn/ui](https://ui.shadcn.com/).

For issues and feature requests: [GitHub Issues](https://github.com/KwaminaWhyte/db-hive/issues)

---

**Built with ❤️ using Claude Code**
