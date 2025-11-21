# Changelog

All notable changes to DB-Hive will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0-beta] - 2025-11-21

### Added

- **Tab Persistence Across App Restarts** (2025-11-21):
  - Tab states now restore automatically when reconnecting to a database
  - Previous tabs (queries and tables) persist in localStorage
  - Seamless experience when closing and reopening the app
  - First tab automatically selected on reconnection

- **Query Plan Visualizer Component** (2025-11-21):
  - New QueryPlanVisualizer component for displaying PostgreSQL EXPLAIN output
  - Tree visualization of query plan nodes with expand/collapse
  - Cost highlighting (green/yellow/red based on cost levels)
  - Detailed metrics display:
    - Startup and total costs
    - Estimated vs actual rows
    - Execution time per node
    - Index usage and conditions
  - Filter and join condition display
  - Support for both EXPLAIN and EXPLAIN ANALYZE
  - TypeScript types for QueryPlanNode and QueryPlanResult
  - JSON parser for PostgreSQL EXPLAIN (FORMAT JSON) output
  - Ready for integration into QueryPanel

- **TanStack Router Migration & Multi-Tab System** (2025-11-21):
  - **Router Implementation**:
    - Migrated from React Router to TanStack Router v1.139.0
    - Type-safe routing with compile-time validation
    - File-based routing structure with 10 routes
    - Search params validation with automatic type inference
    - Browser back/forward navigation support
    - Deep linking support for all routes

  - **Route Structure**:
    - `__root.tsx` - Root layout with theme and connection providers
    - `index.tsx` - Welcome screen
    - `connections.tsx` - Side-by-side connection management
    - `settings.tsx` - Settings page
    - `_connected/route.tsx` - Connected layout with navigation guard
    - `_connected/query.tsx` - Multi-tab query panel
    - `_connected/table.$schema.$tableName/` - Table inspector routes
    - `_connected/er-diagram.$schema.tsx` - ER diagram viewer
    - `_connected/visualization.tsx` - Visualization route

  - **Multi-Tab System**:
    - **URL-Based Tab Management**:
      - Tab state in URL search params: `?tabs=query-1,table-public.users&active=0`
      - Tab IDs: `query-{timestamp}` for queries, `table-{schema}.{tableName}` for tables
      - Comma-separated tab list with active index

    - **TabContext Implementation** (136 lines):
      - React Context API for per-tab state management
      - LocalStorage persistence per connection (`db-hive-tabs-${connectionId}`)
      - Automatic save/restore on connection change
      - Support for query and table tab types
      - Tab state includes: SQL content, filters, pagination, sorting

    - **Component Rendering Strategy**:
      - All tabs rendered simultaneously with absolute positioning
      - CSS show/hide instead of mount/unmount
      - Preserves component state across tab switches
      - No content loss when switching tabs
      - SQL editor content preserved
      - Table filters and pagination preserved

    - **Tab Bar UI**:
      - Tab pills with active/inactive states
      - Close button per tab (hidden if only 1 tab)
      - Plus button to add new query tabs
      - Hover states and smooth transitions
      - Tab labels derived from tab state

    - **Integration Features**:
      - Schema Explorer integration for opening tables
      - Checks for already-open tables before creating new tabs
      - Default query tab created on connection
      - Tab persistence across app restarts

  - **Code Quality Improvements**:
    - App.tsx reduced from 310 lines → 26 lines (92% reduction)
    - Removed 10+ useState hooks for navigation state
    - Removed 10+ handler functions
    - Type-safe navigation throughout application
    - Clean separation of concerns

  - **Benefits**:
    - ✅ No content loss on tab switching
    - ✅ URL state persistence
    - ✅ Browser navigation works (back/forward)
    - ✅ Deep linking support
    - ✅ Multi-table tabs fully functional
    - ✅ Tab state persists across app restarts

- **Native Window Enhancements & System Tray** (2025-11-21):
  - **Window Configuration**:
    - Added minimum window dimensions (1000x700px) for better usability
    - Window now centers on screen at startup
    - Configured window controls (resizable, maximizable, minimizable, closable)
    - Added explicit window behavior configuration (decorations, transparency, taskbar, always-on-top)

  - **System Tray Integration**:
    - System tray icon using application's default icon
    - Tray menu with Show/Hide and Quit actions
    - Click tray icon to toggle window visibility
    - Menu opens on right-click (left-click toggles window)
    - Window can be hidden to system tray instead of closing
    - Quick access to application from system tray

  - **Event Handling**:
    - Left-click tray icon shows/hides window with focus management
    - Menu events properly handle show/hide and quit actions
    - Smooth window visibility toggling with proper focus restoration

  - **Technical Implementation**:
    - Enabled `tray-icon` feature in Tauri dependencies
    - Implemented helper function for window visibility management
    - Used `TrayIconBuilder` with menu and event handlers
    - Integrated with existing application state and window management

