# Backend surface: commands, Tauri coupling, state, drivers

Recorded 2026-09-08 against `src-tauri/src` (23,404 LOC across 61 files by
`wc -l`, matching the 23,397 baseline in `00-baseline-measurements.md` to
within normal drift). This is the reference for anyone replacing the React
WebView frontend with a native Rust GUI while keeping the Rust core: what
the core currently exposes as `#[tauri::command]`s, exactly where it touches
Tauri APIs, and how much of the 23k LOC survives a shell swap untouched.

Method: every `commands/*.rs` file was read in full (not sampled); every
Tauri-coupling category was grep-verified directly (`grep -rn` across
`src-tauri/src` for `AppHandle`, `State<`, `Manager`, `.emit(`, `Emitter`,
`WebviewWindow`, `tauri::Window`, `Channel<`, `tauri_plugin`,
`app_data_dir`/`StoreBuilder`/`.path()`), and cross-checked against
`lib.rs`'s `invoke_handler!` list, which registers exactly 101 commands
from `commands/*.rs` plus one leftover demo command (`greet`, `lib.rs:41`,
not part of any module, not documented in the catalogue below since it
isn't in `commands/*.rs`).

## 1. Command catalogue

101 `#[tauri::command]` functions across 16 files in `src-tauri/src/commands/`.
Counts per file (verified by `grep -c "tauri::command"` and by counting
entries in `lib.rs`'s `generate_handler!` list, which match exactly):
connection 18, ai 12, plugins 13, activity 7, ddl 7, history 7, backup 6,
schema 6, export 5, data_import 4, migrations 3, monitoring 3, procedures 3,
settings 3, query 2, window 2. Total 101.

### connection.rs (18 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `test_connection_command` | connection.rs:42 | `profile: ConnectionProfile, password: String, ssh_password: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<ConnectionStatus, DbError>` | Optionally opens a temporary SSH tunnel, connects with the matching driver, calls `test_connection()`, tears the tunnel down, does not persist anything. |
| `create_connection_profile` | connection.rs:169 | `profile: ConnectionProfile, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<String, DbError>` | Generates a UUID if needed, rejects duplicates, adds the profile to state, persists via `save_profiles_to_store`. |
| `update_connection_profile` | connection.rs:213 | `profile: ConnectionProfile, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<(), DbError>` | Replaces an existing profile in state, persists to store. |
| `delete_connection_profile` | connection.rs:251 | `profile_id: String, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<(), DbError>` | Closes any live connection for the profile, deletes the OS-keyring password, removes the profile from state, persists. |
| `list_connection_profiles` | connection.rs:305 | `state: State<'_, Mutex<AppState>>` | `Result<Vec<ConnectionProfile>, DbError>` | Returns all saved profiles. |
| `get_saved_password` | connection.rs:333 | `profile_id: String, state: State<'_, Mutex<AppState>>` | `Result<Option<String>, DbError>` | Reads password from OS keyring, falling back to the in-memory session cache. |
| `save_password` | connection.rs:372 | `profile_id: String, password: String, state: State<'_, Mutex<AppState>>` | `Result<(), DbError>` | Saves to OS keyring and to the in-memory session cache. |
| `save_ssh_password` | connection.rs:411 | `profile_id: String, ssh_password: String` | `Result<(), DbError>` | Saves the SSH password to the OS keyring under a `-ssh` suffix. |
| `get_ssh_password` | connection.rs:432 | `profile_id: String` | `Result<Option<String>, DbError>` | Reads the SSH password from the OS keyring. |
| `connect_to_database` | connection.rs:458 | `profile_id: String, password: String, ssh_password: Option<String>, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<String, DbError>` | Resolves password/SSH tunnel, connects with the matching driver, stores the live connection + password cache in state, saves credentials to keyring, records connection metadata. |
| `disconnect_from_database` | connection.rs:635 | `connection_id: String, state: State<'_, Mutex<AppState>>` | `Result<(), DbError>` | Removes and closes the connection, closes any associated SSH tunnel. |
| `switch_database` | connection.rs:690 | `connection_id: String, new_database: String, state: State<'_, Mutex<AppState>>` | `Result<String, DbError>` | Reconnects to a different database on the same server/credentials, replacing the connection under the same ID. |
| `record_connection` | connection.rs:825 | `profile_id: String, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<(), DbError>` | Bumps `connection_count` / `last_connected_at` on a profile, persists. |
| `toggle_favorite` | connection.rs:866 | `profile_id: String, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<bool, DbError>` | Flips a profile's favorite flag, persists, returns the new value. |
| `update_connection_folder` | connection.rs:910 | `profile_id: String, folder: Option<String>, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<(), DbError>` | Moves a profile into/out of a folder, persists. |
| `get_connection_stats` | connection.rs:950 | `state: State<'_, Mutex<AppState>>` | `Result<serde_json::Value, DbError>` | Computes totals/favorites/folders/most-used from saved profiles. |
| `get_recent_connections` | connection.rs:995 | `limit: usize, state: State<'_, Mutex<AppState>>` | `Result<Vec<ConnectionProfile>, DbError>` | Returns the `limit` most recently connected profiles. |
| `duplicate_connection` | connection.rs:1034 | `profile_id: String, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<String, DbError>` | Clones a profile with a new ID/name, resets connection metadata, persists. |

### query.rs (2 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `execute_query` | query.rs:127 | `connection_id: String, sql: String, state: State<'_, Mutex<AppState>>` | `Result<QueryExecutionResult, DbError>` | Looks up the live connection, logs start/complete/error via `activity_logger`, times `connection.execute_query(&sql)`, returns columns/rows/rows_affected/execution_time/truncated. |
| `get_table_data_keyset` | query.rs:338 | `connection_id: String, schema: String, table: String, cursor_column: String, cursor_value: Option<serde_json::Value>, page_size: u64, filter_clause: Option<String>, sort_direction: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<KeysetPageResult, DbError>` | Builds a dialect-quoted keyset-paginated `SELECT` (`LIMIT page_size+1` sentinel row) and returns `has_more`/`next_cursor`. |

### schema.rs (6 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_databases` | schema.rs:26 | `connection_id: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<DatabaseInfo>, DbError>` | Delegates to `connection.get_databases()`. |
| `get_schemas` | schema.rs:58 | `connection_id: String, database: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<SchemaInfo>, DbError>` | Delegates to `connection.get_schemas(&database)`. |
| `get_tables` | schema.rs:90 | `connection_id: String, schema: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<TableInfo>, DbError>` | Delegates to `connection.get_tables(&schema)`. |
| `get_table_schema` | schema.rs:123 | `connection_id: String, schema: String, table: String, state: State<'_, Mutex<AppState>>` | `Result<TableSchema, DbError>` | Delegates to `connection.get_table_schema(&schema, &table)`. |
| `get_foreign_keys` | schema.rs:156 | `connection_id: String, schema: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<ForeignKeyInfo>, DbError>` | Delegates to `connection.get_foreign_keys(&schema)`. |
| `get_autocomplete_metadata` | schema.rs:226 | `connection_id: String, database: String, force_refresh: bool, state: State<'_, Mutex<AppState>>` | `Result<AutocompleteMetadata, DbError>` | Returns the 5-minute `MetadataCache` if fresh, else fan-out fetches databases/schemas/tables/columns concurrently (bounded `JoinSet`, `MAX_INFLIGHT=16`), rebuilds the cache, returns flattened data. |

### ddl.rs (7 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `create_database` | ddl.rs:43 | `connection_id: String, name: String, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Validates identifier, builds dialect-specific `CREATE DATABASE`, rejects SQLite/MongoDB/Turso/Redis, executes. |
| `preview_create_table` | ddl.rs:118 | `connection_id: String, table: TableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Resolves the driver's DDL generator, returns `CREATE TABLE` SQL without executing. |
| `create_table` | ddl.rs:161 | `connection_id: String, table: TableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Calls `preview_create_table` then executes the generated SQL. |
| `preview_alter_table` | ddl.rs:202 | `connection_id: String, alter: AlterTableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Returns generated `ALTER TABLE` SQL without executing. |
| `alter_table` | ddl.rs:241 | `connection_id: String, alter: AlterTableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Calls `preview_alter_table` then executes. |
| `preview_drop_table` | ddl.rs:279 | `connection_id: String, drop: DropTableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Returns generated `DROP TABLE` SQL without executing. |
| `drop_table` | ddl.rs:317 | `connection_id: String, drop: DropTableDefinition, state: State<'_, Mutex<AppState>>` | `Result<DdlResult, DbError>` | Calls `preview_drop_table` then executes. |

### export.rs (5 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `export_to_csv` | export.rs:109 | `file_path: String, columns: Vec<String>, rows: Vec<Vec<Value>>` | `Result<(), DbError>` | Writes RFC-4180-escaped CSV to `file_path`. |
| `export_to_json` | export.rs:197 | `file_path: String, columns: Vec<String>, rows: Vec<Vec<Value>>` | `Result<(), DbError>` | Converts rows to an array of `{column: value}` objects, pretty-prints to `file_path`. |
| `export_to_sql` | export.rs:296 | `connection_id: String, file_path: String, options: SqlExportOptions, state: State<'_, Mutex<AppState>>` | `Result<(), DbError>` | Dumps selected/all tables as DROP/CREATE/INSERT SQL, driving `schema::get_tables`, `schema::get_table_schema`, `query::execute_query` internally. |
| `cancel_import` | export.rs:582 | `cancel_flag: State<'_, Arc<AtomicBool>>` | `Result<(), DbError>` | Flips a shared atomic flag to request cancellation of an in-flight `import_from_sql`. |
| `import_from_sql` | export.rs:587 | `connection_id: String, file_path: String, options: SqlImportOptions, state: State<'_, Mutex<AppState>>, cancel_flag: State<'_, Arc<AtomicBool>>` | `Result<SqlImportResult, DbError>` | Streams a `.sql` dump, reassembles statements (handles `DELIMITER`, MySQL quirks, oversized `INSERT` splitting), executes each, optionally transactional, honors cancellation, writes an error log on failure. No `Channel<T>` progress streaming despite being an obvious candidate - progress is not reported incrementally to the frontend. |

### history.rs (7 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `save_to_history` | history.rs:48 | `history: QueryHistory, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<String, DbError>` | Appends a history record, persists the whole history snapshot via `AppState::save_history_to_store`. |
| `get_query_history` | history.rs:101 | `connection_id: Option<String>, limit: Option<usize>, state: State<'_, Mutex<AppState>>` | `Result<Vec<QueryHistory>, DbError>` | Reads/filters/sorts/truncates history from state. |
| `clear_history` | history.rs:152 | `connection_id: Option<String>, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<usize, DbError>` | Clears all or per-connection history, persists, returns count removed. |
| `save_snippet` | history.rs:217 | `snippet: QuerySnippet, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<String, DbError>` | Creates or updates a saved snippet, persists via `save_snippets_to_store`. |
| `list_snippets` | history.rs:272 | `tag: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<Vec<QuerySnippet>, DbError>` | Lists snippets, optionally filtered by tag, sorted by name. |
| `delete_snippet` | history.rs:312 | `snippet_id: String, state: State<'_, Mutex<AppState>>, app: AppHandle` | `Result<(), DbError>` | Removes a snippet, persists. |
| `get_snippet` | history.rs:350 | `snippet_id: String, state: State<'_, Mutex<AppState>>` | `Result<QuerySnippet, DbError>` | Looks up one snippet by ID. |

### migrations.rs (3 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `compute_schema_diff` | migrations.rs:83 | `source_connection_id: String, target_connection_id: String, schema: String, state: State<'_, Mutex<AppState>>` | `Result<SchemaDiff, DbError>` | Loads live table/FK metadata from both connections' drivers, computes a structural diff. Advisory only, no writes. |
| `generate_migration` | migrations.rs:99 | `diff: SchemaDiff, target_connection_id: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<String>, DbError>` | Resolves the target dialect and generates SQL migration statements from a `SchemaDiff`. |
| `apply_migration` | migrations.rs:109 | `connection_id: String, statements: Vec<String>, use_transaction: bool, state: State<'_, Mutex<AppState>>` | `Result<ApplyResult, DbError>` | Executes statements against the live connection, optionally wrapped in BEGIN/COMMIT/ROLLBACK, stops at first failure. |

### monitoring.rs (3 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_active_queries` | monitoring.rs:112 | `connection_id: String, state: State<'_, Mutex<AppState>>` | `Result<Vec<ActiveQuery>, DbError>` | Queries `pg_stat_activity` (Postgres-family) or `information_schema.PROCESSLIST` (MySQL); other drivers error as unsupported. |
| `kill_query` | monitoring.rs:185 | `connection_id: String, pid: i64, state: State<'_, Mutex<AppState>>` | `Result<(), DbError>` | Cancels a session via `pg_cancel_backend()` or `KILL QUERY`. |
| `get_server_stats` | monitoring.rs:210 | `connection_id: String, state: State<'_, Mutex<AppState>>` | `Result<ServerStats, DbError>` | Aggregates server stats from `pg_stat_database` or `SHOW GLOBAL STATUS`. |

### plugins.rs (13 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_installed_plugins` | plugins.rs:9 | `manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<Vec<Plugin>, String>` | Lists all installed plugins. |
| `get_plugin` | plugins.rs:18 | `plugin_id: String, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<Option<Plugin>, String>` | Looks up one installed plugin. |
| `install_plugin` | plugins.rs:28 | `marketplace_plugin: MarketplacePlugin, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<(), String>` | Installs a plugin from a marketplace record. |
| `uninstall_plugin` | plugins.rs:41 | `plugin_id: String, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<(), String>` | Removes an installed plugin. |
| `enable_plugin` | plugins.rs:54 | `plugin_id: String, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<(), String>` | Enables a disabled plugin. |
| `disable_plugin` | plugins.rs:67 | `plugin_id: String, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<(), String>` | Disables an enabled plugin. |
| `update_plugin_config` | plugins.rs:80 | `plugin_id: String, config: Value, manager: State<'_, Arc<Mutex<PluginManager>>>` | `Result<(), String>` | Updates a plugin's stored JSON config. |
| `get_marketplace_plugins` | plugins.rs:94 | `category: Option<String>, search: Option<String>` | `Result<Vec<MarketplacePlugin>, String>` | Returns a hardcoded mock list of 3 marketplace plugins, filtered in-memory. |
| `load_plugin` | plugins.rs:275 | `plugin_id: String, manager: State<'_, Arc<Mutex<PluginManager>>>, loader: State<'_, Arc<Mutex<PluginLoader>>>` | `Result<(), String>` | Loads/executes an enabled plugin's code via the loader. |
| `unload_plugin_runtime` | plugins.rs:296 | `plugin_id: String, loader: State<'_, Arc<Mutex<PluginLoader>>>` | `Result<(), String>` | Unloads a running plugin instance. |
| `execute_plugin_function` | plugins.rs:309 | `plugin_id: String, function_name: String, args: Value, manager: State<'_, Arc<Mutex<PluginManager>>>, loader: State<'_, Arc<Mutex<PluginLoader>>>` | `Result<Value, String>` | Invokes a named function inside a loaded plugin. |
| `get_loaded_plugins` | plugins.rs:339 | `loader: State<'_, Arc<Mutex<PluginLoader>>>` | `Result<Vec<String>, String>` | Lists IDs of loaded plugin runtimes. |
| `is_plugin_loaded` | plugins.rs:348 | `plugin_id: String, loader: State<'_, Arc<Mutex<PluginLoader>>>` | `Result<bool, String>` | Checks whether a plugin is loaded. |

### procedures.rs (3 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `list_procedures` | procedures.rs:69 | `connection_id: String, schema: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<Vec<ProcedureInfo>, DbError>` | Runs driver-specific catalog SQL (`pg_proc` / `information_schema.ROUTINES` / `sys.objects`); empty for SQLite/Turso/MongoDB. |
| `get_procedure_definition` | procedures.rs:210 | `connection_id: String, schema: String, name: String, state: State<'_, Mutex<AppState>>` | `Result<String, DbError>` | Fetches definition text (`pg_get_functiondef` / `SHOW CREATE PROCEDURE`); errors for other drivers. |
| `execute_procedure` | procedures.rs:276 | `connection_id: String, schema: String, name: String, args: Vec<Value>, state: State<'_, Mutex<AppState>>` | `Result<QueryResult, DbError>` | Builds and executes a `SELECT`/`CALL`/`EXEC` with inlined literal args. |

### settings.rs (3 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_settings` | settings.rs:23 | `app: AppHandle` | `Result<AppSettings, DbError>` | Opens the `settings.json` `tauri_plugin_store` store, reads the `"settings"` key, or returns `AppSettings::default()`. |
| `update_settings` | settings.rs:54 | `app: AppHandle, settings: AppSettings` | `Result<(), DbError>` | Writes `settings` into the store's `"settings"` key and calls `store.save()`. |
| `reset_settings` | settings.rs:86 | `app: AppHandle` | `Result<AppSettings, DbError>` | Calls `update_settings` with `AppSettings::default()`. |

### activity.rs (7 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_query_logs` | activity.rs:41 | `filter: Option<QueryLogFilter>, sort: Option<QueryLogSort>, page: usize, page_size: usize, state: State<'_, Mutex<AppState>>` | `Result<QueryLogResponse, DbError>` | Delegates to `state.activity_logger.get_logs(...)` for filtered/sorted/paginated results. |
| `get_activity_stats` | activity.rs:80 | `filter: Option<QueryLogFilter>, state: State<'_, Mutex<AppState>>` | `Result<ActivityStats, DbError>` | Delegates to `activity_logger.get_stats(filter)`. |
| `clear_query_logs` | activity.rs:108 | `state: State<'_, Mutex<AppState>>` | `Result<usize, DbError>` | Clears all query logs, returns count cleared. |
| `clear_old_query_logs` | activity.rs:133 | `state: State<'_, Mutex<AppState>>` | `Result<usize, DbError>` | Clears logs past the retention period. |
| `export_query_logs` | activity.rs:172 | `filter: Option<QueryLogFilter>, format: ExportFormat, file_path: String, state: State<'_, Mutex<AppState>>` | `Result<String, DbError>` | Serializes filtered logs to JSON/CSV/TXT and writes via `tokio::task::spawn_blocking(std::fs::write)`. |
| `update_query_log_tags` | activity.rs:292 | `log_id: String, tags: Vec<String>, state: State<'_, Mutex<AppState>>` | `Result<bool, DbError>` | Updates tags on one log entry. |
| `get_query_logs_count` | activity.rs:321 | `state: State<'_, Mutex<AppState>>` | `Result<usize, DbError>` | Returns total stored log count. |

### ai.rs (12 commands, all `State<'_, AiState>` where `AiState { config: Mutex<AiConfig> }`, `ai.rs:38-40`)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `check_ai_provider_status` | ai.rs:118 | `state: State<'_, AiState>, provider: Option<AiProviderType>` | `Result<ProviderStatus, DbError>` | Instantiates the given/active provider, checks availability + API key presence. |
| `check_ollama_status` | ai.rs:148 | `state: State<'_, AiState>` | `Result<bool, DbError>` | Legacy wrapper around `check_ai_provider_status` for Ollama. |
| `get_ai_config` | ai.rs:157 | `state: State<'_, AiState>` | `Result<AiConfig, DbError>` | Returns a clone of the full AI config (in-memory only, not persisted to store). |
| `set_ai_config` | ai.rs:167 | `state: State<'_, AiState>, config: AiConfig` | `Result<(), DbError>` | Overwrites the whole AI config. |
| `set_active_ai_provider` | ai.rs:179 | `state: State<'_, AiState>, provider: AiProviderType` | `Result<(), DbError>` | Sets the active provider. |
| `set_ai_api_key` | ai.rs:192 | `state: State<'_, AiState>, provider: AiProviderType, api_key: String` | `Result<(), DbError>` | Sets the API key on the matching provider config. |
| `list_ai_models` | ai.rs:213 | `state: State<'_, AiState>, provider: Option<AiProviderType>` | `Result<Vec<AiModelInfo>, DbError>` | Calls `provider.list_models()`. |
| `ai_generate_sql` | ai.rs:231 | `state: State<'_, AiState>, prompt: String, schema_context: String, model: Option<String>, provider: Option<AiProviderType>` | `Result<AiChatResponse, DbError>` | Calls `provider.generate_sql(...)`, times it. |
| `ai_explain_query` | ai.rs:259 | `state: State<'_, AiState>, sql: String, model: Option<String>, provider: Option<AiProviderType>` | `Result<AiChatResponse, DbError>` | Calls `provider.explain_query(...)`. |
| `ai_optimize_query` | ai.rs:286 | `state: State<'_, AiState>, sql: String, schema_context: String, model: Option<String>, provider: Option<AiProviderType>` | `Result<AiChatResponse, DbError>` | Calls `provider.optimize_query(...)`. |
| `ai_fix_query` | ai.rs:314 | `state: State<'_, AiState>, sql: String, error_message: String, schema_context: String, model: Option<String>, provider: Option<AiProviderType>` | `Result<AiChatResponse, DbError>` | Calls `provider.fix_query(...)`. |
| `ai_chat` | ai.rs:343 | `state: State<'_, AiState>, messages: Vec<ChatMessage>, model: Option<String>, provider: Option<AiProviderType>` | `Result<AiChatResponse, DbError>` | Calls `provider.chat(messages, model, 0.7, None)`. |

### backup.rs (6 commands)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `get_backup_directory` | backup.rs:40 | (none) | `Result<String, DbError>` | Returns/creates the OS data dir `db-hive/backups`. |
| `list_backups` | backup.rs:45 | `directory: Option<String>` | `Result<Vec<BackupEntry>, DbError>` | Scans for `.sql`/`.dump`/`.bak`/`.sqlite`/`.gz` files, parses `{name}_{ts}.{ext}` filenames, sorted newest-first. |
| `create_backup` | backup.rs:110 | `connection_id: String, options: BackupOptions, state: State<'_, Mutex<AppState>>` | `Result<BackupEntry, DbError>` | Shells out to `pg_dump`/`mysqldump`/file-copy/`mongodump` depending on driver. |
| `restore_backup` | backup.rs:291 | `connection_id: String, options: RestoreOptions, state: State<'_, Mutex<AppState>>` | `Result<(), DbError>` | Shells out to `psql`/`mysql` (stdin pipe) or file-copy (SQLite). |
| `delete_backup` | backup.rs:383 | `file_path: String` | `Result<(), DbError>` | Deletes a backup file. |
| `open_backup_directory` | backup.rs:389 | (none) | `Result<(), DbError>` | Shells out directly to `open`/`explorer`/`xdg-open` - does not use the Tauri `opener` plugin. |

### data_import.rs (4 commands, errors returned as `String` not `DbError`)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `preview_import_file` | data_import.rs:88 | `file_path: String, sheet_name: Option<String>, delimiter: Option<char>, max_rows: Option<usize>` | `Result<ImportPreview, String>` | Detects CSV/XLSX/XLS by extension, returns columns/sample rows/inferred types. |
| `import_data_to_table` | data_import.rs:367 | `connection_id: String, file_path: String, options: DataImportOptions, state: State<'_, Mutex<AppState>>` | `Result<ImportResult, String>` | Reads the whole file, optionally truncates the target, row-by-row builds/executes `INSERT`s. No `Channel<T>` progress streaming despite being an obvious candidate. |
| `get_tables_for_import` | data_import.rs:645 | `connection_id: String, schema: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<Vec<String>, String>` | Fetches table names via the driver. |
| `get_table_columns_for_import` | data_import.rs:668 | `connection_id: String, table_name: String, schema: Option<String>, state: State<'_, Mutex<AppState>>` | `Result<Vec<TableColumnInfo>, String>` | Fetches column metadata via `get_table_schema`. |

### window.rs (2 commands - see section 2.5 for the full multi-window mechanism)

| Command | file:line | Parameters | Return type | Description |
|---|---|---|---|---|
| `open_database_window` | window.rs:84 | `app: AppHandle, pending: State<'_, PendingWindowProfiles>, profile_id: Option<String>` | `Result<String, String>` | Calls the free function `spawn_window` to create a new OS window, optionally pre-registering a profile ID for auto-connect; returns the new window's label. |
| `take_pending_window_profile` | window.rs:95 | `window: tauri::Window, pending: State<'_, PendingWindowProfiles>` | `Option<String>` | Called once by a freshly booted window; removes and returns its pending profile ID, keyed by `window.label()`. |

## 2. Tauri coupling audit

This is the porting cost if the app stops being a Tauri/WebView app. Findings
below are from direct `grep -rn` over `src-tauri/src`, cross-checked against
`lib.rs` and `Cargo.toml`.

### 2.1 `AppHandle`

24 occurrences across 8 files:

- `lib.rs:19,22` - imported and used in `toggle_window_visibility` (tray/menu helper).
- `plugins/mod.rs:4,184` - stored as a field (`app_handle: AppHandle`) on the plugin execution context struct.
- `plugins/runtime.rs:15,130` - stored on the plugin runtime, used to resolve paths.
- `plugins/manager.rs:9,16,29` - stored on `PluginManager`, used for both path resolution and event emission.
- `plugins/loader.rs:7,18,23` - stored on `PluginLoader`, passed down to sandboxes.
- `state/mod.rs:22,353,389,427,518,550,606,628` - taken by reference (`&AppHandle`) in every `*_from_store`/`*_to_store`/`migrate_passwords_to_keyring` method, purely to call `tauri_plugin_store`'s `StoreExt::store()`.
- `commands/window.rs:17,36,85` - required to build a `WebviewWindowBuilder`.
- `commands/history.rs:10,52,156,221,316` - passed straight through to `AppState::save_*_to_store`.
- `commands/settings.rs:7,23,54,86` - used directly with `tauri_plugin_store::StoreExt`.
- `commands/connection.rs:8,172,216,254,463,828,869,914,1037` - passed straight through to `save_profiles_to_store`/`record_connection`.

**Replacement note**: every one of these is either (a) the DI handle for
`tauri_plugin_store` persistence (→ swap for a plain `serde_json::to_writer`
against a path from `directories::ProjectDirs`), or (b) the handle needed to
build/emit-to a `WebviewWindow` (→ replaced entirely by whatever native
windowing/event system the new GUI uses). None of it is business logic.

### 2.2 `State<'_, ...>`

The single largest coupling surface by line count: 100+ parameter
occurrences across every `commands/*.rs` file (`Mutex<AppState>` in
connection/query/schema/ddl/history/export/migrations/monitoring/
procedures/activity/backup/data_import, `AiState` in ai.rs,
`Arc<Mutex<PluginManager>>`/`Arc<Mutex<PluginLoader>>` in plugins.rs,
`Arc<AtomicBool>` in export.rs, `PendingWindowProfiles` in window.rs).

**Replacement note**: `State<T>` is Tauri's dependency-injection wrapper
around a value previously registered with `app.manage::<T>()`. Under a
native GUI it is replaced by whatever the new command layer uses to reach
shared state - most simply, passing `&Mutex<AppState>` (or an `Arc` clone of
it) directly into each function instead of through Tauri's injection. This
is a mechanical, low-risk rewrite: every command body already does
`state.lock().unwrap()` and works with the inner value exactly like a plain
Rust reference.

### 2.3 `Manager` trait

- `lib.rs:19` (import), used via `app.handle()` (`lib.rs:93,105,117,129,153,169`), `app.manage()` (`lib.rs:141,144,147,150,166,171`), `app.get_webview_window()` (`lib.rs:23,221`), `app.state::<PendingWindowProfiles>()` (`lib.rs:191`).
- `plugins/runtime.rs:15,135-136` - `Manager::path().app_data_dir()` to resolve the plugin data directory.
- `plugins/manager.rs:31-32,235` - same `.path().app_data_dir()` pattern for plugin storage.
- `plugins/api.rs:5,136-137` - same pattern again, for a plugin's own working directory.
- `commands/schema.rs:411,485-486,493,...` - `tauri::Manager`/`tauri::test::mock_app()`/`app.manage()`/`app.state()`, but only inside `#[cfg(test)]` (`create_test_app()` helper used because `tauri::State` has no public constructor for unit tests).

**Replacement note**: `app.path().app_data_dir()` → `directories::ProjectDirs::from(...).data_dir()`. `app.manage()`/`app.state()` → ordinary struct fields/`Arc` clones passed at construction time. The test-only usage in `schema.rs` disappears entirely once commands stop taking `State<T>` params (tests can construct `AppState` and call functions directly, no mock app needed).

### 2.4 `Emitter` / `.emit(...)` calls

Every emit call in the codebase is inside `plugins/`, one of two files:

- `plugins/manager.rs:212-216` → event `"plugin-event"`, payload `PluginEvent::Installed{..}`.
- `plugins/manager.rs:307-311` → event `"plugin-event"`, payload `PluginEvent::Uninstalled{..}`.
- `plugins/manager.rs:331-335` → event `"plugin-event"`, payload `PluginEvent::Enabled{..}`.
- `plugins/manager.rs:356-360` → event `"plugin-event"`, payload `PluginEvent::Disabled{..}`.
- `plugins/api.rs:65-68` → event `"plugin-create-tab"`, payload `CreateTabEvent{tab_id,..}` (a plugin asking the frontend to open a UI tab).
- `plugins/api.rs:81-84` → event `"plugin-notification"`, payload `NotificationEvent{plugin_id,..}`.
- `plugins/api.rs:96-99` → event `"plugin-register-ui"`, payload `RegisterUiEvent{plugin_id,..}`.
- `plugins/api.rs:189-192` → event `"plugin-message"`, payload `PluginMessage{from,..}` (plugin-to-plugin or plugin-to-frontend messaging bus).

No `commands/*.rs` file, and no non-plugin core module, emits any Tauri
event. Query execution, connection lifecycle, DDL, migrations, etc. are all
request/response over `invoke()`, not event-driven.

**Replacement note**: this whole mechanism is how a plugin currently talks
to the UI it doesn't otherwise have access to (open a tab, show a
notification, register a UI extension point, message another plugin). It
must be replaced by whatever event/callback bus the native GUI's plugin
host uses; the plugin loading/sandboxing/hashing logic in `plugins/loader.rs`,
`plugins/sandbox.rs`, and most of `plugins/manager.rs` is unaffected and can
be kept, only the `app_handle.emit(...)` call sites need a new sink.

### 2.5 `WebviewWindow` / `WebviewWindowBuilder` / `tauri::Window` - multi-window machinery

Confined entirely to `commands/window.rs` (105 lines) plus three call sites
in `lib.rs`. This is DB Hive's multi-window architecture: any number of OS
windows, each an independent WebView with its own connection context,
backed by the single shared `AppState`.

- `PendingWindowProfiles` (`window.rs:24`) is `pub struct PendingWindowProfiles(pub Mutex<HashMap<String, String>>)`, mapping a not-yet-existing window's label to the profile ID it should auto-connect to. It is registered once at startup as Tauri-managed state: `app.manage(commands::window::PendingWindowProfiles::default())` (`lib.rs:144`).
- The real work happens in the free function `spawn_window(app: &AppHandle, pending: &PendingWindowProfiles, profile_id: Option<String>)` (`window.rs:35-79`), not in a `#[tauri::command]` directly - this lets it be called both from the `open_database_window` command (`window.rs:84-90`, for windows requested from inside a WebView) and directly from the system-tray "New Window" menu handler (`lib.rs:189-197`, which pulls `PendingWindowProfiles` via `app.state::<...>()` at `lib.rs:191` and calls `spawn_window(app, &pending, None)` at `lib.rs:193`).
- `spawn_window` generates a unique label `format!("win-{}", uuid::Uuid::new_v4())` (`window.rs:40`); if a `profile_id` was passed it inserts `label -> profile_id` into the pending map (`window.rs:42-48`) *before* the window exists, guaranteeing the mapping is present the instant the new WebView asks for it.
- The window is built with `WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))` (`window.rs:50-55`) with title/size/centering, then platform-conditional chrome: macOS gets `title_bar_style(TitleBarStyle::Overlay)` + `hidden_title(true)` (`window.rs:57-62`, the fix documented in project memory against `decorations: false` breaking macOS keyboard input); other platforms get `decorations(false)` (`window.rs:64-67`) for a fully custom titlebar.
- `builder.build()` (`window.rs:69`) does the actual OS window creation; on failure the pending-map entry is rolled back (`window.rs:70-75`) so a nonexistent window never leaves a dangling mapping.
- Once booted, the new WebView calls `take_pending_window_profile` (`window.rs:94-104`), which uses `tauri::Window::label()` (via the injected `window: tauri::Window` param, `window.rs:96`) to look itself up and remove+return its assigned profile ID (`window.rs:99-103`) - a one-shot handoff.
- The shared backend state that every window talks to (`AppState`, connections keyed by connection ID) lives entirely in `state/mod.rs` and requires zero changes per window; only the window-creation/labeling/pending-profile-handoff mechanism above is Tauri-window-specific.

**Replacement note**: this entire file's mechanism (label generation, chrome
configuration, one-shot pending-profile map) is Tauri-WebView-specific and
must be rewritten from scratch against whatever windowing toolkit the
native GUI uses (native multi-window support varies wildly - egui has no
native multi-window primitive comparable to this, GTK/Qt/winit each have
their own). The *design* (windows keyed by opaque label, connections keyed
by ID in one shared state, one-shot per-window handoff map) is portable as
a pattern even though none of the Tauri types are.

