# DB-Hive: Actionable Implementation Roadmap

**A Professional Database Client Built with Tauri + React**

> **Target**: Build a Beekeeper Studio / DbGate competitor (5-9 months to advanced version)
> **Stack**: Tauri 2.0 + React + TypeScript + Rust
> **Goal**: Cross-platform, lightweight, secure, extensible database client

---

## Table of Contents

1. [Project Foundation](#1-project-foundation)
2. [Phase 0: Setup & Architecture](#2-phase-0-setup--architecture-weeks-1-2)
3. [Phase 1: MVP Development](#3-phase-1-mvp-development-weeks-3-14)
4. [Phase 2: Advanced Features](#4-phase-2-advanced-features-weeks-15-28)
5. [Phase 3: Enterprise Features](#5-phase-3-enterprise-features-ongoing)
6. [Implementation Details](#6-implementation-details)
7. [Sub-Agents & Automation](#7-sub-agents--automation)

---

## 1. Architecture Overview

**Tech Stack:**

- Frontend: React 19 + TypeScript + Monaco Editor + TanStack Table + shadcn/ui
- Backend: Tauri 2.0 + Rust + Tokio
- Database Drivers: PostgreSQL (tokio-postgres), MySQL (mysql_async), SQLite (rusqlite), MongoDB (mongodb)
- Security: OS keyring, SSH tunneling
- Storage: Tauri Store (settings, connections, history)

For detailed architecture patterns, see `CLAUDE.md`.

---

## 2. Phase 0: Setup & Architecture ✅ COMPLETED

**Week 1-2 Status:**

- [x] Project structure and dependencies configured
- [x] Tauri 2.0 + React 19 + TypeScript + Tailwind CSS v4
- [x] shadcn/ui components integrated
- [x] DatabaseDriver trait designed
- [x] OS keyring credential encryption implemented
- [x] Error handling with DbError + thiserror
- [x] State management with Mutex<AppState>

---

## 3. Phase 1: MVP Development ✅ COMPLETED (Weeks 3-14)

**Completed Features:**

- ✅ Connection Management (PostgreSQL, MySQL, SQLite, MongoDB)
- ✅ SQL Editor with Monaco (syntax highlighting, autocomplete)
- ✅ Query Execution & Results Viewer (Grid/JSON/Raw modes)
- ✅ Schema Browser with tree view
- ✅ Table Inspector with inline editing
- ✅ Query History & Snippets
- ✅ CSV/JSON/SQL Export & Import
- ✅ Dark/Light theme support
- ✅ OS Keyring credential encryption

---

### Milestone 1.1: Basic Connection Management ✅ COMPLETED (Weeks 3-5)

- [x] Connection CRUD commands (create, update, delete, list, test, connect, disconnect)
- [x] PostgreSQL, MySQL, SQLite, MongoDB drivers
- [x] ConnectionForm & ConnectionList UI components
- [x] OS keyring credential storage
- [x] Persistent storage via Tauri Store
- [x] shadcn/ui design system integration

### Milestone 1.2: SQL Editor ✅ COMPLETED (Weeks 6-8)

- [x] Monaco Editor with SQL syntax highlighting
- [x] Multiple editor tabs with independent state
- [x] Keyboard shortcuts (Ctrl+Enter, Ctrl+K)
- [x] Query execution with TanStack Table results viewer
- [x] Execution time tracking & row counts
- [x] Query history & snippets management
- [x] Resizable panels (editor/results/history)

### Milestone 1.3: Results Viewer ✅ COMPLETED (Weeks 9-11)

- [x] TanStack Table with virtualization & column sorting
- [x] Cell/row/column copying with toast notifications
- [x] CSV/JSON export with proper escaping
- [x] Multiple view modes (Grid/JSON/Raw)
- [x] NULL value indicators & execution metadata

### Milestone 1.4: Schema Explorer ✅ COMPLETED (Weeks 12-14)

- [x] SchemaExplorer with hierarchical tree view
- [x] Lazy loading for schemas & tables
- [x] TableInspector with tabs (Data/Columns/Indexes)
- [x] Context menus (Generate SELECT/INSERT, Copy Name, Refresh)
- [x] Drag-and-drop table names to editor
- [x] Search/filter functionality
- [x] Inline table editing with transaction support

### MVP Polish ✅ COMPLETED

- [x] ErrorBoundary & toast notifications
- [x] Loading states & skeletons
- [x] User documentation (README + USER_GUIDE)
- [x] Light/Dark theme support
- [x] Bug fixes & UX improvements

---

## 4. Phase 2: Advanced Features (Weeks 15-28)

### Milestone 2.1: Additional Database Drivers ✅ COMPLETED

- [x] MySQL/MariaDB driver with metadata queries
- [x] MongoDB driver with CRUD operations & query parser
- [x] SQL Server driver with tiberius (2025-11-22)

### Milestone 2.2: SSH Tunneling ✅ COMPLETED (2025-11-21)

- [x] SSH tunnel manager with password/key auth
- [x] Tunnel lifecycle management
- [x] Connection command integration with separate SSH password handling
- [x] SSH configuration UI with collapsible form section
- [x] SSH password storage in OS keyring (separate from database passwords)
- [x] Backend commands: save_ssh_password, get_ssh_password
- [x] Automatic SSH password retrieval when connecting
- [x] Support for both password and private key authentication
- [x] Full integration with ConnectionForm and ConnectionList

### Milestone 2.3: Advanced SQL Autocomplete ✅ COMPLETED

- [x] Metadata cache with 5-minute expiration
- [x] Context-aware suggestions (tables, columns, keywords, functions)
- [x] Monaco Editor provider integration
- [x] Manual metadata refresh

### Milestone 2.4: Table Editor ✅ COMPLETED

- [x] Inline editing with cell change tracking
- [x] Row selection & bulk delete
- [x] Add/Insert rows with auto-generation detection
- [x] Transaction preview with SQL highlighting
- [x] Commit/rollback UI

### Milestone 2.5: SQL Import/Export ✅ COMPLETED

- [x] Database dump/restore (PostgreSQL, MySQL, SQLite)
- [x] Export options (DROP/CREATE/INSERT statements)
- [x] Import with transaction support & error handling
- [x] SqlExportDialog & SqlImportDialog components

### Milestone 2.6: Query Plan Visualizer ✅ COMPLETED (2025-11-21)

- [x] PostgreSQL EXPLAIN (ANALYZE, FORMAT JSON) parser
- [x] Visual query plan tree with cost highlighting
- [x] QueryPlanVisualizer component with tree visualization
- [x] Cost-based color coding (green/yellow/red)
- [x] Detailed node metrics (costs, rows, execution time)
- [x] Expand/collapse functionality
- [x] Support for index scans, joins, filters
- [x] TypeScript types for query plan structures
- [x] Integration into QueryPanel with toggle UI (2025-11-21)
- [x] Automatic EXPLAIN query detection
- [x] PostgreSQL JSON/JSONB type support in driver (2025-11-21)

### Milestone 2.7: Visual Query Builder ✅ COMPLETED

**Drag-and-Drop SQL Query Builder**

- [x] **Core Query Builder Engine:**
  - [x] Query builder state management (tables, columns, joins, filters, sorting)
  - [x] SQL generation from query builder state with database-specific syntax
  - [x] Support for SELECT queries (INSERT/UPDATE/DELETE deferred)
  - [x] Real-time SQL preview as user builds query
  - [x] Query validation with error messages

- [x] **Visual Query Canvas:**
  - [x] Table selector dropdown with schema organization
  - [x] Lazy loading of tables by schema
  - [x] Search functionality for tables
  - [x] Auto-generated unique table aliases
  - [x] Join type selector (INNER, LEFT, RIGHT, FULL OUTER, CROSS)
  - [x] Auto-suggest joins based on foreign keys
  - [x] Visual join management with color-coded badges

- [x] **Column Selection & Transformation:**
  - [x] Multi-select columns from all tables
  - [x] Column alias input
  - [x] Aggregate functions (COUNT, SUM, AVG, MIN, MAX, COUNT_DISTINCT)
  - [x] DISTINCT toggle (global and per-column)
  - [x] Column ordering with drag-and-drop reordering

- [x] **Filter Builder:**
  - [x] Visual WHERE clause builder with condition groups
  - [x] Support for AND/OR logic operators
  - [x] All comparison operators (=, !=, >, <, >=, <=, LIKE, NOT LIKE, IN, NOT IN, IS NULL, IS NOT NULL, BETWEEN)
  - [x] Type-aware value inputs (text, number, boolean)
  - [x] Nested condition groups (up to 3 levels)
  - [x] HAVING clause support for aggregations

- [x] **Additional Features:**
  - [x] GROUP BY column selection with validation
  - [x] ORDER BY with ASC/DESC toggle and reordering
  - [x] LIMIT and OFFSET inputs with validation
  - [x] Pagination preview calculation
  - [x] Database-specific SQL generation (PostgreSQL, MySQL, SQLite, SQL Server, MongoDB)

- [x] **UI/UX:**
  - [x] Split view: Query Builder (left) + Generated SQL (right)
  - [x] "Execute Query" button with results navigation
  - [x] Monaco Editor for SQL preview with syntax highlighting
  - [x] Collapsible sections for all query components
  - [x] Responsive layout with resizable panels
  - [x] Integrated into View menu in CustomTitlebar
  - [x] Full route integration at /visual-query

**Implementation Date:** 2025-11-22

**Deferred Features:**

- [ ] Import SQL query to visual builder (requires SQL parser)
- [ ] Export query builder state to JSON
- [ ] Save query templates
- [ ] Pan and zoom canvas for large queries
- [ ] INSERT/UPDATE/DELETE query support

### Milestone 2.8: ER Diagram Generator ✅ COMPLETED

- [x] Foreign key relationship parsing (PostgreSQL, MySQL, SQLite)
- [x] Dagre automatic hierarchical layout with top-to-bottom flow
- [x] ReactFlow integration with Handle components for proper connections
- [x] Zoom (0.05x-1.5x), pan, drag controls
- [x] SVG export functionality
- [x] Smart node styling: Junction table detection, column limiting (max 10)
- [x] Performance optimizations: No edge animations, smart minimap coloring
- [x] UI integration: Popover menu for actions, control panel
- [x] Optimized spacing (180px horizontal, 250px vertical)

### Milestone 2.9: Router Migration ✅ COMPLETED (2025-11-21)

**TanStack Router Implementation**

- [x] Migrated from React Router to TanStack Router v1.139.0
- [x] Type-safe routing with search params validation
- [x] File-based routing structure with 10 routes:
  - [x] `__root.tsx` - Root layout with providers
  - [x] `index.tsx` - Welcome screen
  - [x] `connections.tsx` - Side-by-side connection management
  - [x] `settings.tsx` - Settings page
  - [x] `_connected/route.tsx` - Connected layout with schema explorer
  - [x] `_connected/query.tsx` - Multi-tab query panel
  - [x] `_connected/table.$schema.$tableName/route.tsx` - Table route parent
  - [x] `_connected/table.$schema.$tableName/index.tsx` - Table redirect to tabs
  - [x] `_connected/er-diagram.$schema.tsx` - ER diagram viewer
  - [x] `_connected/visualization.tsx` - Visualization route
- [x] URL-based multi-table tabs system:
  - [x] Tab IDs: `query-{timestamp}` and `table-{schema}.{tableName}`
  - [x] Search params: `?tabs=query-1,table-public.users&active=0`
  - [x] Tab bar with close buttons and Plus button
- [x] Per-tab state preservation:
  - [x] TabContext with React Context API
  - [x] LocalStorage persistence per connection
  - [x] Automatic save/restore on connection change
  - [x] SQL content preservation across tab switches
- [x] Component rendering optimization:
  - [x] All tabs rendered simultaneously with absolute positioning
  - [x] CSS show/hide instead of mount/unmount
  - [x] No content loss on tab switching

---

## 5. Phase 3: Enterprise Features (Ongoing)

### Milestone 3.2: Connection Manager Dashboard ✅ COMPLETED (2025-11-23)

**Visual Connection Management**

- [x] Build dashboard page for managing all connections:
  - [x] Category tabs (All, Favorites, Recently Used, Local, Cloud)
  - [x] Grid/list view toggle
  - [x] Connection cards showing:
    - [x] Connection name and DB type icon
    - [x] Host summary with port
    - [x] Driver type badge with color coding
    - [x] Favorite star indicator
  - [x] Search bar with real-time filtering
  - [x] Top actions: New Connection, Refresh
- [x] Responsive grid layout (1/2/3 columns based on screen size)
- [x] Hover states and animations with smooth transitions
- [x] Double-click to connect functionality
- [x] Context actions: Edit, Delete, Toggle Favorite
- [x] Empty states for filtered results
- [x] Password prompt dialog for connections without saved passwords

### Milestone 3.3: Database Schema Management ✅ COMPLETED (2025-11-23)

**DDL Operations - Create/Modify/Delete Database Objects**

**Backend (Rust - Tauri Commands):**

- [x] DDL Type System (src-tauri/src/models/ddl.rs - 380 lines):
  - [x] ColumnType enum with 20+ database types
  - [x] TableDefinition, ColumnDefinition structures
  - [x] ForeignKeyConstraint, UniqueConstraint, CheckConstraint
  - [x] AlterColumnOperation enum for table modifications
  - [x] DropTableDefinition for table removal
  - [x] DdlResult for operation responses
- [x] Database-specific DDL generators (2000+ lines):
  - [x] PostgreSQL (postgres.rs - 450 lines):
    - [x] SERIAL/BIGSERIAL types for auto-increment
    - [x] Array types support (e.g., INTEGER[])
    - [x] JSONB support
    - [x] UUID type support
    - [x] Table and column comments
    - [x] Full constraint support
  - [x] MySQL (mysql.rs - 400 lines):
    - [x] AUTO_INCREMENT support
    - [x] ENGINE=InnoDB specification
    - [x] Inline column comments
    - [x] CHECK constraints (MySQL 8.0.16+)
  - [x] SQLite (sqlite.rs - 330 lines):
    - [x] Type affinity-based column types
    - [x] AUTOINCREMENT for INTEGER PRIMARY KEY
    - [x] Limited ALTER TABLE support (ADD COLUMN, RENAME COLUMN, DROP COLUMN)
    - [x] Clear error messages for unsupported operations
  - [x] SQL Server (sqlserver.rs - 350 lines):
    - [x] IDENTITY columns
    - [x] UNIQUEIDENTIFIER for UUIDs
    - [x] Schema prefix support (defaults to dbo)
    - [x] sp_rename for column renaming
- [x] Tauri commands (src-tauri/src/commands/ddl.rs - 250 lines):
  - [x] preview_create_table - Generate SQL without executing
  - [x] create_table - Create and execute
  - [x] preview_alter_table - Preview modifications
  - [x] alter_table - Execute modifications
  - [x] preview_drop_table - Preview table drop
  - [x] drop_table - Execute table drop
- [x] Foreign key support:
  - [x] ON DELETE/UPDATE actions (CASCADE, SET NULL, RESTRICT, NO ACTION, SET DEFAULT)
  - [x] Named constraints with auto-generation
- [x] Generate preview SQL for all DDL operations
- [ ] Transaction support for complex schema changes (TODO: Future enhancement)

**Frontend (React Components):**

- [x] TableCreationDialog component (src/components/TableCreationDialog.tsx - 450 lines):
  - [x] Multi-step wizard (Basic Info → Columns → Constraints → Preview)
  - [x] Column builder with type dropdown (9 common types)
  - [x] Primary key checkbox (single and composite support)
  - [x] Auto-increment toggle
  - [x] Nullable toggle
  - [x] Add/remove columns dynamically
  - [x] SQL preview panel with syntax highlighting
  - [x] Form validation (table name required, column names required)
  - [x] IF NOT EXISTS toggle
  - [ ] Foreign key builder UI (TODO: Future enhancement - types ready)
  - [ ] Index builder UI (TODO: Future enhancement - types ready)
- [x] SchemaExplorer integration:
  - [x] "Create Table" context menu on schema
  - [x] "Refresh Tables" context menu on schema
  - [x] Auto-refresh tables after creation
- [x] TableEditor component for existing tables (TableEditDialog: add/drop/rename columns, toggle nullability, SQL preview)
- [x] ConfirmationDialog for destructive operations (ConfirmDestructiveDialog)

**TypeScript Types & API:**

- [x] TypeScript types (src/types/ddl.ts - 320 lines):
  - [x] Matching types for all Rust definitions
  - [x] ColumnTypes helper factory functions
  - [x] getColumnTypeLabel() for UI display
- [x] API module (src/api/ddl.ts - 120 lines):
  - [x] Typed wrappers for all 6 Tauri commands
  - [x] Full JSDoc documentation

**Safety & UX:**

- [x] Preview SQL before execution
- [x] Error handling with user-friendly messages
- [ ] Rollback support for failed operations (TODO: Future enhancement)
- [x] Warning dialogs for destructive actions (ConfirmDestructiveDialog, wired into drop-table)
- [x] Foreign key dependency checks (FK-referencing tables listed before drop)

**Integration:**

- [x] Context menu integration in SchemaExplorer
- [x] Auto-refresh after table creation
- [x] Add "Schema" menu to CustomTitlebar (New Table..., Refresh Metadata; shown only when connected)
- [ ] Add schema modification shortcuts to keyboard shortcuts modal (TODO: Future enhancement)
- [x] Update metadata cache after DDL operations (useMetadataCache event bus + SchemaExplorer listener)

**Implementation Date:** 2025-11-23

**Known Issues:**

- Debug logging currently active (will be removed after testing)
- Foreign key and unique constraint UI planned for future enhancement
- Advanced ALTER TABLE operations (change type, set NOT NULL) need frontend UI

**MongoDB Note:**

- DDL operations intentionally not supported (NoSQL database)
- Returns clear error message: "DDL operations not supported for MongoDB"

### Milestone 3.10: ERD (Entity Relationship Diagram) Builder Enhancements ✅ COMPLETED (2025-11-24)

**Visual Schema Designer**

- [x] Implement ERD canvas with:
  - [x] Draggable table boxes with position persistence
  - [x] Column lists inside each table box
  - [x] Visual relationship lines (one-to-one, one-to-many, many-to-many)
  - [x] Primary/foreign key indicators with color coding
- [x] Add ERD toolbar:
  - [x] Auto-Layout algorithm with Dagre
  - [x] Toggle Relationships visibility
  - [x] Export to PNG
- [x] Zoom controls (minimap with React Flow)
- [x] Grid-based canvas with snap-to-grid
- [x] Performance optimizations (React.memo, useCallback, useMemo)
- [x] Visual enhancements with cardinality indicators

**Implementation Date:** 2025-11-24

### Milestone 3.12: Automatic Update System ✅ COMPLETED (2025-11-24)

**Auto-Update with System Notifications**

- [x] Install and configure tauri-plugin-notification
- [x] Install and configure tauri-plugin-updater and tauri-plugin-process
- [x] Implement automatic update checking with configurable interval
- [x] Add system notifications for all update events:
  - [x] Update available notification
  - [x] Download started notification
  - [x] Download progress tracking
  - [x] Update ready notification
  - [x] Auto-install with restart notification
- [x] Create settings for auto-update preferences:
  - [x] Enable/disable automatic update checking
  - [x] Configurable check interval (1-168 hours)
  - [x] Optional auto-download
  - [x] Optional auto-install with 3-second restart delay
- [x] Implement useAutoUpdater hook with:
  - [x] Background periodic checking
  - [x] Notification permission handling
  - [x] Download and install logic
  - [x] Progress tracking
- [x] Add Settings UI for update configuration
- [x] Update TypeScript types and defaults

**Implementation Date:** 2025-11-24

### Milestone 3.4: Native Window & System Tray ✅ COMPLETED (2025-11-21)

**Custom Window Decorations & Tray Integration**

- [x] Enable tray-icon feature in Cargo.toml
- [x] Configure custom window titlebar (decorations: false)
- [x] Implement system tray icon with application icon
- [x] Create system tray menu with:
  - [x] Show/Hide Window action
  - [x] Open Settings action
  - [x] Separator
  - [x] Quit action
- [x] Handle tray menu events (show/hide, settings, quit)
- [x] Handle tray click events (double-click to show window)
- [x] Window minimize-to-tray behavior
- [x] Native window feel with custom controls

### Milestone 3.5: Settings & Configuration ✅ COMPLETED (2025-11-20)

**Application Settings Page** (discovered from redesign.md)

- [x] Build settings page with sidebar navigation: ✅
  - [x] General settings (language, default database, startup behavior) ✅
  - [x] Theme settings (Dark/Light/Auto, accent color, editor font) ✅
  - [x] Keyboard Shortcuts display (read-only for now) ✅
  - [x] Query Execution Settings (timeout, max rows, auto-commit, confirmDestructive) ✅
  - [ ] Backup & Import/Export preferences (TODO: Future enhancement)
  - [ ] Plugins management (TODO: Future enhancement)
- [ ] Add search bar for quick settings filtering (TODO: Future enhancement)
- [x] Use card layout with toggles, dropdowns, text inputs ✅
- [ ] Implement keyboard shortcuts customization modal (TODO: Future enhancement)

### Milestone 3.6: Logs & Activity Monitor ✅ COMPLETED

**Session Monitoring Dashboard**

- [x] Build activity logging backend:
  - [x] QueryLog type with full metadata (connection, database, SQL, type, status, duration, row count, errors)
  - [x] ActivityLogger with thread-safe RwLock for concurrent access
  - [x] Automatic query logging on all execute_query calls
  - [x] Configurable retention period (default 7 days)
  - [x] SQL query type auto-detection (SELECT, INSERT, UPDATE, etc.)
- [x] Build query execution logs UI:
  - [x] QueryLogTable with TanStack Table (expandable rows, sortable columns)
  - [x] Pagination controls (10, 25, 50, 100 per page)
  - [x] Query log filtering (connection, database, query type, status, duration, date range, search)
  - [x] Query log statistics dashboard (total queries, failed count, avg duration, breakdowns)
- [x] Implement comprehensive filters:
  - [x] Filter by connection, database, query type, status
  - [x] Duration range filter (min/max ms)
  - [x] Date range filter
  - [x] Search text in SQL queries
  - [x] Active filter count indicator
- [x] Export functionality:
  - [x] Export to JSON format
  - [x] Export to CSV format
  - [x] Export to TXT format
  - [x] Tauri command for file system export
- [x] Activity statistics:
  - [x] Total queries, failed queries, average duration, total rows
  - [x] Breakdown by query type (SELECT, INSERT, UPDATE, etc.)
  - [x] Breakdown by status (completed, failed, running, cancelled)
- [x] UI Integration:
  - [x] Split view layout with resizable panels
  - [x] Auto-refresh with configurable intervals (5s, 10s, 30s)
  - [x] Manual refresh button
  - [x] Clear logs button with confirmation
  - [x] Color-coded badges for query types and statuses
  - [x] Added to View menu in CustomTitlebar
  - [x] Full route integration at /activity

**Implementation Date:** 2025-11-22

**Deferred Features:**

- [x] Active queries list with real-time updates (PostgreSQL/MySQL via `pg_stat_activity` / `information_schema.processlist`; 2s polling, kill-query support)
- [x] Server metrics chart (connections, active, tx/sec) via `pg_stat_database` / `SHOW GLOBAL STATUS` with rolling 60-sample recharts line chart
- [x] Process list table (database-side session queries with cancel action)
- [ ] Host-level CPU/memory usage chart (would require OS-level metrics collection; server-side DB metrics implemented instead)

### Milestone 3.7: Plugin System ✅ COMPLETED (2025-11-28)

**Extensible Plugin Architecture with JavaScript Runtime**

- [x] Plugin Runtime with boa_engine (pure Rust JS interpreter):
  - [x] Sandboxed JavaScript execution environment
  - [x] IIFE wrapper pattern for export capture
  - [x] Permission-based API access control
  - [x] Resource limits and sandbox isolation
- [x] Plugin Manager:
  - [x] Install/uninstall plugins from marketplace
  - [x] Enable/disable installed plugins
  - [x] Bundled plugin discovery and copying
  - [x] Plugin configuration storage
- [x] DBHive JavaScript API:
  - [x] File read/write (sandboxed to plugin data dir)
  - [x] Key-value storage API
  - [x] Clipboard read/write (arboard)
  - [x] HTTP requests (reqwest blocking)
  - [x] Notification system
  - [x] UI component registration (toolbar, context menu)
- [x] Plugin Marketplace UI:
  - [x] Category filtering and search
  - [x] Plugin cards with stats (downloads, rating)
  - [x] Install/uninstall buttons
  - [x] Sort options (Popular, Recent, Rating)
- [x] Installed Plugins UI:
  - [x] Plugin list with enable/disable toggles
  - [x] Plugin details view (permissions, stats, keywords)
  - [x] Settings dialog for plugin configuration
  - [x] Run button for plugin actions
  - [x] Uninstall confirmation
- [x] Plugin Action Handlers:
  - [x] PluginContext for tracking registered UI components
  - [x] PluginToolbar for toolbar buttons
  - [x] usePluginContextMenu hook for context menus
  - [x] Event listeners for plugin notifications
- [x] CSV Exporter Plugin (bundled):
  - [x] Export query results to CSV
  - [x] Configurable delimiter, headers, quoting
  - [x] Toolbar button and context menu registration
- [x] Plugin Development Documentation

**Implementation Date:** 2025-11-28

### Milestone 3.8: Keyboard Shortcuts Cheat Sheet ✅ COMPLETED (2025-11-23)

**Interactive Shortcuts Guide**

- [x] Build keyboard shortcuts modal:
  - [x] Grouped by category (Editor, Navigation, Welcome Screen)
  - [x] Card-based layout with organized sections
  - [x] Monospace font for shortcut keys with platform detection
  - [x] Search bar for filtering shortcuts with real-time results
  - [x] Platform-specific shortcuts (Cmd vs Ctrl) auto-detected
- [x] Add "?" hotkey to open shortcuts modal globally
- [x] Add menu item in Help menu with keyboard hint
- [x] Comprehensive shortcut definitions for all implemented shortcuts
- [x] Empty state for search with no results
- [x] Platform indicator footer showing current OS
- [ ] Make shortcuts customizable (TODO: Future enhancement - link to Settings)

### Milestone 3.9: Error & Empty States ✅ COMPLETED (2025-11-24)

**Friendly Error Handling & Professional Empty States**

- [x] Design and implement error state components:
  - [x] `ErrorState` - Reusable base component with 3 variants (error/warning/info)
  - [x] `ConnectionLostError` - Database connection failure with reconnect actions
  - [x] `QueryErrorState` - Professional query error display with collapsible details
  - [x] Soft CSS animations (fade-in, slide-up, zoom-in)
  - [x] Minimal, friendly text with helpful tips
- [x] Create empty state components:
  - [x] `NoConnectionsEmpty` - No connections saved
  - [x] `NoHistoryEmpty` - No query history
  - [x] `NoTablesEmpty` - No tables in database
  - [x] `NoSearchResultsEmpty` - No search results
  - [x] `NoResultsEmpty` - No query results
  - [x] `NoDataEmpty` - No table data
- [x] Integration into existing components:
  - [x] ConnectionList and ConnectionDashboard
  - [x] HistoryPanel
  - [x] ResultsViewer
  - [x] SchemaExplorer
  - [x] EnhancedConnectionList
- [x] Design features:
  - [x] Color-coded icons for visual recognition
  - [x] Three size variants (sm, md, lg)
  - [x] Dark/light theme support
  - [x] Responsive, mobile-friendly layouts
  - [x] WCAG 2.1 AA accessibility compliant
  - [x] Smart detection of search/filter states
- [x] Comprehensive documentation with examples and API reference

**Implementation Date:** 2025-11-24

### Milestone 3.10: About Page ✅ COMPLETED (2025-11-24)

**Application Information**

- [x] Create About page (`/about` route):
  - [x] DB-Hive logo matching titlebar branding (centered)
  - [x] Version number (0.7.0-beta) with badge styling
  - [x] App tagline: "A Professional Cross-Platform Database Client"
  - [x] Contributors & Core Team section with "Built with Claude Code" badge
  - [x] GitHub repository link
  - [x] Documentation link (GitHub wiki)
  - [x] Report Issues link (GitHub issues)
  - [x] License information (MIT) with copyright year
  - [x] Third-party credits for 10+ major dependencies
- [x] Professional card-based layout with centered typography
- [x] "Check for Updates" button with toast notification
- [x] Integration into Help menu in CustomTitlebar
- [x] Smart back button with route persistence
- [x] Dark/light theme support
- [x] Responsive design
- [x] External links open in new tabs

**Implementation Date:** 2025-11-24

### Milestone 3.11: Auto-Update System ✅ COMPLETED (2025-11-24)

**Automatic Application Updates**

- [x] Install and configure Tauri updater plugin:
  - [x] Add `tauri-plugin-updater` (v2.9.0) to Rust dependencies
  - [x] Add `@tauri-apps/plugin-updater` to frontend dependencies
  - [x] Initialize updater plugin in `lib.rs`
- [x] Configure `tauri.conf.json`:
  - [x] Enable `createUpdaterArtifacts: true` for build process
  - [x] Set update endpoint to GitHub releases (`latest.json`)
  - [x] Add public key field for signature verification
- [x] Implement update check functionality in About page:
  - [x] Replace placeholder toast with real update checking
  - [x] Add loading states (checking, downloading)
  - [x] Visual feedback with spinner and download icon
  - [x] Toast notifications for all update states
  - [x] Automatic download and installation
  - [x] App relaunch after successful update
- [x] Error handling:
  - [x] Network error handling
  - [x] Download failure handling
  - [x] Installation error handling with user-friendly messages
- [x] User experience:
  - [x] Button disabled during operations
  - [x] Progress indicators
  - [x] Clear success/error messaging
  - [x] Non-blocking UI (toast notifications)

**Files Modified:**

- `src-tauri/src/lib.rs` - Added updater plugin initialization
- `src-tauri/tauri.conf.json` - Added updater configuration
- `src/routes/about.tsx` - Implemented update check logic
- `src-tauri/Cargo.toml` - Added tauri-plugin-updater dependency
- `package.json` - Added @tauri-apps/plugin-updater dependency

**Notes:**

- Signing keys need to be generated for production releases: `npm run tauri signer generate`
- Update artifacts will be created automatically during GitHub release builds
- Updater checks GitHub releases for `latest.json` manifest
- Signature verification ready (requires public key configuration)

**Implementation Date:** 2025-11-24

### Plugin System ✅ COMPLETED

- [x] Design plugin architecture (JS with boa_engine)
- [x] Create plugin API (DBHive JS API)
- [x] Build plugin manager UI (Marketplace + Installed)
- [x] Develop example plugins (CSV Exporter)

### Workspace Sync

- [ ] Design cloud sync architecture
- [ ] Implement E2E encryption
- [ ] Build sync UI
- [ ] Add conflict resolution

### Visual Query Builder

- [ ] Design drag-and-drop interface
- [ ] Implement query generation
- [ ] Support JOINs, WHERE, GROUP BY
- [ ] Add preview mode

### Milestone 3.15: Visual Schema Designer (Database Builder) ✅ COMPLETED (2026-05-15)

**Drag-and-Drop Database Schema Creation**

- [x] **Canvas & Table Creation:**
  - [x] ReactFlow-based canvas (reuses ERD ReactFlow infrastructure)
  - [x] "+ Add Table" button drops new table node at staggered canvas position
  - [x] Draggable table nodes with position sync back to state
  - [x] Delete table button (×) in node header
  - [x] MiniMap + Controls for zoom/pan

- [x] **Table Node Display:**
  - [x] `TableSchemaNode` custom ReactFlow node (280px wide)
  - [x] Header: primary color background + table name + delete button
  - [x] Column rows: PK key icon, FK link icon, type label, UNQ/NN badges
  - [x] ReactFlow handles on left/right for FK edge drawing

- [x] **Right Panel Properties Editor:**
  - [x] Table name + schema inputs
  - [x] Column list: name, type dropdown (12 types), PK/nullable/unique toggles
  - [x] FK reference inputs (references table + column)
  - [x] Add/remove column buttons

- [x] **Relationship Builder:**
  - [x] FK edges auto-drawn when `referencesTable` set on a column
  - [x] Animated edges with primary color
  - [x] ReactFlow `onConnect` for manual edge drawing

- [x] **SQL Generation:**
  - [x] "Preview SQL" → calls `preview_create_table` for each table → shows in Dialog
  - [x] "Create Tables" → calls `create_table` sequentially with success/error toasts
  - [x] Maps 12 designer types to backend `ColumnType` enum

- [x] **UI/UX:**
  - [x] 3-panel layout: table list (200px) + canvas + properties (300px)
  - [x] Left panel table list with click-to-select
  - [x] Route: `/_connected/visual-schema-designer`
  - [x] Accessible from titlebar "Schema Designer" menu item

- [ ] Undo/redo support (TODO: future)
- [ ] Index designer (TODO: future)
- [ ] Check constraints editor (TODO: future)
- [ ] Cardinality indicators (TODO: future)
  - [ ] Not null constraints
  - [ ] Default value expressions

- [ ] **Schema Generation:**
  - [ ] Real-time DDL preview panel
  - [ ] Database-specific SQL generation (PostgreSQL, MySQL, SQLite, SQL Server)
  - [ ] CREATE TABLE ordering based on dependencies
  - [ ] Foreign key dependency resolution
  - [ ] "Generate SQL" button to copy/export
  - [ ] "Execute Schema" to create all tables

- [ ] **Project Management:**
  - [ ] Save schema designs as projects
  - [ ] Load/edit existing designs
  - [ ] Export design to JSON
  - [ ] Import from existing database (reverse engineer)
  - [ ] Version history for designs

- [ ] **UX Features:**
  - [ ] Grid snap for alignment
  - [ ] Auto-layout algorithm
  - [ ] Zoom and pan controls
  - [ ] Minimap for large schemas
  - [ ] Table grouping/coloring by domain
  - [ ] Search/filter tables
  - [ ] Keyboard shortcuts

**Technical Notes:**
- Reuse ReactFlow from ERD viewer
- Reuse DDL generation from Milestone 3.3
- Extend TableDefinition types for visual metadata (position, color)
- Store designs in Tauri Store or separate files

### Schema Migration Tools ✅ COMPLETED (2026-04-21)

- [x] Schema diff algorithm (pure `compute_diff(source, target) -> SchemaDiff`)
- [x] Generate migration SQL (driver-aware, reuses existing DDL generators)
- [x] Apply migrations UI (`<MigrationsDialog>` with diff preview + Monaco SQL preview + transactional apply)
- [x] Command palette entry ("Schema Migrations...")
- [ ] Version control integration (TODO: out of scope for initial pass)

**Driver coverage:**
- PostgreSQL / Supabase / Neon — full
- MySQL — columns, indexes, FKs (no standalone nullable/default toggle)
- SQL Server — type + nullable via `ALTER COLUMN`
- SQLite / Turso — CREATE/DROP, ADD/DROP column, indexes, FKs (no ALTER COLUMN TYPE)
- MongoDB — rejected (`InvalidInput`)

**Known gaps:** no column-rename detection (treated as drop+add), no check-constraint diffing, no view/trigger/sequence/procedure diffing, no FK `ON DELETE` action change detection, cross-table FKs between two newly-added tables omitted.

### Milestone 3.14: AI Assistant ✅ COMPLETED (2025-11-29)

**Multi-Provider AI-Powered SQL Assistant**

- [x] Provider-agnostic architecture:
  - [x] AiProvider trait for common interface
  - [x] Pluggable provider system
  - [x] Per-provider configuration
- [x] Ollama Integration (Local LLM):
  - [x] Local model discovery and selection
  - [x] No API key required (privacy-first)
  - [x] Support for llama3.2, codellama, mistral, etc.
- [x] OpenAI Integration:
  - [x] GPT-4o, GPT-4, GPT-3.5 Turbo support
  - [x] Automatic model listing from API
  - [x] Context window detection
- [x] Anthropic (Claude) Integration:
  - [x] Claude Sonnet 4, Claude 3.5 Sonnet, Claude 3 Opus support
  - [x] System message handling (separate parameter)
  - [x] 200K context window models
- [x] Google (Gemini) Integration:
  - [x] Gemini 2.0 Flash, Gemini 1.5 Pro/Flash support
  - [x] 1M+ context window models
  - [x] Automatic model discovery
- [x] AI Features:
  - [x] Natural language to SQL generation
  - [x] Query explanation in plain English
  - [x] Query optimization suggestions
  - [x] Error fixing with context
  - [x] General chat capabilities
- [x] Frontend UI:
  - [x] Provider selection dropdown
  - [x] Model selection per provider
  - [x] API key management (settings panel)
  - [x] Status indicators (connected, needs key, offline)
  - [x] Schema context viewer
  - [x] Quick action buttons (Explain, Optimize, Fix)
  - [x] Result display with "Apply to Editor" button
- [x] Schema Context:
  - [x] Automatic schema loading on connection
  - [x] Full table/column context for accurate SQL
  - [x] System schema filtering (pg_catalog, information_schema, etc.)

**Implementation Date:** 2025-11-29

### Milestone 3.13: Additional Database Drivers

**Cloud & Serverless Databases**

- [x] **Supabase Driver:** (2026-04-21)
  - [x] PostgreSQL-compatible connection via Supabase URL
  - [x] SSL/TLS enabled by default for cloud connections (native-tls + postgres-native-tls)
  - [x] Reuse existing PostgreSQL driver infrastructure
  - [ ] Connection pooler awareness (Transaction/Session modes) — user-selected by host (pooler vs direct)

- [x] **Neon Driver:** (2026-04-21)
  - [x] PostgreSQL-compatible serverless connection
  - [x] SSL required by default (Neon endpoints mandate TLS)
  - [x] Reuse existing PostgreSQL driver infrastructure
  - [ ] Cold start handling and dedicated pooling tuning

- [x] **Turso Driver:** (2026-04-21)
  - [x] libSQL/SQLite edge database support via `libsql` crate 0.9
  - [x] Remote connection via `libsql://` / `https://` URL
  - [x] Authentication token handling (stored in OS keyring)
  - [x] Form adaptations: URL-accepting host, renamed "Auth Token" password field, hidden username
  - [x] Routes through `SqliteDdlGenerator` in migration + DDL paths
  - [ ] Embedded replica support (TODO: future)

- [x] **Redis Driver:** ✅ COMPLETED (2026-05-15)
  - [x] `DbDriver::Redis` variant, default port 6379
  - [x] `redis` crate 0.27 with `tokio-comp` + `connection-manager` features
  - [x] `MultiplexedConnection` via `Arc<Mutex<...>>` for thread-safe async access
  - [x] Key-value store operations via raw Redis command string execution
  - [x] Data structure types mapped as pseudo-tables: strings, hashes, lists, sets, zsets
  - [x] `get_databases` returns logical DBs 0-15; `get_schemas` returns virtual "keys" namespace
  - [x] `get_tables` samples key types via SCAN; `get_table_schema` returns conceptual column layout per type
  - [x] Wired into `test_connection_command` + `connect_to_database` in `commands/connection.rs`
  - [ ] Key browser and data viewer UI (TODO: future)
  - [ ] Cluster and Sentinel support (TODO: future)

**Frontend Integration:**

- [ ] Connection form updates for each driver type
- [ ] Driver-specific icons and branding
- [ ] Connection testing for cloud databases
- [ ] SSL certificate handling UI

**Notes:**

- Supabase and Neon can leverage existing PostgreSQL driver
- Turso requires libsql Rust crate integration
- Redis requires custom UI (non-relational data model)

---

---

## Status & Next Steps

**Current Status:** Phase 2 in progress - Advanced features being implemented

**Completed:**

- ✅ Phase 0: Architecture & Setup
- ✅ Phase 1: MVP Development (Connection management, SQL editor, schema browser, table editor)
- ✅ Milestone 2.1-2.5: MySQL/MongoDB drivers, autocomplete, table editing, SQL import/export
- ✅ Milestone 2.6: Query Plan Visualizer (2025-11-21)
- ✅ Milestone 2.7: ER Diagram Generator (2025-11-20)
- ✅ Milestone 2.9: Router Migration with TanStack Router (2025-11-21)
- ✅ Settings & Configuration (2025-11-20)
- ✅ Native Window & System Tray (2025-11-21)
- ✅ **Custom Window Titlebar & Menu System** (2025-11-21)
- ✅ **Global Keyboard Shortcuts System** (2025-11-21)
- ✅ **Window State Persistence** (2025-11-21)

**Recently Completed:**

- ✅ **Backup Manager UI** — `<BackupManagerDialog>` over the existing backup commands. Titlebar View-menu entry via `useAppModal` (`"backup"`, rendered in `GlobalModals`). Directory header (Open Folder/Refresh), create panel (schema/data toggles, note, slow-op loading), backups table with per-row Restore (destructive confirm + drop-existing checkbox) and Delete. (2026-05-17)
- ✅ **Redis Key Browser (left sidebar)** — For Redis connections SchemaExplorer renders `<RedisSchemaTree>` instead of the relational tree: five key-type groups lazy-loaded via type-filtered `SCAN … TYPE`, search-box glob, "Load more…". Clicking a key opens `<RedisValuePanel>` in a main-area tab (new `"redis"` TabContext type, `rediskey-` tab id). Replaced the earlier titlebar-menu `/redis-keys` two-pane browser. (2026-05-17)
- ✅ **Foreign-Key Drill-Down** — Table Inspector FK cells expose an external-link button + "Open Referenced Record" context entry; opens the referenced table in a new tab pre-filtered to the related row via a self-describing `tablefk-` tab id (URL-sync recreation rebuilds schema/table/column/value + filter). (2026-05-17)
- ✅ **Redis Driver** — `DbDriver::Redis` variant with `redis` crate 0.27. `MultiplexedConnection` via `Arc<Mutex<...>>`. Key-type pseudo-tables (strings/hashes/lists/sets/zsets) via SCAN sampling. Raw Redis command execution (GET, SET, HGETALL, etc.). Wired into `test_connection_command` + `connect_to_database`. Default port 6379. (2026-05-15)
- ✅ **Visual Schema Designer** — ReactFlow 3-panel drag-and-drop schema builder. Custom `TableSchemaNode` with column display (PK/UNQ/NN badges). Right-panel properties editor for table name, schema, 12 column types, FK references. FK edges auto-drawn between nodes. "Preview SQL" via `preview_create_table` + "Create Tables" via `create_table`. Route `/_connected/visual-schema-designer`, accessible from titlebar. (2026-05-15)
- ✅ **Backup Manager** — `commands/backup.rs` with 6 commands: `get_backup_directory`, `list_backups`, `create_backup` (pg_dump / mysqldump / sqlite copy / mongodump subprocess), `restore_backup` (psql / mysql / sqlite copy), `delete_backup`, `open_backup_directory`. Data models in `models/backup.rs` (BackupEntry, BackupOptions, BackupStatus, RestoreOptions, BackupProgress). (2026-05-15)
- ✅ **Stored Procedures & Functions Viewer** — New commands `list_procedures`, `get_procedure_definition`, `execute_procedure` (PG via `pg_proc` + `pg_get_functiondef`, MySQL via `information_schema.ROUTINES` + `SHOW CREATE`, SQL Server via `sys.objects`). `<StoredProceduresPanel>` lazy-loads definitions and offers an execute dialog with JSON args (2026-04-21)
- ✅ **Write-Query Guards (dbpro parity)** — Pre-execute analyzer intercepts destructive SQL (DELETE/UPDATE without WHERE, DROP/TRUNCATE/DROP COLUMN); `<DestructiveQueryGuard>` AlertDialog; wired through `_connected/query.tsx` so Ctrl+Enter is also guarded; respects `settings.query.confirmDestructive` (2026-04-21)
- ✅ **Query Folders (dbpro parity)** — Nested `SnippetFolder` tree in SnippetSidebar with context-menu create/rename/delete/move, cycle prevention, cascade-delete-to-root, search auto-expand; persisted separately from snippets for backward compat (2026-04-21)
- ✅ **OpenRouter AI Provider (dbpro parity)** — New provider routing through OpenAI-compatible `openrouter.ai/api/v1` with FQID models like `anthropic/claude-3.5-sonnet`; settings entry in `AiAssistant` (2026-04-21)
- ✅ **Turso / libSQL Driver** — New `DbDriver::Turso` variant using `libsql` crate 0.9. Remote mode with auth-token auth. Routes through `SqliteDdlGenerator` for DDL and migrations. Form adapts for URL host + "Auth Token" field (2026-04-21)
- ✅ **Schema Migration Tools** — `src-tauri/src/migrations/` module with pure diff algorithm + driver-aware SQL generator. Tauri commands `compute_schema_diff`, `generate_migration`, `apply_migration`. `<MigrationsDialog>` 3-step flow wired to command palette (2026-04-21)
- ✅ **Activity Monitor — Live Process List + Server Metrics** — `get_active_queries`, `kill_query`, `get_server_stats` commands; PG via `pg_stat_activity`/`pg_stat_database`, MySQL via `PROCESSLIST`/`SHOW GLOBAL STATUS`; `<ProcessList>` (2s polling, per-row cancel) + `<ServerMetricsChart>` (60-sample rolling recharts) (2026-04-21)
- ✅ **Schema Management Deferred Items** — `<TableEditDialog>` for existing tables (add/rename/drop columns, toggle nullability, SQL preview); `<ConfirmDestructiveDialog>` with FK dependency listing + auto-CASCADE; `useMetadataCache` hook + `metadata-changed` CustomEvent; new "Schema" menu in titlebar (2026-04-21)
- ✅ **Supabase + Neon Drivers** — New `DbDriver::Supabase` and `DbDriver::Neon` variants route through the existing PostgreSQL driver. Added TLS support: `native-tls` + `postgres-native-tls` crates, `ConnectionOptions.require_tls` flag, auto-enabled for Supabase/Neon and when `ssl_mode == Require`. Frontend picker tiles activated, driver-specific connection-string placeholders added (Milestone 3.13 partial) (2026-04-21)
- ✅ **v0.19.2 Bug Fixes** — PostgreSQL connection password retrieval resilience (keyring fallback in `connect_to_database`, session cache in `save_password`) + pgvector/array type rendering: `vector` columns now deserialise correctly via the `pgvector` crate and all PostgreSQL array types (`_float4`, `_int4`, `_text`, etc.) emit JSON arrays instead of NULL (2026-03-10)
- ✅ Milestone 3.14: AI Assistant - Multi-provider AI with Ollama, OpenAI, Claude, Gemini support; natural language to SQL, query explanation, optimization, error fixing (2025-11-29)
- ✅ Import Wizards - Import from Excel/CSV with column mapping, data type detection, batch import (2025-11-28)
- ✅ Data Visualization - Interactive charts (bar, line, area, pie, scatter) from query results with recharts (2025-11-28)
- ✅ Query Templates - Save/load query templates with parameters, validation, and SQL preview (2025-11-28)
- ✅ Milestone 3.7: Plugin System - Full plugin architecture with boa_engine JS runtime, marketplace UI, plugin settings, action handlers, and CSV Exporter bundled plugin (2025-11-28)
- ✅ Milestone 3.12: Automatic Update System - Complete auto-update with system notifications, configurable intervals, auto-download, and auto-install with restart (2025-11-24)
- ✅ Milestone 3.10: ERD Builder Enhancements - Interactive ERD canvas with draggable tables, React Flow integration, auto-layout, performance optimizations, and visual enhancements (2025-11-24)
- ✅ Milestone 3.11: Auto-Update System - Tauri updater plugin integration with automatic update checking, download, installation, and app relaunch (2025-11-24)
- ✅ Milestone 3.10: About Page - Professional About page with app information, contributors, third-party credits, and update checker (2025-11-24)
- ✅ Milestone 3.9: Error & Empty States - Complete redesign of empty states and error displays with 10+ reusable components, smooth animations, and full integration across the application (2025-11-24)
- ✅ Milestone 3.3: Database Schema Management - Full DDL operations with multi-database support (PostgreSQL, MySQL, SQLite, SQL Server), visual table creation wizard, and SQL preview (2025-11-23)

**Next Priorities (Pick from these):**

1. **Database Comparison** - Compare schemas between two connections (dev/staging/prod diffs)
2. **Workspace Sync** - Cloud sync with E2E encryption for settings and connections
3. **Query Scheduler** - Schedule and automate recurring queries
4. **Performance Dashboard** - Real-time database performance metrics
5. **Visual Schema Designer Enhancements** - Undo/redo, index designer, cardinality indicators

**Documentation:**

- See `CLAUDE.md` for detailed architecture and development patterns
- See `README.md` for feature list and installation
- See `docs/user-guide.md` for complete user documentation
- See `CHANGELOG.md` for detailed release notes