## [0.5.0-beta] - 2025-11-21

### Fixed

- **Light Theme Compatibility** (2025-11-20):
  - Fixed text visibility issues in light mode across all pages
  - Root cause: Hardcoded dark theme colors (text-slate-100, bg-slate-900, etc.) in UI components
  - Solution: Systematically replaced all hardcoded colors with semantic CSS variables
  - Components updated with theme-aware colors:
    - `button.tsx` - bg-primary, text-primary-foreground, bg-accent, hover states
    - `input.tsx` - text-foreground, bg-background, border-input, focus states
    - `select.tsx` - SelectTrigger, SelectContent, SelectItem with semantic colors
    - `card.tsx` - bg-card, text-card-foreground
    - `dialog.tsx` - bg-background, border-border, close button styling
    - `label.tsx` - Removed hardcoded text-slate-200, uses inherited foreground color
  - All components now work perfectly in both light and dark themes
  - Semantic CSS variables automatically adapt based on theme mode

### Added

- **Settings & Configuration System** (2025-11-20):
  - **Backend Implementation** (369 lines):
    - Complete settings data model with 4 sub-sections
    - `AppSettings` struct with `GeneralSettings`, `ThemeSettings`, `QuerySettings`, `ShortcutsSettings`
    - Tauri commands: `get_settings`, `update_settings`, `reset_settings`
    - Persistent storage via Tauri Store plugin (settings.json)
    - Default settings implementation with unit tests
    - camelCase serialization for TypeScript compatibility

  - **Frontend Implementation** (691 lines):
    - **SettingsPage Component**: Comprehensive settings management UI
      - Sidebar navigation with 4 sections (General, Appearance, Query Execution, Keyboard Shortcuts)
      - Card-based layout with form controls
      - Save Changes and Reset to Defaults buttons
      - Toast notifications for user feedback

    - **General Settings Section**:
      - Language selector (English, Spanish, French, German)
      - Startup behavior options (Connection List, Last Connection, Default Connection, Query Editor)
      - Auto-save connections toggle
      - Telemetry opt-in toggle

    - **Appearance Settings Section**:
      - Theme mode selector (Light, Dark, System)
      - Accent color picker with hex color input
      - Editor font size slider (10-24px)
      - Editor font family input
      - Editor options: line numbers, minimap, word wrap toggles

    - **Query Execution Settings Section**:
      - Query timeout configuration (0-300 seconds, 0 = no timeout)
      - Max rows limit (100-100,000)
      - Auto-commit toggle
      - Confirm destructive queries toggle (DELETE/DROP confirmation)
      - Auto-save to history toggle
      - Max history entries limit (50-2,000)
      - Auto-format SQL toggle

    - **Keyboard Shortcuts Section**:
      - Read-only display of all shortcuts organized by category
      - Editor shortcuts: Execute Query, Clear Editor, Format SQL, Save Snippet
      - Navigation shortcuts: New Tab, Close Tab, Toggle Sidebar, Search, Open Settings, Show Shortcuts
      - Monospace badge display for shortcut keys

    - **Integration**:
      - Settings button in App top-right corner (next to theme toggle)
      - TypeScript types: `AppSettings`, `GeneralSettings`, `ThemeSettings`, `QuerySettings`, `ShortcutsSettings`
      - Type definitions: `StartupBehavior`, `ThemeMode`
      - `defaultSettings` object with complete default values