### 2.6 `Channel<T>` streaming

`grep -rn "Channel<" src-tauri/src` returns **zero matches**. Despite
`export_to_sql`/`import_from_sql` (export.rs) and `import_data_to_table`
(data_import.rs) being long-running, row-by-row operations that are obvious
candidates for progress streaming, none of them use Tauri's `Channel<T>` -
they block until completion and return a single final result. There is
therefore no `Channel`-based IPC to port; this whole CLAUDE.md-documented
pattern is currently aspirational/unused in the actual codebase.

### 2.7 Tauri plugins in use

Registered in `lib.rs:228-233`, versions from `Cargo.toml`:

| Plugin | Version | Used for | file:line |
|---|---|---|---|
| `tauri-plugin-store` | 2.4.1 | JSON key-value persistence for connection profiles, query history, snippets, and app settings | `state/mod.rs:23` (`StoreExt`), `commands/settings.rs:8` |
| `tauri-plugin-dialog` | 2.4.2 | Native file open/save dialogs - referenced only from the frontend (`@tauri-apps/plugin-dialog`), not called from any Rust command in `src-tauri/src` | registered `lib.rs:229`, no Rust call sites found |
| `tauri-plugin-opener` | 2.x | Opening URLs/files with the OS default handler - registered but not called from any Rust command found (`backup.rs`'s `open_backup_directory` shells out to `open`/`explorer`/`xdg-open` directly instead, `backup.rs:389-412`) | registered `lib.rs:230` |
| `tauri-plugin-updater` | 2.9.0 | Auto-update checks - no Rust call sites found in `commands/`, presumably driven from the frontend | registered `lib.rs:231` |
| `tauri-plugin-process` | 2.3.1 | Process control (e.g. relaunch after update) - no Rust call sites found in `commands/` | registered `lib.rs:232` |
| `tauri-plugin-notification` | 2.3.3 | OS notifications - no Rust call sites found in `commands/` | registered `lib.rs:233` |

**Replacement notes**: `tauri-plugin-store` → `serde_json` to files under
`directories::ProjectDirs::data_dir()`, the single biggest and most
mechanical swap since 4 of `state/mod.rs`'s persistence methods plus all of
`settings.rs` depend on it. `tauri-plugin-dialog` → native file dialog crate
(e.g. `rfd`). `tauri-plugin-opener` → `open` crate or the same manual
`open`/`explorer`/`xdg-open` shell-out already used in `backup.rs`.
`tauri-plugin-updater`/`process`/`notification` → whatever the new GUI
framework's ecosystem offers, or hand-rolled (self-update crates,
`std::process`, `notify-rust`); none of these have Rust call sites in the
core to migrate, only registrations, since the frontend/JS side (being
replaced anyway) is what actually drove them.

