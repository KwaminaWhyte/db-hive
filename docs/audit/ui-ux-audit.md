# DB Hive UI/UX Audit

**Date:** 2026-06-10
**Auditor:** Claude Code (automated code review)

## Scope

This audit covers both user-facing surfaces of DB Hive:

- **Public/marketing surface:** the connections landing page (`src/routes/index.tsx`), About (`src/routes/about.tsx`, `src/components/GlobalModals.tsx`), Plugin Manager (`src/routes/plugins.tsx`, `src/components/PluginMarketplace.tsx`), Settings (`src/routes/settings.tsx`), `ConnectionForm`, `CustomTitlebar`, `UpdateBanner`, and the (unused) `ConnectionDashboard` / `ConnectionList` / `EnhancedConnectionList` / `ConnectionCard` components.
- **Connected workspace:** `src/routes/_connected/*` (query, table, ER diagram, activity, visual query, schema designer), `QueryPanel`, `ResultsViewer`, `SchemaExplorer`/`ConnectionTreeView`, `HistoryPanel`, `AiAssistant`, `CommandPalette`, `KeyboardShortcutsModal`, `BackupManagerDialog`, `DataImportWizard`, `RedisValuePanel`, `GlobalModals`, and the `EmptyState`/`ErrorState` component families.

Audit dimensions: theme-token consistency, accessibility, loading/error/empty states, destructive-action feedback, UX flows, responsiveness/layout, and copy.

**What is working well:** destructive-action confirmation coverage is strong (`DestructiveQueryGuard`, `ConfirmDestructiveDialog`, AlertDialogs in `TableInspector`, `RedisValuePanel`, `HistoryPanel`, `BackupManagerDialog` all confirm before destroying data); the command palette has solid keyboard navigation and proper sr-only Dialog titles; `SchemaExplorer` uses skeletons plus the `NoTablesEmpty`/`NoSearchResultsEmpty` components; the connections landing page has loading skeletons and a distinct first-run empty state.

---

## High Severity

### UX-01 — The `?` keyboard-shortcuts hotkey can never fire

**Files:** `src/hooks/useKeyboardShortcuts.ts:84-95` (`matchesShortcut`), `src/routes/__root.tsx:62,71-77`, `src/components/CustomTitlebar.tsx:286`

`parseShortcut("?")` produces `{ key: "?", shift: false }`, but typing `?` on every layout requires Shift, so the keyboard event arrives with `event.shiftKey === true`. `matchesShortcut` requires `event.shiftKey === shortcut.shift`, so the comparison always fails and the shortcuts modal never opens via `?`. The Help menu (`CustomTitlebar.tsx:286`) and `KeyboardShortcutsModal` both advertise `?` as the binding, so users are taught a shortcut that does not work — directly undermining shortcut discoverability.

- [ ] Fix `matchesShortcut` to ignore the Shift modifier when the shortcut key is a shifted character (e.g. `?`), or compare against `event.key` only for punctuation keys; add a unit test for the `?` binding.

### UX-02 — Two nested, conflicting tab systems in the query workspace

**Files:** `src/routes/_connected/query.tsx:461-515` (route-level tab bar), `src/components/QueryPanel.tsx:61-71, 384-418` (internal tab bar)

The `/query` route renders a tab bar (URL-driven, persisted to `localStorage`) and each "query" tab mounts a `QueryPanel` which renders a *second* tab bar with its own "Query 1 / +" tabs (`tab-1`, module-level `tabIdCounter`). Users see two stacked tab strips that look alike but behave differently: the inner tabs are not persisted, not in the URL, not restored on database switch, and their unsaved SQL is silently lost when the outer tab closes. This is the single most confusing structural element of the workspace.

- [ ] Remove the inner `QueryPanel` tab system (or collapse it into the route-level tab system) so there is exactly one tab model, persisted and URL-addressable.

### UX-03 — Saved connections can only be opened by double-click; row and menu are keyboard-invisible

**Files:** `src/routes/index.tsx:561-566` (row `div` with `onDoubleClick` only), `src/routes/index.tsx:596-606` (kebab `opacity-0 group-hover:opacity-100`)

