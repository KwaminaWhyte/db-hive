# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DB Hive is a cross-platform database client: a **native Rust desktop app**
built on egui/eframe, supporting PostgreSQL, MySQL, SQLite, MongoDB, Redis,
SQL Server and Turso.

It was originally Tauri 2.0 + React 19 and was migrated to a single native
binary. The reason was measured, not aesthetic: the Tauri build ran four
processes and 400-650 MB idle, of which the WebView and its two helpers were
about 90%. The native build runs one process at 111-130 MB. See
`docs/rust-ui-migration/` for the measurements, the plan and the parity
inventory.

## Commands

```bash
cargo run -p dbhive-app              # run the app
cargo build --release -p dbhive-app  # release binary at target/release/dbhive
./packaging/build-macos-app.sh       # assemble and sign DB Hive.app

cargo check --workspace
cargo test --workspace
cargo fmt

cargo run -p dbhive-app --example smoke -- "Local MySQL"   # end-to-end check
```

There is no Node, npm or bundler. Do not add one.

Three `credentials::tests::*` failures are expected on macOS: they exercise
the real OS keyring, which the system denies to an unsigned test binary. They
fail identically at commit c57b24e, before the migration. Do not "fix" them
by weakening the tests.

Development aids, all unset in normal use: `DBHIVE_AUTOCONNECT`,
`DBHIVE_AUTODB`, `DBHIVE_AUTOQUERY`, `DBHIVE_AUTOSCREEN`,
`DBHIVE_AUTOWINDOWS`, `DBHIVE_DEV_WINDOW`, `DBHIVE_LOG_VIEWPORTS`.

## Architecture

```
crates/dbhive-core   UI-agnostic. drivers, DDL, SSH, credentials,
                     migrations, AI, plugins, AppState, storage, and the
                     service_* layer. No egui, no UI types.
crates/dbhive-app    the egui shell. screens/, widgets/, task, theme.
```

**Read `docs/rust-ui-migration/03-conventions.md` before adding a screen.**
The rules there are not style preferences; each exists because the
alternative broke something. The three that matter most:

1. **Never block the frame.** `eframe::App::update` runs every frame. All
   database work goes through `crate::task::Job`, which spawns on Tokio and
   is polled from the frame loop, and requests a repaint on completion.
2. **Never hold the `AppState` lock across drawing.** Copy what you need,
   drop the guard, then draw. Clone the `Arc<dyn DatabaseDriver>` out of
   state and drop the guard before any await.
3. **Never use `RichText::strong()` directly.** egui resolves it from
   `widgets.active.fg_stroke`, which this palette uses for text on the honey
   primary, so bold text comes out near-invisible on dark cards. Use
   `theme::text::{strong, heading, muted}`.

Business logic belongs in `dbhive-core`, never in a screen. Operations both
the UI and any future shell need live in `dbhive_core::service*`.

### Multi-window

Extra windows are egui **immediate viewports**, rendered inside the same
`update()` call, which is what lets them borrow the app. Per-window state
(screen, workspace, screens, connections, toasts) is swapped in and out
around each viewport's frame, so the active window's state stays inline on
`DbHiveApp` as ordinary fields and screens keep their `app.screens` field
access.

The Tokio runtime and `AppState` are shared across windows, deliberately:
two windows on the same connection reuse one pool rather than opening a
second. Do not make connections per window.

### Persistence

`dbhive_core::storage::JsonStore` reproduces the exact on-disk format and
location `tauri_plugin_store` used
(`~/Library/Application Support/com.kwamina.db-hive/*.json` and the platform
equivalents), so an existing installation keeps its connections, history and
snippets. Do not change `APP_IDENTIFIER`; it orphans every user's data.

Passwords go to the OS keyring via `credentials::CredentialManager`, never
to the JSON stores.

### SQL construction

