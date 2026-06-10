# DB Hive Security Audit

**Date:** 2026-06-10
**Auditor:** Defensive security review (owner-requested hardening)
**Target:** DB Hive — Tauri 2.0 + React 19 cross-platform database client
**Commit context:** branch `main`, version `0.21.0`

## Scope

This audit covers the application's trust boundaries and sensitive subsystems:

1. Rust backend Tauri commands (`src-tauri/src/commands/`): connection, query, backup, export, data import, plugins, window — input validation, path traversal, SQL/command injection.
2. Credential handling: OS keyring usage, password lifecycle, plaintext persistence, secret exposure to the frontend.
3. Plugin system (`src-tauri/src/plugins/`, `boa_engine` JS runtime, marketplace): sandbox boundaries, granted APIs, manifest validation, remote-install integrity.
4. Tauri configuration (`tauri.conf.json`, `capabilities/default.json`): CSP, permission scope, updater configuration.
5. AI integration (`src-tauri/src/ai/`): API-key storage, data sent to third-party providers, TLS.
6. SSH tunneling (`russh`): host-key verification, key/passphrase handling.
7. Frontend: XSS sinks, sensitive data in `localStorage`.
8. Dependencies: `Cargo.toml` / `package.json` posture (GitHub reports 29 Dependabot alerts).

**Methodology:** Manual source review with targeted searches. No dynamic testing or exploitation was performed. Line references reflect the reviewed revision and may shift as code changes.

---

## Summary of findings

| Severity | Count |
|----------|-------|
| Critical | 2 |
| High     | 4 |
| Medium   | 5 |
| Low      | 3 |

---

## Critical

### SEC-01 — Database passwords stored in plaintext on disk

**Files:**
- `src-tauri/src/state/mod.rs:414` (`load_passwords_from_store`), `src-tauri/src/state/mod.rs:452` (`save_passwords_to_store`)
- `src-tauri/src/commands/connection.rs:385-386` (`save_password`), `src-tauri/src/commands/connection.rs:588-591` (`connect_to_database`)
- `src-tauri/src/lib.rs:105` (loads them at startup)

**Description:**
Although a proper OS-keyring credential manager exists (`src-tauri/src/credentials/mod.rs`), every connection password is *also* written verbatim to a `passwords.json` Tauri store file in the app data directory. `save_passwords_to_store` serializes the in-memory `HashMap<String, String>` of connection IDs to plaintext passwords and persists it to disk (`store.set("passwords", ...)`, `store.save()`). `connect_to_database` and `save_password` both call this on every connect/save, and `lib.rs` reloads it on launch. The code comments acknowledge this is "a temporary solution … stored in plaintext."

**Impact:**
Any process running as the user, any backup/sync tool, any malware, or anyone with read access to the app data directory can recover *all* saved database credentials in cleartext — completely defeating the keyring. This is the single highest-value asset in the app and it is stored in the clear.

**Fix task:**
- [ ] Remove `save_passwords_to_store` / `load_passwords_from_store` entirely and rely solely on the OS keyring (`CredentialManager`). Keep passwords in the in-memory `connection_passwords` map only for the active session; never serialize them to a store file. Delete any existing `passwords.json` on upgrade (migration shim) and document the change in the changelog.

### SEC-02 — SSH tunnel performs no host-key verification (MITM)

**File:** `src-tauri/src/ssh/mod.rs:31-40` (`SshClientHandler` / `check_server_key`)

**Description:**
`SshClientHandler` implements `russh::client::Handler` but leaves `check_server_key` unimplemented (commented out with a `FIXME` and the note "For now, accept all keys (useful for development)"). The crate default (`russh 0.45`, `src/client/mod.rs:1466`) returns `Ok(false)`, which actually *rejects* the connection — so the intended fix, per the FIXME, is to return `Ok(true)`, i.e. accept *any* server key. There is no comparison against `known_hosts` or any trust-on-first-use pinning.

**Impact:**
SSH tunnels are used specifically to reach databases that are not directly exposed. Accepting any host key means an attacker positioned between the client and the bastion (rogue Wi-Fi, compromised network, DNS/ARP spoofing) can transparently man-in-the-middle the SSH session and capture the database credentials and all tunneled traffic. This nullifies the security benefit of using SSH at all.

