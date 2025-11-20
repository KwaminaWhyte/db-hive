# Changelog

All notable changes to DB-Hive will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1] - 2025-11-20

### Fixed

#### Table Editor Critical Fixes
- **Fixed table editor cell editing functionality**:
  - Removed automatic cell copy functionality that was causing toast spam
  - Fixed EditableCell component not showing input field on double-click
  - Added `stopPropagation()` to prevent event bubbling to parent handlers
  - Fixed main table section missing edit mode logic (line 904+ section)
  - Enabled manual text selection for copying cell values

- **Fixed transaction commit for multi-statement SQL**:
  - PostgreSQL driver now uses `batch_execute()` for transactions
  - Added multi-statement SQL detection (counting semicolons)
  - Transactions like `BEGIN; UPDATE ...; COMMIT;` now execute correctly
  - Previously failed with "Query execution failed: db error"

- **Database-specific transaction syntax support**:
  - PostgreSQL: `BEGIN; ... COMMIT;`
  - MySQL: `START TRANSACTION; ... COMMIT;`
  - SQLite: `BEGIN TRANSACTION; ... COMMIT;`
  - MongoDB: Table editing not supported (proper error message)

- **Improved error logging**:
  - Console now shows exact SQL that failed during commit
  - Better error messages for debugging transaction issues

### Technical Details

#### Backend Changes
- Enhanced `execute_query()` in `src-tauri/src/drivers/postgres.rs`:
  - Detects multi-statement SQL by counting semicolons
  - Routes multi-statement SQL to `batch_execute()`
  - Keeps original `query()`/`execute()` logic for single statements
  - Returns empty `QueryResult` for batch operations

#### Frontend Changes
- Updated `TableInspector.tsx`:
  - Removed `copyCellValue()` function and all references
  - Added edit mode logic to main table (previously missing)
  - Fixed row number cells to only open JSON viewer when not in edit mode
  - Added database-specific transaction syntax
  - Enhanced error logging with SQL output
- Updated `EditableCell.tsx`:
  - Added `e.stopPropagation()` in `handleDoubleClick()`
  - Changed cursor from `cursor-pointer` to `cursor-text select-text`

## [0.3.0] - 2025-11-20

### Added

#### Advanced SQL Autocomplete
- **Metadata Caching System**:
  - Intelligent metadata cache with 5-minute expiration
  - Caches databases, schemas, tables, and columns per connection
  - Manual refresh capability via toolbar button
  - Automatic cache invalidation on stale data

- **Context-Aware SQL Suggestions**:
  - **Table suggestions** after `FROM`, `JOIN`, `INTO`, `UPDATE` keywords
  - **Column suggestions** after `SELECT`, `WHERE`, `ON`, `SET`, `GROUP BY`, `ORDER BY`
  - **50+ SQL keywords** (SELECT, FROM, WHERE, JOIN, INSERT, etc.)
  - **40+ SQL functions** (COUNT, SUM, AVG, CONCAT, DATE functions, etc.)
  - Database and schema name suggestions

- **Monaco Editor Integration**:
  - Real-time autocomplete with `Ctrl+Space`
  - Context-sensitive suggestions based on SQL syntax
  - Refresh icon in editor toolbar for metadata updates
  - Proper disposal and re-registration on metadata changes

- **New Tauri Command**: `get_autocomplete_metadata`
  - Returns flattened metadata optimized for autocomplete
  - Supports force refresh parameter
  - Works with all database types (PostgreSQL, MySQL, SQLite, MongoDB)

### Fixed
- JSON syntax highlighting in ResultsViewer and RowJsonViewer
  - Fixed HTML class names appearing in JSON output (e.g., `-400">`)
  - Switched from Tailwind classes to inline styles
  - Added proper HTML escaping to prevent XSS
  - Improved regex patterns to avoid false matches
- TypeScript linting errors in JSON syntax highlighting
- Unused import warnings in frontend components

### Technical Details

#### Backend Changes
- New `MetadataCache` struct in `src-tauri/src/state/mod.rs`
- Enhanced `get_autocomplete_metadata` command in `src-tauri/src/commands/schema.rs`
- Added `metadata_cache` field to `AppState` for per-connection caching

#### Frontend Changes
- New hook: `src/hooks/useAutocompleteMetadata.ts`
- New utility: `src/lib/sqlAutocomplete.ts` (Monaco provider)
- Enhanced `SQLEditor` component with autocomplete integration
- Updated `QueryPanel` to pass database context to editor
- Fixed JSON highlighting in both viewers with inline styles

