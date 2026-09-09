# DB Hive Frontend Feature Inventory (React → Native Rust GUI Migration)

This document is an exhaustive inventory of the DB Hive React/TypeScript frontend
(`src/`, 169 files, ~42,000 LOC as of 2026-09-08, excluding the generated
`src/routeTree.gen.ts`). It exists to serve as the checklist for a full rewrite
of the UI into a native Rust GUI toolkit (no HTML/CSS/DOM).

Sources: direct reading of every file in `src/`, `wc -l` line counts, and
`grep`/`perl` extraction of every `invoke()` call site.

---

## 1. Route Map

All routes are defined with TanStack Router's file-based routing
(`src/routes/`). `src/routes/__root.tsx` is the root layout; `_connected/*` is
a layout-guarded subtree requiring an active database connection.

| Route file | URL | Component | What it does |
|---|---|---|---|
| `src/routes/__root.tsx` | (root layout, wraps every route) | `RootComponent` | App shell: theme/connection/tab providers, custom titlebar, global keyboard shortcuts (Cmd/Ctrl+K palette, Ctrl+, settings, `?` shortcuts help, Cmd/Ctrl+Shift+N new window), global modals host, update banner, toast host. |
| `src/routes/index.tsx` | `/` | `HomeRoute` | Home/connection-manager screen: welcome screen when no connections, otherwise a connection picker/list with search, new-connection flow (choose DB type → `ConnectionForm`), edit/duplicate/delete profile actions. |
| `src/routes/connections.tsx` | `/connections` | `() => null` | Pure redirect stub: `beforeLoad` throws `redirect({ to: "/" })`. Legacy URL kept for compatibility; all connection management now lives on `/`. |
| `src/routes/about.tsx` | `/about` | `AboutRoute` → `AboutContent` (`variant="page"`) | Full-page About screen: version, features, credits, third-party dependency list, license, update-check button. |
| `src/routes/settings.tsx` | `/settings` | `SettingsRoute` → `SettingsPage` | Full-page Settings screen (also reachable as an overlay modal via `GlobalModals`/Ctrl+,). |
| `src/routes/plugins.tsx` | `/plugins` | `PluginsRoute` | Plugin Manager screen with tabs: Marketplace (`PluginMarketplace`) and Installed (`InstalledPlugins`). |
| `src/routes/_connected/route.tsx` | `/_connected` (layout) | `ConnectedLayout` | Layout for all connected views: top bar with connection/database info and disconnect, resizable left sidebar hosting `SchemaExplorer`, guards/redirects to `/` if no active connection. Sidebar collapse/expand, per-window-scoped panel-size persistence. |
| `src/routes/_connected/query.tsx` | `/_connected/query` (as `/query`) | `QueryRoute` | Main SQL Editor + Results workspace: multi-tab query/table tabs, `QueryPanel` (Monaco editor + execute), `TableInspector` embedded per table-tab, `RedisValuePanel` for Redis connections, destructive-query confirmation guard. |
| `src/routes/_connected/er-diagram.$schema.tsx` | `/_connected/er-diagram/$schema` (as `/er-diagram/{schema}`) | `ERDiagramRoute` (lazy-loaded) → `ERDiagram` | Auto-generated entity-relationship diagram for the given schema: table nodes with columns/PK/FK, relationship edges, zoom/pan, dagre auto-layout. |
| `src/routes/_connected/visual-query.tsx` | `/_connected/visual-query` (as `/visual-query`) | `VisualQueryBuilderPage` (lazy-loaded) → `QueryBuilder` | No-SQL visual query construction: pick tables/joins/columns/filters/group-by/order-by/limit, live SQL preview, "Execute" hands the generated SQL off to a SQL Editor tab. |
| `src/routes/_connected/visual-schema-designer.tsx` | `/_connected/visual-schema-designer` (as `/visual-schema-designer`) | `VisualSchemaDesignerRoute` (lazy-loaded) → `VisualSchemaDesigner` | Node-based visual designer for creating new tables/relationships and generating DDL SQL (reactflow canvas). |
| `src/routes/_connected/table.$schema.$tableName/route.tsx` | `/_connected/table/$schema/$tableName` (layout) | pass-through `<Outlet/>` | Layout shell for table-specific views. |
| `src/routes/_connected/table.$schema.$tableName/index.tsx` | `/_connected/table/$schema/$tableName/` | (redirect, no component) | Legacy route: `beforeLoad` redirects to `/query?tabs=...&active=1`, opening the table as a tab in the unified query/table tab system instead of a dedicated page. |
| `src/routes/_connected/activity.tsx` | `/_connected/activity` (as `/activity`) | `ActivityMonitorPage` → `ActivityMonitor` | Activity Monitor: live query log, server stats, active-process list with kill action. |

Note: `/connections` and the `table/$schema/$tableName/` index route are both
now-dead-end redirects to a unified home screen and unified query-tab system,
respectively: a sign that the real "screens" a rewrite must reproduce are
fewer than the raw route count suggests: **Home, Connected Shell (sidebar +
tabs), Query/Table workspace, ER Diagram, Visual Query Builder, Visual Schema
Designer, Activity Monitor, Settings, About, Plugins**.

---

## 2. Feature Areas

### 2.1 Connections

**Files**: `src/components/ConnectionForm.tsx` (885), `src/components/ConnectionTreeView.tsx` (354), `src/contexts/ConnectionContext.tsx` (65), `src/components/ConnectionLostError.tsx` (99), `src/routes/connections.tsx` (10, redirect stub), `src/utils/multiWindow.ts` (30, see 2.16).

**What it does**:
- `ConnectionForm.tsx` fields, driver-conditional: Connection Name (required, `:400-416`); Environment select None/Local/Staging/Production with colored dot (`:419-454`); SQLite Database File path + native "Browse" picker filtered to `.db/.sqlite/.sqlite3/.db3` (`:456-480`); non-SQLite optional "Connection String" input that auto-parses per-driver regex (Postgres/Supabase/Neon, MySql, MongoDb, SqlServer, Redis) to prefill fields (`:136-172,483-497`); Turso gets a single "libSQL URL" field instead of host/port (`:500-518`); Host+Port grid for other drivers (`:520-551`); Database Name / "Database Index" (Redis) (`:553-572`); Username (optional for MongoDb/Redis, hidden for Turso) (`:574-603`); Password/"Auth Token" with show/hide toggle + "Saved to keychain" hint (`:604-639`); SSL Mode select Disabled/Preferred/Required (`:641-661`); SSH Tunnel Off/"Over SSH" segmented toggle (`:663-689`) revealing SSH host/port, username, auth-method segmented toggle (Password vs Private Key), password field or key-file browse (`:693-828`); inline validation banner and pass/fail test-result banner (`:213-230,843-861`); footer Test Connection / Save Connection buttons. On successful test, silently persists password/SSH password to OS keyring when the profile has an id (`:291-306`); save creates/updates the profile then persists secrets (`:345-369`).
- `ConnectionTreeView.tsx` groups saved profiles into alphabetically-sorted folders ("Unfiled Connections" pinned last, `:96-130`); per-folder header (chevron, folder icon, name, count badge, click-to-toggle, `:248-297`); "Expand All"/"Collapse All" (`:311-347`); per-connection row shows server icon, favorite star, name, color-coded driver badge, environment badge, "Connecting…" pulse label, host:port/database subtitle (`:144-194`); double-click connects; hover-reveal Connect button + "…" menu (Toggle Favorite, Edit, Delete) (`:196-243`); summary line "N folders, M connections".
- `ConnectionContext.tsx`: non-persisted React context holding `connectionId`/`connectionProfile`/`currentDatabase`, exposing `setConnection`/`setCurrentDatabase`/`disconnect`.
- `ConnectionLostError.tsx`: `ErrorState`-based panel (WifiOff icon, "Connection Lost", Reconnect + Go to Dashboard actions).
- `/connections` route is a dead redirect to `/` (connection management now lives on Home).

**Third-party libraries**: `lucide-react`, shadcn/Radix `dropdown-menu`/`select`/`button`/`input`/`label`, `@tauri-apps/api/core` (`invoke`), `@tauri-apps/plugin-dialog` (native file picker).

**Tauri invoke() calls**: `test_connection_command` (ConnectionForm.tsx:285), `save_password` (:295,:358), `save_ssh_password` (:302,:365), `update_connection_profile` (:350), `create_connection_profile` (:353).

**REWRITE RISK: MEDIUM-HIGH.** Form fields, selects, and segmented toggles map cleanly onto any native toolkit's widgets. Risk concentrates in the tree view's hover-reveal actions (no native "reveal on hover" idiom in most Rust GUI toolkits) and the dropdown/context menu (Radix Portal) needing a native menu equivalent. Nothing here needs a browser-only capability: it's relayout effort, not blocked functionality.

---

### 2.2 SQL Editor

**Files**: `src/components/SQLEditor.tsx` (209), `src/lib/monacoLoader.ts` (23), `src/lib/sqlAutocomplete.ts` (203), `src/hooks/useAutocompleteMetadata.ts` (80), `src/utils/sqlGuards.ts` (194), `src/components/DestructiveQueryGuard.tsx` (88), `src/components/ConfirmDestructiveDialog.tsx` (75), `src/components/QueryPanel.tsx` (572), `src/routes/_connected/query.tsx` (727), `src/contexts/TabContext.tsx` (186, tab persistence: see 2.19).

**What it does**: Monaco (`@monaco-editor/react` + `monaco-editor`, editor worker bundled locally offline via a Vite `?worker` import in `monacoLoader.ts` rather than fetched from a CDN) with `language="sql"`, minimap off, `wordWrap: 'on'`, `formatOnPaste`/`formatOnType` (effectively inert since no SQL formatting provider is registered), theme synced live to the app's light/dark/system setting (`vs`/`vs-dark`). Cmd/Ctrl+Enter is registered as a native Monaco editor action to execute; the editor becomes `readOnly` while a query runs. No multi-statement "run selection," no custom SQL formatter, no diagnostics/markers: those are just Monaco's stock keybindings (multi-cursor, comment-toggle, bracket-matching) riding along for free.
- **Autocomplete** (`sqlAutocomplete.ts`): a single global `monaco.languages.registerCompletionItemProvider('sql', ...)` registered once app-wide (idempotent, HMR-safe via a `globalThis` disposable slot) rather than per-editor-instance, fed by a mutable module-level metadata slot updated from `useAutocompleteMetadata.ts` (`get_autocomplete_metadata`, refetched on connection/database change or manual refresh). Suggestions: 69 keywords + 44 functions (inserted as callable snippets with `()`) always shown; databases/schemas always shown; tables suggested (qualified + bare forms) only after `FROM|JOIN|INTO|UPDATE`; columns suggested (qualified + bare, with data-type detail) only after `SELECT|WHERE|ON|SET|GROUP BY|ORDER BY`: all via regex-based cursor-context detection on the text before the cursor, each category given a fixed `sortText` rank.
- **Destructive-query guard** (`sqlGuards.ts`): a hand-rolled tokenizer strips `--`/`/* */` comments and quoted strings/identifiers before pattern-matching each `;`-split statement against 7 destructive shapes (delete-no-where, update-no-where, drop-table, truncate, alter-table-drop-column, drop-database, drop-schema). Gated by the "Confirm destructive queries" setting; `DestructiveQueryGuard.tsx` (`AlertDialog`) lists every offending statement with a human description and a 200-char preview; cancelling throws a `QueryCancelledError` sentinel that `QueryPanel` treats as a silent no-op (no error toast, no history entry). `ConfirmDestructiveDialog.tsx` is a second, generic reusable destructive-confirm dialog used by other flows (e.g. Drop Table in the Schema Browser).
- `query.tsx` is the actual tab-bar UI (route-level, not an inner per-panel strip): deterministic tab IDs (`query-<ts>`, `table-<schema>.<table>`, `tablefk-<schema>.<table>::<col>::<val>`, `rediskey-<key>`) map into `TabContext`; only the active tab's component is mounted at a time; New Tab (Ctrl+T)/Close Tab (Ctrl+W)/Close All with unsaved-SQL confirmation dialogs; arrow-key roving-tabindex tab navigation; clicking a foreign-key value opens/reuses a deterministic pre-filtered table tab. `QueryPanel.tsx` keeps its own in-memory `panelStateCache` (keyed by connection:database:tabId) so switching tabs preserves in-progress SQL/results/query-plan without re-executing. A "Stop waiting" affordance uses a generation counter to abandon a slow query client-side (no real backend cancel): a late result/error is silently dropped.

