# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DB-Hive is a cross-platform database client built with Tauri 2.0 + React 19. The project is in **early development phase** with planning and architecture complete. The goal is to build a professional database client competing with Beekeeper Studio and DbGate, supporting PostgreSQL, MySQL, SQLite, MongoDB, and SQL Server.

## Commands

### Development
```bash
# Run development server (uses Bun)
bun run dev              # Frontend only
npm run tauri dev        # Full Tauri app with hot reload

# Build
bun run build            # Frontend build
npm run tauri build      # Production Tauri app

# Rust development
cd src-tauri
cargo build              # Debug build
cargo build --release    # Release build
cargo test               # Run all tests
cargo test test_name     # Run specific test

# Frontend testing (when implemented)
npm test                 # Run all tests
npm test -- ComponentName  # Run specific test
```

### Tauri CLI
```bash
npm run tauri dev        # Development mode
npm run tauri build      # Production build
npm run tauri info       # System information
npm run tauri icon       # Generate app icons
```

## Architecture

### Multi-Process Model

DB-Hive follows Tauri's multi-process architecture:

1. **Core Process (Rust)**: `src-tauri/src/`
   - Entry point: `lib.rs` exports `run()` function
   - Manages database connections, state, credentials
   - Handles all sensitive operations (credentials, SSH tunnels)
   - Exposes commands to frontend via `#[tauri::command]` macro
   - State management uses `Mutex<AppState>` for thread-safe access

2. **WebView Process (React)**: `src/`
   - Entry point: `main.tsx`
   - UI rendering and user interactions
   - Communicates with Core via `@tauri-apps/api` (invoke/events)
   - State management will use Zustand
   - No sensitive data stored in frontend

### IPC Communication Pattern

**Frontend → Rust (Commands)**
```typescript
import { invoke } from '@tauri-apps/api/core';
const result = await invoke<ReturnType>('command_name', { param: value });
```

**Rust → Frontend (Events)**
```rust
use tauri::Emitter;
app.emit("event-name", payload)?;
```

**Streaming Data (Channels)**
```typescript
import { Channel } from '@tauri-apps/api/core';
const channel = new Channel<DataType>();
await invoke('command_name', { channel });
```

### Project Structure Philosophy

```
.claude/
├── agents/          # Specialized sub-agents for development tasks
│   ├── rust-backend-dev.md      # Tauri commands, drivers, async Rust
│   ├── react-ui-dev.md          # React components, state, UI
│   ├── db-driver-specialist.md  # Database-specific implementations
│   ├── test-engineer.md         # Testing strategies
│   └── tech-writer.md           # Documentation
└── skills/          # Reusable code patterns
    ├── tauri-command/          # Command boilerplate generator
    ├── database-driver/        # Driver implementation patterns
    └── react-component/        # React component templates

src-tauri/src/
├── lib.rs           # Tauri app entry, command registration
├── main.rs          # Binary entry (just calls lib::run())
├── commands/        # Tauri command handlers (to be created)
├── drivers/         # Database driver implementations (to be created)
├── models/          # Data models and types (to be created)
└── state/           # Application state management (to be created)

src/
├── main.tsx         # React entry point
├── App.tsx          # Root component
├── components/      # React components (to be created)
├── hooks/           # Custom React hooks (to be created)
├── store/           # Zustand stores (to be created)
└── types/           # TypeScript types (to be created)
```

## Tauri Command Pattern

Commands are the primary way frontend communicates with Rust backend:

**In `src-tauri/src/lib.rs`:**
```rust
#[tauri::command]
fn command_name() -> Result<ReturnType, ErrorType> {
    // Implementation
    Ok(result)
}

// Register in run():
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![command_name])
    .run(tauri::generate_context!())
```

**Module-based commands:**
```rust
// src-tauri/src/commands/mod.rs
mod connection;

// In lib.rs:
.invoke_handler(tauri::generate_handler![
    commands::connection::create_connection
])
```

**Key Rules:**
- Commands in `lib.rs` cannot be `pub`
- Commands in modules must be `pub`
- Use `#[tauri::command(async)]` or `async fn` for async operations
- Return `Result<T, E>` where `E` implements `Serialize`
- Use `State<'_, Mutex<T>>` for accessing global state

## State Management Pattern

**Rust side:**
```rust
use std::sync::Mutex;
use tauri::{State, Manager};

#[derive(Default)]
struct AppState {
    connections: HashMap<String, ConnectionHandle>,
    // ... other state
}

// In setup:
app.manage(Mutex::new(AppState::default()));

// In commands:
#[tauri::command]
fn my_command(state: State<'_, Mutex<AppState>>) -> Result<T, E> {
    let state = state.lock().unwrap();
    // Use state (read-only)
    // For mutations, use `let mut state = state.lock().unwrap()`
}
```

