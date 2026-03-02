//! Export and Import commands for query results and database dumps
//!
//! This module provides Tauri commands for:
//! - Exporting query results to CSV and JSON formats
//! - Exporting database structure and data to SQL dumps
//! - Importing SQL dumps back into databases
//! Uses native file dialogs for save/load locations.

use crate::models::connection::DbDriver;
use crate::models::DbError;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;

/// Options for SQL export
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlExportOptions {
    /// Include DROP statements before CREATE
    pub include_drop: bool,
    /// Include CREATE TABLE statements
    pub include_create: bool,
    /// Include INSERT statements (data)
    pub include_data: bool,
    /// Filter by specific tables (empty = all tables)
    pub tables: Vec<String>,
    /// Schema to export from (PostgreSQL/MySQL)
    pub schema: Option<String>,
}

impl Default for SqlExportOptions {
    fn default() -> Self {
        Self {
            include_drop: false,
            include_create: true,
            include_data: true,
            tables: Vec::new(),
            schema: None,
        }
    }
}

/// Options for SQL import
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlImportOptions {
    /// Continue on error (don't stop at first error)
    pub continue_on_error: bool,
    /// Use transaction (rollback all on error)
    pub use_transaction: bool,
}

/// Result returned by import_from_sql
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlImportResult {
    pub executed: usize,
    pub errors_count: usize,
    pub skipped: usize,
    pub first_error: Option<String>,
    /// True if the import was stopped early by the user
    pub cancelled: bool,
    /// Absolute path to the error log file, or None if there were no errors
    pub log_file: Option<String>,
}

/// Export query results to CSV format
///
/// Takes the query result data (columns and rows) and exports it to a CSV file.
/// The CSV format includes headers and properly escapes special characters.
///
/// # Arguments
///
/// * `file_path` - Absolute path where the CSV file should be saved
/// * `columns` - Column names for the CSV header row
/// * `rows` - Data rows to export (each row is a vector of JSON values)
///
/// # Returns
///
/// Ok(()) if export succeeds, DbError if file writing fails
///
/// # Frontend Usage
///
/// ```typescript
/// import { save } from '@tauri-apps/plugin-dialog';
///
/// const filePath = await save({
///   defaultPath: 'query_results.csv',
///   filters: [{
///     name: 'CSV',
///     extensions: ['csv']
///   }]
/// });
///
/// if (filePath) {
///   await invoke('export_to_csv', {
///     filePath,
///     columns: result.columns,
///     rows: result.rows
///   });
/// }
/// ```
#[tauri::command]
pub fn export_to_csv(
    file_path: String,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
) -> Result<(), DbError> {
    let path = Path::new(&file_path);

    // Create the file
    let mut file = File::create(path).map_err(|e| {
        DbError::InternalError(format!("Failed to create CSV file: {}", e))
    })?;

    // Write CSV header
    let header = columns
        .iter()
        .map(|col| escape_csv_value(col))
        .collect::<Vec<_>>()
        .join(",");

    writeln!(file, "{}", header).map_err(|e| {
        DbError::InternalError(format!("Failed to write CSV header: {}", e))
    })?;

    // Write data rows
    for row in rows {
        let row_str = row
            .iter()
            .map(|val| {
                let str_val = json_value_to_string(val);
                escape_csv_value(&str_val)
            })
            .collect::<Vec<_>>()
            .join(",");

        writeln!(file, "{}", row_str).map_err(|e| {
            DbError::InternalError(format!("Failed to write CSV row: {}", e))
        })?;
    }

    Ok(())
}