The connection row is a plain `div` with `onDoubleClick` and a `title="Double-click to connect"` tooltip as the only affordance. There is no single-click action, no visible Connect button, no `role`/`tabIndex`/Enter handling — a keyboard user cannot connect at all from this list, and a mouse user must discover double-click by accident. The row's actions menu trigger is `opacity-0` until hover, so it is invisible to keyboard focus.

- [ ] Make the row a focusable `button` (or add `role="button" tabIndex={0}` + Enter/Space handlers) that connects on activation, add a visible Connect affordance on hover/focus, and use `focus-visible:opacity-100` alongside `group-hover:opacity-100` on the kebab trigger.

### UX-04 — Cancelling the destructive-query guard is reported as a query *error* and pollutes history

**Files:** `src/routes/_connected/query.tsx:202-209` (`throw new Error("Query cancelled by user")`), `src/components/QueryPanel.tsx:297-341`

When the user clicks Cancel in `DestructiveQueryGuard`, `handleExecuteQuery` throws. `QueryPanel.handleExecute` catches it, paints the results pane with a red `QueryErrorState` ("Query cancelled by user"), and saves a **failed query history entry** (`QueryPanel.tsx:320-340`). Cancelling a guard is the user doing the right thing; it should be a no-op, not an error plus a permanent failure record in history.

- [ ] Use a sentinel (e.g. a `QueryCancelledError` class or `null` return) for guard cancellation; in `QueryPanel.handleExecute`, skip both the error state and the history write when execution was user-cancelled.

### UX-05 — Systemic missing accessible names on icon-only buttons

**Files (representative, not exhaustive):** `src/components/UpdateBanner.tsx:140-145` (dismiss X), `src/routes/_connected/query.tsx:480-489` (tab close X, also `opacity-0` until hover), `src/components/QueryPanel.tsx:399-407` (tab close X, no label *and* no title), `src/routes/index.tsx:598-605` (connection kebab), `src/components/CustomTitlebar.tsx:339-359` (min/max/close use `title` only), `src/components/ConnectionForm.tsx:601-613, 766-778` (password visibility toggles), `src/components/PluginMarketplace.tsx:172-174` (search icon button), `src/components/KeyboardShortcutsModal.tsx:196-201` (clear-search X), `src/routes/_connected/route.tsx:224-235` (sidebar toggle)

The codebase contains 33 `size="icon"` Buttons plus many raw `<button>` elements with icon-only content, but only 10 `aria-label` attributes exist in the entire `src/` tree. Screen-reader users get unlabeled buttons throughout both surfaces; several (tab close buttons) are additionally `opacity-0` until mouse hover, making them unreachable by sight for keyboard users.

- [ ] Add `aria-label` to every icon-only button (sweep `size="icon"` and raw `<button>` with single Lucide child); pair every `opacity-0 group-hover:opacity-100` reveal with `focus-visible:opacity-100`.

### UX-06 — Widespread hardcoded Tailwind palette colors violate the theme-token rule

The project rule is explicit: use theme tokens (honey-gold primary), never hardcoded classes like `bg-orange-500`/`bg-blue-600`. Violations found (grep of `bg-{color}-{n}`, plus text/border variants at the same sites):

