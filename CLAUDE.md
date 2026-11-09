# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## READ FIRST: the shell

The app is `crates/dbhive-slint`. The Tauri+React and egui shells are gone
(kept only in git history and `docs/rust-ui-migration/`). The cutover record
and what remains open are in `docs/slint-migration/plan.md`.

- **Do not change UI toolkit.** The toolkit was never the cause of the
  "looks bad" problem. Read `docs/ui-guidelines.md` section 1 before proposing it.
- **All UI work follows `docs/ui-guidelines.md` and `AGENTS.md`.**
  The loop is: change, render a screenshot, LOOK at it, fix, repeat.
- New screens go in `crates/dbhive-slint/ui/screens/`, built only from
  `ui/kit/kit.slint`, with logic in `crates/dbhive-ui-model` (no UI types)
  and `dbhive-core`.

## Verify

Run before saying a change is done:

```bash
cargo fmt --all --check                             # ~1s; tree is clean, keep it that way
cargo check --workspace                             # <1s warm
cargo clippy --workspace                            # ~100s warm, ~220 warnings today; do not add new ones
cargo test --workspace -- --skip credentials::tests # ~4.5min warm, 1845 pass; same filter as CI
```

Hooks in `.claude/settings.json`: `rustfmt --edition 2021` on every edited
`.rs` file, and `cargo check --workspace` on Stop.

## Project Overview

DB Hive is a cross-platform database client: a **native Rust desktop app**
supporting PostgreSQL, MySQL, SQLite, MongoDB, Redis, SQL Server and Turso.
It was Tauri 2.0 + React 19 and moved to one native binary to cut idle memory
from 400-650 MB to 111-130 MB (`docs/rust-ui-migration/`).

## Commands

```bash
cargo run -p dbhive-slint                  # run the app (binary: dbhive)
cargo build --release -p dbhive-slint      # release binary at target/release/dbhive
./packaging/build-macos-app.sh             # assemble and sign DB Hive.app

cargo run -p dbhive-slint --example smoke -- "Local Post"  # end-to-end check
```

There is no Node, npm or bundler. Do not add one.

Three `credentials::tests::*` failures are expected on macOS: they exercise
the real OS keyring, which the system denies to an unsigned test binary. Do
not "fix" them by weakening the tests; skip them as in Verify.

Development aids, all unset in normal use: `DBHIVE_PLUGIN_SOURCES` (extra
folders searched for bundled plugins) and `DBHIVE_DATA_DIR` (points every
store at another folder; tests already set it, never set it by hand unless
you mean to run against a scratch profile).

## Architecture

```
crates/dbhive-core      UI-agnostic. drivers, DDL, SSH, credentials,
                        migrations, AI, plugins, AppState, storage, and the
                        service_* layer. No UI types.
crates/dbhive-ui-model  UI-independent view logic: SQL builders, validation,
                        layout, formatting, panel state machines. Depends on
                        dbhive-core only. Unit tested; no Slint types.
crates/dbhive-slint     the app. ui/ holds .slint files (kit/, screens/,
                        dialogs/, icons/); src/ holds the controllers, one
                        per window, over one shared AppState. build.rs
                        generates theme.slint from dbhive-ui-model/src/tokens.rs
                        and icons.slint from ui/icons/*.svg. tests/*_controller.rs
                        drive real windows against real SQLite files.
```

Business logic belongs in `dbhive-core` or `dbhive-ui-model`, never in a
screen or controller. Operations the app needs live in `dbhive_core::service*`.

Never block the UI thread: database work spawns on the shared Tokio runtime
and completes through `runtime::spawn`, which routes the result back to the
window that asked. Never hold the `AppState` lock across an await or a
redraw: clone the `Arc<dyn DatabaseDriver>` out and drop the guard. Slint
globals are per window; per-window state lives in `Session`, shared state in
`AppState`.

### Persistence

`dbhive_core::storage::JsonStore` reproduces the exact on-disk format and
location `tauri_plugin_store` used
(`~/Library/Application Support/com.kwamina.db-hive/*.json` and the platform
equivalents), so an existing installation keeps its connections, history and
snippets. Do not change `APP_IDENTIFIER`; it orphans every user's data.

Passwords and API keys go to the OS keyring via
`credentials::CredentialManager`, never to the JSON stores, a view, a toast
or a log.

### SQL construction

Every identifier goes through the driver's `quote_identifier` and every
literal through `escape_string_literal`. The DDL generators escape
identifiers via `ddl::escaping` before formatting. There is a regression
test that builds a table named `t"; DROP TABLE users; --` and asserts
nothing escapes the quoting in any of the four dialects. Do not build SQL by
concatenating user text.

## Agents and skills

`.claude/agents/`: `rust-backend-dev`, `db-driver-specialist`,
`test-engineer`, `tech-writer`. `.claude/skills/`: `database-driver`.

When farming work out to parallel agents, tell each which files it may edit.
Shared files (`app.rs`, `lib.rs`, `ui/app.slint`, `Cargo.toml`) collide; have
agents report the wiring they need and apply it yourself.

## Security Model

1. **Credentials**: OS keyring (via `keyring` crate), never in plaintext.
2. **Validation**: all user input validated before SQL execution.
3. **Network**: TLS/SSL for DB connections, SSH tunneling for remote access.
4. **Plugins**: JavaScript on boa_engine that declares its UI as data; the
   host renders it natively, so a plugin cannot draw arbitrary UI.

## Development Workflow

1. **Logic first**, in `dbhive-core` or `dbhive-ui-model`, with unit tests.
2. **Then the screen**, in `crates/dbhive-slint/ui/screens/`, from kit parts
   only, plus its controller in `src/`.
3. **Verify against a real database.** `cargo test` does not prove a driver
   works; the controller tests and the smoke example do. Screenshot the
   screen and look at it. `scripts/window-shot.sh <out.png>` captures the
   real GPU window (never the whole screen); `scripts/check-ui-tokens.sh`
   guards the token ratchet.

## Important Configuration

- **Workspace root `Cargo.toml`** holds the build profiles. Cargo ignores
  profiles declared on non-root workspace members.
- **Rust**: edition 2021.
- Results are capped at `MAX_RESULT_ROWS` (50,000); table views page.

## Documentation References

- `docs/slint-migration/plan.md`: what is done, what is open, and why
- `docs/ui-guidelines.md`: UI rules, read before any UI work
- `docs/rust-ui-migration/`: historical record (measurements, parity
  inventory, egui conventions); do not follow its egui instructions
- `docs/plugin-development.md`, `docs/user-guide.md`