### 2.8 App config/data paths via Tauri

- `state/mod.rs:23` `use tauri_plugin_store::StoreExt;` - all four persistence pairs (`load/save_profiles_to_store` `:353,389`, `migrate_passwords_to_keyring` `:427`, `load/save_history_to_store` `:518,550`, `load/save_snippets_to_store` `:606,628`) resolve their file location internally via the plugin (`app.store("profiles.json")`-style calls), so the exact on-disk path is opaque to the calling code - it is wherever `tauri_plugin_store` decides for this app's bundle identifier.
- `commands/settings.rs:8,25,56` - same pattern for `settings.json`.
- `plugins/runtime.rs:135-136`, `plugins/manager.rs:31-32,235`, `plugins/api.rs:136-137` - `app.path().app_data_dir()`, Tauri's own resolved app-data directory, used to place plugin files.
- `backup.rs:40-41` - `get_backup_directory` resolves an OS data dir "db-hive/backups" **without** going through Tauri (uses `dirs`-style plain Rust, not `app.path()`) - this one is already portable as-is.

**Replacement note**: the one non-portable pattern here is relying on
`tauri_plugin_store`'s internal file resolution and `app.path().app_data_dir()`;
both replace cleanly with `directories::ProjectDirs::from("com", "dbhive", "DB Hive").data_dir()`
plus explicit `serde_json::to_writer`/`from_reader` calls, which is exactly
what `backup.rs` already does independently for its own directory.