### Added
- **SQL Database Import/Export** (2025-11-20):
  - **Backend Implementation** (400+ lines):
    - Full database dump and restore functionality for PostgreSQL, MySQL, and SQLite
    - Export options: include/exclude DROP, CREATE, and INSERT statements
    - Table filtering: Export specific tables or entire schemas
    - Database-specific SQL syntax (PostgreSQL double quotes, MySQL backticks)
    - Transaction support for imports with ACID guarantees
    - Continue-on-error mode for partial import recovery
    - SQL file parsing with comment support and statement detection
    - Proper SQL value escaping (NULL, booleans, strings, numbers)
    - Tauri commands: `export_to_sql`, `import_from_sql`
    - Comprehensive error reporting per statement during import

  - **Frontend Implementation** (460+ lines):
    - **SqlExportDialog Component**: Comprehensive export configuration
      - File save picker integration with Tauri dialog plugin
      - Checkbox options for DROP, CREATE, INSERT statements
      - Schema/database selection input
      - Table filtering with comma-separated list
      - Real-time error feedback and validation
      - Toast notifications for success/error states

    - **SqlImportDialog Component**: Safe and flexible import
      - File selection with Tauri file open dialog
      - Transaction mode toggle (automatic rollback on error)
      - Continue-on-error mode for partial recovery
      - Success/error result display with statement counts
      - Import progress feedback with loading spinners
      - Disabled states during import operations

    - **Integration**:
      - Export/Import buttons added to SchemaExplorer component
      - Positioned below database selector for easy access
      - TypeScript types: `SqlExportOptions`, `SqlImportOptions`
      - camelCase conversion for Rust snake_case fields
      - Sonner toast integration for user notifications

- **SSH Tunneling** (Completed, 2025-11-21):
  - Complete SSH tunnel management system using russh library
  - Password and private key authentication methods
  - Automatic local port assignment for tunnel endpoints
  - Async port forwarding with bidirectional data transfer
  - Tunnel lifecycle management (create, close, check existence)
  - Support for OpenSSH and PEM format private keys
  - Task-based listener pattern with graceful shutdown
  - Frontend integration: Collapsible SSH configuration section in ConnectionForm
  - UI components: SSH host/port/username inputs, auth method selector, password/key file pickers
  - Automatic temporary tunnel creation for connection testing
  - Seamless integration with test_connection_command, connect_to_database, and disconnect_from_database
  - Fixed thread safety issues with Arc<Mutex<>> cloning pattern for async operations

- **ER Diagram Generator** (Completed, 2025-11-21):
  - **Interactive Entity-Relationship Diagram Visualization**:
    - ReactFlow integration with zoom (0.05x-1.5x), pan, drag, and interactive controls
    - Dagre automatic hierarchical layout algorithm with top-to-bottom flow
    - Optimized spacing: 180px horizontal, 250px vertical between nodes
    - Network-simplex ranker for optimal hierarchical positioning
    - Custom table nodes (300px width) with proper Handle components for edge connections
    - Smart column display: Max 10 columns with "+N more" overflow indicator
    - Junction table detection (M:N relationships) with blue theme and "M:N" badge
    - Hover effects and smooth shadow transitions on nodes
    - Professional PK/FK badges with rounded backgrounds (amber/blue)

  - **Performance & Visual Optimizations**:
    - Disabled edge animations for better performance with complex schemas (37+ relationships)
    - Smart minimap with color coding (blue for junction tables, amber for regular)
    - Custom background: Amber dots at 15% opacity
    - Smooth fitView animation (800ms) with proper timing (150ms delay)
    - Extended zoom range for detailed inspection and overview
    - No console logging for cleaner debugging experience

  - **UI Integration**:
    - Popover menu (⋮) in SchemaExplorer for space-efficient actions
    - "View ER Diagram", "Export SQL", "Import SQL" consolidated in dropdown
    - Positioned next to database selector for easy access
    - Control panel with Refresh and Export SVG buttons
    - Loading and error states with retry functionality

  - **Backend Foreign Key Introspection**:
    - PostgreSQL: `information_schema` joins with composite FK support
    - MySQL: `INFORMATION_SCHEMA.KEY_COLUMN_USAGE` with referential constraints
    - SQLite: `PRAGMA foreign_key_list` per-table iteration
    - MongoDB: Returns empty list (no foreign key support)
    - Handles circular references with greedy acyclicer algorithm

- **OS Keyring Credential Encryption (Critical Security Fix)**:
  - Passwords now stored securely in OS-native credential storage
  - macOS: Keychain integration
  - Windows: Credential Manager integration
  - Linux: Secret Service API (libsecret) integration
  - Service name: "com.dbhive.app"
  - Automatic migration from plaintext storage to keyring
  - Fallback mechanism for backward compatibility during migration
  - Full credential lifecycle: save, retrieve, delete, and check existence