| Location | Violation |
|---|---|
| `src/components/ErrorState.tsx:53-63` | `text-orange-500`, `text-blue-500`, `bg-orange-500/10`, `bg-blue-500/10` for warning/info variants — the shared error component itself bypasses tokens |
| `src/components/UpdateBanner.tsx:44-50, 75` | `bg-blue-500/15`, `text-blue-400`, `bg-emerald-500/15`, `text-emerald-400`, progress bar `bg-blue-500` |
| `src/components/AiAssistant.tsx:641, 649, 657` | Explain/Optimize/Fix buttons in `bg-blue-500/10`, `bg-green-500/10`, `bg-red-500/10` (also raw `<button>` instead of shadcn `Button`) |
| `src/routes/index.tsx:576-584` and `src/routes/_connected/route.tsx:246-252` | Environment badges with `bg-red-500/20`, `bg-yellow-500/20`, `bg-green-500/20` — duplicated verbatim in two files |
| `src/components/ConnectionForm.tsx:420-432, 828` | Environment dots `bg-green-500/yellow/red`; success banner `bg-green-50 dark:bg-green-950/20 text-green-600` |
| `src/components/ResultsViewer.tsx:321-339, 390` | Inline hex colors (`#fb923c`, `#60a5fa`, `#4ade80`, `#c084fc`, `#f87171`) in the JSON view regardless of theme (poor contrast on light theme), `text-green-600` on DML success |
| `src/components/activityMonitor/ActivityStats.tsx:43-61`, `QueryLogTable.tsx:59-116` | Full rainbow of `bg-{blue,green,yellow,red,purple,indigo,pink,cyan,gray}-500` status chips |
| `src/components/queryBuilder/JoinBuilder.tsx:64-72, 428, 618` | Join-type colors `bg-blue/green/orange/purple/gray-500`, `bg-amber-50 dark:bg-amber-950` callouts |
| `src/components/empty-states/*.tsx` (NoConnections:30, NoTables:38, NoData:38, NoHistory:32, NoResults:38, NoSearchResults:36) | Per-component accent tints `bg-blue-950`, `bg-emerald-50`, `bg-cyan-50`, `bg-violet-50`, `bg-amber-50`, `bg-slate-50` |
| `src/components/DataImportWizard.tsx:802` | `border-zinc-700` hardcoded regardless of theme |
| `src/components/WelcomeScreen.tsx`, `CustomTitlebar.tsx:118-127`, `about.tsx:182-191`, `GlobalModals.tsx:234-243` | Logo built from `amber-300/400/500` + `slate-950` (arguably brand artwork, but duplicated four times — see UX-19) |

Semantic status colors (success/warn/error) are a legitimate need, but they should be defined once as tokens (e.g. `--success`, `--warning`, `--info`) rather than ad hoc per component — today the same "success" is `green-500`, `green-600`, `emerald-400`, or `text-green-600 dark:text-green-400` depending on the file.

- [ ] Add semantic status tokens (success/warning/info + environment badge palette) to the theme and replace every hardcoded palette class listed above.
- [ ] Extract a shared `<EnvironmentBadge>` component to kill the `index.tsx`/`route.tsx` duplication.
- [ ] Theme the ResultsViewer JSON highlighter with CSS variables (or reuse Monaco colorization) so light mode is readable.

---

## Medium Severity

### UX-07 — Closing a single tab discards unsaved SQL with no confirmation

**Files:** `src/routes/_connected/query.tsx:242-302` (`handleCloseTab`), `src/components/QueryPanel.tsx:193-223`; contrast with `query.tsx:377-389` where "Close All" *does* check for unsaved work

"Close All Tabs" warns when any tab has SQL, but closing one tab (X button, context menu, or the `Ctrl+W` shortcut at `query.tsx:452-455`) silently destroys that tab's SQL. A mis-aimed `Ctrl+W` wipes work with no undo.

- [ ] Apply the same unsaved-work check from `handleCloseAll` to single-tab close (confirm dialog or a brief "Tab closed — Undo" toast that restores the tab state).

### UX-08 — Raw backend error strings surfaced verbatim; test-connection failure gives no detail

**Files:** `src/routes/index.tsx:150-151, 188, 261-262, 279-280`, `src/components/ConnectionForm.tsx:303, 306-307, 357-358`, `src/components/QueryPanel.tsx:299` → `QueryErrorState`

Errors are stringified Rust/driver output prefixed with "Failed to …: " (e.g. raw libpq/sqlx messages with host/port internals). Meanwhile `ConnectionForm.tsx:303` does the opposite: a failed test shows only "Connection failed" with *no* reason at all. There is no mapping from the structured `DbError { kind, message }` shape (which the architecture defines) to friendly guidance, and `QueryErrorState` supports `errorCode`/docs links that callers never populate.

- [ ] Create a shared `formatDbError(err)` helper that switches on `err.kind` to produce a human headline + collapsible raw detail; use it in `index.tsx`, `ConnectionForm`, and `QueryPanel`, and pass `errorCode` through to `QueryErrorState`.
- [ ] Include the underlying reason in the test-connection failure message (`ConnectionForm.tsx:303`).

### UX-09 — Keyboard Shortcuts modal is hardcoded, stale, and contradicts actual bindings

**File:** `src/components/KeyboardShortcutsModal.tsx:52-152`