**Third-party libraries**: `@monaco-editor/react` (^4.7.0), `monaco-editor` (^0.54.0, raw import for worker/loader setup), `sonner`, radix `alert-dialog`/`context-menu`, `lucide-react`.

**Tauri invoke() calls**: `get_autocomplete_metadata` (useAutocompleteMetadata.ts:47), `execute_query` (query.tsx:236), `save_to_history` (QueryPanel.tsx:311,368).

**Dead code found**: `src/examples/QueryPanelExample.tsx` (234 lines) is unreferenced by any route/component (only mentioned in a README) and calls an outdated `QueryPanel` prop signature that no longer matches the live component: do not treat as a spec.

**REWRITE RISK: VERY HIGH.** Monaco's `registerCompletionItemProvider` is a rich, stateful IntelliSense API (typed `CompletionItemKind`s, snippet insertion rules, word-boundary-aware ranges, sortText-based ranking computed live per keystroke from cursor-context regexes) with no drop-in Rust equivalent: reimplementing it means hand-building a text buffer with cursor/selection model, a filtering/ranking popup widget, and multi-source suggestion merging from scratch. Additionally "free" with Monaco today: SQL tokenizing/syntax highlighting, undo/redo, IME/unicode-correct editing, and live theme swapping: all of which a native rewrite must build from zero (no Rust text-editing widget approaches Monaco's IDE-grade feature depth).

---

### 2.3 Results Grid

**Files**: `src/components/ResultsViewer.tsx` (736), `src/components/EditableCell.tsx` (225), `src/hooks/useTableEditor.ts` (630), `src/components/RowJsonViewer.tsx` (88), `src/components/QueryStatusBar.tsx` (66), `src/components/QueryErrorState.tsx` (219), `src/components/ResultsChart.tsx` (584), `src/components/QueryPlanVisualizer.tsx` (393), `src/components/TransactionPreview.tsx` (470).

Note on ownership: `ResultsViewer.tsx` (ad-hoc query results in `QueryPanel`) is **read-only**; `EditableCell`/`useTableEditor`/`RowJsonViewer`/`TransactionPreview` are consumed only by `TableInspector.tsx` (2.4): the editable-grid feature set lives entirely there, not in `ResultsViewer`.