- **Hierarchical Schema Browser Tree View**:
  - Replaced flat table list with collapsible schema tree structure
  - Expand/collapse functionality for each schema with visual indicators
  - Folder icons (open/closed) for schemas based on expansion state
  - Lazy loading: Tables loaded on-demand when schema is expanded
  - Per-schema loading indicators during table fetch
  - Drag-and-drop support: Drag table names directly to SQL editor
  - Enhanced search: Filter across both schemas and tables
  - Search auto-expands matching schemas to show relevant tables
  - Improved UX: Tables remain cached after loading until database switch
  - Clear visual hierarchy with indentation for tables under schemas

- **Add Row Functionality**:
  - "Add Row" button in table toolbar (visible only in edit mode)
  - New rows rendered at top of table with green visual indicator
  - Remove button (X) to delete new rows before commit
  - Smart auto-generation detection for id, created_at, updated_at columns
  - INSERT statement generation with proper NULL handling
  - Transaction support for mixed INSERT + UPDATE operations
- **Table Metadata Enhancements**:
  - Added `isAutoIncrement` field to `ColumnInfo` type for accurate auto-increment detection
  - PostgreSQL: Row count display in sidebar using `pg_class.reltuples` statistics
  - PostgreSQL: Column headers now visible for empty tables via prepared statement metadata
  - MySQL: Improved UUID column type detection using `COLUMN_TYPE` instead of `DATA_TYPE`

### Changed
- Increased table result limit from 20 to 35 rows per page
- Improved post-commit UX: "Review Changes" button now clears properly after commit
- **Transaction Preview UX**: New rows remain visible with green highlighting when "Review Changes" is opened

### Fixed
- **Auto-increment detection (Critical)**:
  - MySQL: Now reads `EXTRA` column to detect `AUTO_INCREMENT` attribute
  - PostgreSQL: Detects `SERIAL` types and `nextval()` in default values
  - SQLite: Detects `INTEGER PRIMARY KEY` and `AUTOINCREMENT` keyword
  - MongoDB: `_id` field correctly marked as auto-increment
  - Frontend now checks `isAutoIncrement` field first before fallback heuristics
- **UUID primary key handling (Critical)**:
  - UUID columns without database defaults now have UUIDs auto-generated in the application
  - Uses `crypto.randomUUID()` to generate RFC 4122 compliant v4 UUIDs
  - Only skips UUID columns if database has UUID generation function (e.g., `DEFAULT UUID()`)
  - Prevents "cannot be null" errors for UUID primary keys
- **Timestamp column handling (Critical)**:
  - Timestamp columns (`created_at`, `updated_at`) without defaults now have timestamps auto-generated
  - Uses `new Date().toISOString()` to generate current timestamp in SQL format
  - Only skips timestamp columns if database has default (e.g., `DEFAULT CURRENT_TIMESTAMP`)
  - Prevents "cannot be null" errors for required timestamp columns
- Auto-generated columns (id, created_at, updated_at) no longer cause "cannot be null" errors
- New rows now skip auto-increment and auto-timestamp columns in INSERT statements
- PostgreSQL boolean values now correctly generate unquoted literals (true/false instead of 'true'/'false')
- PostgreSQL tables now show row counts in sidebar (previously showed nothing)
- PostgreSQL empty tables now display column headers (previously showed blank headers)
- MySQL row counts now accurate (previously some tables showed 0 incorrectly)
- Transaction preview table now has checkbox column header (previously misaligned)
- New rows now visible in transaction preview panel (previously disappeared)
- discardChanges() now clears both cell edits and new rows

### Technical Details

#### Schema Browser Enhancements
- **SchemaExplorer Component Refactor** (`src/components/SchemaExplorer.tsx`):
  - Replaced schema selector dropdown with hierarchical tree view using Radix UI Collapsible
  - State management: `expandedSchemas` Set tracks which schemas are expanded
  - Lazy loading: `tablesBySchema` record stores tables per schema, loaded only when expanded
  - `loadingTablesForSchema` record tracks loading state per schema independently
  - `toggleSchemaExpansion()` function handles expand/collapse with automatic table fetch
  - Drag-and-drop: `draggable` attribute with `onDragStart` handler sets table name as plain text
  - Visual feedback: Folder icons (FolderOpen/FolderClosed) and chevron indicators
  - Enhanced filtering: `filteredSchemas` includes schemas matching by name or containing matching tables
  - `getFilteredTablesForSchema()` function filters tables within each schema
  - Refresh button now refreshes all expanded schemas instead of just selected one
  - Database switch clears `tablesBySchema` cache and resets expanded state

