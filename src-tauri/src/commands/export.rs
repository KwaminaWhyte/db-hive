//! Export commands for query results
//!
//! This module provides Tauri commands for exporting query results to various
//! formats like CSV and JSON. Uses native file dialogs for save location.

use crate::models::DbError;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::Path;

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
