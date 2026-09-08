# Migration plan and status

Target: replace the Tauri + React shell with a standalone cross-platform
native Rust desktop app (eframe/egui), at full parity, then delete the Tauri
shell.

Decisions taken at the start, and why they constrain everything below:

| Decision | Choice |
| --- | --- |
| Toolkit | egui + eframe |
| Plugin UI | schema-driven, rendered natively; boa_engine scripting kept |
| SQL editor | solid basics: highlighting, schema completion, find/replace, line numbers |
| Parity scope | everything, including the three canvas tools; no phasing |

## Architecture

```
crates/dbhive-core     UI-agnostic. drivers, DDL, SSH, credentials,
                       migrations, AI, AppState, storage, service
crates/dbhive-app      egui shell. screens/, widgets/, task, theme
src-tauri              reference implementation, deleted at parity
```

Both shells depend on `dbhive-core`, so behaviour cannot drift while both
exist. Work that would previously have gone into a `#[tauri::command]` goes
into `dbhive_core::service` instead, where both shells can reach it.

Data is shared too: `JsonStore` reproduces the exact on-disk format and
location `tauri_plugin_store` used, so an existing installation keeps its
connections, history and snippets. This has been verified against a real
installation: the native shell lists the same five saved connections.

## Status

Done:

- [x] Extract 12,494 LOC of Tauri-free logic into `dbhive-core`
- [x] Replace `tauri_plugin_store` with `JsonStore`, move `AppState` into core
- [x] `service::{connect, disconnect, switch_database, load_persisted}`
- [x] Async bridge (`task::Job`), theme (OKLCH tokens as sRGB), toasts
- [x] Results grid (virtualised, sortable, filterable)
- [x] SQL editor (highlighting, completion, find/replace, gutter)
- [x] Connections list, connect/disconnect
- [x] Workspace: schema sidebar, database switching, execute, results

In progress (parallel):

- [ ] Settings, History/Snippets/Templates, Connection editor,
      Table inspector, Command palette + shortcut registry

Not started:

- [ ] Query tabs (multiple statements side by side)
- [ ] Multi-window (eframe multi-viewport, replacing `commands/window.rs`)
- [ ] Table create/edit dialogs (DDL)
- [ ] Migrations, Backup/Restore, Import/Export
- [ ] Activity monitor
- [ ] AI assistant
- [ ] Plugins (schema-driven UI; needs the plugin API redesign)
- [ ] ER diagram, Visual schema designer, Visual query builder
- [ ] Menu bar, About, updater
- [ ] Strip the remaining 10,460 LOC of Tauri command wrappers
- [ ] Delete `src-tauri`, drop the npm toolchain, single-binary packaging

## Known risks

The three canvas tools (ER diagram, schema designer, query builder) have no
Rust equivalent to lean on: reactflow and dagre both have to be replaced with
a hand-written layout and an egui painter. They were rated VERY HIGH risk in
the inventory and are the most likely items to slip.

The plugin system is the other one. Plugins currently render React. The
chosen replacement is a declarative schema the host renders natively, which
means an API redesign and a migration path for any existing plugin.

## Unfinished business inherited from the web shell

Recorded so it is not silently lost in the rewrite:

- Idle WebContent memory was 310-565 MB and was never explained. If the cause
  was retained data rather than the framework, the same mistake can be ported
  straight into the native UI. Watch result-set and schema-metadata retention.
- The accent colour setting was a dead control: written by the picker, read by
  nothing. Either wire it up or drop it; do not reproduce it.