## 3. State model

`AppState` (`src-tauri/src/state/mod.rs:104-135`) is a single struct with no
Tauri types among its fields:

| Field | Type | Holds | Line |
|---|---|---|---|
| `connections` | `HashMap<String, Arc<dyn DatabaseDriver>>` | Live DB connections, keyed by connection ID (UUID, == profile ID once connected) | 107 |
| `connection_profiles` | `HashMap<String, ConnectionProfile>` | Saved connection profiles, keyed by profile ID | 111 |
| `connection_passwords` | `HashMap<String, String>` | In-memory session password cache, not persisted to disk, not cleared on disconnect (kept for easy reconnection) | 116 |
| `query_history` | `Vec<QueryHistory>` | Executed-query history, FIFO-capped at `MAX_HISTORY_ENTRIES = 1000` (line 29) | 120 |
| `query_snippets` | `HashMap<String, QuerySnippet>` | Saved query snippets, keyed by snippet ID | 124 |
| `ssh_tunnel_manager` | `SshTunnelManager` | Active SSH tunnel handles, from `crate::ssh` (no Tauri dependency) | 127 |
| `metadata_cache` | `HashMap<String, MetadataCache>` | Per-connection autocomplete cache (databases/schemas/tables/columns), self-expiring after 5 minutes (`MetadataCache::is_stale`, line 65) | 131 |
| `activity_logger` | `ActivityLogger` | Query execution log (see below) | 134 |