/// Export query results to JSON format
///
/// Exports query results as a JSON array of objects, where each object
/// represents a row with column names as keys.
///
/// # Arguments
///
/// * `file_path` - Absolute path where the JSON file should be saved
/// * `columns` - Column names
/// * `rows` - Data rows to export
///
/// # Returns
///
/// Ok(()) if export succeeds, DbError if file writing fails
///
/// # Output Format
///
/// ```json
/// [
///   {"id": 1, "name": "Alice", "age": 30},
///   {"id": 2, "name": "Bob", "age": 25}
/// ]
/// ```
///
/// # Frontend Usage
///
/// ```typescript
/// import { save } from '@tauri-apps/plugin-dialog';
///
/// const filePath = await save({
///   defaultPath: 'query_results.json',
///   filters: [{
///     name: 'JSON',
///     extensions: ['json']
///   }]
/// });
///
/// if (filePath) {
///   await invoke('export_to_json', {
///     filePath,
///     columns: result.columns,
///     rows: result.rows
///   });
/// }
/// ```
#[tauri::command]
pub fn export_to_json(
    file_path: String,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
) -> Result<(), DbError> {
    let path = Path::new(&file_path);

    // Convert rows to JSON objects
    let json_rows: Vec<serde_json::Map<String, Value>> = rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, value) in row.iter().enumerate() {
                if let Some(col_name) = columns.get(i) {
                    obj.insert(col_name.clone(), value.clone());
                }
            }
            obj
        })
        .collect();

    // Serialize to pretty JSON
    let json_string = serde_json::to_string_pretty(&json_rows).map_err(|e| {
        DbError::InternalError(format!("Failed to serialize JSON: {}", e))
    })?;

    // Write to file
    let mut file = File::create(path).map_err(|e| {
        DbError::InternalError(format!("Failed to create JSON file: {}", e))
    })?;

    file.write_all(json_string.as_bytes()).map_err(|e| {
        DbError::InternalError(format!("Failed to write JSON file: {}", e))
    })?;

    Ok(())
}

/// Convert a JSON value to a string representation
fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// Escape a value for CSV format
///
/// Properly handles quotes, commas, and newlines according to CSV RFC 4180
fn escape_csv_value(value: &str) -> String {
    // Check if value needs quoting (contains comma, quote, or newline)
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        // Escape quotes by doubling them
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

/// Export database to SQL dump file
///
/// Exports database structure and/or data to a SQL file that can be imported later.
/// Supports PostgreSQL, MySQL, and SQLite formats.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `file_path` - Where to save the SQL dump
/// * `options` - Export options (what to include, which tables, etc.)
///
/// # Frontend Usage
///
/// ```typescript
/// import { save } from '@tauri-apps/plugin-dialog';
///
/// const filePath = await save({
///   defaultPath: 'database_dump.sql',
///   filters: [{ name: 'SQL', extensions: ['sql'] }]
/// });
///
/// if (filePath) {
///   await invoke('export_to_sql', {
///     connectionId: 'conn-123',
///     filePath,
///     options: {
///       includeDrop: false,
///       includeCreate: true,
///       includeData: true,
///       tables: [], // empty = all tables
///       schema: 'public'
///     }
///   });
/// }
/// ```
#[tauri::command]
pub async fn export_to_sql(
    connection_id: String,
    file_path: String,
    options: SqlExportOptions,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    // Get driver type and verify connection exists
    let driver = {
        let state_lock = state.lock().unwrap();

        // Verify connection exists
        if !state_lock.connections.contains_key(&connection_id) {
            return Err(DbError::NotFound(format!("Connection {} not found", connection_id)));
        }

        // Get driver type from connection profile (connection_id == profile_id)
        state_lock
            .connection_profiles
            .get(&connection_id)
            .map(|profile| profile.driver.clone())
            .ok_or_else(|| DbError::NotFound(format!("Connection profile {} not found", connection_id)))?
    };

    // Create output file
    let mut file = File::create(&file_path)
        .map_err(|e| DbError::InternalError(format!("Failed to create SQL file: {}", e)))?;

    // Write header comment
    writeln!(file, "-- DB-Hive SQL Dump")
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL header: {}", e)))?;
    writeln!(file, "-- Database: {:?}", driver)
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL header: {}", e)))?;
    writeln!(file, "-- Export time: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL header: {}", e)))?;
    writeln!(file, "")
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL: {}", e)))?;

    // Get list of tables to export
    let schema = options.schema.as_deref().unwrap_or("public");
    let tables = if options.tables.is_empty() {
        // Get all tables from schema
        use crate::commands::schema::get_tables;
        get_tables(connection_id.clone(), schema.to_string(), state.clone()).await?
    } else {
        // Use specified tables
        options.tables.iter().map(|name| crate::models::metadata::TableInfo {
            name: name.clone(),
            schema: schema.to_string(),
            table_type: "TABLE".to_string(),
            row_count: None,
        }).collect()
    };

    // Export each table
    for table in tables {
        export_table_to_sql(
            &mut file,
            &connection_id,
            &table.schema,
            &table.name,
            &driver,
            &options,
            &state,
        ).await?;
    }

    writeln!(file, "\n-- Dump completed")
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL footer: {}", e)))?;

    Ok(())
}