**Fix task:**
- [ ] Implement `check_server_key` to verify the server public key against the user's `~/.ssh/known_hosts` (and/or a profile-pinned fingerprint). On first connection, surface the fingerprint to the user for explicit approval (TOFU) and persist it. Never unconditionally return `Ok(true)`. Reject unknown/changed keys with a clear error.

---

## High

### SEC-03 — Database passwords leaked via process command line (mysqldump/mysql/mongodump)

**Files:**
- `src-tauri/src/commands/backup.rs:181` (`mysqldump … -p{password}`)
- `src-tauri/src/commands/backup.rs:308` (`mysql … -p{password}` on restore)
- `src-tauri/src/commands/backup.rs:221` (`mongodump --password <pw>`)

**Description:**
For MySQL backup/restore and MongoDB backup, the password is passed as a command-line argument (`-p{password}`, `--password <pw>`). On most operating systems, process arguments are visible to all local users via `ps`, `/proc/<pid>/cmdline`, or process-listing APIs. (PostgreSQL is handled correctly via the `PGPASSWORD` env var, lines 149 and 284.)

**Impact:**
Any local user or process can read the database password for the duration of the dump/restore by listing processes. On shared or multi-user machines this is a straightforward credential disclosure.

**Fix task:**
- [ ] Pass MySQL credentials via a temporary defaults-extra-file (`--defaults-extra-file=`) or the `MYSQL_PWD` env var, and MongoDB credentials via `--config`/stdin/`MONGODB_*` env, instead of argv. Ensure any temp file is created `0600` and deleted promptly. Avoid `-p<pw>` / `--password <pw>` on the command line.

### SEC-04 — SQL injection via CSV/Excel data import

**File:** `src-tauri/src/commands/data_import.rs:411-467` (`import_data` table-name interpolation and value escaping at line 457)

**Description:**
The importer builds raw SQL by string interpolation rather than parameterized queries (explicitly noted in the code comment at line 433). Three issues compound:
- Cell values are escaped only by doubling single quotes (`value.replace('\'', "''")`, line 457). This is insufficient for MySQL/MariaDB, where backslash is an escape character by default — a value such as `\'` lets the closing quote be escaped, breaking out of the string literal.
- The table name (`full_table_name`, line 411) and target column names (line 430) are interpolated unquoted and unvalidated into `INSERT INTO {table} ({cols}) …` and `TRUNCATE TABLE {table}` (line 418).
- Import sources are commonly untrusted files (a CSV/XLSX received from a third party).