## [0.3.0] - 2025-11-20

### Added

#### SSH Tunneling Infrastructure (WIP)
- SSH tunnel manager architecture and data models
- Enhanced SSH configuration with flexible authentication methods:
  - Password-based authentication support
  - Private key authentication support
  - Configurable local port binding
- SSH tunnel lifecycle management structure
- Integration with application state management
- Dependencies added: `russh = "0.45"`, `russh-keys = "0.45"`, `dirs = "5.0"`

**Note**: Full SSH tunneling implementation is in progress. The infrastructure and APIs are in place, but the actual tunnel creation and port forwarding will be completed in the next release.

### Fixed
- TypeScript linting errors in JSON syntax highlighting
- Unused import warnings in frontend components

### Technical Details

#### Backend Changes
- New SSH module at `src-tauri/src/ssh/mod.rs` with tunnel manager stub
- Enhanced `SshConfig` model with `SshAuthMethod` enum and authentication options
- Added `SshTunnelManager` to `AppState` for centralized SSH tunnel management
- Updated Cargo dependencies for SSH support

#### Frontend Changes
- Fixed unused parameters in `ResultsViewer.tsx` and `RowJsonViewer.tsx`
- Removed unused `ScrollArea` import

### Known Limitations
- SSH tunneling feature not yet fully functional (infrastructure only)
- MongoDB aggregation pipeline builder UI not yet implemented
- SQL Server driver not yet implemented

## [0.2.0] - 2025-11-20

### Added

#### MongoDB Support
- MongoDB database driver with full CRUD operations support
- MongoDB query parser supporting JavaScript-like syntax:
  - `db.collection.find({...})` - Query documents
  - `db.collection.findOne({...})` - Query single document
  - `db.collection.insertOne({...})` / `insertMany([...])` - Insert documents
  - `db.collection.updateOne/updateMany(filter, update)` - Update documents
  - `db.collection.deleteOne/deleteMany({...})` - Delete documents
  - `db.collection.aggregate([...])` - Run aggregation pipelines
- MongoDB metadata support:
  - List databases
  - List collections (shown as tables)
  - Schema inference from sample documents
  - BSON to JSON conversion for data display
- Optional authentication for local MongoDB instances
- Connection string builder with `authSource=admin` support

#### UI/UX Improvements
- Password visibility toggle in connection form (eye/eye-off icon)
- JSON syntax highlighting in:
  - JSON Row Viewer with color-coded tokens
  - Results Viewer JSON tab with color-coded tokens
- Cell content truncation at 100 characters with tooltips showing full content
- Empty state messages for tables and query results:
  - "No data found in this table" for empty collections/tables
  - "No results returned" for successful queries with no rows
- Improved MongoDB username/password validation (both fields now optional)

### Changed
- Connection form validation now allows optional username/password for MongoDB
- TableInspector now detects MongoDB driver and uses MongoDB query syntax
- Database switching now supports MongoDB in addition to PostgreSQL, MySQL, and SQLite
- ResultsViewer cell rendering now truncates long values for better readability

### Fixed
- MongoDB authentication issues with remote servers (added `authSource=admin` parameter)
- Local MongoDB connections no longer require username/password
- JSON Row Viewer scrolling functionality restored
- Cell content overflow in both TableInspector and ResultsViewer

### Technical Details

#### New Dependencies
- `mongodb = "3.1.0"` - Official MongoDB Rust driver
- `futures-util = "0.3"` - Async stream utilities for MongoDB cursors

#### Backend Changes
- New MongoDB driver implementation at `src-tauri/src/drivers/mongodb.rs` (459 lines)
- Enhanced connection commands to support MongoDB driver type
- MongoDB-specific connection string builder with conditional authentication
- BSON document to JSON conversion utilities

#### Frontend Changes
- Enhanced `ConnectionForm.tsx` with password toggle and MongoDB validation
- Updated `TableInspector.tsx` with MongoDB query support and empty states
- Updated `ResultsViewer.tsx` with cell truncation and empty states
- Enhanced `RowJsonViewer.tsx` with syntax highlighting

### Known Limitations
- MongoDB aggregation pipeline builder UI not yet implemented
- SQL Server driver not yet implemented
- SSH tunneling not yet supported
- MongoDB pagination not yet implemented

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