Two more pieces of state are managed **outside** `AppState`, as separate
`app.manage()` values: `PendingWindowProfiles` (per-window pending-connect
map, see 2.5) and `AiState` (`commands/ai.rs:38-40`, `{config: Mutex<AiConfig>}`)
- `AiState` is in-memory only, `set_ai_config`/`get_ai_config` never call
into `tauri_plugin_store`, so AI provider configuration (including API
keys) does **not** survive an app restart, unlike profiles/history/snippets.
There is also a bare `Arc<AtomicBool>` cancel flag for long-running imports
(`lib.rs:147`), also a standalone global, not a field of `AppState`.

**Lifecycle**: constructed once, `AppState::default()` (`lib.rs:90`), then
hydrated synchronously before the app is usable - `load_profiles_from_store`
(`lib.rs:93`), `migrate_passwords_to_keyring` (`lib.rs:105`),
`load_history_from_store` (`lib.rs:117`), `load_snippets_from_store`
(`lib.rs:129`). Registered as one global singleton: `app.manage(Mutex::new(state))`
(`lib.rs:141`) - there is exactly one `AppState` for the whole process,
shared by every window. Commands access it via `state: State<'_, Mutex<AppState>>`
and `state.lock().unwrap()`. The only other `app.manage(Mutex::new(state))`
call anywhere is a test helper (`commands/schema.rs:486`, inside
`create_test_app()`, needed because `tauri::State` has no public
constructor for unit tests) - not production state.