**Frontend side (Zustand, to be implemented):**
```typescript
import { create } from 'zustand';

interface Store {
    data: Data;
    setData: (data: Data) => void;
}

export const useStore = create<Store>((set) => ({
    data: initialData,
    setData: (data) => set({ data }),
}));
```

## Database Driver Interface (Planned)

All drivers will implement a unified trait:

```rust
#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError> where Self: Sized;
    async fn execute_query(&self, sql: &str) -> Result<QueryStream, DbError>;
    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError>;
    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError>;
    // ... other methods
}
```

Each database (PostgreSQL, MySQL, SQLite, MongoDB) will have its own implementation in `src-tauri/src/drivers/`.

## Sub-Agents System

The `.claude/agents/` directory contains 5 specialized sub-agents for different development tasks. Each agent has specific expertise and access to relevant tools.

### 1. rust-backend-dev
**Purpose**: Rust backend development for Tauri application

**Responsibilities**:
- Implementing Tauri commands using `#[tauri::command]` macro
- Writing database drivers with async Rust (Tokio)
- Managing application state with `Mutex<T>` and `State<'_, T>`
- Building IPC handlers with Channels for streaming
- Creating custom error types with `thiserror`
- Working with `serde` for serialization/deserialization
- Implementing secure credential storage
- Managing SSH tunnel connections

**When to use**:
- Creating new Tauri commands
- Implementing database connection logic
- Building async operations with Tokio
- Setting up state management
- Handling IPC with streaming data

**Example invocation**:
```
"Use rust-backend-dev to implement a Tauri command for creating database connections"
"Use rust-backend-dev to add connection pooling to the PostgreSQL driver"
```

### 2. react-ui-dev
**Purpose**: React frontend development with TypeScript

**Responsibilities**:
- Building React components with TypeScript
- Implementing Zustand stores for state management
- Integrating Monaco Editor for SQL editing
- Using TanStack Table for virtualized data grids
- Calling Tauri commands using `@tauri-apps/api`
- Creating responsive layouts with TailwindCSS
- Handling async operations and loading states
- Building accessible UI components

**When to use**:
- Creating new React components
- Building forms and user input handling
- Implementing data tables and results viewers
- Integrating with Tauri backend commands
- Managing frontend state with Zustand

**Example invocation**:
```
"Use react-ui-dev to create a connection form component with validation"
"Use react-ui-dev to build a virtualized results table with TanStack Table"
```

### 3. db-driver-specialist
**Purpose**: Database driver implementation and optimization

**Responsibilities**:
- Implementing the `DatabaseDriver` trait for each database
- Writing database-specific metadata queries (schemas, tables, columns)
- Handling connection pooling and lifecycle management
- Optimizing query performance
- Implementing streaming result sets
- Supporting multiple database versions
- Handling database-specific features and quirks
- Creating error handling for database operations

**Databases**:
- PostgreSQL (tokio-postgres, sqlx)
- MySQL/MariaDB (mysql_async, sqlx)
- SQLite (rusqlite, sqlx)
- MongoDB (mongodb official crate)
- SQL Server (tiberius, odbc-api)

**When to use**:
- Implementing a new database driver
- Adding metadata query support
- Optimizing connection handling
- Debugging database-specific issues
- Writing database migration logic

**Example invocation**:
```
"Use db-driver-specialist to implement the PostgreSQL driver with metadata queries"
"Use db-driver-specialist to add connection pooling to the MySQL driver"
```

### 4. test-engineer
**Purpose**: Testing and quality assurance

**Responsibilities**:
- Writing Rust unit tests with `#[test]` and `#[tokio::test]`
- Creating integration tests for Tauri commands
- Writing React component tests with Vitest and Testing Library
- Implementing E2E tests with Tauri's testing utilities
- Mocking dependencies (mockall for Rust, vi.mock for React)
- Measuring and improving test coverage
- Setting up CI/CD test automation

**When to use**:
- Adding tests for new features
- Creating test fixtures and helpers
- Setting up mocking for complex dependencies
- Writing integration tests
- Improving test coverage

**Example invocation**:
```
"Use test-engineer to write tests for the connection manager commands"
"Use test-engineer to create integration tests for query execution"
```

### 5. tech-writer
**Purpose**: Technical documentation

**Responsibilities**:
- Writing API documentation with examples
- Creating user guides and tutorials
- Documenting architecture and design decisions
- Writing clear README files
- Maintaining changelog following semantic versioning
- Creating installation and deployment guides
- Writing troubleshooting documentation
- Documenting configuration options