/// Export a single table to SQL
async fn export_table_to_sql(
    file: &mut File,
    connection_id: &str,
    schema: &str,
    table: &str,
    driver: &DbDriver,
    options: &SqlExportOptions,
    state: &State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    writeln!(file, "\n-- Table: {}.{}", schema, table)
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL: {}", e)))?;

    // DROP statement
    if options.include_drop {
        let drop_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite => {
                format!("DROP TABLE IF EXISTS \"{}\".\"{}\" CASCADE;", schema, table)
            }
            DbDriver::MySql => {
                format!("DROP TABLE IF EXISTS `{}`.`{}`;", schema, table)
            }
            DbDriver::MongoDb => {
                format!("// db.{}.drop();", table)
            }
            DbDriver::SqlServer => {
                format!("IF OBJECT_ID('{}.{}', 'U') IS NOT NULL DROP TABLE {}.{};", schema, table, schema, table)
            }
        };
        writeln!(file, "{}", drop_stmt)
            .map_err(|e| DbError::InternalError(format!("Failed to write DROP statement: {}", e)))?;
    }

    // CREATE statement
    if options.include_create {
        let create_stmt = get_create_table_statement(connection_id, schema, table, driver, state).await?;
        writeln!(file, "{}", create_stmt)
            .map_err(|e| DbError::InternalError(format!("Failed to write CREATE statement: {}", e)))?;
    }

    // INSERT statements (data)
    if options.include_data {
        export_table_data_to_sql(file, connection_id, schema, table, driver, state).await?;
    }

    Ok(())
}

/// Get CREATE TABLE statement for a table
async fn get_create_table_statement(
    connection_id: &str,
    schema: &str,
    table: &str,
    driver: &DbDriver,
    state: &State<'_, Mutex<AppState>>,
) -> Result<String, DbError> {
    // Get table schema
    use crate::commands::schema::get_table_schema;
    let table_schema = get_table_schema(connection_id.to_string(), schema.to_string(), table.to_string(), state.clone()).await?;

    // Build CREATE TABLE statement
    let mut create_stmt = match driver {
        DbDriver::Postgres | DbDriver::Sqlite => {
            format!("CREATE TABLE \"{}\".\"{}\" (\n", schema, table)
        }
        DbDriver::MySql => {
            format!("CREATE TABLE `{}`.`{}` (\n", schema, table)
        }
        _ => {
            return Err(DbError::InvalidInput(format!("CREATE TABLE export not supported for {:?}", driver)));
        }
    };

    // Add columns
    let columns_sql: Vec<String> = table_schema.columns.iter().map(|col| {
        let mut parts = vec![
            format!("  \"{}\"", col.name),
            col.data_type.clone(),
        ];

        if !col.nullable {
            parts.push("NOT NULL".to_string());
        }

        if let Some(default) = &col.default_value {
            parts.push(format!("DEFAULT {}", default));
        }

        parts.join(" ")
    }).collect();

    create_stmt.push_str(&columns_sql.join(",\n"));
    create_stmt.push_str("\n);");

    Ok(create_stmt)
}

/// Export table data as INSERT statements
async fn export_table_data_to_sql(
    file: &mut File,
    connection_id: &str,
    schema: &str,
    table: &str,
    driver: &DbDriver,
    state: &State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    // Query all data from table
    use crate::commands::query::execute_query;

    let query = match driver {
        DbDriver::Postgres | DbDriver::Sqlite => {
            format!("SELECT * FROM \"{}\".\"{}\"", schema, table)
        }
        DbDriver::MySql => {
            format!("SELECT * FROM `{}`.`{}`", schema, table)
        }
        _ => {
            return Ok(()); // Skip data export for unsupported drivers
        }
    };

    let result = execute_query(connection_id.to_string(), query, state.clone()).await?;

    if result.rows.is_empty() {
        writeln!(file, "-- No data in table")
            .map_err(|e| DbError::InternalError(format!("Failed to write comment: {}", e)))?;
        return Ok(());
    }

    // Generate INSERT statements
    writeln!(file, "\n-- Data for table {}.{}", schema, table)
        .map_err(|e| DbError::InternalError(format!("Failed to write comment: {}", e)))?;

    for row in result.rows {
        let insert_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite => {
                let values: Vec<String> = row.iter().map(|v| sql_value_to_string(v)).collect();
                format!("INSERT INTO \"{}\".\"{}\" VALUES ({});", schema, table, values.join(", "))
            }
            DbDriver::MySql => {
                let values: Vec<String> = row.iter().map(|v| sql_value_to_string(v)).collect();
                format!("INSERT INTO `{}`.`{}` VALUES ({});", schema, table, values.join(", "))
            }
            _ => String::new(),
        };

        writeln!(file, "{}", insert_stmt)
            .map_err(|e| DbError::InternalError(format!("Failed to write INSERT: {}", e)))?;
    }

    Ok(())
}

