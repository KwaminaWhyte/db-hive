# DB Hive Performance Audit

**Date:** 2026-06-10
**Scope:** Frontend rendering (`src/components/`, `src/routes/`), state management (`src/store/`, `src/contexts/`), bundle configuration (`vite.config.ts`, Monaco/chart libraries), Rust backend drivers (`src-tauri/src/drivers/`), IPC payload handling, and application startup (`src-tauri/src/lib.rs`).

All findings reference actual code at the cited file and line numbers. Impact estimates are based on code analysis (data volumes, round-trip counts, allocation patterns), not profiler measurements.

---

## High Impact

### PERF-01: QueryPanel reimplements schema-context fetching as a sequential N+1 IPC waterfall

**Files:** `src/components/QueryPanel.tsx:93-163`

On every mount (and on every `connectionId`/`currentDatabase` change), `QueryPanel` builds an AI schema context by calling `get_schemas`, then looping over each schema calling `get_tables` (line 122), then looping over each table calling `get_table_schema` (line 129) — all sequentially with `await` inside nested `for` loops. A 200-table database means 200+ serialized IPC round-trips, each of which acquires the driver lock in Rust. The backend already solves this exact problem: `get_autocomplete_metadata` (`src-tauri/src/commands/schema.rs:225-352`) fetches the same data with bounded 16-way concurrency and caches it for 5 minutes in `MetadataCache` (`src-tauri/src/state/mod.rs:29-71`). The frontend duplication bypasses that cache entirely.

**Estimated impact:** Tens of seconds of background IPC churn after connecting to a medium-sized database; multiplied by the number of mounted query tabs (see PERF-02). On MySQL/SQL Server this also serializes against user queries (PERF-07).

- [ ] Replace the manual loop in `QueryPanel.tsx:93-163` with a single `invoke('get_autocomplete_metadata', ...)` call and derive the AI context string from its cached result.

### PERF-02: All query tabs render simultaneously, each with a full QueryPanel (Monaco, history, schema fetch)

**Files:** `src/routes/_connected/query.tsx:517-576`, `src/components/QueryPanel.tsx:420-441`

The query route renders **every** tab in `tabIds`, hiding inactive ones with `visibility: hidden` (query.tsx:545). Each query tab mounts a complete `QueryPanel`, which itself keeps every internal editor tab's Monaco instance mounted (QueryPanel.tsx:420-441, deliberate to avoid flicker), plus a `HistoryPanel`, `SnippetSidebar`, `TemplatesPanel`, and `AiAssistant`. Each mounted `QueryPanel` independently runs the PERF-01 schema-context waterfall. With 5 open tabs this is 5× the IPC storm, 5+ live Monaco editors (each ~10-30 MB heap), and 5 sets of result rows held in memory.

**Estimated impact:** Memory and CPU scale linearly with open tab count; tab-heavy sessions can hold hundreds of MB of editor + result state and trigger redundant metadata fetch storms.

- [ ] Mount only the active tab (or active ± 1) and persist inactive tab state (SQL text, results reference) outside the React tree; at minimum, hoist the schema-context fetch out of `QueryPanel` so it runs once per connection, not per tab.

### PERF-03: Drivers buffer entire result sets in memory before the 50k-row cap is applied

**Files:** `src-tauri/src/drivers/postgres.rs:387-412`, `src-tauri/src/drivers/mysql.rs:66-106`, `src-tauri/src/drivers/sqlserver.rs:148-192`, `src-tauri/src/drivers/sqlite.rs:97-135`, `src-tauri/src/commands/query.rs:23,76-81`

`MAX_RESULT_ROWS = 50_000` is enforced in `QueryExecutionResult::from_query_result` (query.rs:76-81) — **after** the driver has already collected every row. Postgres uses `client.query(sql, &[])` which materializes all rows (postgres.rs:387), then maps them all to `Vec<Vec<serde_json::Value>>` (postgres.rs:409-410). MySQL iterates `query_iter` to exhaustion (mysql.rs:82-90), SQL Server collects `into_first_result` (sqlserver.rs:170-179), SQLite loops every row (sqlite.rs:114-124). A `SELECT * FROM ten_million_row_table` allocates all 10M rows as JSON values in Rust before truncating to 50k. The code comment at query.rs:17-22 acknowledges this OOM risk but the cap does not address driver-side buffering.

