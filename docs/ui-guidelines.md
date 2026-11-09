# UI guidelines for DB Hive

Read this before any UI change. It exists because the UI was rebuilt several times chasing
"looks nicer", and each time the real causes stayed. These rules come from studying three well-made
apps (see section 9) and from rendering this app's own screens.

## 1. Do not switch toolkit

- "Looks horrible" has never been a toolkit problem. The reference apps in section 9 use plain
  native renderers and look finished. What they have is a design system, a screenshot loop and a
  shared kit, none of which depend on the toolkit.
- Slint is the stack (`docs/slint-migration/plan.md`). If you think a switch is needed, stop and
  ask the owner with: a screenshot of the actual problem, why a kit or token change cannot fix it,
  and a measured cost. A toolkit switch is the owner's decision, never an agent's.
- Fix "ugly" by fixing the kit and the tokens. One fix there restyles every screen.

## 2. Design tokens only

- Colours, spacing, radii, type sizes and control heights come from the `Theme` global
  (generated from `crates/dbhive-ui-model/src/tokens.rs`). No literal `#rrggbb`, `rgba(...)` or
  `font-size: 13px` in a component or screen. Add a token instead.
- Spacing is a 4px scale (2, 4, 8, 12, 16, 24, 32). Do not invent 5px, 10px, 14px.
- Type: at most 5 sizes (caption, small, body, title, heading) and 2 weights (regular, semibold).
- One accent colour (honey). It marks the primary action and the selection, nothing else.
- Light and dark both ship. A component that only looks right in dark is broken.
- Enforced by a ratchet: `scripts/check-ui-tokens.sh` fails when literals increase. Run it before
  every UI commit. `--list` shows the current offenders; burn them down, then `--update`.
  Known offenders at the time of writing: `app.slint` (drop shadow, scrim), `components/sql_editor.slint`
  (caret, selection, gutter, background, border colours).

## 3. The kit is the only way to build a screen

- `crates/dbhive-slint/ui/kit/kit.slint` has Button, Card, Badge, TextField, Dropdown, Toggle,
  Divider, ScreenHeader, SectionLabel, Banner, EmptyState and friends. Screens compose these.
- Need something the kit lacks? Add it to the kit first, with a snapshot in `kit-dark` and
  `kit-light`, then use it. Never style a one-off `Rectangle` in a screen to imitate a component.
- Lists of items (tables, connections, history) are flat rows: hover fill and selected fill, no
  border per row, no card per row. A bordered card per row looks like a form, not a list.
- Icon and label in a row share one vertical centre. Use `alignment: center` on the cross axis, never
  hand-placed offsets.
- At most one primary button per view. Everything else is secondary or ghost.

## 4. Every view has four states, and each is snapshotted

Populated, empty, loading, error. `examples/snapshot.rs` already has `workspace`, `workspace-empty`,
`workspace-loading`. A new screen adds all four before it is called done. Empty states say what to do
next. Error states show the real message with a way to retry, not a raw stack text.

## 5. Look at the result. This is the rule that was missing.

Writing Slint without looking at it is how an unaligned icon ships. After every UI change:

```bash
S=/tmp/dbhive-shots; mkdir -p $S
cargo run -q -p dbhive-slint --example snapshot -- workspace $S/ws.ppm 1280 800 1
sips -s format png $S/ws.ppm --out $S/ws.png      # macOS
```

Then open the PNG with the Read tool and actually look. Components available: `app`, `workspace`,
`workspace-light`, `workspace-long`, `workspace-empty`, `workspace-loading`, `workspace-error`, `about`, `confirm`, `connection-editor`,
`connection-editor-sqlite`, `kit-dark`, `kit-light`, `sql`, `sql-selection`, `sql-debug`.

Check, every time:

1. Alignment: icons, labels and numbers share baselines and centres. Columns line up.
2. Contrast: all text readable on its background. Secondary text is dimmer, never invisible.
3. Density: nothing cramped, nothing floating in a sea of empty space.
4. Two window sizes: 1280x800 and 1000x640. Plus scale 2 for Retina.
5. Light and dark.
6. Long strings: a 60 character table name, a long error message. Elide, never overflow.

Limits of the snapshot tool: it uses the software renderer, the real window uses the GPU renderer
(FemtoVG), so anti-aliasing and font weight differ slightly. It does not draw the native macOS menu
bar correctly (the File/View/Help text looks dark on dark in snapshots; verify in the real window,
do not "fix" it from the snapshot). When in doubt run the real app: `cargo run -p dbhive-slint`.

## 6. Layout rules that already bit us

- A `VerticalLayout` or `ScrollView` whose children have no stretch spreads leftover space as gaps
  between every row. Set `alignment: start`, or give one child `vertical-stretch: 1`
  (`docs/slint-migration/plan.md` lists the two places this happened).
- Buttons keep natural size (`horizontal-stretch: 0`) or they become slabs.
- Paths and canvases must be sized to their bounding box.
- Never hold app state across a draw, never block the UI thread.
- Logic lives in `dbhive-ui-model` and `dbhive-core`, never in `.slint` expressions beyond trivial
  display logic. That is what keeps screens testable without a window.

## 7. Compare against real products, then decide

DB Hive competes with TablePlus, DataGrip, DBeaver, Beekeeper Studio. Before designing a screen,
look at how two of them lay it out (title bar, sidebar tree, query tabs, results grid with a
status strip, inspector). Copy the information architecture, not their assets. Keep any reference
screenshots in a gitignored `plan/refs/` folder, never in the repo. Target: a user of those tools
finds every control where they expect it.

## 8. Known polish gaps

1. Literal colours remain in `sql_editor.slint` and `app.slint` (section 2); the token check counts
   them, so burn them down when touching those files.
2. Connection list and workspace header have large dead areas. Fill with useful content (recent
   queries, connection health), do not just centre a lone empty state in a 1280 pixel window.
3. Menu text in snapshots: check in the real window before touching it (section 5).
4. The native menu bar is not drawn in snapshots; verify menu changes by running the app and
   capturing the window with `scripts/window-shot.sh`.

## 9. What the reference apps do

Studied 2026-10-08: github.com/storytold/filmcraft, wordcraft, pdfcraft (clean-room Rust apps).
A fuller write-up is in `~/Desktop/craft-apps-learnings.md` on the owner's machine.
What transfers to a database client:

- One `Tokens` struct holds every colour, radius, gap and height. Three themes come from it.
  Widgets never use literals. (This repo has the same idea in `tokens.rs`; the gap is discipline.)
- Icons are drawn or vendored under a clear licence, one set, one stroke weight (Lucide here, keep it).
- Every action is a command with an id, a label, a shortcut and an `enabled()` reason. Menus, the
  command palette and tests all dispatch by id. `command_palette.rs` in `dbhive-ui-model` is the start.
- The UI is a client of a headless engine. Features are tested without a window.
- They keep an honest assessment in the roadmap. Do the same in
  `docs/slint-migration/plan.md`: say what was verified and what was not.
- Screenshots are part of the work product. A UI task is not done without one that was looked at.

## 10. Definition of done for a UI change

- [ ] Built only from kit components (or added to the kit with snapshots)
- [ ] `scripts/check-ui-tokens.sh` passes
- [ ] Populated, empty, loading and error snapshots rendered and looked at
- [ ] Checked at 1280x800 and 1000x640, light and dark
- [ ] `cargo test --workspace` passes (credentials keyring failures are expected, see CLAUDE.md)
- [ ] Keyboard path works and `accessible-label` is set on custom interactive elements
- [ ] The final message says what was looked at in the screenshots, not just "UI updated"
