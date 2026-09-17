# Slint migration

Replace the egui shell (`crates/dbhive-app`) with Slint (`crates/dbhive-slint`).
`dbhive-core` is untouched. Branch: `slint-ui`, off `native-rust-migration`.

## Decisions

| | |
| --- | --- |
| Toolkit | Slint `=1.18.0`, pinned exactly; upgrades are deliberate commits |
| License | Royalty-free 2.0. Requires `AboutSlint` in the About dialog, reachable from the top menu. Plugins must never expose Slint APIs |
| Look | Custom honey-brand components on Slint primitives, not a stock style |
| Renderer | GPU (FemtoVG). The software renderer cannot carry the diagram |

## Phase 0: spikes (done)

| Spike | Result |
| --- | --- |
| SQL editor | Pass. Coloured runs under a transparent `TextInput`, placed by column x measured advance. Pixel-exact by debug-tint snapshot; typing verified with real key events; caret and selection verified by snapshot. Limit: wide glyphs misalign the colour layer |
| 50k-row grid | Pass. `ListView` + lazy `Model`. First frame 16 ms, jump to row 25,000 in 6 ms (software renderer, upper bound) |
| Diagram canvas | Pass on GPU: 120 fps dragging among 60 tables / 88 edges. Paths must be sized to their bounding box |
| Memory | On par with egui: 106 MB empty, 211 MB with the 60-table diagram (GPU). Not a memory win |

Tools this produced, kept for the rest of the migration:

- `examples/snapshot.rs`: render any component to an image headlessly
- `tests/*`: drive components with real key and pointer events
- `examples/grid_perf.rs`, `examples/diagram_perf.rs`, `examples/diagram_live.rs`

## Phases

1. **Lift logic.** New `crates/dbhive-ui-model`, no UI dependencies. Pure logic and its tests move out of the egui screens; egui keeps compiling against it.
2. **Shell.** Theme global generated from Rust tokens, SVG icons, async bridge, toasts, navigation, command palette, `MenuBar`, About with `AboutSlint`.
3. **Daily flow.** Connections, connection editor, password prompt, workspace, history.
4. **Data tools.** Table inspector, table editor, procedures, import/export, backup, migrations, settings.
5. **Canvas and extras.** ER diagram, schema designer, query builder, activity monitor charts, AI assistant.
6. **Plugins, multi-window, updater.**
7. **Cut over.** Interaction tests, smoke tests, memory, a human walkthrough of every screen, then delete `dbhive-app` and update docs, CI and packaging.

## Status

- [x] Phase 0
- [ ] Phase 1