**Per-window-scoped state**: unlike everything in `AppState`, exactly one
construct is genuinely per-window: `PendingWindowProfiles`
(`commands/window.rs:24`). It is registered as one global map, but its
*keys* are window labels, making each entry window-scoped by construction -
see section 2.5 for the full mechanism. This is the one piece of the state
model that depends on Tauri's window-label addressing (`tauri::Window::label()`)
and needs a different mechanism entirely under a non-Tauri GUI.

**`ActivityLogger`** (`state/activity_logger.rs`, 509 lines): in-memory
query-execution log with a full lifecycle - `log_query_start` (`:48`),
`log_query_complete` (`:64`), `log_query_error` (`:85`), `log_query_cancel`
(`:105`) - plus filtering/sorting/pagination (`get_logs`, `:127-185`),
aggregate stats (`get_stats`, `:196-255`), retention-based cleanup
(`clear_old_logs`, `:262-270`, driven by a `retention_days: u32` set at
construction, `AppState::default()` passes `7`), and tag editing
(`update_tags`, `:318-326`). Storage is a plain `RwLock<Vec<QueryLog>>`
(`:20`) - **no disk persistence**, logs are lost on restart. **Zero Tauri
dependency**: the file imports only `crate::models::*`, `chrono`, and
`std::{collections::HashMap, sync::RwLock}` - no `tauri::` import anywhere.
It is driven entirely through plain method calls from `commands/query.rs`
and `commands/activity.rs`, which are the only layer touching Tauri types.