**When to use**:
- Documenting new features or APIs
- Creating user guides
- Writing architecture documentation
- Maintaining the changelog
- Creating troubleshooting guides

**Example invocation**:
```
"Use tech-writer to document the database driver API with examples"
"Use tech-writer to create a user guide for SSH tunneling"
```

## Skills System

Skills in `.claude/skills/` provide reusable code generation patterns:

### 1. tauri-command
Generates Tauri command boilerplate with proper error handling, async support, and state management.

**Use when**:
- Creating new Tauri commands
- Need boilerplate for async commands with state
- Want to ensure proper error handling patterns

**Example invocation**:
```
"Use tauri-command skill to generate a command for executing queries"
```

### 2. database-driver
Provides database driver implementation patterns, metadata query templates, connection management, and streaming implementations.

**Use when**:
- Implementing a new database driver
- Adding metadata queries for a database
- Setting up connection pooling
- Implementing query streaming

**Example invocation**:
```
"Use database-driver skill to implement the MySQL driver interface"
```

### 3. react-component
Creates React component templates with TypeScript, proper props typing, error handling, and TailwindCSS styling.

**Use when**:
- Creating new UI components
- Building components that interact with Tauri commands
- Need proper TypeScript typing
- Building components with async operations

**Example invocation**:
```
"Use react-component skill to create a connection list component"
```

## Security Model

1. **Credentials**: Stored in OS keyring (via `keyring` crate), never in plaintext
2. **State**: Sensitive data only in Rust (Core process), never in frontend
3. **Validation**: All user input validated before SQL execution
4. **Network**: TLS/SSL support for DB connections, SSH tunneling for remote access

## Error Handling Pattern

Custom error types with serialization for IPC:

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Connection failed: {0}")]
    ConnectionError(String),
    #[error("Query failed: {0}")]
    QueryError(String),
    // ...
}

impl serde::Serialize for DbError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as { kind: "connection", message: "..." }
    }
}
```

Frontend receives typed errors:
```typescript
interface DbError {
    kind: 'connection' | 'query' | 'auth' | ...;
    message: string;
}
```

## Development Workflow

1. **Backend Development**: Start with Rust implementation in `src-tauri/`
   - Define data models
   - Implement Tauri commands
   - Write tests (`cargo test`)
   - Register commands in `lib.rs`

2. **Frontend Development**: Build UI in `src/`
   - Create TypeScript types matching Rust types (camelCase)
   - Implement components
   - Call Tauri commands via `invoke()`
   - Handle loading/error states

3. **Integration**: Test full flow with `npm run tauri dev`

## Important Configuration

- **Tauri config**: `src-tauri/tauri.conf.json`
  - Uses Bun for build commands (`beforeDevCommand`, `beforeBuildCommand`)
  - Dev server on `http://localhost:1420`
  - Frontend build output: `dist/`

- **TypeScript**: Strict mode enabled, React 19 types

- **Rust**: Edition 2021, library outputs multiple formats for Tauri

## Implementation Status

**Completed:**
- ✅ Project structure and documentation
- ✅ Sub-agents and skills framework
- ✅ Architecture planning
- ✅ Detailed implementation roadmap (see `docs/implementation-roadmap.md`)

**Next Steps (Phase 1 MVP - Weeks 3-14):**
- Connection management (backend + UI)
- PostgreSQL and SQLite drivers
- SQL editor with Monaco
- Query execution and results viewer
- Schema browser
- Query history

See `docs/implementation-roadmap.md` for complete week-by-week plan.

## Key Design Decisions

1. **Tauri 2.0**: Provides small binaries, security, and system integration
2. **React 19**: Latest React with improved performance
3. **Zustand**: Lightweight state management (not Redux)
4. **Monaco Editor**: VS Code's editor for SQL editing
5. **TanStack Table**: Virtualized tables for large result sets
6. **Streaming Results**: Use Channels to stream large datasets from Rust
7. **Async Throughout**: All I/O operations are async (Tokio runtime)

## Testing Strategy

- **Rust**: Unit tests with `#[test]`, async tests with `#[tokio::test]`
- **React**: Vitest + Testing Library (to be set up)
- **Integration**: Test Tauri commands with mocked state
- **E2E**: Tauri's testing utilities (future)

Run tests:
```bash
cd src-tauri && cargo test           # Rust tests
npm test                             # Frontend tests (when implemented)
```

## Documentation References

- **Full Implementation Plan**: `docs/implementation-roadmap.md` (772 lines, week-by-week breakdown)
- **Original Design**: `docs/base-plan.md`
- **Complexity Analysis**: `docs/difficulty.md`
- **Tauri Docs**: `docs/tauri/` (architecture, IPC, state management)

These documents should be read when starting major features or making architectural decisions.
