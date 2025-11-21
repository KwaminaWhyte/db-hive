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

### Milestone 2.2: SSH Tunneling 🚧 IN PROGRESS
- [x] SSH tunnel manager with password/key auth
- [x] Tunnel lifecycle management
- [ ] Connection command integration (TODO)
- [ ] SSH configuration UI (TODO)

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

### Milestone 2.7: Visual Query Builder

**Drag-and-Drop SQL Query Builder**

- [ ] **Core Query Builder Engine:**
  - [ ] Query builder state management (tables, columns, joins, filters, sorting)
  - [ ] SQL generation from query builder state
  - [ ] Support for SELECT, INSERT, UPDATE, DELETE queries
  - [ ] Real-time SQL preview as user builds query

- [ ] **Visual Query Canvas:**
  - [ ] Table selector dropdown with schema organization
  - [ ] Draggable table cards showing columns and types
  - [ ] Visual join line creation (drag from column to column)
  - [ ] Join type selector (INNER, LEFT, RIGHT, FULL OUTER)
  - [ ] Auto-suggest joins based on foreign keys
  - [ ] Pan and zoom canvas controls

- [ ] **Column Selection & Transformation:**
  - [ ] Multi-select columns with checkboxes
  - [ ] Column alias input
  - [ ] Aggregate functions (COUNT, SUM, AVG, MIN, MAX)
  - [ ] DISTINCT toggle
  - [ ] Column ordering (drag to reorder)

- [ ] **Filter Builder:**
  - [ ] Visual WHERE clause builder with condition groups
  - [ ] Support for AND/OR logic operators
  - [ ] Condition operators (=, !=, >, <, >=, <=, LIKE, IN, IS NULL)
  - [ ] Value input with type validation
  - [ ] Nested condition groups
  - [ ] HAVING clause support for aggregations

- [ ] **Additional Features:**
  - [ ] GROUP BY column selection
  - [ ] ORDER BY with ASC/DESC toggle
  - [ ] LIMIT and OFFSET inputs
  - [ ] Switch between Query Builder and SQL Editor modes
  - [ ] Import SQL query to visual builder (SQL parser)
  - [ ] Export query builder state to JSON
  - [ ] Save query templates

- [ ] **UI/UX:**
  - [ ] Split view: Query Builder (top) + Generated SQL (bottom)
  - [ ] "Run Query" button to execute generated SQL
  - [ ] Syntax highlighting for generated SQL
  - [ ] Responsive layout for different screen sizes
  - [ ] Keyboard shortcuts for common actions

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

### Milestone 3.1: Connection Wizard (Multi-Step Flow)

**Enhanced Connection Setup** (discovered from redesign.md)

- [ ] Design multi-step wizard flow:
  - [ ] Step 1: Select Database Type (PostgreSQL, MySQL, SQLite, SQL Server, MongoDB)
  - [ ] Step 2: Enter Credentials (host, port, username, password)
  - [ ] Step 3: Advanced Options (SSL, SSH tunnel, connection timeout)
  - [ ] Step 4: Test Connection (verify connectivity)
  - [ ] Step 5: Save Connection (name, color tag, favorite)
- [ ] Add progress indicator with check marks for completed steps
- [ ] Implement validation at each step
- [ ] Add toggleable advanced options panel
- [ ] Create database type cards with icons

### Milestone 3.2: Connection Manager Dashboard

**Visual Connection Management** (discovered from redesign.md)

- [ ] Build dashboard page for managing all connections:
  - [ ] Left sidebar with categories (All, Favorites, Recently Used, Local, Cloud)
  - [ ] Grid/list view toggle
  - [ ] Connection cards showing:
    - [ ] Connection name and DB type icon
    - [ ] Host summary
    - [ ] Color label/tag
    - [ ] Status indicator (online/offline)
  - [ ] Search bar with filters
  - [ ] Top actions: Create Connection, Import/Export Connections
- [ ] Three-column responsive card grid
- [ ] Hover states and animations
- [ ] Drag-and-drop to organize connections

### Milestone 3.3: ERD (Entity Relationship Diagram) Builder

**Visual Schema Designer** (discovered from redesign.md)

- [ ] Implement ERD canvas with:
  - [ ] Draggable table boxes
  - [ ] Column lists inside each table box
  - [ ] Visual relationship lines (one-to-one, one-to-many, many-to-many)
  - [ ] Primary/foreign key indicators