**Persistence mechanisms**: `tauri_plugin_store` for profiles/history/
snippets/settings (see 2.8, the primary porting cost in this section); the
OS keyring (via the `keyring` crate, `crate::credentials::CredentialManager`)
for actual connection/SSH passwords - already Tauri-free and portable as-is;
`connection_passwords` and `metadata_cache` are deliberately session-only
and never persisted.

## 4. Driver layer

The `DatabaseDriver` trait (`src-tauri/src/drivers/mod.rs:139-263`):

| Method | Signature | Line |
|---|---|---|
| `connect` | `async fn connect(opts: ConnectionOptions) -> Result<Self, DbError> where Self: Sized` | 149 |
| `test_connection` | `async fn test_connection(&self) -> Result<(), DbError>` | 158 |
| `execute_query` | `async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError>` | 175 |
| `get_databases` | `async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError>` | 182 |
| `get_schemas` | `async fn get_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>, DbError>` | 193 |
| `get_tables` | `async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError>` | 204 |
| `get_table_schema` | `async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError>` | 216 |
| `get_foreign_keys` | `async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError>` | 228 |
| `close` | `async fn close(&self) -> Result<(), DbError>` | 239 |
| `quote_identifier` | `fn quote_identifier(&self, ident: &str) -> String` (default impl, dialect-overridable) | 248 |
| `escape_string_literal` | `fn escape_string_literal(&self, value: &str) -> String` (default impl, dialect-overridable) | 260 |