**Estimated impact:** Multi-GB transient allocations and potential OOM/abort on large unbounded queries; long GC-free stalls even when it survives.

- [ ] Push the row cap into each driver (stop iterating/fetching after `MAX_RESULT_ROWS + 1` rows; for Postgres use `query_raw` and a row stream) so memory is bounded regardless of query result size.

### PERF-04: ResultsViewer computes JSON, raw-text, and syntax-highlighted strings for all view modes on every result

**Files:** `src/components/ResultsViewer.tsx:269-341, 603-617`

`resultsAsJSON` (line 269: full `JSON.stringify(jsonRows, null, 2)` after building an object per row), `resultsAsRaw` (line 284), and `highlightedJSON` (line 306: five sequential regex passes over the entire JSON string) are all `useMemo`s keyed on `[columns, rows]` — they run whenever new results arrive, even though the default view is the virtualized grid. For a 50,000-row × 20-column result this is three full materializations of a multi-megabyte string plus five regex rewrites on the main thread. The JSON tab then injects the entire highlighted string into the DOM via `dangerouslySetInnerHTML` (lines 604-610) and the Raw tab renders the full text node (lines 614-617) with no virtualization, while `TabsContent` (Radix) keeps the content mounted.

**Estimated impact:** Multi-second main-thread freeze after executing any large query, before the user has even opened the JSON/Raw tabs; tens of MB of duplicated string state per results panel.

- [ ] Compute `resultsAsJSON` / `resultsAsRaw` / `highlightedJSON` lazily — only when `viewMode` is `"json"` or `"raw"` — and virtualize or chunk the JSON/Raw views (or cap them at a few thousand rows with an export hint).

### PERF-05: Query history is unbounded and the full history file is re-serialized and written to disk on every query execution

**Files:** `src-tauri/src/state/mod.rs:476-479, 531-546`, `src-tauri/src/commands/history.rs:49-63`, `src-tauri/src/lib.rs:117-124`

`add_history` pushes onto an unbounded `Vec<QueryHistory>` (state/mod.rs:477) and `save_to_history` then calls `save_history_to_store`, which serializes the **entire** history vector with `serde_json::to_value` and writes + flushes `history.json` (state/mod.rs:531-546) — on every single query, from a synchronous command that holds the global `AppState` mutex for the duration of the disk write (history.rs:54-63). The whole file is also deserialized at startup (lib.rs:117-124). Each query entry stores the full SQL text, so a few months of usage produces a multi-MB file rewritten per query.

**Estimated impact:** Per-query latency grows linearly with history size (O(n) serialize + full-file fsync while blocking all other state access); startup slows as the file grows.

- [ ] Cap history length (e.g. retain last 1,000 entries on insert), and move persistence off the hot path — debounce writes or persist asynchronously without holding the `AppState` lock.

### PERF-06: Monaco editor is loaded from the jsdelivr CDN at runtime instead of being bundled

**Files:** `src/components/SQLEditor.tsx:2`, `src/components/QueryBuilder.tsx:9`, `src/components/MigrationsDialog.tsx:12`, `package.json:13,48`

All three editor consumers import `@monaco-editor/react` with no `loader.config` call anywhere in the repo (verified by repo-wide grep). The package's default behavior downloads the entire Monaco distribution (~3-5 MB across dozens of files) from `cdn.jsdelivr.net` the first time an editor mounts. The `monaco-editor` npm dependency (package.json:48) is only used for types (`import type * as monaco`). For a desktop app this means: editor startup blocked on a network fetch every cold launch, and the SQL editor — the core feature — is **completely broken offline**.

**Estimated impact:** 1-5 s added to first editor mount on every launch (network-dependent); total feature failure with no connectivity.

- [ ] Bundle Monaco locally: `import * as monaco from 'monaco-editor'` + `loader.config({ monaco })` (and configure the worker entry points in Vite) so no CDN fetch occurs.

---

## Medium Impact

