//! Data Import Commands
//!
//! Provides commands for importing CSV and Excel files into database tables
//! with column mapping support.

use crate::models::DbError;
use crate::state::AppState;
use calamine::{open_workbook, Reader, Xlsx, Xls};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

/// Preview data from a file (first N rows)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    /// Detected columns from the file
    pub columns: Vec<String>,
    /// Preview rows (first 100 rows)
    pub rows: Vec<Vec<String>>,
    /// Total row count (if known)
    pub total_rows: Option<usize>,
    /// Detected data types for each column
    pub detected_types: Vec<String>,
    /// File type (csv, xlsx, xls)
    pub file_type: String,
    /// Sheet names (for Excel files)
    pub sheet_names: Option<Vec<String>>,
}

/// Column mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMapping {
    /// Source column name from the file
    pub source_column: String,
    /// Target column name in the database table
    pub target_column: String,
    /// Target data type (for type conversion)
    pub target_type: Option<String>,
    /// Default value if source is empty
    pub default_value: Option<String>,
    /// Whether to skip this column
    pub skip: bool,
}

/// Import options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataImportOptions {
    /// Target table name
    pub table_name: String,
    /// Target schema (optional)
    pub schema: Option<String>,
    /// Column mappings
    pub column_mappings: Vec<ColumnMapping>,
    /// Number of rows to skip (e.g., for headers)
    pub skip_rows: usize,
    /// Whether to create the table if it doesn't exist
    pub create_table: bool,
    /// Whether to truncate the table before import
    pub truncate_before: bool,
    /// Batch size for inserts
    pub batch_size: usize,
    /// CSV delimiter (for CSV files)
    pub delimiter: Option<char>,
    /// Sheet name (for Excel files)
    pub sheet_name: Option<String>,
    /// Whether first row is header
    pub first_row_is_header: bool,
}

/// Import result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// Number of rows successfully imported
    pub rows_imported: usize,
    /// Number of rows that failed
    pub rows_failed: usize,
    /// Error messages for failed rows
    pub errors: Vec<String>,
    /// Whether the import completed successfully
    pub success: bool,
}

/// Preview a file for import
#[tauri::command]
pub async fn preview_import_file(
    file_path: String,
    sheet_name: Option<String>,
    delimiter: Option<char>,
    max_rows: Option<usize>,
) -> Result<ImportPreview, String> {
    let path = Path::new(&file_path);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let max_rows = max_rows.unwrap_or(100);

    match extension.as_str() {
        "csv" | "tsv" | "txt" => preview_csv(&file_path, delimiter, max_rows).map_err(|e| e.to_string()),
        "xlsx" => preview_xlsx(&file_path, sheet_name, max_rows).map_err(|e| e.to_string()),
        "xls" => preview_xls(&file_path, sheet_name, max_rows).map_err(|e| e.to_string()),
        _ => Err(format!("Unsupported file type: {}", extension)),
    }
}

/// Preview CSV file
fn preview_csv(file_path: &str, delimiter: Option<char>, max_rows: usize) -> Result<ImportPreview, DbError> {
    let delimiter = delimiter.unwrap_or(',') as u8;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .from_path(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open CSV: {}", e)))?;

    // Get headers
    let headers = reader
        .headers()
        .map_err(|e| DbError::ImportError(format!("Failed to read headers: {}", e)))?
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut type_samples: Vec<Vec<String>> = vec![Vec::new(); headers.len()];

    for (i, result) in reader.records().enumerate() {
        if i >= max_rows {
            break;
        }

        match result {
            Ok(record) => {
                let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();

                // Collect samples for type detection
                for (j, value) in row.iter().enumerate() {
                    if j < type_samples.len() && !value.is_empty() {
                        type_samples[j].push(value.clone());
                    }
                }

                rows.push(row);
            }
            Err(e) => {
                eprintln!("Error reading row {}: {}", i, e);
            }
        }
    }

    // Detect types
    let detected_types = type_samples
        .iter()
        .map(|samples| detect_column_type(samples))
        .collect();

    Ok(ImportPreview {
        columns: headers,
        rows,
        total_rows: None, // Would need to count all rows
        detected_types,
        file_type: "csv".to_string(),
        sheet_names: None,
    })
}

/// Preview XLSX file
fn preview_xlsx(file_path: &str, sheet_name: Option<String>, max_rows: usize) -> Result<ImportPreview, DbError> {
    let mut workbook: Xlsx<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;

    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

    let default_sheet = String::new();
    let sheet = sheet_name
        .as_ref()
        .unwrap_or_else(|| sheet_names.first().unwrap_or(&default_sheet));

    match workbook.worksheet_range(sheet) {
        Ok(range) => {
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut headers: Vec<String> = Vec::new();
            let mut type_samples: Vec<Vec<String>> = Vec::new();

            for (i, row) in range.rows().enumerate() {
                if i == 0 {
                    // First row as headers
                    headers = row.iter().map(|c| cell_to_string(c)).collect();
                    type_samples = vec![Vec::new(); headers.len()];
                } else if i <= max_rows {
                    let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                    // Collect samples for type detection
                    for (j, value) in row_data.iter().enumerate() {
                        if j < type_samples.len() && !value.is_empty() {
                            type_samples[j].push(value.clone());
                        }
                    }

                    rows.push(row_data);
                }
            }

            let detected_types = type_samples
                .iter()
                .map(|samples| detect_column_type(samples))
                .collect();

            Ok(ImportPreview {
                columns: headers,
                rows,
                total_rows: Some(range.rows().len().saturating_sub(1)),
                detected_types,
                file_type: "xlsx".to_string(),
                sheet_names: Some(sheet_names),
            })
        }
        Err(e) => Err(DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))),
    }
}