/// Convert JSON value to SQL literal
fn sql_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")), // Escape single quotes
        Value::Array(_) | Value::Object(_) => format!("'{}'", value.to_string().replace('\'', "''")),
    }
}

/// Import SQL dump file into database
///
/// Imports a SQL dump file by executing all SQL statements in it.
/// Supports transaction mode for atomic imports.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `file_path` - Path to the SQL dump file
/// * `options` - Import options (transaction mode, error handling)
///
/// # Frontend Usage
///
/// ```typescript
/// import { open } from '@tauri-apps/plugin-dialog';
///
/// const filePath = await open({
///   filters: [{ name: 'SQL', extensions: ['sql'] }]
/// });
///
/// if (filePath) {
///   await invoke('import_from_sql', {
///     connectionId: 'conn-123',
///     filePath,
///     options: {
///       continueOnError: false,
///       useTransaction: true
///     }
///   });
/// }
/// ```
/// Signal an in-progress import to stop after the current statement.
#[tauri::command]
pub async fn cancel_import(cancel_flag: State<'_, Arc<AtomicBool>>) -> Result<(), DbError> {
    cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn import_from_sql(
    connection_id: String,
    file_path: String,
    options: SqlImportOptions,
    state: State<'_, Mutex<AppState>>,
    cancel_flag: State<'_, Arc<AtomicBool>>,
) -> Result<SqlImportResult, DbError> {
    use crate::commands::query::execute_query;

    // Reset cancel flag at the start of each import
    cancel_flag.store(false, Ordering::Relaxed);

    // Get driver type from connection profile
    let driver = {
        let state_lock = state.lock().unwrap();
        state_lock
            .connection_profiles
            .get(&connection_id)
            .map(|profile| profile.driver.clone())
            .ok_or_else(|| DbError::NotFound(format!("Connection profile {} not found", connection_id)))?
    };

    // Open SQL file (stream it — don't load into memory)
    let file = File::open(&file_path)
        .map_err(|e| DbError::InternalError(format!("Failed to open SQL file: {}", e)))?;
    let reader = BufReader::new(file);

    // For MySQL dumps: disable FK/unique checks and strict mode for duration of import
    if matches!(driver, DbDriver::MySql) {
        for stmt in &[
            "SET SESSION foreign_key_checks = 0",
            "SET SESSION unique_checks = 0",
            "SET SESSION sql_notes = 0",
            "SET SESSION sql_mode = ''",
            // MariaDB ignores SET SESSION for max_allowed_packet (global-only variable).
            // Use SET GLOBAL so single-row statements with large BLOBs/TEXT can be imported.
            // Requires SUPER privilege — silently ignored if the user lacks it.
            "SET GLOBAL max_allowed_packet = 1073741824",
        ] {
            let _ = execute_query(connection_id.clone(), stmt.to_string(), state.clone()).await;
        }
    }

    // Begin transaction if requested
    if options.use_transaction {
        let begin_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite => "BEGIN",
            DbDriver::MySql => "START TRANSACTION",
            _ => return Err(DbError::InvalidInput("Transactions not supported for this driver".to_string())),
        };
        execute_query(connection_id.clone(), begin_stmt.to_string(), state.clone()).await
            .map_err(|e| DbError::QueryError(format!("Failed to begin transaction: {}", e)))?;
    }

    let mut executed: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: Vec<String> = Vec::new(); // all errors, no cap
    let mut stmt_index: usize = 0;
    let mut current_statement = String::new();
    // Track current statement delimiter (mysqldump uses DELIMITER ;; for triggers/procedures)
    let mut current_delimiter = ";".to_string();

    for line_result in reader.lines() {
        // Check for user-requested cancellation before processing each line
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let line = line_result
            .map_err(|e| DbError::InternalError(format!("Failed to read SQL file: {}", e)))?;
        let trimmed = line.trim();

        // Skip empty lines and full-line comments
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }

        // Handle DELIMITER meta-command (mysql client command, not SQL)
        // e.g. "DELIMITER ;;" or "DELIMITER ;"
        if trimmed.to_uppercase().starts_with("DELIMITER") {
            if let Some(new_delim) = trimmed.split_whitespace().nth(1) {
                current_delimiter = new_delim.to_string();
            }
            continue; // Never send DELIMITER to the server
        }

        current_statement.push_str(&line);
        current_statement.push('\n');

        // Statement is complete when the line ends with the current delimiter
        if trimmed.ends_with(current_delimiter.as_str()) {
            let stmt = current_statement.trim().to_string();
            current_statement.clear();

            // Strip trailing delimiter (when delimiter is not plain ";", strip it)
            let stmt = if current_delimiter != ";" {
                stmt.trim_end_matches(current_delimiter.as_str()).trim().to_string()
            } else {
                // Strip the trailing semicolon for clean execution
                stmt.trim_end_matches(';').trim().to_string()
            };

            if stmt.is_empty() {
                continue;
            }

            // Normalize known mysqldump quirks before execution.
            // MySQL 8.0.x client dumping from MariaDB generates "REPLACE IGNORE INTO"
            // which is invalid syntax on both MySQL and MariaDB — normalize to
            // "INSERT IGNORE INTO" which preserves the duplicate-skip semantics.
            let stmt = normalize_dump_stmt(stmt);

            stmt_index += 1;

            // Skip advisory/client-only statements that the server can't handle
            let first_word = stmt
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_uppercase();
            if matches!(first_word.as_str(), "LOCK" | "UNLOCK") {
                skipped += 1;
                continue;
            }

            // Proactively split large INSERT batches before sending to avoid
            // exceeding the server's max_allowed_packet. MariaDB 10.x ignores
            // SET SESSION for this variable, so large mysqldump batches must be
            // split client-side. Threshold: 4 MB — conservative enough to stay
            // under any reasonable server configuration (default is 16 MB).
            const SPLIT_THRESHOLD: usize = 4 * 1024 * 1024;
            let sub_stmts: Vec<String> = if stmt.len() > SPLIT_THRESHOLD {
                let split = split_insert_values(&stmt, 50);
                if split.len() > 1 {
                    split
                } else {
                    // Single row larger than the server can accept — skip it.
                    skipped += 1;
                    errors.push(format!(
                        "Statement {}: skipped — single row is {} MB, exceeds server max_allowed_packet",
                        stmt_index,
                        stmt.len() / 1024 / 1024
                    ));
                    continue;
                }
            } else {
                vec![stmt.clone()]
            };

            'sub: for sub_stmt in &sub_stmts {
                match execute_query(connection_id.clone(), sub_stmt.clone(), state.clone()).await {
                    Ok(_) => executed += 1,
                    Err(ref e) => {
                        let msg = e.to_string().to_lowercase();
                        // "broken pipe" / "os error 32" = server closed connection
                        // because the packet exceeded its max_allowed_packet.
                        let oversized = msg.contains("packet too large")
                            || msg.contains("broken pipe")
                            || msg.contains("os error 32");

                        errors.push(format!(
                            "Statement {}{}: {}",
                            stmt_index,
                            if oversized { " (oversized)" } else { "" },
                            e
                        ));
                        if !options.continue_on_error {
                            if options.use_transaction {
                                let rollback = match driver {
                                    DbDriver::Postgres | DbDriver::Sqlite | DbDriver::MySql => "ROLLBACK",
                                    _ => "",
                                };
                                if !rollback.is_empty() {
                                    let _ = execute_query(
                                        connection_id.clone(),
                                        rollback.to_string(),
                                        state.clone(),
                                    ).await;
                                }
                            }
                            return Err(DbError::QueryError(format!(
                                "Import failed at statement {}: {}",
                                stmt_index, e
                            )));
                        }
                        // After a broken pipe the connection is dead — skip remaining
                        // sub-statements to avoid a cascade of connection errors.
                        if oversized {
                            break 'sub;
                        }
                    }
                }
            }
        }
    }

    // Execute any remaining statement that lacked a trailing delimiter
    let remaining = current_statement.trim().to_string();
    if !remaining.is_empty() && !remaining.starts_with("--") {
        let _ = execute_query(connection_id.clone(), remaining, state.clone()).await;
    }

    // Commit transaction
    if options.use_transaction {
        let commit_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite | DbDriver::MySql => "COMMIT",
            _ => "",
        };
        if !commit_stmt.is_empty() {
            execute_query(connection_id.clone(), commit_stmt.to_string(), state.clone()).await
                .map_err(|e| DbError::QueryError(format!("Failed to commit: {}", e)))?;
        }
    }

    // Restore MySQL session settings
    if matches!(driver, DbDriver::MySql) {
        for stmt in &[
            "SET SESSION foreign_key_checks = 1",
            "SET SESSION unique_checks = 1",
            "SET SESSION sql_notes = 1",
        ] {
            let _ = execute_query(connection_id.clone(), stmt.to_string(), state.clone()).await;
        }
    }

    // Write error log file if there were any errors
    let log_file: Option<String> = if errors.is_empty() {
        None
    } else {
        let log_path = derive_log_path(&file_path);
        match write_import_log(&log_path, &file_path, executed, skipped, &errors) {
            Ok(()) => Some(log_path),
            Err(_) => None, // Don't fail the import just because log writing failed
        }
    };

    Ok(SqlImportResult {
        executed,
        errors_count: errors.len(),
        skipped,
        first_error: errors.first().cloned(),
        cancelled: cancel_flag.load(Ordering::Relaxed),
        log_file,
    })
}