### PERF-07: MySQL and SQL Server drivers serialize all queries through a single connection mutex

**Files:** `src-tauri/src/drivers/mysql.rs:19-21, 66-67, 289`, `src-tauri/src/drivers/sqlserver.rs:148-150`

The MySQL driver creates a `Pool` (mysql.rs:56) but `execute_query` and every metadata method lock a single shared `Arc<TokioMutex<Conn>>` (mysql.rs:67) — the pool is only used in one method (line 289). SQL Server likewise funnels everything through one `TokioMutex<Client>` (sqlserver.rs:149). One long-running user query blocks all concurrent work on that connection: autocomplete metadata, schema browsing, activity polling. Postgres, by contrast, correctly uses deadpool (postgres.rs:26-32).

**Estimated impact:** UI-visible stalls (frozen schema tree, autocomplete timeouts) whenever a slow query runs on MySQL/SQL Server connections.

- [ ] Route MySQL queries through `pool.get_conn()` per call instead of the shared `conn`, and consider a small tiberius connection pool for SQL Server.

### PERF-08: SQLite driver runs blocking rusqlite work directly on the Tokio runtime

**Files:** `src-tauri/src/drivers/sqlite.rs:25-26, 97-135`

`execute_query` takes a `std::sync::Mutex` (sqlite.rs:98) and executes the entire prepare/step loop synchronously inside an `async fn` with no `tokio::task::spawn_blocking`. A slow SQLite query (large scan, write contention) blocks a Tokio worker thread, and the held std mutex blocks any other command touching the same connection. The same pattern applies to every metadata method in the file.

**Estimated impact:** Runtime worker starvation during long SQLite queries; other async commands (even for different connections) can be delayed.

- [ ] Wrap rusqlite operations in `spawn_blocking` (move the `Arc<Mutex<Connection>>` into the closure) so blocking work leaves the async executor.

### PERF-09: Redis `sample_key_types` issues up to 100 sequential TYPE round-trips

**Files:** `src-tauri/src/drivers/redis.rs:327-372`

After a `SCAN 0 COUNT 100` (redis.rs:336-342), the driver loops over each returned key and issues an individual `TYPE` command, awaiting each one (redis.rs:346-351). That is up to 100 serialized network round-trips on every `get_schemas` call. The frontend tree (`src/components/RedisSchemaTree.tsx:124-170`) already demonstrates the right approach — `SCAN ... TYPE <t>` server-side filtering with no per-key calls.

**Estimated impact:** 100× RTT (e.g. ~2 s against a 20 ms-latency remote Redis) on every schema load of a Redis connection.

- [ ] Replace the per-key loop with a single pipeline (`redis::pipe()` of TYPE commands), or run five `SCAN 0 COUNT 1 TYPE <t>` probes instead.

### PERF-10: MongoDB `find` fetches entire collections with no limit

**Files:** `src-tauri/src/drivers/mongodb.rs:176-208`

The `find` branch drains the cursor to exhaustion (`while let Some(result) = cursor.next().await`, mongodb.rs:194) with no `.limit()` applied. `TableInspector` acknowledges this (`src/components/TableInspector.tsx:585-586`: "For now, return all documents"). Combined with PERF-03, browsing a million-document collection buffers every document as BSON → JSON in Rust and ships up to 50k rows over IPC.

**Estimated impact:** Multi-second to OOM-level cost opening any large collection.

- [ ] Apply a default `.limit(MAX_RESULT_ROWS)` in the `find` branch and add `.skip()`/`.limit()` support for TableInspector pagination.

### PERF-11: Postgres `execute_query` re-executes failing statements and re-prepares for empty results

**Files:** `src-tauri/src/drivers/postgres.rs:368-424`

Three issues in the hot path: (1) on any `client.query` error, the same SQL is executed **again** via `client.execute` (postgres.rs:414-417) — a long statement that fails late runs twice, and a partially-applied erroring DML statement is re-applied; (2) an empty result set triggers a second full `client.prepare(sql)` round-trip just to recover column names (postgres.rs:392) — `prepare` first and then `query` on the statement would give both in one pass; (3) multi-statement detection is `sql.matches(';').count() > 1` (postgres.rs:371), so a single query containing semicolons inside string literals is silently routed to `batch_execute` and returns no rows.