Every identifier goes through the driver's `quote_identifier` and every
literal through `escape_string_literal`. The DDL generators escape
identifiers via `ddl::escaping` before formatting. There is a regression
test that builds a table named `t"; DROP TABLE users; --` and asserts
nothing escapes the quoting in any of the four dialects. Do not build SQL by
concatenating user text.

## Sub-Agents System

`.claude/agents/` holds specialised sub-agents. They were written for the
Tauri + React codebase and were updated for the native stack:

| Agent | Use for |
| --- | --- |
| `rust-backend-dev` | `dbhive-core` logic and `dbhive-app` screens |
| `db-driver-specialist` | drivers, metadata queries, dialect differences |
| `test-engineer` | unit and integration tests |
| `tech-writer` | documentation |
| `react-ui-dev` | **obsolete**, there is no React in this project |

When farming a screen out to an agent, give it
`docs/rust-ui-migration/03-conventions.md` and tell it which single file it
may edit. Parallel agents editing shared files (`app.rs`, `screens/mod.rs`,
`lib.rs`, `Cargo.toml`) will collide; have them report the wiring they need
instead and apply it yourself.

## Skills System

`.claude/skills/` holds reusable patterns. `database-driver` still applies.
`react-component` is obsolete. `tauri-command` is obsolete: its replacement
is a plain function in `dbhive_core::service*` taking `&Mutex<AppState>`.

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

1. **Logic first, in `crates/dbhive-core`.** Models, drivers, and the
   `service*` operations, with unit tests.
2. **Then the screen, in `crates/dbhive-app/src/screens/`.** Compose the
   shared widgets (`results_grid`, `sql_editor`, `file_dialog`, `toast`);
   do not write a second table or editor.
3. **Wire it in `app.rs`**: the screen dispatch chain, a `CommandId` arm,
   and a menu entry if it deserves one.
4. **Verify against a real database.** `cargo test` does not prove a driver
   works; the smoke example does.

## Important Configuration

- **Workspace root `Cargo.toml`** holds the release profile. Cargo ignores
  profiles declared on non-root workspace members, so it must live there.
- **`crates/dbhive-app/Cargo.toml`**: eframe with `accesskit` (screen
  reader support, deliberately kept), `wgpu` and `persistence`.
- **Rust**: edition 2021.

## Testing Strategy

- Pure logic (SQL generation, filtering, sorting, parsing, validation) is
  unit tested in the same file. Drawing code is not tested.
- If a function is hard to test because it is tangled with `ui`, split the
  logic out. That is the point.
- `cargo run -p dbhive-app --example smoke` exercises the real path end to
  end against a live database.

```bash
cargo test --workspace
cargo run -p dbhive-app --example smoke -- "Local MySQL"
```

## Key Design Decisions

1. **egui + eframe**: one native binary, no WebView. Chosen for what it
   already ships that this app needs: `egui_extras::TableBuilder` for the
   results grid, `egui_plot` for charts, AccessKit for screen readers, and
   multi-viewport for the multi-window model.
2. **One shared core**: `dbhive-core` has no UI dependency, so the logic is
   testable without a window and reusable by any other shell.
3. **Async through a Job, never a block**: see the conventions doc.
4. **Layered graph layout in `widgets::graph_layout`** replaces dagre for
   the ER diagram and schema designer. There is no Rust equivalent to lean
   on, so it is hand written and unit tested.
5. **Schema-driven plugin UI**: plugins are JavaScript on boa_engine and
   declare their UI as data, which the host renders natively. They cannot
   render arbitrary UI, which is also a security property.
6. **Streaming and pagination over unbounded reads**: results are capped at
   `MAX_RESULT_ROWS` (50,000) and the table inspector pages.

## Documentation References

- **Migration record**: `docs/rust-ui-migration/`
  - `00-baseline-measurements.md` measured memory, before and after
  - `01-frontend-inventory.md` the React feature inventory and parity list
  - `02-backend-surface.md` the original command surface
  - `03-conventions.md` **read before adding a screen**
  - `04-plan.md` status and the parity items that cannot be met as specified
- **Original design**: `docs/base-plan.md`
