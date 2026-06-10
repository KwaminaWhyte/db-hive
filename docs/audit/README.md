# DB Hive — System Audit (2026-06-10)

Full-system audit covering security, performance, and UI/UX across the public pages (connections landing, about, plugins, settings) and the connected dashboard workspace, plus the entire Rust backend.

| Report | Findings | Worst severity |
|---|---|---|
| [Security](./security-audit.md) | 14 (2 Critical, 4 High, 5 Medium, 3 Low) | Critical |
| [Performance](./performance-audit.md) | 22 (6 High, 11 Medium, 5 Low) | High |
| [UI/UX](./ui-ux-audit.md) | 24 (6 High, 11 Medium, 7 Low) | High |

Each report contains per-finding `file:line` references and an actionable checkbox task. The execution order below is the plan for working through them.

## Execution Plan

### Phase 1 — Critical security (do first, before next release)

- [x] **SEC-01**: Stop writing database passwords in plaintext to `passwords.json` (`state/mod.rs:452`, `commands/connection.rs:588`). Route all password persistence through the OS keyring that already exists; migrate and delete any existing `passwords.json` on upgrade.
- [x] **SEC-02**: Implement SSH host-key verification (`ssh/mod.rs:31`). Persist known hosts on first connect (TOFU) and prompt the user on key change instead of accepting all keys.
- [x] **SEC-04**: Fix SQL injection in CSV/Excel import (`commands/data_import.rs:457`). Use parameterized inserts per driver; validate/quote table and column identifiers with the dialect-aware quoting added in c724c01.

### Phase 2 — High-impact stability and correctness

- [x] **PERF-03**: Enforce the 50k-row cap inside each driver while streaming, not after buffering the full result set (`postgres.rs:387`, `mysql.rs:82`, `sqlserver.rs:170`, `sqlite.rs:114`, `commands/query.rs:76`).
- [x] **PERF-05**: Stop re-serializing and fsync'ing the entire query history on every execution while holding the global state lock (`state/mod.rs:531-546`, `models/history.rs:49-63`). Cap history size and write asynchronously/debounced.
- [x] **UX-04**: Cancelling the destructive-query guard must not surface as a query error or pollute history (`routes/_connected/query.tsx:207`, `QueryPanel.tsx:297-341`). Treat cancellation as a no-op.
- [x] **UX-01**: Fix the `?` keyboard-shortcuts hotkey — `matchesShortcut` rejects Shift, but `?` requires it (`hooks/useKeyboardShortcuts.ts:84-95`).
- [x] Remaining Security High findings (SEC-03, SEC-05, SEC-06) per [security-audit.md](./security-audit.md).

### Phase 3 — Performance polish

- [x] **PERF-01/02**: Replace QueryPanel's sequential per-table schema fetch with the backend's cached `get_autocomplete_metadata` (`QueryPanel.tsx:93-163`), and lazily mount query tabs instead of mounting every tab and Monaco instance at once (`query.tsx:517-576`).
- [x] Remaining Performance High findings (PERF-12 channel streaming deferred — architectural change), then Medium, per [performance-audit.md](./performance-audit.md).

### Phase 4 — UX coherence

- [x] **UX-02**: Unify the two nested tab systems in the query workspace (route-level URL tabs vs QueryPanel's inner unpersisted tabs) so SQL is never silently lost (`query.tsx:461-515`, `QueryPanel.tsx:384-418`).
- [x] Remaining UX High findings (UX-03, UX-05, UX-06), then Medium, per [ui-ux-audit.md](./ui-ux-audit.md).

### Phase 5 — Hygiene

- [x] Run `bun audit`; updated deps + overrides cut 28 npm vulnerabilities to 3 (all build-time-only: picomatch ReDoS and jsdiff DoS via vite/router-plugin — fixes pending upstream releases). `cargo audit` not installed locally; run `cargo install cargo-audit && cargo audit` in src-tauri and re-check the GitHub Dependabot alerts.
- [ ] Security/Performance/UX Low findings as time permits.