**Estimated impact:** Doubled execution time on failure paths (and a correctness hazard); one extra round-trip per empty SELECT.

- [ ] Prepare the statement once, derive columns from the prepared statement, and execute via the prepared handle; distinguish SELECT vs DML from statement metadata instead of the retry-on-error pattern.

### PERF-12: No streaming IPC anywhere — full result sets cross the bridge as one serde_json payload

**Files:** `src-tauri/src/commands/query.rs:38-58`, repo-wide (no `tauri::ipc::Channel` usage in `src-tauri/src/commands/`)

Despite CLAUDE.md's stated design ("Streaming Results: Use Channels to stream large datasets from Rust"), no command uses `Channel`. A 50k-row result is serialized into one JSON string in Rust, copied across the WebView boundary, and parsed into JS objects in one shot — the triple-allocation the comment at query.rs:17-22 describes. Binary data makes this worse: MySQL hex-encodes non-UTF-8 bytes (`mysql.rs:377`), doubling payload size for blobs (SQLite sensibly substitutes a `<BLOB n bytes>` placeholder, `sqlite.rs:50-52`).

**Estimated impact:** 10-100 MB single-shot IPC payloads on wide 50k-row results; main-thread JSON.parse stall in the WebView proportional to payload size.

- [ ] Stream query results in row batches over a `Channel` (e.g. 1,000-row chunks) and assemble incrementally in the UI; truncate or placeholder large binary cells server-side.

### PERF-13: ERDiagram saves layout to disk on a broken "debounce" during node drags

**Files:** `src/components/ERDiagram.tsx:284-319`

`handleNodesChange` schedules `setTimeout(... saveLayout ..., 500)` for every position change and **returns the cleanup function from the callback** (line 317) — a no-op, since `useCallback` callbacks' return values are discarded. Nothing ever clears the timers, so a one-second drag (~60 position events) queues ~60 timeouts, each triggering a `setNodes` pass plus `store.set` + `store.save()` disk write (lines 257-259). The `setNodes(currentNodes => { saveLayout(...); return currentNodes; })` pattern also performs side effects inside a state updater.

**Estimated impact:** Dozens of redundant full-layout JSON disk writes per drag gesture; visible jank while dragging on diagrams with many tables.

- [ ] Implement a real debounce (store the timer id in a ref, clear before re-arming) and save once on drag end (`onNodeDragStop`) instead of per change event.

### PERF-14: ERDiagram fetches per-table schemas with unbounded concurrency

**Files:** `src/components/ERDiagram.tsx:352-361`

`Promise.all(tables.map(... invoke('get_table_schema') ...))` fires one IPC call per table simultaneously. For a 300-table schema that is 300 concurrent commands; on MySQL/SQL Server they all serialize on the driver mutex (PERF-07), and on Postgres they compete for the 16-connection pool. The backend's autocomplete path solved this with a 16-task `JoinSet` cap (`src-tauri/src/commands/schema.rs:270-330`) and caches the result — the ER diagram bypasses both.

**Estimated impact:** Burst of hundreds of IPC calls and DB round-trips on every diagram open/refresh; duplicated work already cached for autocomplete.

- [ ] Reuse `get_autocomplete_metadata` (or add a single `get_schema_overview` command) for ER diagram data instead of N per-table invokes from JS.

### PERF-15: TabContext invalidates all consumers on any tab state change

**Files:** `src/contexts/TabContext.tsx:95-97, 124-126, 133-140`

`getTabState` and `getAllTabStates` are `useCallback`s with `[tabStates]` as a dependency, so the memoized `contextValue` (line 133) gets a new identity on **every** tab state mutation. Every `useTabContext()` consumer — the connected layout, query route, and anything else — re-renders whenever any single tab's state changes. The localStorage persistence effect (lines 85-93) also `JSON.stringify`s the entire tab-state map on each change; `TabState.results?: any` (line 15) means a careless future write of query results into tab state would synchronously serialize megabytes per update.