**Impact:**
A crafted spreadsheet cell or column header can inject arbitrary SQL executed against the connected database (data exfiltration, modification, or destruction within the connection's privileges). Because import files are frequently received from outside parties, the attacker need not have direct app access.

**Fix task:**
- [ ] Use parameterized/prepared statements for row values (bind parameters per driver) instead of string interpolation. Quote identifiers per dialect (reuse the dialect-aware quoting added in commit `c724c01`) and validate table/column names against the actual schema before use. At minimum, escape backslashes as well as quotes for MySQL until parameterization lands.

### SEC-05 — Content Security Policy disabled

**File:** `src-tauri/tauri.conf.json:33-35` (`"security": { "csp": null }`)

**Description:**
The application ships with no CSP (`csp: null`). The WebView therefore permits inline scripts, arbitrary remote script/connect/img sources, etc. The frontend renders database-derived content and uses `dangerouslySetInnerHTML` (see SEC-13), and bundles Monaco; a single injection or a compromised dependency has no second line of defense.

**Impact:**
With no CSP, any XSS (from result data, a dependency, or a plugin-rendered surface) escalates to full use of the granted Tauri command surface — which includes arbitrary file write/delete, process restart, and credential-bearing commands. CSP is the primary mitigation that is currently absent.

**Fix task:**
- [ ] Define a strict CSP in `tauri.conf.json` (e.g. `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' https://openrouter.ai https://api.openai.com https://api.anthropic.com …; img-src 'self' data:`). Scope `connect-src` to the AI/updater endpoints actually used. Verify Monaco works under the policy (it may need a worker/`blob:` allowance) and avoid `unsafe-eval`.

### SEC-06 — Plugins installed and executed without integrity or permission enforcement

**Files:**
- `src-tauri/src/plugins/manager.rs:143-212` (`install_plugin` — `hash`/`signature` never checked; remote download is a TODO at line 177)
- `src-tauri/src/plugins/manager.rs:391-400` (`check_permission` is a no-op `Ok(())`)
- `src-tauri/src/plugins/runtime.rs:517-582` (`httpRequest` — unrestricted network egress) and `:418-476` (clipboard), file APIs `:215-304`
- `src-tauri/src/plugins/mod.rs:106-126` (permission enum incl. `NetworkAccess`, `RunCommand`)

**Description:**
The `MarketplacePlugin` model carries `hash`, `signature`, and `verified` fields, but `install_plugin` never validates any of them; the actual remote download from `download_url` is an unimplemented TODO (currently it copies a bundled source or writes a placeholder). `PluginManager::check_permission` is a stub that returns `Ok(())` for everything ("For now, we'll allow all permissions"). Plugins declare their own permissions in their manifest with no install-time user consent. The boa runtime grants permission-gated APIs based purely on those self-declared flags, including `httpRequest` (arbitrary outbound HTTP with no SSRF/allowlist restriction), clipboard read/write, and file read/write.

**Impact:**
When remote install is wired up, plugins will be fetched and run with no signature or checksum verification — a tampered or malicious package would execute unchecked. Even today, a plugin can self-grant `NetworkAccess` and use `httpRequest` to exfiltrate any data it can reach (including to internal/localhost endpoints) with no user approval. The "verified/signature" surface gives a false impression of vetting.

**Fix task:**
- [ ] Before any remote plugin install, verify the downloaded artifact's SHA-256 against `hash` and verify `signature` against a trusted publisher key over HTTPS; refuse install on mismatch. Replace the `check_permission` stub with real enforcement and require explicit user consent for sensitive permissions (`NetworkAccess`, `RunCommand`, `WriteFiles`, `AccessClipboard`) at install time. Add an egress allowlist (or at least block private/loopback ranges) to `httpRequest`.

---

## Medium

### SEC-07 — AI provider API keys held in plaintext and returned to the frontend; schema sent to third parties

**Files:**
- `src-tauri/src/commands/ai.rs:38-48` (`AiState` holds keys in a plaintext in-memory `AiConfig`), `:156-163` (`get_ai_config` returns full config incl. `api_key` to the WebView), `:189-208` (`set_ai_api_key`)
- `src-tauri/src/ai/openrouter.rs` (and sibling providers): keys sent as bearer tokens; `ai_generate_sql`/`ai_optimize_query` send `schema_context` to the provider

**Description:**
AI API keys are stored as plaintext strings in `AiConfig` and are returned wholesale to the frontend by `get_ai_config` (the `apiKey` fields are part of the serialized config consumed by `AiAssistant.tsx`). Keys are not stored in the OS keyring like DB passwords are. Separately, SQL-assistance commands forward `schema_context` (table/column names) — and potentially query text — to the configured third-party provider (OpenRouter/OpenAI/Anthropic/Google). Transport itself is HTTPS (`https://openrouter.ai/api/v1`, etc.), which is good.

**Impact:**
Returning secrets to the WebView widens their exposure (any XSS or logging of the config leaks them, especially with CSP disabled — see SEC-05). Database schema metadata leaving the machine to a third party may violate data-handling expectations for sensitive environments.

**Fix task:**
- [ ] Store AI API keys in the OS keyring (reuse `CredentialManager`); have `get_ai_config` return a redacted/`hasKey` boolean instead of the raw key. Keep keys server-side (Rust) only. Add a clear in-app disclosure (and ideally an opt-in toggle) before sending schema/query context to external AI providers.

### SEC-08 — Argument injection in backup/restore CLI invocations

**Files:** `src-tauri/src/commands/backup.rs:162-164` (table args to `pg_dump`), `:190-192` (table args to `mysqldump`), and host/username/database args throughout `create_backup`/`restore_backup`

**Description:**
Backup invocations append profile- and option-derived strings (table names, host, username, database) as individual `arg()` values to `pg_dump`/`mysqldump`/`mongodump`. While this avoids a shell (so no classic shell-metacharacter injection), values that begin with `-`/`--` are interpreted by the target tool as options rather than data. A table name like `--help`, or a crafted value mapping to a real dangerous flag, can alter tool behavior. `restore_backup` also runs `psql -f <file>`, executing whatever SQL the file contains.

**Impact:**
Lower than a shell injection, but an attacker who controls a profile field or table list can manipulate the external tool's behavior (e.g. force unintended options). Combined with arbitrary-path file handling (SEC-10), restore can execute attacker-chosen SQL.

**Fix task:**
- [ ] Insert a `--` end-of-options separator before positional/table arguments where the tools support it, and reject values beginning with `-` for identifier fields. Validate table/host/username/database against expected character sets before passing them to external processes.

### SEC-09 — SQL export value escaping is incomplete (backslash handling)

**File:** `src-tauri/src/commands/export.rs:539-547` (`sql_value_to_string`)

**Description:**
Generated SQL dumps escape string values by doubling single quotes only (`s.replace('\'', "''")`), with no handling of backslashes. For MySQL/MariaDB (backslash is an escape character), this produces dumps that can be malformed or, on re-import, allow a value to break out of its string literal — mirroring SEC-04 on the export side. JSON/array/object values are stringified and quoted the same way.

**Impact:**
A row containing backslashes/quotes can corrupt the exported SQL and become an injection vector when the dump is imported back (especially via `import_from_sql`). Data round-trips may silently break.

**Fix task:**
- [ ] Make value serialization dialect-aware: escape backslashes for MySQL and prefer driver-native escaping or `E'...'`/hex-literal encodings as appropriate. Add tests covering values containing `\`, `'`, newlines, and NUL.

### SEC-10 — Tauri commands allow arbitrary filesystem read/write/delete by path

**Files:**
- `src-tauri/src/commands/backup.rs:346-349` (`delete_backup` removes any `file_path`), `:46-56` (`list_backups` reads any directory), `:289`/`:331` (restore reads/copies any path)
- `src-tauri/src/commands/export.rs:118` / `:225` (`export_to_csv`/`export_to_json` create files at any path), `:611` (`import_from_sql` opens any path)

**Description:**
These commands accept a fully attacker-influenced `file_path`/`directory` from the WebView with no confinement to an allowed directory. `delete_backup` will `remove_file` on any path; the export commands will `File::create` (overwrite) any path; `list_backups`/import will read any path/dir. The intended caller is the native file dialog, but the commands themselves impose no constraint.

**Impact:**
If the WebView is ever compromised (XSS — made more likely by the absent CSP, SEC-05), these commands become primitives for arbitrary file overwrite, deletion, and disclosure within the user's privileges. Defense-in-depth gap.

**Fix task:**
- [ ] Validate that paths passed to backup/export/import commands resolve within expected directories (canonicalize and check prefix), or route file selection exclusively through the dialog plugin's returned handles. Reject path traversal and refuse to operate outside an allowlisted root.

### SEC-11 — Dependency vulnerabilities not triaged (29 Dependabot alerts)

**Files:** `src-tauri/Cargo.toml`, `package.json`

**Description:**
GitHub reports 29 Dependabot alerts; no `cargo audit` / `bun audit` baseline is present in the repo or CI. The dependency surface includes several historically advisory-prone crates: `rsa` (RUSTSEC Marvin timing advisory, pulled transitively by SQL/TLS stacks), `time`/`chrono`, `native-tls`/`openssl`, `mysql_async`, `tiberius`, `mongodb`, `boa_engine 0.19`, and `russh 0.45`. Frontend pins include `monaco-editor`, `reactflow`, and Radix packages.

**Impact:**
Unpatched transitive vulnerabilities (timing side-channels, parsing DoS, TLS issues) may be exploitable depending on usage. Without an audit baseline, regressions go unnoticed.

**Fix task:**
- [ ] Run `cargo audit` (add `cargo-audit` and `cargo-deny` to CI) and `bun audit` / `npm audit`; triage all 29 Dependabot alerts, upgrade or patch affected crates/packages, and add the audit step to the release workflow so new advisories fail CI.

---

## Low

### SEC-12 — Hardcoded developer absolute path in plugin loader

**File:** `src-tauri/src/plugins/manager.rs:221` (`PathBuf::from("/home/kwamina/Desktop/others/db-hive/plugins")`)

**Description:**
`find_bundled_plugin` probes a hardcoded developer home path as a candidate plugin source location. This is a leftover dev artifact that leaks a developer username/layout and clutters the install path search.

**Fix task:**
- [ ] Remove the hardcoded absolute path; resolve bundled plugins only from the packaged resource directory (and a relative dev path during development).

### SEC-13 — Query result data rendered via `dangerouslySetInnerHTML`

**Files:** `src/components/ResultsViewer.tsx:606` (with `escapeHtml` at `:308-315`), `src/components/RowJsonViewer.tsx:81` (with `escapeHtml` at `:28-35`)

**Description:**
Both JSON viewers inject HTML strings built from query-result data via `dangerouslySetInnerHTML`. Currently each path HTML-escapes (`&`, `<`) *before* applying regex-based syntax highlighting, which neutralizes tag injection — so this is presently mitigated. It remains a fragile pattern: future edits to the highlighter, or escaping that omits a character, would reintroduce stored XSS from malicious cell contents. With CSP disabled (SEC-05), the blast radius would be large.

**Fix task:**
- [ ] Replace manual escape-then-regex HTML construction with a sanitizer (e.g. DOMPurify) or token-based React rendering (no raw HTML). Add a test asserting that a cell value like `<img src=x onerror=alert(1)>` renders inert.

### SEC-14 — Verbose backend/database errors forwarded to the frontend

**Files:** `src-tauri/src/commands/backup.rs:169-171, 197-199, 230-232` (raw tool `stderr` returned), plus `DbError::QueryError` paths broadly

**Description:**
Raw `stderr` from external tools and underlying driver error strings are returned verbatim to the UI. These can include connection strings, hostnames, file paths, and schema details.

**Impact:**
Information disclosure that aids reconnaissance; low direct risk but useful to an attacker and potentially exposed in logs/screenshots.

**Fix task:**
- [ ] Map backend errors to user-facing messages and log full detail only locally (behind a debug flag). Avoid echoing raw `stderr`/driver internals into the UI.

---

## Prioritized task checklist

**Critical (do first):**
- [x] SEC-01: Stop writing DB passwords to `passwords.json`; use the OS keyring exclusively and purge any existing plaintext store.
- [x] SEC-02: Implement SSH host-key verification (known_hosts / TOFU pinning); never accept all keys.

**High:**
- [x] SEC-03: Stop passing MySQL/MongoDB passwords on the command line; use defaults-file/env.
- [x] SEC-04: Parameterize import inserts and quote/validate identifiers; fix backslash escaping.
- [x] SEC-05: Define a strict Content Security Policy in `tauri.conf.json`.
- [x] SEC-06: Verify plugin checksum/signature before install; enforce permissions with user consent; restrict plugin `httpRequest` egress.

**Medium:**
- [ ] SEC-07: Move AI API keys to the keyring; redact them from `get_ai_config`; disclose schema sent to AI providers.
- [ ] SEC-08: Add `--` option terminators and reject `-`-prefixed identifiers in backup/restore CLI calls.
- [ ] SEC-09: Make SQL export value escaping dialect-aware (backslashes for MySQL).
- [ ] SEC-10: Confine backup/export/import file paths to allowed directories.
- [ ] SEC-11: Run `cargo audit` + `bun audit`, triage the 29 Dependabot alerts, and add audit gates to CI.

**Low:**
- [ ] SEC-12: Remove the hardcoded developer path from the plugin loader.
- [ ] SEC-13: Sanitize/replace `dangerouslySetInnerHTML` usage with DOMPurify or token rendering.
- [ ] SEC-14: Sanitize backend error messages surfaced to the UI.