- **New UI Component**:
  - Added Collapsible component from shadcn/ui (`src/components/ui/collapsible.tsx`)
  - Uses @radix-ui/react-collapsible for accessible expand/collapse behavior

#### Security Implementation
- **Credential Manager Module** (`src-tauri/src/credentials/mod.rs`):
  - New `CredentialManager` struct with static methods for keyring operations
  - `save_password()`: Stores password in OS keyring using connection_id as key
  - `get_password()`: Retrieves password from OS keyring, returns `Option<String>`
  - `delete_password()`: Removes password from OS keyring, idempotent operation
  - `has_password()`: Checks if password exists in keyring
  - Uses `keyring` crate v3.6 for cross-platform compatibility
  - Proper error handling with `DbError::CredentialError` variant

- **Updated Connection Commands** (`src-tauri/src/commands/connection.rs`):
  - `get_saved_password()`: Now checks keyring first, falls back to in-memory store for migration
  - `save_password()`: Saves to keyring and removes from plaintext store automatically
  - `delete_connection_profile()`: Deletes password from keyring when profile is removed
  - Seamless migration path from plaintext to encrypted storage

- **Error Handling** (`src-tauri/src/models/error.rs`):
  - Added `CredentialError(String)` variant to `DbError` enum
  - Serializes as `"credential"` kind for frontend error handling
  - Comprehensive error messages for keyring operations

#### Type System Changes
- Added `isAutoIncrement: boolean` field to `ColumnInfo` interface (`src/types/database.ts`)
- Updated Rust `ColumnInfo` struct with `is_auto_increment` field (`src-tauri/src/models/metadata.rs`)

#### Backend Driver Changes
- **MySQL Driver** (`src-tauri/src/drivers/mysql.rs`):
  - Now reads `EXTRA` column from `information_schema.COLUMNS`
  - Sets `is_auto_increment = true` when EXTRA contains "auto_increment"
  - Changed UUID detection to use `COLUMN_TYPE` instead of `DATA_TYPE` for full type info
- **PostgreSQL Driver** (`src-tauri/src/drivers/postgres.rs`):
  - Detects `SERIAL`/`BIGSERIAL`/`SMALLSERIAL` types in data_type field
  - Detects `nextval()` and `sequence` in column default values for proper SERIAL detection
  - Enhanced detection logic to handle all PostgreSQL auto-increment patterns
  - Added row count fetching using `pg_class.reltuples` statistics
  - Fixed empty table column header extraction using prepared statements
- **SQLite Driver** (`src-tauri/src/drivers/sqlite.rs`):
  - Detects `INTEGER PRIMARY KEY` (implicit autoincrement in SQLite)
  - Detects `AUTOINCREMENT` keyword in data type
- **MongoDB Driver** (`src-tauri/src/drivers/mongodb.rs`):
  - Marks `_id` field as auto-increment (ObjectId auto-generated by MongoDB)

#### Frontend/Hook Changes
- Enhanced `useTableEditor` hook (`src/hooks/useTableEditor.ts`):
  - Added `NewRow` interface and state management for new rows
  - Implemented `addRow()`, `removeNewRow()`, and `updateNewRowValue()` functions
  - **Enhanced `shouldSkipColumnInInsert()`**:
    - Now checks `col.isAutoIncrement` field FIRST (most reliable)
    - UUID primary keys now only skipped if they have database default value
    - Timestamp columns now only skipped if they have database default value
    - Checks for CURRENT_TIMESTAMP, now(), getdate(), and other timestamp functions
    - Removed redundant primary key + numeric type check (now handled by backend)
  - **Enhanced `generateInsertStatements()`** with auto-generation:
    - Auto-generates UUIDs using `crypto.randomUUID()` for UUID columns without values
    - Auto-generates timestamps using `new Date().toISOString()` for timestamp columns
    - Smart column detection and proper type handling
    - Generates RFC 4122 compliant v4 UUIDs and SQL-formatted timestamps
  - Enhanced `generateUpdateStatements()` with proper boolean value formatting
  - Updated `discardChanges()` to clear new rows
  - Updated `getCellValue()` and `applyChange()` to handle negative row indices
  - Added comprehensive debug logging throughout INSERT generation
  - Fixed value formatting: booleans unquoted, strings quoted, numbers unquoted