**Estimated impact:** Whole-route re-renders on every tab mutation today; latent multi-second stringify hazard.

- [ ] Store `tabStates` in a ref + subscription (or per-tab selector pattern) so `getTabState` is identity-stable, and exclude `results` from the persisted/serialized shape.

### PERF-16: Duplicate global Monaco completion providers — one per mounted editor

**Files:** `src/components/SQLEditor.tsx:102-120`, `src/lib/sqlAutocomplete.ts`

Each `SQLEditor` instance registers a `registerSqlAutocomplete(monacoInstance, metadata)` provider keyed on `[metadata]`. Monaco completion providers are **global per language**, not per editor — so with PERF-02 keeping N editors mounted (multiple QueryPanel tabs × internal editor tabs), N providers are live simultaneously. Every keystroke in any editor runs all N providers and produces N× duplicate suggestion lists.

**Estimated impact:** Autocomplete latency and suggestion duplication scaling with open tab count.

- [ ] Register the SQL completion provider once at module/app level and feed it current metadata via a mutable ref, instead of per-editor registration.

### PERF-17: Activity route runs three independent polling loops

**Files:** `src/components/activityMonitor/ProcessList.tsx:38,75-81`, `src/components/activityMonitor/ServerMetricsChart.tsx:37,99-105`, `src/components/ActivityMonitor.tsx:110,217-226`

While the activity route is open: `ProcessList` polls `SHOW PROCESSLIST`-style data every 2 s, `ServerMetricsChart` polls server stats every 2 s, and `ActivityMonitor` refreshes logs + stats every 10 s (default on). That is ~70 IPC calls/minute, each acquiring the driver connection — on MySQL/SQL Server these queue behind user queries (PERF-07) and on slow links the 2 s interval can fire before the previous fetch resolves (no in-flight guard in either poller).

**Estimated impact:** Constant background DB load while the tab is open; overlapping requests on slow connections.

- [ ] Add an in-flight guard (skip the tick if the previous fetch is pending), pause polling when the window/tab is not visible, and consider raising the 2 s default to 5 s.

---

## Low Impact

### PERF-18: ResultsViewer memo defeated by unstable empty-array props; column defs rebuilt on row identity

**Files:** `src/components/QueryPanel.tsx:485-492`, `src/components/ResultsViewer.tsx:170-244`

`QueryPanel` passes `activeTab.results?.columns || []` and `... || []` (QueryPanel.tsx:486-487), creating fresh array identities on every parent render while results are null — so the `memo(ResultsViewerComponent)` wrapper (ResultsViewer.tsx:631) never bails out in the empty state. Inside, `columnDefs`' `useMemo` lists `rows` as a dependency (line 244) only because `copyColumnValues` closes over it, so all column definitions (and the TanStack table) are rebuilt on every result set even when columns are unchanged.

- [ ] Hoist stable `EMPTY_COLUMNS`/`EMPTY_ROWS` constants in QueryPanel, and read `rows` through a ref inside the copy callbacks so `columnDefs` depends only on `columns`.

### PERF-19: ResultsChart performs setState inside useMemo and re-aggregates on every input

**Files:** `src/components/ResultsChart.tsx:105-116, 119-154`

Auto-selection of axes runs `setXAxisColumn`/`setYAxisColumns` inside a `useMemo` (lines 105-116) — a side effect in render that triggers an immediate second render pass. `chartData` (lines 119-154) iterates all rows on each recompute; acceptable at the 50k cap but it runs even when the Chart tab is not visible if inputs change (mitigated today because the chart only mounts in its tab). Severity is low; correctness of the hook usage is the main concern.

- [ ] Move axis auto-selection into a `useEffect` (or derive defaults without state) to avoid render-phase setState.

### PERF-20: RedisSchemaTree renders unvirtualized key lists and re-scans on every search keystroke

**Files:** `src/components/RedisSchemaTree.tsx:197-210, 271-289`

Each expanded group renders all accumulated keys as plain divs (lines 271-289); repeated "Load more" (300 keys per SCAN, line 23) can build multi-thousand-node lists. The `searchQuery` effect (lines 197-210) fires a SCAN per expanded group on every keystroke with no debounce — typing "users" issues up to 5 groups × 5 keystrokes = 25 SCAN commands.