/// Derive a log file path from the SQL file path.
/// e.g. `/path/to/dump.sql` → `/path/to/dump_import_errors.log`
fn derive_log_path(sql_path: &str) -> String {
    let stem = sql_path.strip_suffix(".sql").unwrap_or(sql_path);
    format!("{}_import_errors.log", stem)
}

/// Write all import errors to a plain-text log file.
fn write_import_log(
    log_path: &str,
    sql_path: &str,
    executed: usize,
    skipped: usize,
    errors: &[String],
) -> std::io::Result<()> {
    let mut f = File::create(log_path)?;
    writeln!(f, "DB-Hive SQL Import Error Log")?;
    writeln!(f, "Source file : {}", sql_path)?;
    writeln!(f, "Timestamp   : {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))?;
    writeln!(f, "Executed    : {}", executed)?;
    writeln!(f, "Skipped     : {}", skipped)?;
    writeln!(f, "Errors      : {}", errors.len())?;
    writeln!(f, "{}", "-".repeat(60))?;
    for err in errors {
        writeln!(f, "{}", err)?;
    }
    Ok(())
}

/// Split a multi-row INSERT statement into smaller batches.
///
/// Large mysqldump INSERT statements can exceed MySQL's `max_allowed_packet`.
/// This splits `INSERT ... VALUES (r1),(r2),...` into multiple statements
/// each containing at most `max_rows` value groups.
///
/// Returns a vec with the original single statement if splitting isn't possible
/// or unnecessary.
fn split_insert_values(stmt: &str, max_rows: usize) -> Vec<String> {
    // Find the VALUES keyword (case-insensitive).
    // mysqldump may emit "VALUES (" or "VALUES\n(" so we must not require a
    // trailing space — just locate the keyword and skip any following whitespace.
    let upper = stmt.to_uppercase();
    let values_pos = match upper.find("VALUES") {
        Some(p) => p,
        None => return vec![stmt.to_string()],
    };

    let prefix = &stmt[..values_pos + "VALUES".len()]; // "INSERT ... VALUES"
    let values_str = stmt[values_pos + "VALUES".len()..]
        .trim_start()           // skip the whitespace / newline after VALUES
        .trim_end_matches(';')
        .trim();

    let groups = parse_value_row_groups(values_str);

    if groups.len() <= max_rows {
        return vec![stmt.to_string()];
    }

    groups
        .chunks(max_rows)
        .map(|chunk| format!("{} {}", prefix, chunk.join(",")))
        .collect()
}

/// Parse the VALUES portion `(a,b),(c,d),...` into individual row strings `["(a,b)","(c,d)",...]`.
/// Handles nested parentheses, single-quoted strings, and backslash escapes.
fn parse_value_row_groups(s: &str) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_single_quote = false;
    let mut escape_next = false;
    let mut group_start: Option<usize> = None;
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        if in_single_quote {
            if b == b'\\' {
                escape_next = true;
            } else if b == b'\'' {
                in_single_quote = false;
            }
        } else {
            match b {
                b'\'' => in_single_quote = true,
                b'(' => {
                    depth += 1;
                    if depth == 1 {
                        group_start = Some(i);
                    }
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start) = group_start {
                            groups.push(s[start..=i].to_string());
                            group_start = None;
                        }
                    }
                }
                _ => {}
            }
        }

        i += 1;
    }

    groups
}