`ConnectionOptions` (`drivers/mod.rs:39-61`) and `QueryResult`
(`drivers/mod.rs:68-108`) are plain serde-able data structs, no Tauri types.

Implementations, all complete (no `todo!`/`unimplemented!` found):

| Driver | Struct | `impl DatabaseDriver` at | Notes |
|---|---|---|---|
| PostgreSQL | `PostgresDriver` | `postgres.rs:434` | Full; only driver with native, real `schema`/`database` args throughout (`postgres.rs:152` struct, `:1091` file total). |
| MySQL | `MysqlDriver` | `mysql.rs:46` | Full; `get_schemas`/`get_tables` take unused `_database`/`_schema` (MySQL treats database==schema). Overrides `quote_identifier` (backticks) and `escape_string_literal`. |
| SQLite | `SqliteDriver` | `sqlite.rs:87` | Full; `get_schemas` takes unused `_database` (no schema concept beyond `main`). Has unit tests (`sqlite.rs:405,453`). |
| MongoDB | `MongoDbDriver` | `mongodb.rs:109` | Full, mapped onto the SQL-shaped trait (`execute_query` parses a JSON-ish command string, not SQL); schema/FK methods return best-effort/empty data by design, not gaps. |
| SQL Server | `SqlServerDriver` | `sqlserver.rs:151` | Full; overrides `quote_identifier` (bracket quoting). |
| Redis | `RedisDriver` | `redis.rs:37` | Full, mapped onto key/value semantics; adds an extra inherent method `sample_key_types` (`redis.rs:296`) beyond the trait. |
| Turso (libSQL) | `TursoDriver` | `turso.rs:69` | Full; `get_schemas` takes unused `_database` (SQLite-like). |

**Tauri dependency in the driver layer**: `grep -rn "tauri" src-tauri/src/drivers/`
returns **zero matches**. The entire `drivers/` directory (mod.rs + 7 driver
files, 4,454 LOC) is Tauri-free - no `AppHandle`, `State`, `Manager`,
`Emitter`, events, or `Channel`s. It depends only on `async_trait`, `serde`,
and each driver's own client crate (tokio-postgres, mysql_async, rusqlite,
mongodb, tiberius, redis, libsql). This is the cleanest layer in the
codebase and can move to a new GUI verbatim, unmodified.

**Supporting modules, Tauri dependency check**:

- `ddl/` (mod.rs, postgres.rs, mysql.rs, sqlite.rs, sqlserver.rs) - no `tauri` references found.
- `ssh/mod.rs` - no `tauri` references found.
- `credentials/mod.rs` - no `tauri` references found.
- `migrations/` (mod.rs, diff.rs, sql_gen.rs) - no `tauri` references found.
- `ai/` (mod.rs, anthropic.rs, google.rs, ollama.rs, openai.rs, openrouter.rs, provider.rs) - no `tauri` references found (AI providers are plain async HTTP clients).
- `plugins/` (mod.rs, loader.rs, sandbox.rs, runtime.rs, manager.rs, api.rs) - **is** Tauri-coupled, exclusively via `AppHandle` for `.path().app_data_dir()` resolution and `Emitter::emit()` to push events to the frontend (full detail in section 2.3/2.4). The sandboxing/hashing/loading logic itself has no other Tauri touchpoints.

## 5. Verdict

Bucketed from the same per-file `wc -l` used throughout (23,404 LOC total,
matches the sum below exactly):

| Bucket | LOC | Arithmetic |
|---|---:|---|
| (a) Reusable untouched | **12,494** | `drivers/` 4,454 (263+1091+435+592+481+620+577+395) + `ddl/` 1,654 (48+527+404+342+333) + `ssh/mod.rs` 394 + `credentials/mod.rs` 219 + `migrations/` 606 (14+226+366) + `ai/` 2,048 (33+293+409+286+396+319+312) + `models/` 2,610 (32+103+116+310+311+319+368+417+634) + `state/activity_logger.rs` 509 |
| (b) Strip the Tauri wrapper, logic intact | **10,460** | `commands/*.rs` minus `window.rs` = 7,378 - 104 = 7,274 + `plugins/` (engine) 2,400 (263+154+236+845+499+403) + `state/mod.rs` 786 |
| (c) Genuinely Tauri-specific, must be rewritten | **450** | `commands/window.rs` 104 + `lib.rs` 340 + `main.rs` 6 |
| **Total** | **23,404** | matches `wc -l` sum across all 61 files |

**(a) Reusable untouched - 12,494 LOC (53%)**: every database driver, every
DDL generator, SSH tunneling, OS-keyring credential storage, schema-diff/
migration-SQL generation, all AI provider clients, every plain data model,
and the activity/query-log engine. None of this imports `tauri` at all;
it can be linked into a native GUI binary with zero changes.

**(b) Needs the Tauri wrapper stripped, logic intact - 10,460 LOC (45%)**:
every command function's *body* (validation, driver dispatch, SQL building,
error mapping) is portable as-is; only the function *signature*
(`State<'_, T>` params, `AppHandle` params passed through to
`tauri_plugin_store`) needs to change to plain function arguments/direct
struct access. Includes the plugin engine (`plugins/`), whose sandboxing and
lifecycle logic is intact and only its two Tauri touchpoints (path
resolution, event emission) need a new implementation, and `state/mod.rs`,
whose data structure and accessor methods are pure and only its
`tauri_plugin_store`-backed persistence methods need a new backend.

**(c) Genuinely Tauri-specific, must be rewritten - 450 LOC (2%)**:
`commands/window.rs` (the entire multi-window mechanism: `WebviewWindowBuilder`,
window labels, per-window chrome), and `lib.rs`/`main.rs` (the
`tauri::Builder` setup: menu, tray icon, plugin registration, window
decorations bootstrap). This is genuinely small - the app's Tauri
"shell" is a thin sliver of the 23k LOC core; the 98% that isn't this
bucket is either already portable or portable after a mechanical
signature change.