The modal documents "Clear Editor ⌘K" while ⌘K globally opens the Command Palette (`__root.tsx:28-41`) — a direct conflict in the documentation. The "Welcome Screen" section lists "New Connection ⌘K", but the landing page binds ⌘N (`index.tsx:116-126`); "Recent Connections ⌘R" and "View Sample ⌘O" describe features that do not exist. The list also ignores `settings.shortcuts` (users can rebind New Tab/Close Tab in Settings, `query.tsx:443-444`, and the modal will show the defaults).

- [ ] Rebuild the modal's data from a single shortcut registry that also feeds `useRouteShortcuts`/settings, and delete the fictional "Welcome Screen" entries.

### UX-10 — Connected-layout auth guard navigates during render

**File:** `src/routes/_connected/route.tsx:62-66`

`if (!connectionId || !connectionProfile) { navigate({ to: "/" }); return null; }` runs inside the render body. React forbids side effects during render; under StrictMode/concurrent rendering this double-fires and can produce a flash of broken UI or a router warning. The `beforeLoad` hook above it (`route.tsx:26-30`) is an empty stub.

- [ ] Move the redirect into a `useEffect` (or wire connection state into router context so `beforeLoad` can `throw redirect(...)`).

### UX-11 — Schema sidebar is fixed-width with no resize handle

**File:** `src/routes/_connected/route.tsx:283-299` (`w-80` / `w-0` only)

The query area uses `react-resizable-panels` (`QueryPanel.tsx:377`), but the schema explorer is a hard `w-80` (320px). Long table/column names truncate with no way to widen; on small windows 320px is a third of the screen with no way to narrow short of fully collapsing.

- [ ] Wrap the sidebar + outlet in a `PanelGroup` with a `PanelResizeHandle` (persist the size per window, keyed by `getCurrentWindow().label` per the multi-window rule).

### UX-12 — Exports give no success feedback; running queries cannot be cancelled

**File:** `src/components/ResultsViewer.tsx:112-167` (export), `344-355` (loading)

CSV/JSON export shows a toast only on failure; on success the dialog closes and nothing confirms the file was written or where. The loading state is a spinner with "Executing query..." and no Cancel action, so a runaway query locks the tab until it finishes.

- [ ] Add `toast.success` with the file path (and an "Open folder" action) after export.
- [ ] Add a Cancel button to the query-loading state wired to a backend `cancel_query` command (or at minimum let the UI abandon the wait).

### UX-13 — QueryPanel loads the entire database schema via N+1 calls on every mount

**File:** `src/components/QueryPanel.tsx:93-163`

For the AI assistant's context, every `QueryPanel` instance (one per route-level tab) serially invokes `get_schemas`, then `get_tables` per schema, then `get_table_schema` per table. On a database with hundreds of tables this is hundreds of IPC round-trips per tab, on mount, even if the AI tab is never opened. A stray `console.log` of the context remains at line 154.

- [ ] Load schema context lazily (first time the AI tab is activated), reuse the existing metadata cache (`useMetadataCache`), and remove the `console.log`.

### UX-14 — Settings/About "Back" depends on a sessionStorage key nothing ever writes

**Files:** `src/routes/settings.tsx:16-23`, `src/routes/about.tsx:39-46`; no writer of `db-hive-previous-route` exists anywhere in `src/`

Both full-page routes read `sessionStorage.getItem("db-hive-previous-route")` to return the user to where they came from, but a repo-wide search finds no code that ever *sets* the key (the titlebar/palette now open these screens as modals instead). The Back button therefore always falls through to `window.history.back()` or `/`, and `about.tsx:48-74` carries 25 lines of dead route-matching logic.

- [ ] Either write the key when navigating to the full-page routes, or delete the sessionStorage logic and use router history alone.

### UX-15 — Four parallel, unused connection-list implementations

**Files:** `src/components/ConnectionDashboard.tsx` (697 lines), `EnhancedConnectionList.tsx` (493), `ConnectionList.tsx` (476), `ConnectionCard.tsx` (169) — none imported by any route; only `ConnectionList` is re-exported from `src/components/index.ts:6`. The live UI is a fifth implementation, inline in `src/routes/index.tsx:520-655`.

