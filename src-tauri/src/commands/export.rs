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
use std::sync::Mutex;
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
#[tauri::command]
pub async fn import_from_sql(
    connection_id: String,
    file_path: String,
    options: SqlImportOptions,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, DbError> {
    // Read SQL file
    let file = File::open(&file_path)
        .map_err(|e| DbError::InternalError(format!("Failed to open SQL file: {}", e)))?;

    let reader = BufReader::new(file);
    let mut sql_statements = Vec::new();
    let mut current_statement = String::new();

    // Parse SQL file into statements
    for line in reader.lines() {
        let line = line.map_err(|e| DbError::InternalError(format!("Failed to read SQL file: {}", e)))?;

        // Skip comments
        let trimmed = line.trim();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            continue;
        }

        current_statement.push_str(&line);
        current_statement.push('\n');

        // Check if statement is complete (ends with semicolon)
        if trimmed.ends_with(';') {
            sql_statements.push(current_statement.clone());
            current_statement.clear();
        }
    }

    // Add last statement if not empty
    if !current_statement.trim().is_empty() {
        sql_statements.push(current_statement);
    }

    // Execute statements
    use crate::commands::query::execute_query;

    let mut executed = 0;
    let mut errors = Vec::new();

    // Get driver type from connection profile
    let driver = {
        let state_lock = state.lock().unwrap();
        state_lock
            .connection_profiles
            .get(&connection_id)
            .map(|profile| profile.driver.clone())
            .ok_or_else(|| DbError::NotFound(format!("Connection profile {} not found", connection_id)))?
    };

    // Check transaction support before starting
    if options.use_transaction {
        let begin_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite => "BEGIN;",
            DbDriver::MySql => "START TRANSACTION;",
            _ => return Err(DbError::InvalidInput("Transactions not supported for this driver".to_string())),
        };

        execute_query(connection_id.clone(), begin_stmt.to_string(), state.clone()).await?;
    }

    for (i, stmt) in sql_statements.iter().enumerate() {
        match execute_query(connection_id.clone(), stmt.clone(), state.clone()).await {
            Ok(_) => {
                executed += 1;
            }
            Err(e) => {
                errors.push(format!("Statement {}: {}", i + 1, e));
                if !options.continue_on_error {
                    if options.use_transaction {
                        let rollback_stmt = match driver {
                            DbDriver::Postgres | DbDriver::Sqlite | DbDriver::MySql => "ROLLBACK;",
                            _ => "",
                        };
                        let _ = execute_query(connection_id.clone(), rollback_stmt.to_string(), state.clone()).await;
                    }
                    return Err(DbError::QueryError(format!("Import failed at statement {}: {}", i + 1, e)));
                }
            }
        }
    }

    if options.use_transaction {
        let commit_stmt = match driver {
            DbDriver::Postgres | DbDriver::Sqlite | DbDriver::MySql => "COMMIT;",
            _ => "",
        };
        execute_query(connection_id, commit_stmt.to_string(), state).await?;
    }

    let result = if errors.is_empty() {
        format!("Successfully imported {} statements", executed)
    } else {
        format!("Imported {} statements with {} errors: {}", executed, errors.len(), errors.join("; "))
    };

    Ok(result)
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