- [ ] Debounce the search-triggered rescan (~300 ms) and virtualize the key list once it exceeds a few hundred entries (reuse `@tanstack/react-virtual`, already a dependency).

### PERF-21: Single-chunk bundle with zero route-level code splitting

**Files:** `vite.config.ts:32-43`, repo-wide (no `React.lazy`/dynamic `import()` in `src/`)

Manual vendor chunking was deliberately removed (vite.config.ts comment) after it caused circular-chunk init bugs — reasonable for a desktop app where network transfer is free. However, with no `lazy()` anywhere, the WebView still parses and evaluates the entire bundle — recharts, reactflow + dagre/graphlib, the query builder, import wizard, ER diagram — before the first connection screen paints. Parse/eval of a ~4 MB+ JS bundle is CPU cost on every window open (and each extra window in the multi-window model repeats it).

- [ ] Add route-level `lazy()` boundaries for heavy, rarely-first-used surfaces (ER diagram / VisualSchemaDesigner [reactflow+dagre], ResultsChart/ServerMetricsChart [recharts], DataImportWizard) — dynamic `import()` avoids the eager-evaluation ordering problem that manual chunking had.

### PERF-22: Synchronous store loads in Tauri setup before first window paint

**Files:** `src-tauri/src/lib.rs:88-141`

`setup` synchronously loads profiles, passwords, query history, and snippets from their JSON stores (lib.rs:93-136) before `app.manage` completes. Profiles and snippets are small, but `history.json` is unbounded (PERF-05) and is fully read + deserialized here, delaying window creation as it grows. Plugin initialization is correctly deferred to a spawned task (lib.rs:158-163).

- [ ] Defer history loading to a background task (or load lazily on first history query) once PERF-05's cap is in place; startup then stays O(small) regardless of usage age.

---

## Prioritized Task Checklist

**Do first (High):**
- [ ] PERF-01 — Replace QueryPanel's sequential per-table schema fetch with the cached `get_autocomplete_metadata` command.
- [ ] PERF-02 — Stop mounting every query tab (and its Monaco editors) simultaneously; mount active tab only.
- [ ] PERF-03 — Enforce the 50k row cap inside each driver instead of after full materialization.
- [ ] PERF-04 — Compute JSON/Raw/highlighted result strings lazily per view mode; virtualize or cap JSON/Raw views.
- [ ] PERF-05 — Cap query history length and move full-file persistence off the per-query hot path.
- [ ] PERF-06 — Bundle Monaco locally via `loader.config` to eliminate the runtime CDN fetch.

**Next (Medium):**
- [ ] PERF-07 — Use the MySQL pool per query; un-serialize SQL Server access.
- [ ] PERF-08 — Move SQLite work onto `spawn_blocking`.
- [ ] PERF-09 — Pipeline Redis TYPE probes in `sample_key_types`.
- [ ] PERF-10 — Default-limit MongoDB `find` and add skip/limit pagination.
- [ ] PERF-11 — Prepare-once Postgres execution; remove retry-on-error double execution.
- [ ] PERF-12 — Stream query results over Tauri Channels in row batches.
- [ ] PERF-13 — Fix the ERDiagram layout-save debounce; save on drag end.
- [ ] PERF-14 — Replace ERDiagram's N-per-table invokes with one cached metadata command.
- [ ] PERF-15 — Make TabContext getters identity-stable; exclude results from serialization.
- [ ] PERF-16 — Register one global SQL completion provider fed by a metadata ref.
- [ ] PERF-17 — Guard and pause activity-monitor polling loops.

**Later (Low):**
- [ ] PERF-18 — Stabilize ResultsViewer empty props and columnDef dependencies.
- [ ] PERF-19 — Move ResultsChart axis auto-selection out of `useMemo`.
- [ ] PERF-20 — Debounce Redis tree search; virtualize long key lists.
- [ ] PERF-21 — Add route-level lazy boundaries for reactflow/recharts surfaces.
- [ ] PERF-22 — Defer history store loading out of app setup.