/// Normalize SQL statements from mysqldump to fix known cross-tool quirks.
///
/// MySQL 8.0.x client dumping from a MariaDB server generates invalid SQL like
/// "REPLACE IGNORE INTO" which neither MySQL nor MariaDB accept. This function
/// rewrites known bad patterns into equivalent valid SQL before execution.
fn normalize_dump_stmt(stmt: String) -> String {
    let words: Vec<&str> = stmt.split_ascii_whitespace().collect();

    // "REPLACE [IGNORE] INTO ..." — MySQL 8.0 client / MariaDB dump artifact.
    // REPLACE has no IGNORE modifier; convert to INSERT IGNORE which has the
    // same "skip duplicate key errors" semantics.
    if words.len() >= 3
        && words[0].eq_ignore_ascii_case("REPLACE")
        && words[1].eq_ignore_ascii_case("IGNORE")
        && words[2].eq_ignore_ascii_case("INTO")
    {
        let after_replace = stmt
            .find(|c: char| c.is_ascii_whitespace())
            .map(|i| &stmt[i..])
            .unwrap_or("");
        return format!("INSERT{}", after_replace);
    }

    stmt
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn test_escape_csv_value_simple() {
        assert_eq!(escape_csv_value("hello"), "hello");
        assert_eq!(escape_csv_value("123"), "123");
    }

    #[test]
    fn test_escape_csv_value_with_comma() {
        assert_eq!(escape_csv_value("hello,world"), "\"hello,world\"");
    }

    #[test]
    fn test_escape_csv_value_with_quotes() {
        assert_eq!(escape_csv_value("hello \"world\""), "\"hello \"\"world\"\"\"");
    }

    #[test]
    fn test_escape_csv_value_with_newline() {
        assert_eq!(escape_csv_value("hello\nworld"), "\"hello\nworld\"");
    }

    #[test]
    fn test_json_value_to_string() {
        assert_eq!(json_value_to_string(&Value::Null), "");
        assert_eq!(json_value_to_string(&json!(true)), "true");
        assert_eq!(json_value_to_string(&json!(42)), "42");
        assert_eq!(json_value_to_string(&json!("hello")), "hello");
        assert_eq!(json_value_to_string(&json!({"key": "value"})), "{\"key\":\"value\"}");
    }

    #[test]
    fn test_export_csv() {
        let temp_file = std::env::temp_dir().join("test_export.csv");
        let file_path = temp_file.to_str().unwrap().to_string();

        let columns = vec!["id".to_string(), "name".to_string(), "age".to_string()];
        let rows = vec![
            vec![json!(1), json!("Alice"), json!(30)],
            vec![json!(2), json!("Bob"), json!(25)],
        ];

        let result = export_to_csv(file_path.clone(), columns, rows);
        assert!(result.is_ok());

        // Read and verify the file
        let content = fs::read_to_string(&temp_file).unwrap();
        assert!(content.contains("id,name,age"));
        assert!(content.contains("1,Alice,30"));
        assert!(content.contains("2,Bob,25"));

        // Cleanup
        let _ = fs::remove_file(temp_file);
    }

    #[test]
    fn test_export_json() {
        let temp_file = std::env::temp_dir().join("test_export.json");
        let file_path = temp_file.to_str().unwrap().to_string();

        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec![json!(1), json!("Alice")],
            vec![json!(2), json!("Bob")],
        ];

        let result = export_to_json(file_path.clone(), columns, rows);
        assert!(result.is_ok());

        // Read and verify the file
        let content = fs::read_to_string(&temp_file).unwrap();
        let parsed: Vec<serde_json::Map<String, Value>> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["name"], json!("Alice"));
        assert_eq!(parsed[1]["name"], json!("Bob"));

        // Cleanup
        let _ = fs::remove_file(temp_file);
    }
}