These dead components contain features the live list lacks (favorites, folders, color tags, last-connected metadata — fields that exist on `ConnectionProfile`, see `ConnectionForm.tsx:253-258` where they are always saved as `null`/`false`). Keeping them invites divergence and confuses contributors about which list is real.

- [ ] Delete (or consciously revive) the unused components; if features like favorites/folders are wanted, port them into the live list in `index.tsx` instead.

### UX-16 — Schema tree has no keyboard navigation

**File:** `src/components/SchemaExplorer.tsx` (1,240 lines, a single `onKeyDown` at line 1114; no `role="tree"`, no roving tabindex, expand/collapse is click-only)

The primary navigation surface of the connected workspace cannot be traversed with arrow keys, contradicting the tree-view conventions users expect from database clients.

- [ ] Add `role="tree"`/`role="treeitem"` + `aria-expanded`, and arrow-key navigation (Up/Down move, Right expand, Left collapse, Enter open table).

### UX-17 — Platform-incorrect shortcut copy and misleading "Exit" menu item

**Files:** `src/routes/_connected/route.tsx:238` (tooltip hardcodes "(Cmd+B)" on all platforms), `src/components/CustomTitlebar.tsx:163-165` ("Exit" calls `appWindow.close()` — closes only the current window in a multi-window app), `CustomTitlebar.tsx:265-269` (the "Window" menu contains only "Settings")

On Windows/Linux the sidebar tooltip still says Cmd+B. "Exit" implies quitting the app, but with multiple windows open it closes one window. Settings living under "Window" is an information-architecture surprise (convention: File/Edit menu or app menu).

- [ ] Use the `isMacOS` check for the sidebar tooltip; rename "Exit" to "Close Window" (and/or add a true Quit via the process plugin); move Settings out of the "Window" menu.

---

## Low Severity

### UX-18 — Copy inconsistencies

**Files:** `src/routes/index.tsx:672` ("DB Hive.app" in the footer — reads like a leftover placeholder), `index.tsx:593` (driver names rendered `.toLowerCase()` so "PostgreSQL" shows as "postgresql"), `index.tsx:441-443` vs `about.tsx:198-199` (tagline is "The Modern Database Desktop App" on the landing page but "A Professional Cross-Platform Database Client" in About), `src/hooks/useQueryTemplates.ts:4` (`dbhive_query_templates` vs the `db-hive-*` key convention used elsewhere).

- [ ] Fix the footer string, stop lowercasing driver display names, pick one tagline, and standardize the storage key prefix (with a migration read of the old key).

### UX-19 — About screen and logo duplicated wholesale

**Files:** `src/routes/about.tsx:142-419` vs `src/components/GlobalModals.tsx:149-446` (features list, credits table with pinned version numbers, update-check logic, and the 20-line CSS logo all duplicated); the same hand-built logo also appears in `CustomTitlebar.tsx:116-131` and `WelcomeScreen.tsx:48-86`.

Pinned dependency versions in two places (`about.tsx:153-164`, `GlobalModals.tsx:217-228`) will drift from `package.json` and from each other.

- [ ] Extract a shared `<AboutContent>` and a `<HiveLogo size>` component; derive credit versions from `package.json` at build time.

### UX-20 — Plugin Marketplace polish