**What it does**:
- **ResultsViewer** (read-only ad-hoc results): `@tanstack/react-table` (column model, client-side sort via header click) + `@tanstack/react-virtual` (windowed row virtualization, dynamic row-height measurement, `overscan: 10`). Four tabs: **Grid** (sticky header/row-number column, per-row/column/cell copy via double-click or hover buttons, type-aware cell rendering: NULL/undefined/object-JSON/plain, truncated at 100 chars), **JSON** (hand-rolled regex syntax highlighter, not a library, capped at 2000 rows with a "use Export for the full set" notice), **Raw** (TSV-like dump, same cap), **Chart** (lazy-loads `ResultsChart` via `React.lazy`/`Suspense` so Recharts isn't in the main bundle). CSV/JSON export via native save dialog (`@tauri-apps/plugin-dialog`) then `export_to_csv`/`export_to_json`, with a post-export "Reveal in folder" toast action (`@tauri-apps/plugin-opener`).
- **EditableCell** (Table Inspector only): double-click to edit (PK cells permanently read-only), Enter/blur commits, Escape cancels, with a `justMountedRef` guard against a false immediate-blur-commit when opened via context menu. Type coercion on commit (empty→NULL if nullable, int/float parsing, bool string-matching). Same type-aware display rendering as ResultsViewer's grid cells plus a warning-colored highlight for modified-but-uncommitted cells.
- **useTableEditor** (Table Inspector only, 630 lines): `Map`-based per-row change tracking, negative synthetic IDs for new/unsaved rows, row selection Set, and client-side **SQL statement generators** (`generateUpdateStatements`/`generateDeleteStatements`/`generateInsertStatements`) that hand-build UPDATE/DELETE/INSERT text with manual identifier quoting and string-escaping (not parameterized). INSERT generation has real heuristics: skips auto-increment/function-default columns, client-generates UUIDs (`crypto.randomUUID()`) or ISO timestamps when no DB-side default exists for a PK/created_at/updated_at column. Throws if the table has no primary key (UPDATE/DELETE require one).
- **RowJsonViewer**: side-panel single-row JSON view with its own separate hand-rolled regex syntax highlighter (a second, slightly different implementation from ResultsViewer's).
- **QueryStatusBar**: connection/db name, query-type badge, row count (falls back to rows-affected), formatted execution time, pulses while loading.
- **QueryErrorState**: headline/detail/error-code badge, copy-to-clipboard of assembled error text, collapsible original-query display, pluggable Retry/Docs/custom actions.
- **ResultsChart** (recharts): 5 chart types: Bar, Line, Area, Pie, Scatter: each its own `ResponsiveContainer` block. Auto-detects numeric vs. categorical columns (samples first 100 rows), auto-picks X (first categorical, else first column) and Y (first numeric) on first render. X-axis single-select dropdown; Y-axis multi-select checklist (Popover) restricted to numeric columns (multiple series for bar/line/area). Data is aggregated (summed) by X-value bucket, capped at 100 output points (10 for pie slices). Grid-lines/legend toggle. PNG export by serializing the live `.recharts-wrapper svg` to an off-screen canvas at 2x scale and triggering a synthetic download. Chart tooltip/background colors are hardcoded dark hex values, not theme-aware.
- **QueryPlanVisualizer**: renders EXPLAIN output as a **recursive indented div tree with colored left borders** (not a canvas/SVG node-graph): top summary cards for Planning/Execution/Total time (ANALYZE only); per-node relation/index/cost-range/estimated-and-actual-rows/actual-time/join-type/index-cond/filter/hash-cond fields shown conditionally; 3-tier cost-based color coding (<10 green, 10–100 amber, >100 red) applied to both text and node border, with a static legend; per-node and global expand/collapse. Parser is **PostgreSQL-`EXPLAIN (FORMAT JSON)`-only**: no MySQL/SQLite/MSSQL plan format support exists. Auto-triggered in `QueryPanel` whenever an executed query text starts with `EXPLAIN`.
- **TransactionPreview** (Table Inspector's commit flow): Visual tab shows a red-old/green-new diff per changed column (breadcrumbed `schema.table > pk=value > column`) for UPDATEs, a single green `+` summary line for INSERTs, and a red `-` "DELETE row" line for deletes (detected by scanning statement text, since no rich delete-row data is plumbed in); falls back to truncated per-statement summaries when no rich change data is available. SQL tab shows full un-truncated statement text with **its own third, independent hand-rolled regex SQL syntax highlighter** (VS-Code-like colors): a completely separate implementation from both JSON-highlighter instances and from Monaco. Footer: Clear All / Commit All (N), with an inline error banner on commit failure.

**Third-party libraries**: `@tanstack/react-table` (^8.21.3), `@tanstack/react-virtual` (^3.14.2), `recharts` (^3.8.1: Bar/Line/Pie/Scatter/Area chart families + Cartesian/Tooltip/Legend/ResponsiveContainer), `react-resizable-panels` (QueryPanel's editor/results and sidebar splits), `@tauri-apps/plugin-dialog` (save), `@tauri-apps/plugin-opener` (revealItemInDir), `sonner`, radix `tabs`/`alert-dialog`/`popover`/`select`/`checkbox`, `lucide-react`. No drag/reorder or column-resize state exists anywhere in this area (`useReactTable` only configures `sorting`).

**Tauri invoke() calls**: `export_to_csv` (ResultsViewer.tsx:175), `export_to_json` (:205), `save_to_history` (QueryPanel.tsx:311,368), `execute_query` (query.tsx:236); Table Inspector's `get_table_data_keyset`/`execute_query` paging calls are documented under 2.4.

**REWRITE RISK**: **VERY HIGH** for the virtualized editable grid (`EditableCell` + `useTableEditor`, 855 combined lines of type-coercing, driver-aware edit/diff/SQL-generation logic with no off-the-shelf Rust equivalent for a spreadsheet-like double-click-to-edit virtualized table) and for ResultsViewer's read-only virtualization+sort stack (**HIGH** on its own: `egui_extras::TableBuilder`-class widgets exist but won't match TanStack's ergonomics without real engineering). **HIGH** for the 5-chart-type ResultsChart (Rust has `plotters`/`egui_plot` but far less polished for interactive theming/tooltips/legends). **MEDIUM-HIGH** for QueryPlanVisualizer (the tree rendering itself is a trivial native port; the risk is the Postgres-only JSON parser: a rewrite must decide whether multi-driver plan support is in scope). **MEDIUM** for TransactionPreview (pure diff-rendering logic plus a third bespoke SQL highlighter, all portable).

---

### 2.4 Schema Browser (+ Table Inspector / Data Grid + Redis UI)

**Files**: `src/components/SchemaExplorer.tsx` (1291), `src/components/TableInspector.tsx` (1854), `src/hooks/useSchemaContext.ts` (163), `src/hooks/useTreeKeyboardNav.ts` (121), `src/components/StoredProceduresPanel.tsx` (410), `src/components/RedisSchemaTree.tsx` (313), `src/components/RedisValuePanel.tsx` (474), `src/hooks/useMetadataCache.ts` (58), `src/routes/_connected/table.$schema.$tableName/{index,route}.tsx` (31 + 20, both pass-through/redirect).

**Schema Browser behavior**: Header has a database `Select` (green-dot marker on the connected DB) and a "Database actions" popover: Create Database (Postgres/MySQL/SqlServer only, via `create_database`), View ER Diagram, Visual Query Builder, Activity Monitor, Export/Import SQL, Import CSV/Excel. Search box filters schemas/tables live. Tree: schema nodes (Collapsible + ContextMenu: Create Table, Import CSV/Excel, Refresh Tables) → lazy-loaded table/view nodes (icon distinguishes table vs view, shows row count, draggable: payload is the quoted qualified identifier for dropping into the SQL editor) with a rich context menu (View Data, Generate SELECT/INSERT, Copy Name, Import Data, Refresh, Edit Table, Drop Table with FK-dependent CASCADE detection via `get_foreign_keys`). Full WAI-ARIA `role="tree"` keyboard navigation (`useTreeKeyboardNav.ts`: arrow keys, Home/End, Enter/Space, roving tabindex). A pinned "Procedures" footer entry opens `StoredProceduresPanel` (list/search/expand-to-view-definition/execute-with-argument-inputs). Cross-component refresh via a `window` CustomEvent bus (`useMetadataCache.ts`).

**Table Inspector / Data Grid behavior** (rendered as a tab inside the query workspace, not its own route: the `/table/$schema/$tableName` route is a legacy redirect into `/query`): Tabs Data/Columns(N)/Indexes(N): no Foreign Keys/Constraints/Triggers tabs exist. Structured filter builder (column+operator+value, 12 operators including IS NULL, generates raw SQL with manual quote-escaping, not parameterized). Plain unvirtualized `<table>` grid: PK/type-badge/name column headers, per-column copy, new-row (green-tinted) + committed-row rendering, row selection with tri-state header checkbox, double-click cell to inline-edit via `EditableCell` (single-line text input regardless of type, with commit-time coercion to int/float/bool/null; no date-picker/JSON-editor/blob-viewer), double-click row number opens `RowJsonViewer`, per-cell context menu (Open Referenced Record on FK columns, Copy Cell/Row/Row-as-JSON, Edit Cell, Set to NULL, Delete Row). Bulk copy-as-JSON and bulk delete (transaction-wrapped) for selected rows. "Review Changes" opens `TransactionPreview` showing generated INSERT/UPDATE SQL before Commit (transaction-wrapped per driver dialect) or Discard. **Dual pagination strategy**: keyset (`get_table_data_keyset`, cursor-stack, single-PK tables) vs. offset (`LIMIT/OFFSET`, multi-column-PK/no-PK/MongoDB) with a 100,000-row deep-offset guard that blocks further paging and offers a "jump to max safe page" shortcut. MongoDB is explicitly blocked from editing/committing/bulk-delete. No column sort-by-click exists.

**Redis UI behavior**: `RedisSchemaTree` replaces the whole schema tree for Redis connections: five fixed type groups (Strings/Hashes/Lists/Sets/Sorted Sets), each independently lazy-scanned server-side (`SCAN ... TYPE <t>` via the generic `execute_query` bridge) with "Load more" cursor continuation and glob-pattern search. `RedisValuePanel` opens a key as a tab: TYPE badge, TTL label, type-specific read-only viewers (string with JSON auto-pretty-print, hash as field/value table, list first 200 items, set as member table, sorted set with scores first 200, generic fallback for streams/unsupported types), Delete-key with confirmation. No inline value editing exists (whole-key delete only); no TTL-set/rename actions.

**Third-party libraries**: shadcn/Radix (`Collapsible`, `Select`, `ScrollArea`, `ContextMenu`, `Popover`, `Dialog`, `Table`, `AlertDialog`, `Checkbox`), `react-resizable-panels` (row-JSON and transaction-preview side panels), `lucide-react`, `sonner`. No `@tanstack/react-table`/`@tanstack/react-virtual` anywhere in this area.

**Tauri invoke() calls**: `create_database`, `get_databases`, `get_schemas`, `get_tables`, `get_foreign_keys`, `switch_database`, `get_table_columns_for_import`, `list_procedures`, `get_procedure_definition`, `execute_procedure`, `get_table_schema`, `execute_query` (data fetch, keyset/offset paging, commit, bulk delete, and: for Redis: every raw Redis command including SCAN/GET/HGETALL/LRANGE/SMEMBERS/ZRANGE/TTL/TYPE/DEL), `get_table_data_keyset`, `get_autocomplete_metadata`.

**REWRITE RISK: VERY HIGH** (Table Inspector/Data Grid is the single most complex, most data-safety-critical UI surface in the app: editable unvirtualized grid + dirty-tracking + generated-SQL commit engine + dual pagination strategy with a hand-tuned deep-offset guard). Schema Browser tree itself is **HIGH** (exact WAI-ARIA keyboard-nav contract, two-level context menus, drag payload contract with the SQL editor). Redis UI is **MEDIUM** (simpler, read-mostly, but has real quirks: unescaped key interpolation into raw command strings, hard 200-item caps with no pagination).

---

### 2.5 ER Diagram

**Files**: `src/components/ERDiagram.tsx` (710), `src/routes/_connected/er-diagram.$schema.tsx` (53).

**What it does**: Auto-generated ER diagram for a schema. Fetches tables (`get_tables`), per-table schema in batches of 8 concurrent (`get_table_schema`), and all FKs in one call (`get_foreign_keys`); builds ReactFlow nodes/edges. **dagre** computes hierarchical top-to-bottom layout (`rankdir: 'TB'`, network-simplex ranker, dynamic per-node sizing by column count): confirmed via `dagre.graphlib.Graph()`/`setGraph`/`setNode`/`setEdge`/`dagre.layout()` at `ERDiagram.tsx:159-213`. Table nodes show up to 10 columns with PK/FK badges and data types, junction/M:N tables visually distinguished; edges use `smoothstep` routing, color/style-coded 1:N (amber, solid) vs M:N (blue, dashed) with arrowheads both ends. Manual drag-to-reposition with optional 16px grid-snap, layout persisted per-schema to a Tauri `Store` file (`erd-layouts.json`), debounced (500ms) auto-save plus flush-on-drag-stop. Zoom (5%-150%), pan, minimap (color-coded junction vs regular), show/hide relationships toggle, "Auto-layout" button to re-run dagre, Refresh, and SVG export (raw DOM-serialization of the ReactFlow viewport, not true vector SVG).

**Third-party libraries**: **reactflow** (`ReactFlow`, `Handle`, `Controls`, `MiniMap`, `Panel`, `useNodesState`/`useEdgesState`/`useReactFlow`, `Background`), **dagre**, `@tauri-apps/plugin-store`, `lucide-react`.

**Tauri invoke() calls**: `get_tables` (ERDiagram.tsx:372), `get_table_schema` (:394, batched 8-concurrent), `get_foreign_keys` (:405).

**REWRITE RISK: VERY HIGH.** Architecturally dependent on ReactFlow's interactive node/edge rendering engine (DOM+SVG hybrid, drag hit-testing, viewport pan/zoom transform, minimap sync, connection handles) and dagre's graph-layout solver (network-simplex ranking, cycle-breaking). Neither has a mature, drop-in Rust-native equivalent: no widely-used Rust crate replicates dagre's algorithm or ReactFlow's interaction model with comparable maturity.

---

### 2.6 Visual Schema Designer

**Files**: `src/components/VisualSchemaDesigner.tsx` (616), `src/routes/_connected/visual-schema-designer.tsx` (34).

**What it does**: From-scratch node-based table designer (creating brand-new tables, distinct from ER Diagram's read-mostly visualization). "Add Table" creates an empty table node at a cascading offset position (no automatic layout/dagre: 100% manual positioning). Each node is draggable with left/right `Handle`s (horizontal flow, unlike ER Diagram's top/bottom); selecting a node opens a right-side properties panel: table name/schema text inputs, per-column rows (name, 12-option type dropdown, PK/Nullable/Unique checkboxes, free-text FK target table/column). Two edge sources merge: derived edges auto-computed from FK text fields (real, semantic), and manually-drawn edges via drag-between-handles (`onConnect`/`addEdge`) which are **purely cosmetic and do not affect the generated DDL**: a known gap. "Preview SQL" calls `preview_create_table` once per table and shows combined SQL in a read-only dialog. "Create Tables" calls `create_table` sequentially per valid table with per-table success/failure toasts (no cross-table transaction).

**Third-party libraries**: **reactflow** (no dagre: layout is fully manual), `lucide-react`, `sonner`.

**Tauri invoke() calls**: `preview_create_table` (VisualSchemaDesigner.tsx:355, looped per table), `create_table` (:378, looped per table).

**REWRITE RISK: VERY HIGH.** Same ReactFlow dependency as ER Diagram, plus a strictly harder requirement: the user *creates* graph edges by dragging between connection handles (genuine node-graph-editor input, not just visualization), which is even less replicable in current Rust GUI ecosystems. Mitigated slightly by no dagre dependency (no auto-layout algorithm to port).

---

### 2.7 Table Creation / Edit Dialogs

**Files**: `src/components/TableCreationDialog.tsx` (957), `src/components/TableEditDialog.tsx` (564), `src/types/ddl.ts` (306), `src/api/ddl.ts` (121).

**What it does**: `TableCreationDialog` is a 4-step wizard (Basic Info → Columns → Constraints → Preview): table name/description/IF-NOT-EXISTS; column rows (name, 9-preset type dropdown, PK/Auto-Increment/Nullable checkboxes: no default-value or comment UI, no check-constraint UI, no index-creation UI anywhere in the frontend despite backend types supporting them); unique constraints (named, checkbox column picker) and foreign keys (checkbox source-column picker, live-loaded referenced-table dropdown via `get_tables`, live-loaded referenced-column checkboxes via `get_table_schema`, ON DELETE/UPDATE action selects); read-only SQL preview (`preview_create_table`) before "Create Table" (`create_table`). `TableEditDialog` is a 2-step wizard (Columns → Preview) for ALTER TABLE: loads current schema, supports add-column, rename (with "renamed" badge), soft-delete/restore (drop marked, greyed with "will drop" badge, blocked for PK columns), toggle-nullable (with "modified" badge): no type-change, no set-default, no new-PK-column support. Diffs drafts vs. originals into `AlterColumnOperation[]`, previews (`preview_alter_table`) then applies (`alter_table`), triggering a metadata-cache refresh on success.

**Third-party libraries**: shadcn/Radix (`Tabs`, `Card`, `Select`, `Checkbox`, `Dialog`, `ScrollArea`), `lucide-react`, `sonner`. No reactflow/dagre.

**Tauri invoke() calls**: `get_tables` (TableCreationDialog.tsx:130), `get_table_schema` (:156, and TableEditDialog.tsx:115), `preview_create_table`/`create_table` (via `src/api/ddl.ts`), `preview_alter_table`/`alter_table` (via `src/api/ddl.ts`); `src/api/ddl.ts` also exposes unused-by-these-dialogs `preview_drop_table`/`drop_table`.

**REWRITE RISK: LOW.** Standard multi-step tabbed forms with client-side validation and two backend round-trips (preview, then execute); no canvas/graph widget, animation, or drag gesture involved. Any native toolkit handles tabs/selects/checkboxes/scrollable lists comfortably.

---

### 2.8 Visual Query Builder

**Files**: `src/components/QueryBuilder.tsx` (616), `src/routes/_connected/visual-query.tsx` (152), `src/components/queryBuilder/JoinBuilder.tsx` (656), `FilterBuilder.tsx` (638), `TableSelector.tsx` (503), `GroupByPanel.tsx` (493), `ColumnSelector.tsx` (337), `OrderByPanel.tsx` (252), `LimitOffsetControls.tsx` (245), `src/hooks/useQueryBuilder.ts` (378), `src/types/queryBuilder.ts` (262), plus `src/lib/sqlGenerator.ts` (422, SELECT-statement generator/validator for this feature: not to be confused with DDL generation).

**What it does**: No-SQL construction of a SELECT query via seven collapsible sections with live item-count badges: **Tables** (searchable schema/table tree, lazy-loaded via `get_schemas`/`get_tables`, add-with-one-click fetching full column metadata via `get_table_schema`, supports selecting the same table multiple times with auto-generated aliases for self-joins, remove-with-confirmation cascades to delete dependent columns/joins/group-by); **Joins** (shown once 2+ tables selected; FK-based auto-suggestions via `get_foreign_keys` per table, one-click accept; manual join editor with type INNER/LEFT/RIGHT/FULL OUTER/CROSS and column pickers, CROSS hides columns with a Cartesian-product warning); **Columns** (two-panel available/selected picker, per-column aggregate function COUNT/SUM/AVG/MIN/MAX/COUNT-DISTINCT, alias with non-blocking uniqueness warning, per-column DISTINCT checkbox; drag-to-reorder UI exists but is a non-functional stub: `console.warn`, no actual reorder); **Filters/WHERE** (recursive AND/OR condition-group tree up to 3 levels deep, operator-and-datatype-adaptive value editors: comma-list for IN/NOT IN, two-input range for BETWEEN, no input for IS NULL, checkbox for booleans); **Group By/Having** (live validation that non-aggregated selected columns are in GROUP BY, HAVING with a restricted operator set); **Order By** (direction-toggle per clause; reorder UI defined in code but never actually wired: priority is fixed to insertion order); **Limit/Offset** (validated numeric inputs, plain-English pagination summary). Live read-only Monaco SQL preview regenerated on every state change (no debounce), copy-to-clipboard. "Execute Query" does not run the query directly: it writes the generated SQL into `localStorage` under the Tab-persistence key and navigates to `/query`, reusing or creating a "Visual Query" tab.

**Third-party libraries**: `@monaco-editor/react` (read-only SQL preview only), shadcn/Radix (`Collapsible`, `Select`, `Checkbox`, `Tooltip`, `AlertDialog`, `ResizablePanelGroup`, `ScrollArea`, `Skeleton`), `lucide-react`, `sonner`, `@tanstack/react-router`. Native HTML5 Drag-and-Drop API used (non-functionally) in `ColumnSelector.tsx`: no drag library.

**Tauri invoke() calls**: `get_schemas` (TableSelector.tsx:78), `get_tables` (:96), `get_table_schema` (:156), `get_foreign_keys` (JoinBuilder.tsx:108).

**REWRITE RISK: MEDIUM.** Core mechanics are plain form/reducer state (Selects, Inputs, Checkboxes) plus a pure-function SQL generator/validator: highly portable. Elevated by the recursive nested-WHERE condition-group UI (arbitrary-depth AND/OR trees), FK-based join auto-suggestion (driver-conditional backend plumbing), and the fact that "Execute" hands off to a separate tab system via localStorage rather than running in-place (web-specific plumbing that needs a genuinely different native mechanism, e.g. directly focusing a tab in the app's own tab manager). Two existing UI bugs (non-functional column/order-by drag-reorder) are a scope decision for the rewrite, not something to blindly reproduce.

---

### 2.9 Migrations

**Files**: `src/components/MigrationsDialog.tsx` (445), `src/types/migrations.ts` (76).

**What it does**: 3-step wizard (Select → Diff → SQL). Select: source/target connection dropdowns (from `list_connection_profiles`) + free-text schema name (no live schema list: user must type it, e.g. "public"). Diff: `compute_schema_diff` renders added/removed tables as flat colored lists and modified tables as collapsible rows showing column/index/FK-level +/-/~ changes; empty-diff shows "Schemas are identical." SQL: `generate_migration` produces statements shown in a **read-only Monaco editor**; "Run inside a transaction" checkbox (default true) and a required "I understand this will modify the target database" confirmation checkbox gate the destructive "Apply Migration" button (`apply_migration`), which reports partial-failure counts via toast. No persisted migration history/audit list exists anywhere in the frontend.

**Third-party libraries**: `@monaco-editor/react` (read-only SQL preview), shadcn/Radix (`Dialog`, `Checkbox`, `Select`, `ScrollArea`), `lucide-react`, `sonner`.

**Tauri invoke() calls**: `list_connection_profiles` (MigrationsDialog.tsx:77), `compute_schema_diff` (:89), `generate_migration` (:107), `apply_migration` (:127).

**REWRITE RISK: MEDIUM-HIGH.** The 3-step state machine itself is simple; risk concentrates in re-embedding a syntax-highlighted read-only SQL preview and faithfully reproducing the nested collapsible diff-list UX (expand/collapse per table, color-coded add/remove/modify at column/index/FK granularity) without a diff-view library. The diffing/generation logic itself lives entirely in Rust and is reused as-is.

---

### 2.10 Backup/Restore

**Files**: `src/components/BackupManagerDialog.tsx` (576).

**What it does**: Single-panel dialog (no wizard). Header shows the resolved backup directory (`get_backup_directory`) with "Open Folder" (`open_backup_directory`) and Refresh. Create-backup panel always targets the *currently connected* database (no connection picker): "Include Schema"/"Include Data" checkboxes (independently toggleable, at least one required), optional note, "Create Backup" (`create_backup`): no per-table selection UI and no custom output-directory picker exist despite the underlying options supporting them. Backup list: sortable table (file, connection, driver badge, human-formatted size, created date, Completed/Failed status with tooltip reason), per-row Restore/Delete. Restore opens an `AlertDialog` naming the exact file/target, with a "Drop & recreate database first" checkbox, blocked while restoring (`restore_backup`); Delete similarly confirmed (`delete_backup`). No progress bar/percentage for backup or restore: just a button spinner.

**Third-party libraries**: shadcn/Radix (`Dialog`, `AlertDialog`, `Table`, `Checkbox`, `Badge`), `lucide-react`, `sonner`.

**Tauri invoke() calls**: `list_backups` (BackupManagerDialog.tsx:159), `get_backup_directory` (:172), `open_backup_directory` (:187), `create_backup` (:204), `restore_backup` (:230), `delete_backup` (:245).

**REWRITE RISK: LOW-MEDIUM.** Purely a CRUD-list-plus-two-confirmations UI over 6 backend commands; no client-side algorithmic complexity. Main native-GUI cost is a sortable table widget, two modal confirmations, and OS "reveal in file manager" integration (already backend-side).

---

### 2.11 Import/Export

**Files**: `src/components/DataImportWizard.tsx` (831), `src/components/SqlImportDialog.tsx` (308), `src/components/SqlExportDialog.tsx` (221), `src/api/data-import.ts` (70), `src/types/data-import.ts` (51).

**What it does**: `DataImportWizard` is a 6-step linear wizard (select-file → preview → mapping → options → importing → complete) for CSV/XLSX/XLS: native file picker; up to 100-row preview with per-column detected types and Excel sheet-name selection; column mapping grid (auto-suggested by case-insensitive name match, 14-type dropdown, default value, per-column skip); options (first-row-header, truncate-before-import, batch size, CSV delimiter, skip-rows); a bare spinner during import with **no progress bar and no cancel** (a real gap vs. `SqlImportDialog`, which does support cancel); a result screen with imported/failed row counts and a truncated (first-10) error list. No "create new table" option is actually wired despite existing in the options type. `SqlImportDialog` is a single-screen SQL-dump importer: file picker, "use transaction" / "continue on error" mutually-exclusive checkboxes, non-dismissible while running, a "Stop" button (`cancel_import`), color-coded result (success/cancelled/partial) with an "Open error log" action via the OS default app. `SqlExportDialog` is a single-screen SQL-only exporter: schema + comma-separated table-list inputs, DROP/CREATE/INSERT inclusion checkboxes, native save dialog with a date-stamped default filename.

**Third-party libraries**: `@tauri-apps/plugin-dialog` (`open`/`save`), `@tauri-apps/plugin-opener` (`openPath`), shadcn/Radix, `lucide-react`, `sonner`.

**Tauri invoke() calls**: `preview_import_file`, `import_data_to_table`, `get_tables_for_import`, `get_table_columns_for_import` (all via `src/api/data-import.ts`), `import_from_sql` (SqlImportDialog.tsx:85), `cancel_import` (:121), `export_to_sql` (SqlExportDialog.tsx:88).

**REWRITE RISK: HIGH.** Largest surface area of the three DB-maintenance areas (~1360 lines across 3 components); a genuinely multi-step stateful wizard with derived auto-mapping logic and conditional per-file-type UI, plus two more distinct mental models (raw SQL-dump import/export) that must be unified or reimplemented three times. Native file/save dialogs (multi-filter extensions, default filenames) must be replaced with a Rust-native equivalent (e.g. `rfd`).

---

### 2.12 Activity Monitor

**Files**: `src/components/ActivityMonitor.tsx` (939, the actually-live page), `src/components/activityMonitor/QueryLogTable.tsx` (564, **orphaned/unused**), `QueryLogFilter.tsx` (432, **orphaned/unused**), `ActivityStats.tsx` (291, **orphaned/unused**), `ServerMetricsChart.tsx` (234, live, lazy-loaded), `ProcessList.tsx` (218, live, lazy-loaded), `index.ts` (3), `src/routes/_connected/activity.tsx` (24).

**Important finding**: `activityMonitor/index.ts` exports `QueryLogTable`, `ProcessList`, `ServerMetricsChart`, but the live `ActivityMonitor.tsx` only actually renders `ProcessList` and `ServerMetricsChart`: it reimplements a cruder inline query-log list and inline filters/stats instead of using the fully-built `QueryLogTable`/`QueryLogFilter`/`ActivityStats`. Those three components are dead code but represent the more polished intended design and should be treated as the real spec for a rewrite.

**What it does (live)**: Header: total-logs badge, auto-refresh play/pause toggle with interval Select (5s/10s/30s/Off), manual Refresh, Export (disabled if empty), destructive Clear Logs with confirmation. Two resizable panels: left (query log cards: type/status badges, SQL, connection/database, duration, row count: with Previous/Next pagination at fixed page size 50) and right (collapsible Live Activity/Statistics/Filters cards). Filters: connection/database selects, query-type and status checkboxes (**bug**: UI allows multi-select but only the first selected value of each is actually sent to the backend), min/max duration, SQL search text. Statistics: 2×2 stat cards (total/failed/avg-duration/total-rows) plus type/status count breakdowns. `ServerMetricsChart` (recharts `LineChart`, dual Y-axis, 3 series: Connections/Active/Tx-per-sec) polls `get_server_stats` every 5s while a connection is selected, pausing when the tab isn't visible. `ProcessList` polls `get_active_queries` every 5s, table of PID/User/DB/Client/State/Duration/Query with a per-row Kill action (`kill_query`, no confirmation).

**What it does (orphaned but well-built)**: `QueryLogTable`: full `@tanstack/react-table` implementation with manual sorting, colored type/status badges, expandable rows (full SQL + error + tags), pagination footer with page-size selector. `QueryLogFilter`: popover-based combobox filters for connection/database, collapsible query-type/status checkbox sections, duration range, date range, and a disabled "Tags: Coming soon" field. `ActivityStats`: 4 icon stat cards with K/M/B number formatting and horizontal percentage bars per query type/status (hand-rolled CSS bars, not a chart library).

**Third-party libraries**: **recharts** (`ServerMetricsChart` only: `LineChart`/`CartesianGrid`/`Legend`/`Tooltip`/`XAxis`/`YAxis`), **@tanstack/react-table** (orphaned `QueryLogTable` only), shadcn/Radix (`Select`, `AlertDialog`, `Collapsible`, `Popover`, `Checkbox`, `ScrollArea`), `lucide-react`.

**Tauri invoke() calls**: `get_query_logs` (ActivityMonitor.tsx:155), `get_activity_stats` (:178), `list_connection_profiles` (:195), `get_databases` (:203), `clear_query_logs` (:256), `export_query_logs` (:269), `get_server_stats` (ServerMetricsChart.tsx:55), `get_active_queries` (ProcessList.tsx:54), `kill_query` (ProcessList.tsx:100).

**REWRITE RISK: MEDIUM.** All data is plain structured polling/paged queries against existing commands: a native app collapses the IPC boundary to in-process calls. The line chart is directly replaceable with `egui_plot`/`plotters`; the table is standard for `egui_extras::TableBuilder` or similar. Main risk is deciding which spec to port (the polished orphaned components vs. the live placeholder) and replicating `document.visibilityState`-gated polling pause with a native window-focus equivalent.

---

### 2.13 Plugins

**Files**: `src/components/InstalledPlugins.tsx` (664), `src/components/PluginMarketplace.tsx` (435), `src/components/PluginToolbar.tsx` (74), `src/contexts/PluginContext.tsx` (192), `src/hooks/usePluginContextMenu.ts` (34, **orphaned/unused**), `src/api/plugins.ts` (107), `src/routes/plugins.tsx` (69), `src/types/plugins.ts` (174).

**What it does**: Marketplace tab: debounced search, sort (popular/recent/top-rated/most-downloaded/name), verified-only filter, 9 category tabs, plugin cards (name, verified badge, version, author, description, downloads, rating, size, keywords) with Install (`install_plugin`) and homepage link. Installed tab: master/detail list with per-row enable/disable `Switch` (disabling calls `unload_plugin_runtime` then `disable_plugin`; enabling calls `enable_plugin` then `load_plugin`), detail panel (Run default action, Settings dialog dynamically generated from the plugin's JSON-schema-like `configSchema`, Uninstall with confirmation), info/statistics/permissions cards (12 human-readable permission labels). **Plugin-contributed UI extension mechanism**: `PluginContext` listens for a Tauri event `plugin-register-ui` (backend-driven, emitted when a plugin's JS code calls a host registration API) populating `toolbarButtons`/`contextMenuItems`/`sidebarPanels`/`statusBarItems` arrays; only `toolbarButtons` is actually rendered (via `PluginToolbar`, mounted in the custom titlebar): `sidebarPanels`/`statusBarItems` have no renderer at all, and `contextMenuItems` has a consuming hook (`usePluginContextMenu`) with zero callers anywhere in the app. Plugin execution happens inside a Rust-embedded pure-Rust JS interpreter (Boa engine, confirmed via `src-tauri/Cargo.toml`), permission-gated (file/network-with-SSRF-guard/clipboard/query access). `webassembly` is a declared `PluginType` with no backend implementation (no wasmtime/wasmer dependency).

**Third-party libraries**: shadcn/Radix (`Tabs`, `AlertDialog`, `Dialog`, `Select`, `Switch`, `ScrollArea`, `Tooltip`, `Checkbox`), `lucide-react`, `sonner`, `date-fns` (install/last-used date formatting), `@tauri-apps/api/event` (`listen`).

**Tauri invoke() calls** (all via `src/api/plugins.ts`): `get_installed_plugins`, `get_plugin`, `install_plugin`, `uninstall_plugin`, `enable_plugin`, `disable_plugin`, `update_plugin_config`, `get_marketplace_plugins`, `load_plugin`, `unload_plugin_runtime`, `execute_plugin_function`, `get_loaded_plugins`, `is_plugin_loaded`.

**REWRITE RISK: VERY HIGH.** Beyond a metadata-management UI, this embeds and orchestrates a real permission-gated JavaScript interpreter with a dynamic runtime UI-extension mechanism (arbitrary plugins injecting toolbar buttons that round-trip into their sandboxed code): no established pattern exists for this in current Rust-native GUI toolkits (egui/gpui/iced/Slint). A rewrite must decide up front which speculative/half-wired extension points (sidebar panels, status-bar items, context-menu items, WASM plugin type) are real product requirements before re-implementing, since several exist only as unused type/event plumbing today.

---

### 2.14 AI Assistant

**Files**: `src/components/AiAssistant.tsx` (726), `src/api/ai.ts` (219). Mounted inside `QueryPanel.tsx`.

**What it does**: Single-shot (not multi-turn chat) request/response panel with 5 providers (Ollama, OpenAI, Anthropic, Google/Gemini, OpenRouter) selectable via dropdown, each with API-key configuration (password input, provider-specific hint links) except Ollama. Live status indicator (available/configured/unavailable) with manual refresh. Model list fetched per-provider (`list_ai_models`). Four actions, each a single non-streaming invoke: **Generate SQL** from natural language + schema context, **Explain** current SQL, **Optimize** current SQL, **Fix** current SQL using the last error message. Results render as raw preformatted text: **no markdown rendering, no syntax highlighting, no streaming**: with a regex-based extraction of a fenced ```sql block for "Apply to Editor". A collapsible panel shows the raw schema-context string sent to the AI.

**Third-party libraries**: shadcn/Radix `Select`, local `Input`/`Button`/`Badge`. No markdown library, no icon library (raw inline SVGs).

**Tauri invoke() calls** (via `src/api/ai.ts`): `check_ai_provider_status`, `check_ollama_status`, `get_ai_config`, `set_ai_config`, `set_active_ai_provider`, `set_ai_api_key`, `list_ai_models`, `ai_generate_sql`, `ai_explain_query`, `ai_optimize_query`, `ai_fix_query`, `ai_chat` (defined but unused by this component).

**REWRITE RISK: MEDIUM-HIGH.** The heavy lift (5 provider clients, config/key storage) is entirely backend. The frontend itself is plain state/fetch/render with no markdown or streaming renderer to replicate: genuinely simpler than its 726 lines suggest. Risk is in faithfully reproducing the 5-provider status/config/key flow, not UI complexity. Note: if the rewrite is expected to *add* proper markdown/code rendering, that is new scope, not parity.

---

### 2.15 Settings

**Files**: `src/components/SettingsPage.tsx` (894), `src/routes/settings.tsx` (38), `src/hooks/useSettings.ts` (58), `src/types/settings.ts` (196).

**What it does**: Four sections via sidebar nav. **General**: language (en/es/fr/de, no i18n implementation behind it), startup behavior (4 options), auto-save connections, auto-check/download/install updates with configurable check interval (1-168h). **Appearance**: theme mode (light/dark/system, applied live), accent color (color picker + hex text, not visibly wired to live CSS), editor font size/family, line numbers/minimap/word-wrap toggles. **Query Execution**: timeout seconds (0=none), max rows (100-100,000), confirm-destructive toggle, auto-save-history toggle, max history entries. **Keyboard Shortcuts**: 10 rebindable shortcuts (click-to-record next keydown, cross-platform modifier normalization, single-character keys require a modifier, non-blocking duplicate-binding warning), persisted immediately per-edit (not gated behind the page Save button) and broadcast via a custom event so other mounted consumers re-fetch live. General/Appearance/Query changes are saved in bulk via one "Save Changes" button; "Reset to Defaults" resets the entire settings object. Two settings-type fields (`defaultDatabase`, `enableTelemetry`) and two query fields (`autoCommit`, `autoFormatSql`) exist in the schema but have no UI.

**Third-party libraries**: `sonner`, shadcn/Radix (`Select`, `Card`, `Separator`, `ScrollArea`, `AlertDialog`), `lucide-react`.

**Tauri invoke() calls**: `get_settings` (SettingsPage.tsx:112, useSettings.ts:34), `update_settings` (:125 bulk, :160 per-shortcut), `reset_settings` (:138).

**REWRITE RISK: LOW-MEDIUM.** Large but mechanical form-binding over a flat settings object. The one nontrivial piece is the shortcut-recording/validation/duplicate-detection logic (~230 lines) requiring a native "capture next keydown, normalize cross-platform Ctrl/Cmd, validate, detect collisions" equivalent.

---

### 2.16 Command Palette

**File**: `src/components/CommandPalette.tsx` (450). Global state/hotkey live in `__root.tsx`.

**What it does**: Opens on hardcoded (non-rebindable) Cmd/Ctrl+K, a capture-phase `window.keydown` listener in `__root.tsx`. Search is plain case-insensitive substring matching (no fuzzy-scoring library: no `cmdk`, no `fuse.js`) against 16 static commands grouped Navigation/Actions/Theme/Window (route jumps, theme switch, window minimize/maximize/close, new connection, open Migrations, show shortcuts). Arrow-key navigation with wraparound, Enter executes, Escape closes, mouse hover syncs selection, auto-scroll-into-view. Settings/About/Plugins/Migrations are opened via the `useAppModal` overlay store rather than route navigation, preserving underlying route state.

**Third-party libraries**: `@radix-ui/react-dialog` used directly (`DialogPrimitive`) rather than via a prebuilt command-palette library, `lucide-react`, `@tauri-apps/api/window`.

**Tauri invoke() calls**: none directly; window controls go through `@tauri-apps/api/window` (`minimize`/`toggleMaximize`/`close`).

**REWRITE RISK: MEDIUM.** The command list and substring filter are trivial to port. The real complexity is the Radix Dialog/Portal/Overlay stack providing a floating, focus-stealing, ESC-dismissible overlay text input over the rest of the app: reproducing that overlay/focus/animation behavior in a native Rust GUI toolkit is the nontrivial part, not the "fuzzy" matching (which isn't actually fuzzy).

---

### 2.17 Keyboard Shortcuts (registry + help modal)

**Files**: `src/lib/shortcutRegistry.ts` (201), `src/hooks/useKeyboardShortcuts.ts` (179), `src/components/KeyboardShortcutsModal.tsx` (191).

**What it does**: `shortcutRegistry.ts` is a hand-maintained **static array** of 11 entries (despite recent commit messaging calling it a "live shortcut registry": the "live" part is only that display values are resolved against current user rebindings at render time, not that entries are dynamically registered by mounting components), grouped General/Connections/Query Workspace, with platform-correct glyph formatting (⌘⇧⌥ on Mac). `useKeyboardShortcuts.ts` provides two near-identical hooks (global and route-scoped) implementing a string-based shortcut parser/matcher with a "bare Ctrl means Cmd on Mac" cross-platform remap and an input/textarea/contentEditable guard. `KeyboardShortcutsModal` renders the registry grouped by category with a text search box (again plain substring, not fuzzy) and a live platform indicator. No conflict detection exists outside the Settings page's rebinding flow (non-blocking warning only).

**Third-party libraries**: shadcn/Radix (`Dialog`, `ScrollArea`, `Card`), `lucide-react`. No hotkey library (`hotkeys-js`/`mousetrap`/`react-hotkeys-hook`): fully hand-rolled.

**Tauri invoke() calls**: none directly (indirectly via `useSettings()` → `get_settings`).

**REWRITE RISK: LOW.** Static array, string-parsing matcher, filterable list view: the simplest of the app's cross-cutting systems. The hand-rolled `window.keydown` + focus-guard approach must be replaced with the native toolkit's accelerator API, explicitly preserving the "bare Ctrl means Cmd on Mac" remap since native OS shortcut APIs don't do this automatically.

---

### 2.18 History / Snippets / Templates

**Files**: `src/components/HistoryPanel.tsx` (277), `src/components/SnippetSidebar.tsx` (733), `src/components/SnippetFolderTree.tsx` (211), `src/components/SnippetFolderPicker.tsx` (134), `src/hooks/useSnippetFolders.ts` (149), `src/components/QueryTemplateDialog.tsx` (594), `src/components/TemplatesPanel.tsx` (379), `src/hooks/useQueryTemplates.ts` (229), `src/types/templates.ts` (229), `src/types/history.ts` (126).

**History**: List of past queries (SQL, relative/absolute timestamp, connection/database, execution time, row count, success/error). Row-limit select (25/50/100/500), implicit connection/database scoping, manual refresh, re-run action (successful entries only), clear-all with confirmation. No per-entry delete, no search box, no pinning/favoriting.

**Snippets**: Folder tree (arbitrary nesting, alphabetical sort, expand-state persisted to localStorage, search auto-expands matching branches) built via context menus (New Folder/New Snippet here, Rename, Delete-with-promote-children-to-root-not-delete). Snippet CRUD (name/query/description/tags), insert-into-editor via double-click or an "Open" menu item (single click opens Edit instead), search by name/query/description, single-select tag-pill filter, "Move to folder" via a picker dialog. **No drag-and-drop exists** despite a `moveFolder` function in the hook (dead/uncalled code): folders can only be reorganized via the picker dialog for snippets, and folders themselves cannot be re-parented from the UI at all. Two separate persistence layers: snippets live in the Rust backend (`list_snippets`/`save_snippet`/`delete_snippet`), folders/assignments live entirely in a client-side JSON file via `@tauri-apps/plugin-store`: a split worth unifying in the rewrite.

**Templates**: SQL templates with `{{param_name}}` / `{{param_name:type}}` placeholder syntax (7 types: string/integer/decimal/boolean/date/datetime/text), two-way live sync between SQL text and an auto-detected parameter list, manual parameter add/remove, per-parameter label/default/required/description config. Execute mode renders a type-appropriate fill-in form (text/textarea/date/datetime/boolean-select) with live per-field validation and a live-updating substituted-SQL preview. Flat category (single string, not hierarchical) + free-text search + tag search. Duplicate/Delete/Export-to-JSON/Import-from-JSON (dedup by name). **Zero Tauri invoke() calls anywhere**: entirely `localStorage`-backed, meaning template storage has no existing backend counterpart to port; the rewrite must design new native-equivalent storage for it.

**Third-party libraries**: shadcn/Radix (`context-menu`, `dialog`, `alert-dialog`, `select`, `checkbox`, `scroll-area`, `dropdown-menu`), `@tauri-apps/plugin-store` (Snippets folders only), `sonner` (Templates only), `lucide-react`.

**Tauri invoke() calls**: `get_query_history` (HistoryPanel.tsx:91), `clear_history` (:118), `list_snippets` (SnippetSidebar.tsx:116), `save_snippet` (:193,:237), `delete_snippet` (:269). Templates: none.

**REWRITE RISK**: History **LOW** (plain list + two backend calls + one client filter). Snippets **MEDIUM-HIGH** (recursive tree UI, two persistence layers to reconcile, context-menu-driven parent-scoped mutation). Templates **MEDIUM** (flat, no tree, no drag-and-drop: but the regex-based placeholder extraction/validation/formatting logic and the two-way SQL↔parameter live-sync must be ported byte-for-byte to avoid breaking saved templates, and storage must move off `localStorage` entirely).

---

### 2.19 Multi-window

**Files**: `src/utils/multiWindow.ts` (30), `src/utils/windowState.ts` (120), `src/contexts/TabContext.tsx` (186, per-connection-per-database tab persistence).

**What it does**: Window creation is Rust-side only (`open_database_window`); the frontend reads back a one-shot pending profile via `take_pending_window_profile` so a freshly-opened window can auto-connect once. `windowState.ts` persists per-window x/y/width/height/maximized to `localStorage`, keyed by `getCurrentWindow().label` (the `main` window keeps a legacy unsuffixed key for backward compatibility); restores on mount, debounce-saves (2s) on resize/move, force-saves on `beforeunload`. `TabContext` persists query/table/redis tab state to `localStorage` under a key scoped to `{connectionId}-{currentDatabase}`, explicitly stripping large `results` payloads before serializing (a documented perf concern) and clearing/reloading automatically when connection or database changes.

**Third-party libraries**: `@tauri-apps/api/window`.

**Tauri invoke() calls**: `open_database_window` (multiWindow.ts:19), `take_pending_window_profile` (:29).

**REWRITE RISK: LOW.** Pure window-manager plumbing (position/size/maximize state, per-window keying) that every native GUI toolkit and windowing layer (winit, native window managers) supports natively, typically better than a webview does.

---

### 2.20 Titlebar / Shell

**Files**: `src/components/CustomTitlebar.tsx` (368), `src/routes/__root.tsx` (164), `src/App.tsx` (26), `src/main.tsx` (19), `src/components/WelcomeScreen.tsx` (393), `src/components/HiveLogo.tsx` (149), `src/components/DatabaseBrandIcon.tsx` (72), `src/components/EnvironmentBadge.tsx` (37), `src/components/GlobalModals.tsx` (147), `src/components/UpdateBanner.tsx` (164), `src/hooks/useAutoUpdater.ts` (181), `src/routes/about.tsx` (39), `src/components/AboutContent.tsx` (429), `src/store/useAppModal.ts` (62).

**What it does**: `CustomTitlebar` is a `data-tauri-drag-region` bar (macOS reserves 78px left padding for native traffic lights and hides custom window buttons; other platforms get custom Minimize/Maximize-Restore/Close). Menus: **File** (New Connection, New Window, Settings, Close Window), **Schema** (connected-only: New Table, Refresh Metadata), **View** (connected-only: SQL Editor/Visual Query Builder/Schema Designer/Activity Monitor/Backup Manager, plus Plugin Manager and a Theme submenu Light/Dark/System), **Help** (Keyboard Shortcuts, Documentation/GitHub external links, About). Embeds `PluginToolbar` (2.13) and the Command Palette trigger button. `GlobalModals` renders Settings/Plugins/Migrations(lazy)/Backup Manager/About as in-place overlay dialogs driven by the `useAppModal` hand-rolled `useSyncExternalStore` store, preserving underlying route state. `WelcomeScreen` is the no-connection landing screen: animated hex-ring/shimmer/glow-pulse logo (pure CSS keyframes), primary "Connect to database" CTA, optional Recent-Connections/Sample-Workspace/Documentation actions, a 4-item keyboard cheatsheet, and a 10-entry database-type grid with availability badges. `UpdateBanner`/`useAutoUpdater` implement a configurable-interval auto-update-check flow (idle/checking/available/downloading/ready/error states, OS notifications, progress bar with byte counts, dismiss/retry/restart). `AboutContent` (shared page+modal variant) shows version, features, credits, live-versioned third-party dependency list, MIT license.

**Third-party libraries**: `@tanstack/react-router`(-devtools), `sonner`, `lucide-react`, `react-icons/si` (brand icons), shadcn/Radix (`dropdown-menu` with sub-menus, `dialog`, `tabs`, `card`, `separator`), `@tauri-apps/api/window`, `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`, `@tauri-apps/plugin-notification`.

**Tauri invoke() calls**: none directly (window/updater/notification/process calls go through dedicated Tauri plugin JS APIs rather than raw `invoke()`).

**REWRITE RISK: HIGH**, driven by distinct sub-risks: the custom titlebar's nested dropdown/submenu system (File/Schema/View→Theme/Help) is **VERY HIGH** to replicate pixel-for-pixel (most native Rust GUI toolkits lack polished nested-menu/portal primitives: likely the right call is deferring to native OS menus instead); the CSS keyframe animations (HiveLogo, WelcomeScreen decorations, UpdateBanner slide-in) are **VERY HIGH** (no direct analog without a custom animation system on the toolkit's paint loop); the command-palette trigger, toast system, and overlay-modal stack are **MEDIUM**; window controls, auto-updater, and About content are **LOW-MEDIUM** (largely literal ports of Tauri plugin calls and static content).

---

## 3. Cross-Cutting UI Infrastructure

### 3.1 Theming / Design Tokens (`src/index.css`, 383 lines)

Tailwind v4 (`@import "tailwindcss"`) + `tw-animate-css`. Dark mode is **class-based** (`@custom-variant dark (&:is(.dark *))`), toggled by adding/removing `.dark` on `<html>`: not media-query-only, so both an explicit user choice and a `prefers-color-scheme` fallback must be supported. Token layer (`@theme inline`, lines 15-77) maps semantic Tailwind utilities to CSS custom properties defined per mode: core (background/foreground/card/popover/primary/secondary/muted/accent/destructive), semantic status (success/warning/info, each with a `-foreground` pair), border/input/ring, 5 chart colors, 8 sidebar-specific colors, and a full 10-step `honey-*` (50-900) brand ramp: all in **OKLCH**. Radius scale: sm 8px / md 10px / lg 12px / xl 16px, plus a root default 12px. Light mode: near-white bg, honey primary `oklch(0.74 0.18 75)`; dark mode: charcoal bg `oklch(0.12 0.01 270)`, brighter honey primary `oklch(0.78 0.19 78)`: both confirmed matching project memory. Distinct JSON-syntax-highlight color tokens (key/string/number/boolean/null) and a separate chart palette exist per light/dark mode, specifically for the Results Grid (2.3). Font stack: "DM Sans" + system fallback globally, "JetBrains Mono" for code/pre/monospace. Global `user-select: none` on `body` with explicit re-enable for inputs/textareas/contenteditable/pre/code/`.monaco-editor`: a deliberate desktop-app UX choice. Custom (non-library) webkit/Firefox scrollbar styling. 8 custom `@keyframes` purpose-built for the HiveLogo brand animation (float, glow-pulse, hex-spin cw/ccw, shimmer, cluster-drift ×2, entrance).

### 3.2 Design System Primitives (`src/components/ui/`, 26 files)

Complete widget inventory (file, lines, what it wraps):

| File | Lines | Wraps |
|---|---|---|
| button.tsx | 59 | Custom (cva: default/destructive/outline/secondary/ghost/link × 6 sizes) + `@radix-ui/react-slot` |
| input.tsx | 24 | Custom, no Radix |
| textarea.tsx | 18 | Custom, no Radix |
| label.tsx | 22 | `@radix-ui/react-label` |
| select.tsx | 187 | `@radix-ui/react-select` |
| dialog.tsx | 141 | `@radix-ui/react-dialog` |
| alert-dialog.tsx | 155 | `@radix-ui/react-alert-dialog` |
| dropdown-menu.tsx | 255 | `@radix-ui/react-dropdown-menu` (incl. Sub-menus) |
| context-menu.tsx | 250 | `@radix-ui/react-context-menu` |
| popover.tsx | 46 | `@radix-ui/react-popover` |
| tooltip.tsx | 59 | `@radix-ui/react-tooltip` (delayDuration 0) |
| tabs.tsx | 64 | `@radix-ui/react-tabs` |
| table.tsx | 114 | Custom semantic `<table>`, no Radix |
| card.tsx | 92 | Custom, no Radix |
| badge.tsx | 46 | Custom cva + `@radix-ui/react-slot` |
| alert.tsx | 66 | Custom cva |
| checkbox.tsx | 34 | `@radix-ui/react-checkbox` (supports indeterminate) |
| radio-group.tsx | 43 | `@radix-ui/react-radio-group` |
| switch.tsx | 29 | `@radix-ui/react-switch` |
| progress.tsx | 29 | `@radix-ui/react-progress` |
| scroll-area.tsx | 56 | `@radix-ui/react-scroll-area` |
| resizable.tsx | 54 | `react-resizable-panels` (not Radix) |
| collapsible.tsx | 31 | `@radix-ui/react-collapsible` |
| separator.tsx | 26 | `@radix-ui/react-separator` |
| skeleton.tsx | 13 | Custom |
| spinner.tsx | 16 | Custom (wraps lucide `Loader2Icon`) |

20 of 26 wrap Radix; 1 wraps `react-resizable-panels`; 5 are pure custom Tailwind/cva components.

### 3.3 Error Handling

- **`src/components/ErrorBoundary.tsx`** (110): class component, `getDerivedStateFromError`/`componentDidCatch`, fallback UI with a collapsible stack trace and "Try Again"/"Reload Application" actions. Mounted once in `src/main.tsx`.
- **`src/components/ErrorState.tsx`** (129): generic reusable error display (title/message/icon/actions/variant error-warning-info) with staggered entrance animations.
- **`src/components/ConnectionLostError.tsx`** (99) and **`src/components/QueryErrorState.tsx`** (219): specializations (see 2.1 and 2.3).
- **`src/utils/formatDbError.ts`** (148): normalizes three possible `invoke()` rejection shapes (structured Rust `DbError{kind,message}`, plain string, native `Error`) into a friendly `{headline, detail?, kind?}`, with a fixed per-kind headline map (connection/query/auth/timeout/invalid_input/not_found/internal/credential/import/ai) and defensive fallbacks.
- **`src/utils/queryErrors.ts`** (20): `QueryCancelledError` sentinel so destructive-guard cancellations are never surfaced as failures.
- Demo/reference (not shipped UI, but a useful spec of every intended error-state permutation): `src/components/ErrorStateExamples.tsx` (353, dead code, unreferenced) and `src/pages/ErrorStatesDemo.tsx` (352, dead code, unreferenced).

### 3.4 Empty States

`src/components/EmptyState.tsx` (122) generic base (title/message/icon/illustration/actions/size sm-md-lg). Six specializations in `src/components/empty-states/` (`NoConnectionsEmpty` 33, `NoDataEmpty` 41, `NoHistoryEmpty` 35, `NoResultsEmpty` 42, `NoSearchResultsEmpty` 40, `NoTablesEmpty` 41; `index.tsx` 18 re-exports all), consumed live only by `SchemaExplorer.tsx`, `HistoryPanel.tsx`, and `ResultsViewer.tsx`. `EmptyStateShowcase.tsx` (271) is demo/dead code, unreferenced outside itself.

### 3.5 Toast/Notification System

`sonner` (^2.0.7), single `<Toaster richColors position="bottom-right">` mounted once in `__root.tsx`, theme-synced. 25 call sites across 24 files. Only `toast.success`/`.error`/`.warning`/`.info` are used: no `.loading` or `.promise` variants anywhere. Consistently used for "operation result" feedback (plugin lifecycle, settings save/reset, import/export outcomes, schema-object-drop confirmations, migration steps, template CRUD, update check/install): never for progress/streaming.

### 3.6 Accessibility

114 `aria-*` attribute occurrences across 52 files (90 are `aria-label`); 21 `role=` occurrences. Notable real (non-decorative) usages: a full `role="tree"/"treeitem"/"group"` accessible tree in `SchemaExplorer.tsx` with `aria-expanded`/`aria-level`, a custom `role="tablist"/"tab"/"tabpanel"` query-tab bar in `query.tsx` (distinct from the base Tabs primitive), `role="status"`/`aria-live` for async loading announcements (ActivityMonitor, UpdateBanner, QueryPanel, ResultsViewer, route-chunk loading fallbacks), and `role="combobox"` in the Activity Monitor's filter popover. Coverage is concentrated in custom interactive widgets built late in the project (per recent commit history: "aria-label sweep," "keyboard-accessible connections, schema tree navigation," "tab a11y") rather than systematic across all components. Focus trapping/return relies entirely on Radix's built-in behavior for Dialog/AlertDialog/DropdownMenu/Popover/Select: no custom focus-trap code exists, meaning a native rewrite must hand-build this for every interactive primitive.

### 3.7 Window-Scoped / Local State

- **`src/utils/windowState.ts`** (120): per-OS-window position/size/maximized persisted to `localStorage`, keyed by `getCurrentWindow().label` (`main` keeps a legacy unsuffixed key). Restore-on-mount, 2s-debounced auto-save on resize/move, force-save on `beforeunload`.
- **`src/contexts/TabContext.tsx`** (186): per-connection-per-database query/table/redis tab state (see 2.2/2.19), explicitly excludes large result payloads from persistence for performance.
- **`src/store/useAppModal.ts`** (62): a hand-rolled `useSyncExternalStore`-based global store (deliberately avoiding Zustand, per its own code comment: contradicts CLAUDE.md's stated future Zustand plan) tracking which single overlay (`settings|about|plugins|migrations|backup|null`) is open, decoupled from routing so opening a global dialog never loses in-progress editor state.

### 3.8 Contexts and Stores summary

- `src/contexts/ConnectionContext.tsx` (65): active connection/profile/database, non-persisted (2.1).
- `src/contexts/TabContext.tsx` (186): per-connection-per-database tab state, localStorage-persisted (2.2/2.19).
- `src/contexts/PluginContext.tsx` (192): plugin-contributed UI component registries + notification bridge (2.13).
- `src/store/useAppModal.ts` (62): single global overlay-modal visibility (3.7).

### 3.9 Rewrite Risk Summary for This Layer

Individually, most primitives are **LOW** risk (Button/Input/Checkbox/Switch/Card/Alert/Table markup/Skeleton/Spinner/Progress all have first-class native-toolkit equivalents). **MEDIUM**: Tabs, Collapsible, Dialog/AlertDialog modal semantics (focus trap + escape + overlay: native toolkits support the concept but exact behavior needs hand-respecification). **HIGH**: Select/DropdownMenu/ContextMenu/Popover/Tooltip (collision-aware positioning and animation-origin logic is nontrivial outside a DOM/CSS environment) and ScrollArea (custom scrollbar rendering has no consistent native-toolkit analog). **VERY HIGH**: `react-resizable-panels` (drives the app's entire core layout: sidebar/editor/results split panes: with persisted sizes and an imperative panel API; no drop-in native equivalent) and the theming system itself (CSS-custom-property + `.dark`-class + OKLCH + Tailwind `@theme inline` mapping that every other primitive depends on: in a native rewrite this becomes a whole new global style/theme registry subsystem, not a port). **Overall layer rating: HIGH**: because of universal dependency (nothing renders without it), the CSS-cascade-dependent theming mechanism having no native analog, and hand-rebuilding Radix's free accessibility/focus-management behavior for every interactive primitive.

---

## 4. Flat Parity Checklist

Every discrete user-facing capability the rewrite must reproduce, grouped by feature area.

### Connections
- [ ] User can enter a connection name
- [ ] User can pick an environment (None/Local/Staging/Production) with a colored indicator
- [ ] User can browse for a SQLite database file via native file picker, or type a path manually
- [ ] User can paste a connection string that auto-fills host/port/user/database/password
- [ ] User can enter a libSQL URL for Turso connections (no host/port fields shown)
- [ ] User can enter host/port for other drivers, and database name or Redis DB index
- [ ] User can enter username (optional for MongoDB/Redis, hidden for Turso)
- [ ] User can enter password/auth token with a show/hide toggle and a "saved to keychain" indicator
- [ ] User can select SSL mode (Disabled/Preferred/Required)
- [ ] User can toggle SSH tunneling on/off and configure host/port/username
- [ ] User can pick SSH auth method (password vs. private key) and provide either a password or a browsable key file
- [ ] User can Test Connection and see pass/fail feedback with failure detail
- [ ] User can Save Connection to create or update the profile; successful test/save silently persists secrets to the OS keyring
- [ ] User sees inline validation errors before test/save
- [ ] User can view saved connections grouped into folders (alphabetical, unfiled last), expand/collapse individually or all at once
- [ ] User can double-click, or click a dedicated icon, to connect to a saved profile
- [ ] User sees a "Connecting…" indicator, plus color-coded driver and environment badges, per connection row
- [ ] User can mark/unmark a connection as favorite
- [ ] User can edit or delete a connection profile from a per-row actions menu
- [ ] User sees a folder/connection count summary
- [ ] User sees a "Connection Lost" error screen with Reconnect and Go to Dashboard actions when a live connection drops

### Multi-Window
- [ ] User can open multiple independent OS windows, each with its own connection context
- [ ] Each window remembers its own position, size, and maximized state independently, restored on relaunch
- [ ] The main/first window keeps legacy unsuffixed state storage for backward compatibility
- [ ] A window opened for a specific connection profile auto-connects to it once on boot
- [ ] A window opened with no profile shows the connection picker

### Titlebar / Shell
- [ ] User can drag the window by the titlebar; minimize/maximize/close via custom buttons on non-macOS, native traffic lights on macOS
- [ ] User can open File menu (New Connection, New Window, Settings, Close Window), Schema menu (New Table, Refresh Metadata, connected-only), View menu (SQL Editor/Visual Query Builder/Schema Designer/Activity Monitor/Backup Manager/Plugin Manager/Theme submenu), and Help menu (Keyboard Shortcuts, Documentation, GitHub, About)
- [ ] Menu items show live, user-rebindable keyboard-shortcut hints
- [ ] User can open the Command Palette via a titlebar button or global Cmd/Ctrl+K
- [ ] User sees the Welcome/landing screen when no connection is active, with an animated hero logo, a primary "Connect to database" CTA, a keyboard-shortcut cheatsheet, and a grid of 10 supported/planned database types
- [ ] Settings, Plugins, About, Backup Manager, and Migrations open as in-place overlay dialogs that preserve underlying route/editor state (not full navigation)
- [ ] App automatically checks for updates on a configurable interval, with optional auto-download/auto-install
- [ ] User sees an in-app update banner (available/downloading-with-progress/ready/error) with dismiss/retry/download/restart actions, plus OS-level notifications
- [ ] User can manually check for updates from the About screen (page or modal variant), which shows features, credits, live-versioned dependency list, and license
- [ ] Environment badges and database-brand icons render consistently everywhere a driver/environment is shown

### SQL Editor
- [ ] User can type SQL in a syntax-highlighted editor with word-wrap on and minimap off
- [ ] User can execute via toolbar button or Cmd/Ctrl+Enter; Execute is disabled with no connection, empty text, or mid-execution
- [ ] User can clear the editor; editor becomes read-only while executing; editor theme follows the app theme live
- [ ] User can manually or automatically (on connection/database change) refresh autocomplete metadata
- [ ] User sees keyword and function (snippet-inserted) suggestions always; table suggestions after FROM/JOIN/INTO/UPDATE; column suggestions (with data type) after SELECT/WHERE/ON/SET/GROUP BY/ORDER BY; database/schema suggestions always: each in both qualified and bare forms, ranked by category
- [ ] User is warned before running DELETE/UPDATE without WHERE, DROP TABLE/DATABASE/SCHEMA, TRUNCATE, or ALTER TABLE DROP COLUMN, with a per-statement description and preview; warnings are togglable via settings and ignore keywords inside string/comment literals
- [ ] Cancelling a destructive warning silently aborts with no error/history entry; "Run anyway" proceeds
- [ ] User can abandon ("stop waiting on") a slow query client-side; a late result/error is discarded silently (no true backend cancel)
- [ ] User can open/close/close-all query and table tabs (buttons, Ctrl+T/Ctrl+W, context menu), with unsaved-SQL close confirmation and arrow-key tab navigation
- [ ] Open tabs and their SQL/results/query-plan state persist across app restarts and tab switches (scoped per connection+database)
- [ ] Only the active tab's panel is mounted at a time
- [ ] Clicking a foreign-key value opens or refocuses a deterministic pre-filtered table tab

### Results Grid
- [ ] User sees results in a sortable, virtualized table with sticky header and row-number column
- [ ] Cells render type-aware (NULL/undefined/JSON-object/plain), truncated at 100 chars with full value on hover
- [ ] User can copy a cell (double-click), a row, or a column to the clipboard
- [ ] User can export results to CSV or JSON via a native save dialog, with a post-export "Reveal in folder" action
- [ ] User can view results as pretty-printed syntax-highlighted JSON or raw TSV-like text (capped at 2000 rows with a truncation notice)
- [ ] User sees a rows-affected success message with execution time for DML, a distinct empty-results state, and a structured error state (headline/detail/error-code, copyable, with collapsible original query and Retry/Docs actions)
- [ ] User can double-click a table-browsing cell to edit it inline (except primary-key cells); Enter/blur commits, Escape cancels, with automatic type coercion (empty→NULL if nullable, int/float/bool)
- [ ] Modified-but-uncommitted cells are visually highlighted
- [ ] User can select individual or all rows for bulk copy-as-JSON or bulk delete
- [ ] User can add a new (unsaved) row and remove it before committing; auto-increment/function-default columns are excluded from generated INSERT, with client-side UUID/timestamp generation when no DB-side default exists
- [ ] User can preview all pending INSERT/UPDATE/DELETE statements (Visual diff view and raw-SQL view, each with its own syntax highlighting) before committing, or discard them all
- [ ] Commit executes all pending changes atomically in a driver-appropriate transaction, with inline error feedback on failure
- [ ] User can view a single row's full data as a formatted JSON side panel
- [ ] Table data supports keyset (cursor) pagination for single-PK tables (First/Prev/Next, no random jump) and falls back to capped offset pagination otherwise, with structured column filters (12 operators)
- [ ] User can chart result columns as Bar, Line, Area, Pie, or Scatter, with auto-detected numeric/categorical axis defaults, a multi-select Y-axis picker, gridline/legend toggles, and PNG export
- [ ] Running EXPLAIN/EXPLAIN ANALYZE automatically shows a Results/Query-Plan toggle; the plan renders as a color-coded (cost-tier), expandable/collapsible indented tree with planning/execution-time summary cards (PostgreSQL EXPLAIN JSON format only)

### Schema Browser
- [ ] User can select the active database (with a connected-DB marker) and create a new database (Postgres/MySQL/SQL Server)
- [ ] User can search/filter the schema/table tree live, and manually refresh it
- [ ] User can expand/collapse schema nodes to lazy-load tables, distinguishing tables from views by icon and showing row counts
- [ ] User can single-click to highlight, double-click to open, or drag a table (payload: quoted qualified identifier) into the SQL editor
- [ ] User can right-click a schema (Create Table, Import CSV/Excel, Refresh) or a table (View Data, Generate SELECT/INSERT, Copy Name, Import Data, Refresh, Edit Table, Drop Table with FK-dependent CASCADE warning)
- [ ] Full WAI-ARIA keyboard tree navigation (arrows, Home/End, Enter/Space, roving tabindex) works throughout
- [ ] User can open a Procedures & Functions panel: search, expand to lazily view SQL definition, execute with per-argument inputs and see a result-count toast, with the equivalent CALL/SELECT SQL insertable into the editor
- [ ] User sees contextual empty states ("Create Table" CTA for empty schemas, "Clear search" for no matches)
- [ ] User can browse Redis keys grouped into 5 fixed type buckets with incremental "Load more" scanning, glob-pattern search, and refresh
- [ ] User can open a Redis key to see its TYPE badge, TTL, and a type-specific read-only value view (string with JSON auto-pretty-print, hash table, list, set, sorted-set-with-scores, generic fallback for streams), copy the key name, and delete it with confirmation

### ER Diagram
- [ ] User can view a schema's tables auto-laid-out top-to-bottom, each showing PK/FK badges, column data types, and a "+N more" indicator past 10 columns
- [ ] Junction/many-to-many tables are visually distinguished
- [ ] User can drag to reposition a table (with optional 16px grid-snap), with manual layout persisted per schema and restored next visit
- [ ] User can re-run automatic layout, zoom (5%-150%)/pan, and use on-screen zoom/fit controls
- [ ] User sees a minimap color-coded by table type
- [ ] Relationship lines are drawn along foreign keys with labels, color/style distinguishing 1:N (amber, solid) from M:N (blue, dashed), and can be toggled on/off entirely
- [ ] User can refresh the diagram and export it as SVG

### Visual Schema Designer
- [ ] User can add a new empty table node to the canvas (cascading default position) and drag to reposition it
- [ ] User can select a table to rename it, set its schema, and add/rename/remove columns with a 12-type dropdown and PK/Nullable/Unique checkboxes
- [ ] User can set a column as a foreign key via free-text referenced table/column, which auto-draws a relationship line
- [ ] User can manually draw a connection between two table nodes by dragging between handles (cosmetic only: does not affect generated SQL)
- [ ] User can preview combined CREATE TABLE SQL for all named tables, then create them all against the live connection with per-table success/failure feedback
- [ ] User can pan/zoom the canvas and see a minimap

### Table Creation / Edit Dialogs
- [ ] User can create a table via a 4-step wizard: name/description/IF-NOT-EXISTS, columns (9 preset types, PK/Auto-Increment/Nullable), unique constraints (named, multi-column), foreign keys (source columns, live referenced-table/column pickers, ON DELETE/UPDATE actions)
- [ ] User can preview the generated CREATE TABLE SQL before executing
- [ ] User can edit an existing table: add columns, rename columns, toggle nullable, soft-delete (with restore) columns: primary-key columns cannot be dropped
- [ ] User sees a live pending-operations count and can preview the generated ALTER TABLE SQL before applying

### Visual Query Builder
- [ ] User can search/browse and add tables to the query (including the same table twice with auto-aliasing for self-joins), removing one cascades its columns/joins/group-by
- [ ] User can add JOINs manually (type, tables, columns) or accept FK-based auto-suggestions, with a Cartesian-product warning for CROSS JOIN
- [ ] User can select output columns with per-column aggregate function, alias, and DISTINCT
- [ ] User can build WHERE conditions with an operator-and-type-adaptive value editor, nested to 3 levels of AND/OR groups
- [ ] User can add GROUP BY columns (with live validation against non-aggregated selected columns) and HAVING conditions
- [ ] User can add ORDER BY clauses with a direction toggle, and set LIMIT/OFFSET with validation and a plain-English pagination summary
- [ ] User sees a live, read-only, regeneratable SQL preview with copy-to-clipboard and can collapse/expand each builder section
- [ ] User can reset the whole builder, and "Execute" hands the generated SQL to a SQL Editor tab (reusing or creating one) rather than running in place

### Migrations
- [ ] User can pick source and target connections and type a schema name to diff
- [ ] User can view added/removed tables and expandable modified-table rows showing column/index/FK-level changes
- [ ] User can generate migration SQL from the diff and preview it in a read-only editor
- [ ] User can toggle "run inside a transaction" and must confirm an explicit destructive-action checkbox before Apply is enabled
- [ ] User can apply the migration and see success/partial-failure feedback

### Backup / Restore
- [ ] User can view and open the backup storage directory, and refresh the backup list
- [ ] User can create a backup with independent schema/data toggles and an optional note (for the currently connected database)
- [ ] User can view the backup list (file, connection, driver, size, date, status) and see failure reasons
- [ ] User can restore a backup with a destructive-action warning and an optional "drop & recreate database first" toggle
- [ ] User can delete a backup with confirmation

### Import / Export
- [ ] User can import CSV/XLSX/XLS via a 6-step wizard: file picker, row/type preview (with Excel sheet selection), column mapping (auto-suggested, per-column type/default/skip), options (header row, truncate, batch size, delimiter, skip rows), and a result screen with imported/failed counts and a truncated error list
- [ ] User can import a raw SQL dump file with transaction/continue-on-error options, cancel an in-progress import, and open an error log on partial failure
- [ ] User can export a database/schema to SQL with table-list scoping and independent DROP/CREATE/INSERT inclusion toggles, via a native save dialog with a date-stamped filename

### Activity Monitor
- [ ] User can view a live/paginated query log (SQL, type, status, duration, row count, connection/database) with expandable full-SQL/error/tag detail and sortable columns
- [ ] User can filter the log by connection, database, query type(s), status(es), duration range, date range, and free-text search, and reset filters
- [ ] User can enable/disable auto-refresh with a selectable interval, manually refresh, export logs, and clear all logs with confirmation
- [ ] User can view aggregate statistics (total/failed/avg-duration/total-rows) and per-type/per-status breakdowns
- [ ] User can view a live line chart of server metrics (connections/active/transactions-per-second) and point-in-time stat tiles
- [ ] User can view and manually refresh a live active-process list (PID/user/database/client/state/duration/query) and kill a query by PID
- [ ] Background polling pauses automatically when the window isn't visible and never overlaps an in-flight request

### Plugins
- [ ] User can browse a plugin marketplace with search, category filter, sort options, and a verified-only filter
- [ ] User can install a plugin and see per-card install-state feedback
- [ ] User can view installed plugins, toggle enable/disable (triggering backend load/unload), and see full detail (author/license/category/type/install date/execution & error counts/permissions/keywords/links)
- [ ] User can run a plugin's default action and configure it via a dynamically-generated settings form
- [ ] User can uninstall a plugin with confirmation
- [ ] Plugins can register a toolbar button that invokes an action inside a sandboxed, permission-gated execution environment, and can trigger toast notifications

### AI Assistant
- [ ] User can select an AI provider (Ollama/OpenAI/Anthropic/Google/OpenRouter) and configure an API key (except Ollama)
- [ ] User sees live provider status and can select a model from a fetched list
- [ ] User can generate SQL from a natural-language prompt, explain the current query, optimize it, or fix it using the last error, each with schema context included
- [ ] User can apply a generated/fixed result back into the SQL editor and toggle a raw schema-context viewer
- [ ] User sees guided empty states when an API key is missing or Ollama isn't running

### Settings
- [ ] User can navigate 4 settings sections (General, Appearance, Query Execution, Keyboard Shortcuts)
- [ ] User can configure language, startup behavior, auto-save-connections, and update-check/download/install behavior with interval
- [ ] User can set theme mode (applied live), accent color, editor font size/family, and line-number/minimap/word-wrap toggles
- [ ] User can set query timeout, max rows, confirm-destructive toggle, auto-save-history toggle, and max history entries
- [ ] User can rebind any of 10 keyboard shortcuts by pressing a new combo, clear a binding, and see a non-blocking duplicate-binding warning
- [ ] User can bulk-save General/Appearance/Query changes or reset all settings to factory defaults with confirmation (shortcuts save immediately per-edit)

### Command Palette
- [ ] User can open the palette with Cmd/Ctrl+K and filter ~16 commands by substring match
- [ ] User can navigate results with arrow keys (wraparound) and execute with Enter, or close with Escape
- [ ] User can jump to any major screen, switch theme, control the window (minimize/maximize/close), start a new connection, or open Migrations/Keyboard-Shortcuts from the palette, grouped under labeled headers

### Keyboard Shortcuts
- [ ] User can open a searchable Keyboard Shortcuts help modal (via "?") showing shortcuts grouped by category with platform-correct symbols
- [ ] The help modal reflects the user's live rebindings, not just defaults
- [ ] Shortcuts never fire while typing in an input/textarea/contenteditable element

### History
- [ ] User can view executed-query history (SQL, relative/absolute timestamp, connection/database, execution time, row count, status) with a selectable row limit
- [ ] User can filter history to the current connection/database, refresh manually, and re-run a past successful query
- [ ] User can clear all history with confirmation, and see per-entry error messages for failures

### Snippets
- [ ] User can create/edit/delete named SQL snippets with description and tags, search by name/query/description, and filter by a single tag
- [ ] User can insert a snippet into the editor via double-click or an explicit menu action
- [ ] User can organize snippets into arbitrarily nested folders (create/rename/delete via context menu, with delete promoting children/snippets to root rather than deleting them), expand/collapse with persisted state, and move a snippet to a different folder via a picker dialog (no drag-and-drop exists)

### Query Templates
- [ ] User can author a SQL template with `{{param}}`/`{{param:type}}` placeholders, auto-synced to an editable parameter list as the SQL is typed
- [ ] User can configure each parameter's label, type, default value, and required flag
- [ ] User can browse/search templates by category or free text, and execute a zero-parameter template with one click
- [ ] User can fill in a type-appropriate parameter form with live validation and a live-updating substituted-SQL preview before executing
- [ ] User can duplicate or delete a template, and export/import templates as JSON (import skips duplicate names)

### Cross-Cutting Infrastructure
- [ ] Light/dark/system theme switching with persisted choice and live `prefers-color-scheme` detection
- [ ] A centralized, OKLCH-based design-token palette (core, semantic status, chart, sidebar, JSON-syntax-highlight, 10-step brand ramp) shared consistently across light and dark modes
- [ ] A consistent corner-radius scale and font stack (sans + monospace) applied uniformly
- [ ] Global text-selection policy: UI chrome non-selectable, editors/code/inputs selectable
- [ ] A global error boundary with fallback UI, reset, and reload actions
- [ ] A reusable generic error-state component (icon/title/message/variant/actions) and empty-state component (icon/illustration/title/message/size/actions), each with 5+ specialized variants used across Schema Browser, History, and Results Grid
- [ ] A uniform backend-error-to-UI-message formatting layer covering structured, string, and native-Error shapes, plus a cancellation sentinel that's never shown as a failure
- [ ] A global toast/notification system (success/error/warning/info) with consistent theming and positioning
- [ ] An accessible custom tree widget (schema browser) and custom tab bar (query tabs) with correct ARIA roles and keyboard nav
- [ ] Keyboard-accessible dropdown/context menus with focus trap, arrow-key nav, and typeahead
- [ ] A resizable, persisted multi-pane layout (sidebar/editor/results splits) with min/max constraints
- [ ] Per-OS-window-scoped local state keying (position/size, with legacy-key backward compatibility) and per-connection-per-database-scoped tab state that excludes large result payloads from persistence
- [ ] A lightweight global "which single overlay is open" modal store decoupled from routing
- [ ] Focus-visible styling applied consistently to every interactive primitive

