# Connection Wizard & Dashboard Design

**Milestone 3.1** - Professional connection management with folders, recent connections, and multi-step wizard

## Overview

Transform the current connection management system into a professional dashboard with:
- Folder/group organization for connections
- Recent connections with quick access
- Multi-step connection wizard
- Connection statistics and health indicators
- Search, filter, and sort capabilities
- Visual feedback for connection status

---

## 1. Data Model Enhancements

### 1.1 Connection Metadata (Backend - Rust)

Add new fields to track connection usage:

```rust
// src-tauri/src/models/connection.rs
pub struct ConnectionProfile {
    // ... existing fields ...

    /// Folder/group name for organization (already exists)
    pub folder: Option<String>,

    /// Last connected timestamp (NEW)
    pub last_connected_at: Option<i64>, // Unix timestamp

    /// Total connection count (NEW)
    pub connection_count: u32,

    /// Favorite/starred status (NEW)
    pub is_favorite: bool,

    /// Connection color tag (NEW)
    pub color: Option<String>, // hex color like "#3b82f6"

    /// Notes/description (NEW)
    pub description: Option<String>,

    /// Created timestamp (NEW)
    pub created_at: i64,

    /// Last modified timestamp (NEW)
    pub updated_at: i64,
}
```

### 1.2 TypeScript Types

```typescript
// src/types/database.ts
export interface ConnectionProfile {
  // ... existing fields ...
  folder?: string | null;
  lastConnectedAt?: number | null;
  connectionCount: number;
  isFavorite: boolean;
  color?: string | null;
  description?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ConnectionFolder {
  name: string;
  color?: string;
  isExpanded: boolean;
  connectionCount: number;
}

export interface ConnectionStats {
  totalConnections: number;
  favoriteCount: number;
  recentCount: number;
  folderCount: number;
  mostUsedConnection?: ConnectionProfile;
}
```

---

## 2. Backend Commands (Rust)

### 2.1 New Commands

```rust
// src-tauri/src/commands/connection.rs

/// Update connection metadata after successful connection
#[tauri::command]
pub fn record_connection(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    // Update last_connected_at and increment connection_count
}

/// Toggle favorite status
#[tauri::command]
pub fn toggle_favorite(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    // Toggle is_favorite field
}

/// Update connection folder
#[tauri::command]
pub fn update_connection_folder(
    profile_id: String,
    folder: Option<String>,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    // Update folder field
}

/// Get connection statistics
#[tauri::command]
pub fn get_connection_stats(
    state: State<'_, Mutex<AppState>>,
) -> Result<ConnectionStats, DbError> {
    // Calculate and return statistics
}

/// Get recent connections (last 5-10)
#[tauri::command]
pub fn get_recent_connections(
    limit: usize,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ConnectionProfile>, DbError> {
    // Sort by last_connected_at and return top N
}

/// Duplicate connection profile
#[tauri::command]
pub fn duplicate_connection(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    // Create a copy with new ID and " (Copy)" suffix
}
```

---

## 3. UI Components

### 3.1 Connection Dashboard (Main View)

**Location**: `src/components/ConnectionDashboard.tsx`