/// Preview XLS file
fn preview_xls(file_path: &str, sheet_name: Option<String>, max_rows: usize) -> Result<ImportPreview, DbError> {
    let mut workbook: Xls<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;

    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

    let default_sheet = String::new();
    let sheet = sheet_name
        .as_ref()
        .unwrap_or_else(|| sheet_names.first().unwrap_or(&default_sheet));

    match workbook.worksheet_range(sheet) {
        Ok(range) => {
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut headers: Vec<String> = Vec::new();
            let mut type_samples: Vec<Vec<String>> = Vec::new();

            for (i, row) in range.rows().enumerate() {
                if i == 0 {
                    headers = row.iter().map(|c| cell_to_string(c)).collect();
                    type_samples = vec![Vec::new(); headers.len()];
                } else if i <= max_rows {
                    let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                    for (j, value) in row_data.iter().enumerate() {
                        if j < type_samples.len() && !value.is_empty() {
                            type_samples[j].push(value.clone());
                        }
                    }

                    rows.push(row_data);
                }
            }

            let detected_types = type_samples
                .iter()
                .map(|samples| detect_column_type(samples))
                .collect();

            Ok(ImportPreview {
                columns: headers,
                rows,
                total_rows: Some(range.rows().len().saturating_sub(1)),
                detected_types,
                file_type: "xls".to_string(),
                sheet_names: Some(sheet_names),
            })
        }
        Err(e) => Err(DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))),
    }
}

/// Convert calamine cell to string
fn cell_to_string(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::Empty => String::new(),
        calamine::Data::String(s) => s.clone(),
        calamine::Data::Float(f) => {
            // Check if it's a whole number
            if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                f.to_string()
            }
        }
        calamine::Data::Int(i) => i.to_string(),
        calamine::Data::Bool(b) => b.to_string(),
        calamine::Data::DateTime(dt) => format!("{}", dt),
        calamine::Data::DateTimeIso(s) => s.clone(),
        calamine::Data::DurationIso(s) => s.clone(),
        calamine::Data::Error(e) => format!("#ERROR: {:?}", e),
    }
}

/// Detect the likely data type of a column based on sample values
fn detect_column_type(samples: &[String]) -> String {
    if samples.is_empty() {
        return "TEXT".to_string();
    }

    let mut is_integer = true;
    let mut is_float = true;
    let mut is_boolean = true;
    let mut is_date = true;

    for value in samples.iter().take(50) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check integer
        if is_integer && trimmed.parse::<i64>().is_err() {
            is_integer = false;
        }

        // Check float
        if is_float && trimmed.parse::<f64>().is_err() {
            is_float = false;
        }

        // Check boolean
        let lower = trimmed.to_lowercase();
        if is_boolean && !matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "1" | "0") {
            is_boolean = false;
        }

        // Check date (simple patterns)
        if is_date {
            let date_patterns = [
                r"^\d{4}-\d{2}-\d{2}",  // YYYY-MM-DD
                r"^\d{2}/\d{2}/\d{4}",  // MM/DD/YYYY
                r"^\d{2}-\d{2}-\d{4}",  // DD-MM-YYYY
            ];
            let is_date_match = date_patterns.iter().any(|p| {
                regex::Regex::new(p).map(|r| r.is_match(trimmed)).unwrap_or(false)
            });
            if !is_date_match {
                is_date = false;
            }
        }
    }

    if is_boolean {
        "BOOLEAN".to_string()
    } else if is_integer {
        "INTEGER".to_string()
    } else if is_float {
        "DECIMAL".to_string()
    } else if is_date {
        "DATE".to_string()
    } else {
        "TEXT".to_string()
    }
}