#### Frontend UI Changes
- Updated `TableInspector.tsx`:
  - Added "Add Row" button in toolbar
  - Increased pageSize from 20 to 35 rows per page
  - Rendered new rows at top with green styling and remove buttons
  - Updated handleCommit to include INSERT statements
  - Enhanced TransactionPreview to show INSERT + UPDATE statements
  - **Fixed transaction preview table structure**:
    - Added checkbox column header in both table views (with/without row viewer)
    - Added checkbox cells for existing rows in all table views
    - Removed `editMode &&` condition from new row rendering (now always visible when present)
    - Fixed "Select All" checkbox to use `selectAll()` with no arguments
  - Added comprehensive debug logging in commit flow

#### Auto-Generation Detection Logic
- **Primary**: Checks `isAutoIncrement` field from backend drivers
- **Fallback heuristics**:
  - Detects columns with function defaults (uuid_generate, gen_random_uuid, current_timestamp, now(), getdate())
  - Skips UUID/GUID primary keys (char/varchar columns named id, uuid, guid, *_id)
  - Skips created_at/updated_at timestamp columns
- Handles integer auto-increment (MySQL AUTO_INCREMENT, PostgreSQL SERIAL, SQLite INTEGER PRIMARY KEY)
- Handles UUID primary keys with auto-generation functions
- Handles MongoDB ObjectId auto-generation

## [0.4.0] - 2025-11-20

### Added

#### Table Editor - Bulk Operations & Enhanced Transaction Preview

- **Row Selection with Checkboxes**:
  - Checkbox column in edit mode for multi-row selection
  - Select all checkbox in table header
  - Individual row checkboxes with visual feedback (highlighted rows)
  - Selection count badge in toolbar
  - Selection state management (toggle, select all, clear)

- **Bulk Delete Functionality**:
  - "Delete Selected" button (appears when rows are selected)
  - Confirmation dialog with row count and warning message
  - Transaction-based bulk deletion using primary keys
  - Database-specific DELETE transaction syntax
  - Auto-refresh after successful deletion
  - Error handling with detailed feedback

- **Enhanced Transaction Preview**:
  - **SQL Syntax Highlighting**:
    - Keywords (UPDATE, SET, WHERE, etc.) - blue/bold
    - String literals - orange
    - Numbers - green
    - Operators - default color
  - **Statistics Display**:
    - Modified rows count with database icon
    - Changed cells count with edit icon
    - Operation breakdown badges (UPDATE/INSERT/DELETE counts)
  - **Statement Type Badges**:
    - Color-coded badges per statement (UPDATE=blue, INSERT=green, DELETE=red)
    - Visual statement numbering
    - Improved readability with better formatting

- **New UI Components**:
  - Created `Checkbox` component using Radix UI primitives
  - AlertDialog for bulk delete confirmation

### Technical Details

#### Backend Changes
- Enhanced `useTableEditor` hook (`src/hooks/useTableEditor.ts`):
  - Added `generateDeleteStatements()` function
  - DELETE statement generation using primary key WHERE clauses
  - NULL value handling in WHERE clauses
  - Database identifier quoting support

#### Frontend Changes
- Updated `TransactionPreview.tsx`:
  - Added `highlightSQL()` function for syntax highlighting
  - New props: `totalChanges`, `modifiedRows`
  - Statistics bar with operation breakdown
  - Statement type detection and badge coloring

- Updated `TableInspector.tsx`:
  - Checkbox column header with select all functionality
  - Checkbox cells for individual row selection
  - Bulk delete handler with transaction support
  - AlertDialog for delete confirmation
  - Selection state tracking and visual feedback
  - Database-specific DELETE transaction syntax

- Created `checkbox.tsx`:
  - Radix UI checkbox primitive wrapper
  - Accessible with ARIA labels
  - Styled with Tailwind classes

#### New Dependencies
- `@radix-ui/react-checkbox@1.3.3`

### Changed
- Table rows in edit mode now show checkboxes before row numbers
- Selected rows have distinct background color (`bg-muted`)
- Toolbar dynamically shows selection count and delete button in edit mode

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
- **Passwords now stored securely in OS-native keyring** (as of Unreleased version)
- Automatic migration from plaintext to encrypted storage on password save
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