**Layout**:
```
┌─────────────────────────────────────────────────────────────────┐
│  Connection Dashboard                    [Search...] [+ New]     │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  📊 Quick Stats                                                   │
│  ┌─────────┬─────────┬─────────┬─────────┐                      │
│  │ Total   │ Recent  │ Folders │ Favorite│                      │
│  │   12    │    3    │    2    │    4    │                      │
│  └─────────┴─────────┴─────────┴─────────┘                      │
│                                                                   │
│  ⭐ Recent Connections                                            │
│  ┌─────────────────────────────────────────┐                    │
│  │ 🟢 Production DB (PostgreSQL)            │ [Connect] [•••]   │
│  │ 🟢 Dev MySQL (MySQL)                     │ [Connect] [•••]   │
│  │ 🟢 Local SQLite (SQLite)                 │ [Connect] [•••]   │
│  └─────────────────────────────────────────┘                    │
│                                                                   │
│  📁 All Connections                          [List] [Grid] [⚙]  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ 📁 Production (3 connections) ▼                              ││
│  │   🟢 Main DB        PostgreSQL   localhost:5432  [Actions]   ││
│  │   🟢 Analytics DB   PostgreSQL   10.0.1.5:5432   [Actions]   ││
│  │   🔴 Archive DB     PostgreSQL   10.0.1.6:5432   [Actions]   ││
│  │                                                                │
│  │ 📁 Development (2 connections) ▼                             ││
│  │   🟢 Local Dev      MySQL        localhost:3306  [Actions]   ││
│  │   🟢 Test DB        SQLite       /path/to/db     [Actions]   ││
│  │                                                                │
│  │ 📁 Ungrouped (4 connections) ▼                               ││
│  │   ...                                                          │
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

**Features**:
- Quick stats cards (total, recent, folders, favorites)
- Recent connections section (last 5-10 connections)
- Folder-based organization with expand/collapse
- Connection status indicators (🟢 connected, 🔴 disconnected, 🟡 connecting)
- Search bar with instant filtering
- List/Grid view toggle
- Bulk actions (move to folder, delete multiple)

### 3.2 Connection Wizard (Multi-Step)

**Location**: `src/components/ConnectionWizard.tsx`

**Steps**:

#### Step 1: Database Type Selection
```
┌─────────────────────────────────────────────────────────┐
│  New Connection - Select Database Type      [1/3]       │
├─────────────────────────────────────────────────────────┤
│                                                           │
│  Choose the type of database you want to connect to:     │
│                                                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │    🐘    │  │    🐬    │  │    📦    │              │
│  │PostgreSQL│  │  MySQL   │  │  SQLite  │              │
│  │          │  │          │  │          │              │
│  └──────────┘  └──────────┘  └──────────┘              │
│                                                           │
│  ┌──────────┐  ┌──────────┐                             │
│  │    🍃    │  │    🪟    │                             │
│  │ MongoDB  │  │SQL Server│                             │
│  │          │  │          │                             │
│  └──────────┘  └──────────┘                             │
│                                                           │
│                               [Cancel]  [Next →]         │
└─────────────────────────────────────────────────────────┘
```

#### Step 2: Connection Details
```
┌─────────────────────────────────────────────────────────┐
│  New Connection - Connection Details       [2/3]        │
├─────────────────────────────────────────────────────────┤
│                                                           │
│  Connection Name: [Production Database            ]      │
│  Host:           [localhost                      ]       │
│  Port:           [5432                           ]       │
│  Username:       [postgres                       ]       │
│  Password:       [••••••••                       ] 👁     │
│  Database:       [myapp_production              ]       │
│                                                           │
│  Folder:         [📁 Production          ▼]             │
│  Color Tag:      [🔵 Blue    ▼]                         │
│                                                           │
│  [Test Connection]                                        │
│  ✅ Connection successful!                                │
│                                                           │
│                        [← Back]  [Cancel]  [Next →]      │
└─────────────────────────────────────────────────────────┘
```

#### Step 3: Advanced Options
```
┌─────────────────────────────────────────────────────────┐
│  New Connection - Advanced Options         [3/3]        │
├─────────────────────────────────────────────────────────┤
│                                                           │
│  SSL/TLS Settings:                                        │
│  ☑ Enable SSL  [Prefer ▼]                               │
│                                                           │
│  SSH Tunnel (Optional):                                   │
│  ☐ Use SSH Tunnel                                        │
│    SSH Host:     [ssh.example.com              ]         │
│    SSH Port:     [22                           ]         │
│    SSH User:     [ubuntu                       ]         │
│    Auth Method:  [Password ▼]                           │
│                                                           │
│  Connection Timeout:                                      │
│  [30] seconds                                             │
│                                                           │
│  Description (Optional):                                  │
│  [Production database for main application    ]          │
│                                                           │
│                        [← Back]  [Cancel]  [Finish]      │
└─────────────────────────────────────────────────────────┘
```

### 3.3 Connection Card Component

**Location**: `src/components/ConnectionCard.tsx`

**Two Variants**:

#### List View
```
┌────────────────────────────────────────────────────────┐
│ 🟢 [🔵] Production DB                    ⭐ [Connect]  │
│ PostgreSQL • localhost:5432 • postgres                 │
│ Last connected: 2 hours ago • 127 connections          │
│                                               [•••]     │
└────────────────────────────────────────────────────────┘
```

#### Grid View
```
┌────────────────────────────┐
│ 🟢 [🔵]              ⭐ [•••]│
│                             │
│      Production DB          │
│                             │
│     PostgreSQL              │
│   localhost:5432            │
│                             │
│ Connected 2 hours ago       │
│ 127 connections             │
│                             │
│      [Connect]              │
└────────────────────────────┘
```

**Action Menu (•••)**:
- Connect
- Edit
- Duplicate
- Move to Folder
- Toggle Favorite
- Delete
- View Details

### 3.4 Folder Management

**Location**: `src/components/FolderManager.tsx`

**Features**:
- Create/rename/delete folders
- Drag-and-drop connections between folders
- Folder color customization
- Nested folders (future enhancement)

---

## 4. Implementation Plan

### Phase 1: Backend Foundation (Week 1)
- [ ] Add new fields to ConnectionProfile model
- [ ] Update database migration/store structure
- [ ] Implement new Tauri commands:
  - `record_connection`
  - `toggle_favorite`
  - `update_connection_folder`
  - `get_connection_stats`
  - `get_recent_connections`
  - `duplicate_connection`
- [ ] Update existing commands to track metadata

### Phase 2: Connection Dashboard (Week 2)
- [ ] Create ConnectionDashboard component
- [ ] Implement stats cards
- [ ] Create Recent Connections section
- [ ] Implement folder grouping with expand/collapse
- [ ] Add search/filter functionality
- [ ] Create ConnectionCard component (list and grid views)
- [ ] Implement connection status indicators

### Phase 3: Connection Wizard (Week 3)
- [ ] Create ConnectionWizard component with stepper
- [ ] Implement Step 1: Database type selection
- [ ] Implement Step 2: Connection details with test
- [ ] Implement Step 3: Advanced options (SSL, SSH, timeout)
- [ ] Add form validation and error handling
- [ ] Integrate with existing ConnectionForm logic

### Phase 4: Advanced Features (Week 4)
- [ ] Implement folder management UI
- [ ] Add drag-and-drop for folder organization
- [ ] Implement bulk actions (multi-select, move, delete)
- [ ] Add connection duplication
- [ ] Implement favorite/star functionality
- [ ] Add color tag picker
- [ ] Create connection details modal

### Phase 5: Polish & Testing (Week 5)
- [ ] Add animations and transitions
- [ ] Implement keyboard shortcuts
- [ ] Add loading states and skeletons
- [ ] Test with many connections (100+)
- [ ] Performance optimization
- [ ] Documentation and user guide

---

## 5. Technical Considerations

### 5.1 State Management
- Use React Context for dashboard state
- Cache folder structure to avoid re-computation
- Implement optimistic UI updates

### 5.2 Performance
- Virtualize connection list for 100+ connections
- Lazy load folder contents
- Debounce search input
- Cache connection status checks

### 5.3 UX Details
- Smooth expand/collapse animations
- Visual feedback for all actions
- Keyboard navigation support
- Drag-and-drop visual indicators
- Empty state illustrations
- Loading skeletons

### 5.4 Accessibility
- ARIA labels for all interactive elements
- Keyboard shortcuts for common actions
- Screen reader support
- Focus management in wizard

---

## 6. Future Enhancements

- [ ] Nested folders (folders within folders)
- [ ] Connection templates
- [ ] Import/export connection profiles
- [ ] Connection sharing (team features)
- [ ] Connection health monitoring
- [ ] Automatic reconnection on disconnect
- [ ] Connection groups with shared settings
- [ ] Connection versioning/history

---

## 7. Dependencies

**New NPM Packages**:
- `@dnd-kit/core` - Drag and drop functionality
- `@dnd-kit/sortable` - Sortable lists
- `react-window` or `@tanstack/react-virtual` - List virtualization (if needed)

**Existing Packages**:
- `lucide-react` - Icons
- `shadcn/ui` - UI components
- `@tanstack/react-router` - Routing

---

## 8. Success Metrics

- ✅ Users can organize connections into folders
- ✅ Recent connections are easily accessible
- ✅ Connection wizard is intuitive and completes in < 2 minutes
- ✅ Dashboard loads instantly with 100+ connections
- ✅ Search finds connections in < 100ms
- ✅ All actions have visual feedback within 200ms