/// Import data from a file into a database table
#[tauri::command]
pub async fn import_data_to_table(
    connection_id: String,
    file_path: String,
    options: DataImportOptions,
    state: State<'_, Mutex<AppState>>,
) -> Result<ImportResult, String> {
    // Read file data
    let path = Path::new(&file_path);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (columns, rows) = match extension.as_str() {
        "csv" | "tsv" | "txt" => read_csv_data(&file_path, options.delimiter, options.skip_rows, options.first_row_is_header)
            .map_err(|e| e.to_string())?,
        "xlsx" => read_xlsx_data(&file_path, options.sheet_name.as_deref(), options.skip_rows, options.first_row_is_header)
            .map_err(|e| e.to_string())?,
        "xls" => read_xls_data(&file_path, options.sheet_name.as_deref(), options.skip_rows, options.first_row_is_header)
            .map_err(|e| e.to_string())?,
        _ => return Err(format!("Unsupported file type: {}", extension)),
    };

    // Build column mapping (used for potential future enhancements)
    let _mapping: HashMap<String, &ColumnMapping> = options
        .column_mappings
        .iter()
        .filter(|m| !m.skip)
        .map(|m| (m.source_column.clone(), m))
        .collect();

    // Get connection
    let connection = {
        let state = state.lock().map_err(|e| e.to_string())?;
        state
            .get_connection(&connection_id)
            .ok_or_else(|| format!("Connection not found: {}", connection_id))?
            .clone()
    };

    // Build table name with schema
    let full_table_name = if let Some(schema) = &options.schema {
        format!("{}.{}", schema, options.table_name)
    } else {
        options.table_name.clone()
    };

    // Truncate if requested
    if options.truncate_before {
        let truncate_sql = format!("TRUNCATE TABLE {}", full_table_name);
        connection
            .execute_query(&truncate_sql)
            .await
            .map_err(|e| format!("Failed to truncate table: {}", e))?;
    }

    // Build INSERT statement
    let target_columns: Vec<String> = options
        .column_mappings
        .iter()
        .filter(|m| !m.skip)
        .map(|m| m.target_column.clone())
        .collect();

    // Note: Using string formatting for VALUES instead of prepared statements
    // for cross-database compatibility (placeholder syntax differs)

    let mut rows_imported = 0;
    let mut rows_failed = 0;
    let mut errors: Vec<String> = Vec::new();

    // Import in batches
    for (row_idx, row) in rows.iter().enumerate() {
        // Build values based on mapping
        let mut values: Vec<String> = Vec::new();

        for col_mapping in options.column_mappings.iter().filter(|m| !m.skip) {
            let source_idx = columns.iter().position(|c| c == &col_mapping.source_column);

            let value = if let Some(idx) = source_idx {
                row.get(idx).cloned().unwrap_or_default()
            } else {
                col_mapping.default_value.clone().unwrap_or_default()
            };

            values.push(if value.is_empty() {
                "NULL".to_string()
            } else {
                format!("'{}'", value.replace('\'', "''"))
            });
        }

        // Build actual INSERT for this row
        let row_sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            full_table_name,
            target_columns.join(", "),
            values.join(", ")
        );

        match connection.execute_query(&row_sql).await {
            Ok(_) => rows_imported += 1,
            Err(e) => {
                rows_failed += 1;
                if errors.len() < 10 {
                    errors.push(format!("Row {}: {}", row_idx + 1, e));
                }
            }
        }
    }

    Ok(ImportResult {
        rows_imported,
        rows_failed,
        errors,
        success: rows_failed == 0,
    })
}

