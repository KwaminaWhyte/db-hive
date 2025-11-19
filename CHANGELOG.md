# Changelog

All notable changes to DB-Hive will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-mvp] - 2025-11-19

### Added

#### Database Support
- PostgreSQL database driver with full connection and metadata support
- MySQL/MariaDB database driver with connection handling
- SQLite database driver for local database management
- Connection management (create, edit, delete, test connections)
- Password persistence using Tauri plugin-store
- Database-specific SQL identifier quoting (backticks for MySQL, double quotes for PostgreSQL/SQLite)

#### SQL Editor
- Monaco Editor integration with SQL syntax highlighting
- Execute single or multiple SQL statements
- Query execution with Ctrl+Enter keyboard shortcut
- Support for SELECT, INSERT, UPDATE, DELETE, and DDL statements
- Execution time display for all queries
- Multi-cursor editing and find/replace

#### Schema Browser
- Browse databases, schemas, and tables
- Database dropdown to switch between databases
- Table list with schema information
- Table Inspector with three tabs:
  - Data tab: Sample data with pagination (20 rows per page)
  - Columns tab: Column definitions, data types, nullability, defaults
  - Indexes tab: Index information with unique and primary key indicators
- Right-click context menus on tables:
  - Generate SELECT query
  - Generate INSERT template
  - Copy table name
  - Refresh table data

#### Results Viewer
- Multiple view modes:
  - Grid view with sortable columns
  - JSON view with pretty-printing
  - Raw view with tab-delimited text
- Click-to-copy functionality:
  - Copy cell values
  - Copy entire rows (tab-separated)
  - Copy entire columns (newline-separated)
- NULL value indicators
- Export results to CSV or JSON
- Row count and execution time display

#### Query Management
- Automatic query history saving
- Search through query history
- Query snippets management:
  - Create snippets from current query
  - Load snippets into editor
  - Delete snippets
  - Snippet descriptions

#### UI/UX
- Dark/Light/System theme support
- Theme toggle in top-right corner
- Toast notifications for user feedback (using Sonner)
- Loading states with skeleton components
- Error boundary for graceful error handling
- Responsive layout with resizable panels
- Welcome screen with DB-Hive logo
- Connection form with validation

### Fixed
- Connection editing bug where updating a profile returned "duplicate ID" error
- Form data not updating when switching between profiles for editing
- Toast notifications now follow the selected theme mode
- Alert dialogs replaced with toast notifications for better UX

### Technical Details

#### Frontend Stack
- React 19 with TypeScript
- Tauri 2.0 for native functionality
- Monaco Editor for SQL editing
- TanStack Table v8 for virtualized data grids
- shadcn/ui + TailwindCSS for styling
- Sonner for toast notifications
- Vite for build tooling

#### Backend Stack
- Rust with Tokio async runtime
- tokio-postgres for PostgreSQL
- mysql_async for MySQL/MariaDB
- rusqlite for SQLite
- tauri-plugin-store for persistence
- serde for serialization

#### Architecture
- Multi-process design (Rust core + React WebView)
- IPC communication via Tauri commands and events
- State management with Mutex<AppState>
- Async operations throughout

### Documentation
- Comprehensive README.md with feature list, installation, and usage
- USER_GUIDE.md with step-by-step tutorials (533 lines)
- Implementation roadmap tracking development progress
- Architecture documentation in CLAUDE.md
- E2E test report template with 62 test cases

### Known Limitations
- MongoDB and SQL Server drivers not yet implemented
- SSH tunneling not yet supported
- Advanced SQL autocomplete not yet implemented
- Table data editing not yet supported
- No query plan visualizer
- No ER diagram generator

### Security Notes
- Passwords currently stored in plaintext in persistent store (temporary solution)
- Future releases will use OS keyring for secure password storage
- All user input is validated before SQL execution
- No SQL injection vulnerabilities detected

---

## Upcoming in v0.2.0

### Planned Features
- MongoDB database driver
- SQL Server database driver
- SSH tunneling support
- Advanced SQL autocomplete with metadata
- Table data inline editing
- Query plan visualizer for PostgreSQL
- ER diagram generator
- Plugin system

For detailed roadmap, see [docs/implementation-roadmap.md](docs/implementation-roadmap.md).

---

**Legend:**
- `Added` - New features
- `Changed` - Changes in existing functionality
- `Deprecated` - Soon-to-be removed features
- `Removed` - Removed features
- `Fixed` - Bug fixes
- `Security` - Security improvements