- [ ] Add ERD toolbar:
  - [ ] Add Table button
  - [ ] Auto-Layout algorithm
  - [ ] Toggle Relationships visibility
  - [ ] Export to PNG/SVG
- [ ] Zoom controls (bottom right corner)
- [ ] Grid-based canvas with snap-to-grid
- [ ] Save/load ERD layouts

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

### Milestone 3.6: Logs & Activity Monitor

**Session Monitoring Dashboard** (discovered from redesign.md)

- [ ] Build database session monitor showing:
  - [ ] Active queries list with real-time updates
  - [ ] CPU usage chart (line graph)
  - [ ] Memory usage chart (line graph)
  - [ ] Query execution logs with filtering
- [ ] Add process list table with sorting
- [ ] Implement filters by query type, duration, status
- [ ] Export logs to file
- [ ] Technical but clean UI design

### Milestone 3.7: Plugin Marketplace

**In-App Plugin Discovery** (discovered from redesign.md)

- [ ] Design plugin marketplace page:
  - [ ] Three-column grid of plugin cards
  - [ ] Each card showing:
    - [ ] Plugin name and description
    - [ ] Category (Drivers, Themes, Tools, Export Formats)
    - [ ] Star rating and downloads count
    - [ ] Install/Uninstall button
  - [ ] Search bar with category filters
  - [ ] Sort options (Popular, Recent, Rating)
- [ ] Implement plugin installation flow
- [ ] Add plugin update notifications
- [ ] Create plugin developer documentation

### Milestone 3.8: Keyboard Shortcuts Cheat Sheet

**Interactive Shortcuts Guide** (discovered from redesign.md)

- [ ] Build keyboard shortcuts modal:
  - [ ] Grouped by category (Editor, Navigation, Query, Tables)
  - [ ] Grid layout with card-style sections
  - [ ] Monospace font for shortcut keys
  - [ ] Search bar for filtering shortcuts
  - [ ] Platform-specific shortcuts (Cmd vs Ctrl)
- [ ] Add "?" hotkey to open shortcuts modal
- [ ] Make shortcuts customizable (link to Settings)

### Milestone 3.9: Error & Empty States

**Friendly Error Handling** (discovered from redesign.md)

- [ ] Design error states:
  - [ ] "Connection Lost" page with bee icon holding broken cable
  - [ ] Reconnect and Go to Dashboard buttons
  - [ ] Soft animations for error states
  - [ ] Minimal, friendly text
- [ ] Add empty states for:
  - [ ] No connections saved
  - [ ] No query history
  - [ ] No tables in database
  - [ ] No search results
- [ ] Use illustrations and helpful calls-to-action

### Milestone 3.10: About Page

**Application Information** (discovered from redesign.md)

- [ ] Create About page showing:
  - [ ] DB Hive logo (centered)
  - [ ] Version number
  - [ ] Contributors & Core Team section
  - [ ] GitHub link
  - [ ] Documentation link
  - [ ] License information (MIT)
  - [ ] Third-party credits
- [ ] Centered typography with minimal layout
- [ ] Add "Check for Updates" button

### Plugin System

- [ ] Design plugin architecture (JS or WASM)
- [ ] Create plugin API
- [ ] Build plugin manager UI (see Milestone 3.6)
- [ ] Develop example plugins

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

### Schema Migration Tools

- [ ] Schema diff algorithm
- [ ] Generate migration SQL
- [ ] Version control integration
- [ ] Apply migrations UI

### AI Assistant

- [ ] Integrate OpenAI/Claude API
- [ ] Natural language to SQL
- [ ] Query explanation
- [ ] Optimization suggestions

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

**In Progress:**
- 🚧 Milestone 2.2: SSH Tunneling (infrastructure complete, UI integration pending)

**Next 3 Priorities:**
1. **Complete SSH Tunnel UI Integration** - Wire up SSH configuration form with tunnel manager backend, test connections through tunnels
2. **Connection Wizard & Dashboard** - Multi-step connection setup with visual feedback, connection testing, and profile management (Phase 3, Milestone 3.1)
3. **Visual Query Builder** - Drag-and-drop query construction interface with table/column selectors (Milestone 2.8 - now renamed from 2.7)

**Documentation:**
- See `CLAUDE.md` for detailed architecture and development patterns
- See `README.md` for feature list and installation
- See `docs/USER_GUIDE.md` for complete user documentation
- See `CHANGELOG.md` for detailed release notes