**File:** `src/components/PluginMarketplace.tsx:164-174` (search fires only on Enter/button, uses deprecated `onKeyPress`), `110-125` (after install, the card still shows "Install" — no installed-state, so users can re-install repeatedly), `253-256` (loading is a bare spinner rather than skeleton cards, inconsistent with the landing page's `Skeleton` pattern).

- [ ] Debounce search on change (replace `onKeyPress`), track installed plugin IDs to render a disabled "Installed" state, and use skeleton cards while loading.

### UX-21 — Click-to-copy on every results cell is a surprising side effect

**File:** `src/components/ResultsViewer.tsx:231-241`

A single left-click on any cell copies it and fires a toast. This collides with click-to-select/drag-select expectations and makes casual clicking noisy. Copy already exists per row/column buttons.

- [ ] Move cell copy to a context-menu item or double-click, and keep single click side-effect-free.

### UX-22 — UpdateBanner is not announced to assistive tech

**File:** `src/components/UpdateBanner.tsx:34-35`

The banner appears via `position: fixed` with no `role="status"`/`aria-live="polite"`, so screen readers never hear that an update is available/downloading/failed. (Its unlabeled dismiss button is covered by UX-05.)

- [ ] Add `role="status" aria-live="polite"` to the banner container.

### UX-23 — Custom tab bars lack tab semantics

**Files:** `src/routes/_connected/query.tsx:463-502`, `src/components/QueryPanel.tsx:385-409`

Both hand-rolled tab strips are clickable `div`s: no `role="tablist"`/`role="tab"`, no `aria-selected`, no arrow-key switching (the shadcn `Tabs` used elsewhere, e.g. `ResultsViewer.tsx:461`, provides all of this). Largely subsumed by the UX-02 consolidation, but whichever tab bar survives needs semantics.

- [ ] Give the surviving tab bar `tablist`/`tab` roles, `aria-selected`, and Left/Right-arrow tab switching.

### UX-24 — Minor flow nits

- `src/routes/index.tsx:336-343`: typing in the "New Connection" connection-string field instantly navigates away to the form the moment a protocol prefix matches, yanking focus mid-paste/typo; prefer parsing on paste/blur or a "Continue" button.
- `src/routes/_connected/route.tsx:267-276`: Disconnect is a single click on a slim bar with no confirmation; an in-flight query's results are lost. Consider confirming when a query is running.
- `src/routes/index.tsx:285-288` (`handleCopyDetails`): copies silently — add a confirmation toast like `ResultsViewer` does.
- `src/components/GlobalModals.tsx:62-75`: Settings renders inside a Dialog *and* exists as a full route; the modal's Escape-close can swallow nested dialog Escapes — verify nested-dialog focus behavior in `SettingsPage`.

---

## Prioritized Task Checklist

**P0 — broken or rule-violating**
- [x] UX-01: Fix Shift handling so the `?` shortcut opens the shortcuts modal (`useKeyboardShortcuts.ts`).
- [x] UX-04: Stop reporting destructive-guard cancellation as a failed query and stop writing it to history.
- [x] UX-06: Introduce semantic status tokens and remove hardcoded palette classes (ErrorState, UpdateBanner, AiAssistant, env badges, ResultsViewer hex colors, ActivityMonitor, JoinBuilder, empty-states, DataImportWizard).
- [x] UX-02: Collapse the nested QueryPanel tab system into the route-level tabs.

**P1 — accessibility and data-loss**
- [ ] UX-05: aria-label sweep over all icon-only buttons; pair hover-reveals with `focus-visible`.
- [ ] UX-03: Keyboard-accessible connection rows with a visible Connect affordance.
- [x] UX-07: Unsaved-work protection (or undo) for single-tab close.
- [ ] UX-16: Arrow-key navigation + tree roles in SchemaExplorer.
- [ ] UX-08: `formatDbError` helper; include detail in test-connection failures.

**P2 — correctness and consistency**
- [ ] UX-09: Generate the shortcuts modal from the live shortcut registry; remove fictional entries.
- [ ] UX-10: Move the connected-layout redirect out of render.
- [ ] UX-11: Resizable schema sidebar via PanelGroup.
- [ ] UX-12: Export success toasts; cancellable query loading state.
- [ ] UX-13: Lazy AI schema context using the metadata cache.
- [ ] UX-17: Platform-aware shortcut copy; rename "Exit"; relocate Settings menu item.
- [ ] UX-14: Fix or delete the `db-hive-previous-route` back logic.
- [ ] UX-15: Remove or revive the four dead connection-list components.

**P3 — polish**
- [ ] UX-18: Copy fixes (footer, driver casing, tagline, storage-key prefix).
- [ ] UX-19: Deduplicate About content and the CSS logo; derive credit versions from package.json.
- [ ] UX-20: Marketplace search debounce, installed state, skeleton loading.
- [ ] UX-21: Replace single-click cell copy with context menu/double-click.
- [ ] UX-22: `aria-live` on UpdateBanner.
- [x] UX-23: Tab semantics on the surviving tab bar.
- [ ] UX-24: Connection-string auto-navigation, disconnect confirmation, copy-details toast, nested Settings dialog Escape behavior.