/// Read CSV data
fn read_csv_data(
    file_path: &str,
    delimiter: Option<char>,
    skip_rows: usize,
    first_row_is_header: bool,
) -> Result<(Vec<String>, Vec<Vec<String>>), DbError> {
    let delimiter = delimiter.unwrap_or(',') as u8;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(first_row_is_header)
        .flexible(true)
        .from_path(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open CSV: {}", e)))?;

    let headers = if first_row_is_header {
        reader
            .headers()
            .map_err(|e| DbError::ImportError(format!("Failed to read headers: {}", e)))?
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        // Generate column names
        Vec::new()
    };

    let mut rows: Vec<Vec<String>> = Vec::new();
    for (i, result) in reader.records().enumerate() {
        if i < skip_rows {
            continue;
        }
        if let Ok(record) = result {
            rows.push(record.iter().map(|s| s.to_string()).collect());
        }
    }

    // If no headers, generate them
    let headers = if headers.is_empty() && !rows.is_empty() {
        (0..rows[0].len())
            .map(|i| format!("Column{}", i + 1))
            .collect()
    } else {
        headers
    };

    Ok((headers, rows))
}

/// Read XLSX data
fn read_xlsx_data(
    file_path: &str,
    sheet_name: Option<&str>,
    skip_rows: usize,
    first_row_is_header: bool,
) -> Result<(Vec<String>, Vec<Vec<String>>), DbError> {
    let mut workbook: Xlsx<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;

    let sheet_names = workbook.sheet_names().to_vec();
    let sheet = sheet_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| sheet_names.first().cloned().unwrap_or_default());

    match workbook.worksheet_range(&sheet) {
        Ok(range) => {
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut headers: Vec<String> = Vec::new();

            for (i, row) in range.rows().enumerate() {
                if i < skip_rows {
                    continue;
                }

                let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                if first_row_is_header && headers.is_empty() {
                    headers = row_data;
                } else {
                    rows.push(row_data);
                }
            }

            // Generate headers if needed
            if headers.is_empty() && !rows.is_empty() {
                headers = (0..rows[0].len())
                    .map(|i| format!("Column{}", i + 1))
                    .collect();
            }

            Ok((headers, rows))
        }
        Err(e) => Err(DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))),
    }
}

/// Read XLS data
fn read_xls_data(
    file_path: &str,
    sheet_name: Option<&str>,
    skip_rows: usize,
    first_row_is_header: bool,
) -> Result<(Vec<String>, Vec<Vec<String>>), DbError> {
    let mut workbook: Xls<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;

    let sheet_names = workbook.sheet_names().to_vec();
    let sheet = sheet_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| sheet_names.first().cloned().unwrap_or_default());

    match workbook.worksheet_range(&sheet) {
        Ok(range) => {
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut headers: Vec<String> = Vec::new();

            for (i, row) in range.rows().enumerate() {
                if i < skip_rows {
                    continue;
                }

                let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                if first_row_is_header && headers.is_empty() {
                    headers = row_data;
                } else {
                    rows.push(row_data);
                }
            }

            if headers.is_empty() && !rows.is_empty() {
                headers = (0..rows[0].len())
                    .map(|i| format!("Column{}", i + 1))
                    .collect();
            }

            Ok((headers, rows))
        }
        Err(e) => Err(DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))),
    }
}

/// Get list of tables for import target selection
#[tauri::command]
pub async fn get_tables_for_import(
    connection_id: String,
    schema: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, String> {
    let connection = {
        let state = state.lock().map_err(|e| e.to_string())?;
        state
            .get_connection(&connection_id)
            .ok_or_else(|| format!("Connection not found: {}", connection_id))?
            .clone()
    };

    let tables = connection
        .get_tables(&schema.unwrap_or_else(|| "public".to_string()))
        .await
        .map_err(|e| e.to_string())?;

    Ok(tables.into_iter().map(|t| t.name).collect())
}

/// Get columns for a specific table
#[tauri::command]
pub async fn get_table_columns_for_import(
    connection_id: String,
    table_name: String,
    schema: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TableColumnInfo>, String> {
    let connection = {
        let state = state.lock().map_err(|e| e.to_string())?;
        state
            .get_connection(&connection_id)
            .ok_or_else(|| format!("Connection not found: {}", connection_id))?
            .clone()
    };

    let schema_name = schema.unwrap_or_else(|| "public".to_string());
    let table_schema = connection
        .get_table_schema(&schema_name, &table_name)
        .await
        .map_err(|e| e.to_string())?;

    Ok(table_schema
        .columns
        .into_iter()
        .map(|c| TableColumnInfo {
            name: c.name,
            data_type: c.data_type,
            nullable: c.nullable,
            default_value: c.default_value,
            is_primary_key: c.is_primary_key,
        })
        .collect())
}

/// Column info for import mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}
