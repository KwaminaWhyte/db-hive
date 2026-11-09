//! Shell-agnostic import and export operations.
//!
//! Every operation takes a plain `&Mutex<AppState>` and knows nothing about
//! windows, dialogs or IPC.
//!
//! The file is split into pure builders and the async operations that use
//! them. That split is deliberate: every string that ends up in a file or in
//! a SQL statement is produced by a function that can be tested without a
//! database, because both are escaping surfaces where a mistake is either a
//! corrupt export or a SQL injection.
//!
//! Two escaping rules run through the whole file:
//!
//! * CSV values that contain the delimiter, a quote or a newline are quoted
//!   and their quotes doubled (RFC 4180). The delimiter is configurable, so
//!   the check is against the configured delimiter, never a hardcoded comma.
//! * Every identifier reaching SQL goes through `quote_ident` and every value
//!   through `escape_literal`, both per dialect. File contents are untrusted
//!   input; a CSV cell containing `'); DROP TABLE users; --` has to survive
//!   as data.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use calamine::{open_workbook, Data, Reader, Xls, Xlsx};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drivers::{DatabaseDriver, QueryResult};
use crate::models::metadata::ColumnInfo;
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

/// How many rows a file preview reads.
pub const MAX_PREVIEW_ROWS: usize = 100;

/// Types offered in the column-mapping dropdown.
pub const DATA_TYPES: [&str; 14] = [
    "TEXT",
    "INTEGER",
    "BIGINT",
    "DECIMAL",
    "NUMERIC",
    "REAL",
    "DOUBLE PRECISION",
    "BOOLEAN",
    "DATE",
    "TIMESTAMP",
    "TIME",
    "UUID",
    "JSON",
    "JSONB",
];

/// Statements shown to the user before an import runs. Enough to see what is
/// about to happen without rendering a million-line preview.
pub const PLAN_SAMPLE_STATEMENTS: usize = 3;

// ---------------------------------------------------------------------------
// Dialect helpers
// ---------------------------------------------------------------------------

/// Quote a SQL identifier for `driver`.
///
/// Mirrors each driver's own `quote_identifier`, but works from the profile's
/// `DbDriver` so dump and import SQL can be generated (and tested) without a
/// live connection.
pub fn quote_ident(driver: &DbDriver, ident: &str) -> String {
    match driver {
        DbDriver::MySql => format!("`{}`", ident.replace('`', "``")),
        DbDriver::SqlServer => format!("[{}]", ident.replace(']', "]]")),
        _ => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
}

/// Escape a value for inclusion in a single-quoted SQL string literal.
///
/// MySQL also treats backslash as an escape character inside literals, so
/// doubling quotes alone would let `\'` slip through and end the literal.
pub fn escape_literal(driver: &DbDriver, value: &str) -> String {
    match driver {
        DbDriver::MySql => value.replace('\\', "\\\\").replace('\'', "''"),
        _ => value.replace('\'', "''"),
    }
}

/// `schema.table`, both quoted. An empty schema yields the bare table, which
/// is what SQLite and Turso want.
pub fn qualify(driver: &DbDriver, schema: &str, table: &str) -> String {
    // Turso addresses a single database file; a schema prefix is a syntax
    // error there rather than a nicety.
    if schema.trim().is_empty() || matches!(driver, DbDriver::Turso) {
        quote_ident(driver, table)
    } else {
        format!(
            "{}.{}",
            quote_ident(driver, schema),
            quote_ident(driver, table)
        )
    }
}

/// The schema to put in front of a table name for `driver`, given the one the
/// workspace is on (a schema, a database name, a file path or blank).
///
/// Only Postgres-family databases and MySQL qualify with it. SQLite and Turso
/// address one file and reject `"path"."table"`; SQL Server's workspace
/// "schema" is really the database, which `[db].[table]` would read as a
/// schema. Blank on Postgres means `public`.
pub fn effective_schema(driver: &DbDriver, requested: &str) -> String {
    let requested = requested.trim();
    match driver {
        DbDriver::Sqlite | DbDriver::Turso | DbDriver::SqlServer => String::new(),
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon if requested.is_empty() => {
            "public".to_string()
        }
        _ => requested.to_string(),
    }
}

/// What a driver can express in a SQL dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpSupport {
    pub drop: bool,
    pub create: bool,
    pub data: bool,
}

pub fn dump_support(driver: &DbDriver) -> DumpSupport {
    match driver {
        DbDriver::Postgres
        | DbDriver::Supabase
        | DbDriver::Neon
        | DbDriver::Sqlite
        | DbDriver::Turso
        | DbDriver::MySql => DumpSupport {
            drop: true,
            create: true,
            data: true,
        },
        // T-SQL can be dropped and its rows read, but its CREATE TABLE needs
        // the SQL Server dialect, so dump creation is off.
        DbDriver::SqlServer => DumpSupport {
            drop: true,
            create: false,
            data: true,
        },
        // Not SQL at all; the dump degrades to commented-out shell commands.
        DbDriver::MongoDb | DbDriver::Redis => DumpSupport {
            drop: false,
            create: false,
            data: false,
        },
    }
}

/// The statement that empties a table before an import.
///
/// SQLite and Turso have no `TRUNCATE`, so they get the `DELETE FROM` that
/// means the same thing there.
pub fn truncate_statement(driver: &DbDriver, qualified_table: &str) -> String {
    match driver {
        DbDriver::Sqlite | DbDriver::Turso => format!("DELETE FROM {}", qualified_table),
        _ => format!("TRUNCATE TABLE {}", qualified_table),
    }
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

/// Flatten a JSON cell to text. NULL becomes empty, which is what a CSV
/// reader expects; objects and arrays keep their JSON form.
pub fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// Render a JSON cell as a SQL literal for `driver`.
///
/// This is the injection surface of a SQL dump: everything that is not a
/// number, a bool or NULL becomes a quoted literal with its quotes escaped.
pub fn sql_value_to_string(driver: &DbDriver, value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", escape_literal(driver, s)),
        Value::Array(_) | Value::Object(_) => {
            format!("'{}'", escape_literal(driver, &value.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// CSV / JSON / Excel rendering
// ---------------------------------------------------------------------------

/// When a CSV field gets wrapped in quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsvQuoting {
    /// Only when the value would otherwise break the format (RFC 4180).
    Minimal,
    /// Always. Some importers (older Excel builds) prefer it.
    All,
}

impl CsvQuoting {
    pub fn label(self) -> &'static str {
        match self {
            CsvQuoting::Minimal => "When needed",
            CsvQuoting::All => "Always quote",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvOptions {
    pub delimiter: char,
    pub include_headers: bool,
    pub quoting: CsvQuoting,
    /// Text written for a database NULL. Empty by default so a NULL and an
    /// empty string look the same, matching the exporter; set it to
    /// something like `\N` to tell them apart.
    pub null_text: String,
    /// CRLF line endings, which Excel on Windows is happier with.
    pub crlf: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            include_headers: true,
            quoting: CsvQuoting::Minimal,
            null_text: String::new(),
            crlf: false,
        }
    }
}

impl CsvOptions {
    fn newline(&self) -> &'static str {
        if self.crlf {
            "\r\n"
        } else {
            "\n"
        }
    }
}

/// Escape one CSV field.
///
/// A field is quoted when it contains the configured delimiter, a double
/// quote, or a line break; embedded quotes are doubled. The delimiter is a
/// parameter because exporting tab- or semicolon-separated data with a
/// hardcoded comma check silently produces a broken file.
pub fn escape_csv_value(value: &str, delimiter: char, quoting: CsvQuoting) -> String {
    let must_quote = value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');

    if must_quote || quoting == CsvQuoting::All {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Join already-stringified fields into one CSV record.
pub fn csv_record(fields: &[String], opts: &CsvOptions) -> String {
    fields
        .iter()
        .map(|f| escape_csv_value(f, opts.delimiter, opts.quoting))
        .collect::<Vec<_>>()
        .join(&opts.delimiter.to_string())
}

pub fn render_csv(columns: &[String], rows: &[Vec<Value>], opts: &CsvOptions) -> String {
    let nl = opts.newline();
    let mut out = String::new();

    if opts.include_headers {
        out.push_str(&csv_record(columns, opts));
        out.push_str(nl);
    }

    for row in rows {
        let fields: Vec<String> = row
            .iter()
            .map(|v| match v {
                Value::Null => opts.null_text.clone(),
                other => json_value_to_string(other),
            })
            .collect();
        out.push_str(&csv_record(&fields, opts));
        out.push_str(nl);
    }
    out
}

/// Rows as an array of objects keyed by column name.
pub fn render_json(
    columns: &[String],
    rows: &[Vec<Value>],
    pretty: bool,
) -> Result<String, DbError> {
    let objects: Vec<serde_json::Map<String, Value>> = rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, value) in row.iter().enumerate() {
                if let Some(name) = columns.get(i) {
                    obj.insert(name.clone(), value.clone());
                }
            }
            obj
        })
        .collect();

    let out = if pretty {
        serde_json::to_string_pretty(&objects)
    } else {
        serde_json::to_string(&objects)
    };
    out.map_err(|e| DbError::InternalError(format!("Failed to serialize JSON: {}", e)))
}

/// Escape text for XML content. Control characters that XML 1.0 forbids are
/// dropped rather than emitted, since a single stray byte makes Excel refuse
/// the whole workbook.
pub fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => {}
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Excel rejects these in a sheet name, and caps it at 31 characters.
pub fn sanitize_sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            ':' | '\\' | '/' | '?' | '*' | '[' | ']' => '_',
            c => c,
        })
        .take(31)
        .collect();
    if cleaned.trim().is_empty() {
        "Sheet1".to_string()
    } else {
        cleaned
    }
}

/// Render a SpreadsheetML 2003 workbook.
///
/// Not a zipped `.xlsx`: writing that needs a zip writer, which is not a
/// dependency of this crate. SpreadsheetML is a single XML file that Excel,
/// Numbers and LibreOffice all open natively, keeps numeric cells numeric,
/// and needs nothing but a string writer.
pub fn render_excel_xml(sheet: &str, columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\"?>\n<?mso-application progid=\"Excel.Sheet\"?>\n");
    out.push_str(
        "<Workbook xmlns=\"urn:schemas-microsoft-com:office:spreadsheet\" \
         xmlns:ss=\"urn:schemas-microsoft-com:office:spreadsheet\">\n",
    );
    out.push_str(&format!(
        " <Worksheet ss:Name=\"{}\">\n  <Table>\n",
        xml_escape(&sanitize_sheet_name(sheet))
    ));

    if !columns.is_empty() {
        out.push_str("   <Row>\n");
        for c in columns {
            out.push_str(&format!(
                "    <Cell><Data ss:Type=\"String\">{}</Data></Cell>\n",
                xml_escape(c)
            ));
        }
        out.push_str("   </Row>\n");
    }

    for row in rows {
        out.push_str("   <Row>\n");
        for value in row {
            match value {
                // An empty cell is a genuinely blank spreadsheet cell, which
                // is the closest thing Excel has to NULL.
                Value::Null => out.push_str("    <Cell/>\n"),
                Value::Number(n) => out.push_str(&format!(
                    "    <Cell><Data ss:Type=\"Number\">{}</Data></Cell>\n",
                    n
                )),
                Value::Bool(b) => out.push_str(&format!(
                    "    <Cell><Data ss:Type=\"Boolean\">{}</Data></Cell>\n",
                    if *b { 1 } else { 0 }
                )),
                other => out.push_str(&format!(
                    "    <Cell><Data ss:Type=\"String\">{}</Data></Cell>\n",
                    xml_escape(&json_value_to_string(other))
                )),
            }
        }
        out.push_str("   </Row>\n");
    }

    out.push_str("  </Table>\n </Worksheet>\n</Workbook>\n");
    out
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Sql,
    Excel,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 4] = [
        ExportFormat::Csv,
        ExportFormat::Json,
        ExportFormat::Sql,
        ExportFormat::Excel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Csv => "CSV",
            ExportFormat::Json => "JSON",
            ExportFormat::Sql => "SQL (INSERT statements)",
            ExportFormat::Excel => "Excel (SpreadsheetML)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Sql => "sql",
            ExportFormat::Excel => "xls",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Absolute path to write.
    pub path: String,
    pub csv: CsvOptions,
    pub json_pretty: bool,
    /// Worksheet name for the Excel format.
    pub sheet_name: String,
    /// Table the generated INSERT statements target, for the SQL format.
    pub sql_table: String,
    pub sql_schema: String,
    /// Stop after this many rows. None exports everything.
    pub row_limit: Option<usize>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Csv,
            path: String::new(),
            csv: CsvOptions::default(),
            json_pretty: true,
            sheet_name: "Results".to_string(),
            sql_table: "exported_rows".to_string(),
            sql_schema: String::new(),
            row_limit: None,
        }
    }
}

/// INSERT statements for a result set, one per row, with an explicit column
/// list so the target's column order does not have to match.
pub fn render_sql_inserts(
    driver: &DbDriver,
    schema: &str,
    table: &str,
    columns: &[String],
    rows: &[Vec<Value>],
) -> String {
    let target = qualify(driver, schema, table);
    let cols: Vec<String> = columns.iter().map(|c| quote_ident(driver, c)).collect();

    let mut out = String::new();
    out.push_str(&format!("-- DB Hive export: {}\n", target));
    out.push_str(&format!("-- Rows: {}\n\n", rows.len()));
    for row in rows {
        let values: Vec<String> = row.iter().map(|v| sql_value_to_string(driver, v)).collect();
        out.push_str(&format!(
            "INSERT INTO {} ({}) VALUES ({});\n",
            target,
            cols.join(", "),
            values.join(", ")
        ));
    }
    out
}

/// Rows actually exported, after `row_limit`.
pub fn limited<'a>(rows: &'a [Vec<Value>], limit: Option<usize>) -> &'a [Vec<Value>] {
    match limit {
        Some(n) if n < rows.len() => &rows[..n],
        _ => rows,
    }
}

/// Produce the exact bytes an export writes. Pure, so every format is
/// testable without touching the filesystem.
pub fn render_export(
    driver: &DbDriver,
    columns: &[String],
    rows: &[Vec<Value>],
    opts: &ExportOptions,
) -> Result<String, DbError> {
    let rows = limited(rows, opts.row_limit);
    match opts.format {
        ExportFormat::Csv => Ok(render_csv(columns, rows, &opts.csv)),
        ExportFormat::Json => render_json(columns, rows, opts.json_pretty),
        ExportFormat::Excel => Ok(render_excel_xml(&opts.sheet_name, columns, rows)),
        ExportFormat::Sql => {
            if opts.sql_table.trim().is_empty() {
                return Err(DbError::InvalidInput(
                    "A target table name is required to export as SQL".to_string(),
                ));
            }
            Ok(render_sql_inserts(
                driver,
                &opts.sql_schema,
                &opts.sql_table,
                columns,
                rows,
            ))
        }
    }
}

fn write_file(path: &str, contents: &str) -> Result<(), DbError> {
    if path.trim().is_empty() {
        return Err(DbError::InvalidInput(
            "Choose where to save the file first".to_string(),
        ));
    }
    let mut file = File::create(Path::new(path))
        .map_err(|e| DbError::InternalError(format!("Failed to create {}: {}", path, e)))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| DbError::InternalError(format!("Failed to write {}: {}", path, e)))
}

/// Write a result set to disk. Returns the number of rows written.
///
/// Rendering and writing happen on a blocking thread: a large result set is
/// megabytes of string building, which would otherwise stall the async
/// runtime the driver is using.
pub async fn export_rows_to_file(
    driver: DbDriver,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    opts: ExportOptions,
) -> Result<usize, DbError> {
    tokio::task::spawn_blocking(move || {
        let written = limited(&rows, opts.row_limit).len();
        let body = render_export(&driver, &columns, &rows, &opts)?;
        write_file(&opts.path, &body)?;
        Ok(written)
    })
    .await
    .map_err(|e| DbError::InternalError(format!("Export task failed: {}", e)))?
}

/// `SELECT * FROM table` with an optional row cap.
///
/// T-SQL has no `LIMIT`; using one there is a syntax error rather than an
/// unlimited read, so it gets `TOP n` instead.
pub fn limited_select(
    driver: &DbDriver,
    schema: &str,
    table: &str,
    limit: Option<usize>,
) -> String {
    let target = qualify(driver, schema, table);
    match (limit, driver) {
        (None, _) => format!("SELECT * FROM {}", target),
        (Some(n), DbDriver::SqlServer) => format!("SELECT TOP {} * FROM {}", n, target),
        (Some(n), _) => format!("SELECT * FROM {} LIMIT {}", target, n),
    }
}

/// Read a whole table and export it. Returns the number of rows written.
pub async fn export_table_to_file(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: &str,
    table: &str,
    opts: ExportOptions,
) -> Result<usize, DbError> {
    let driver = connection(state, connection_id)?;
    let kind = driver_kind(state, connection_id)?;

    let sql = limited_select(&kind, schema, table, opts.row_limit);
    let result = driver.execute_query(&sql).await?;

    let mut opts = opts;
    if opts.sql_table.trim().is_empty() || opts.sql_table == "exported_rows" {
        opts.sql_table = table.to_string();
        opts.sql_schema = schema.to_string();
    }
    export_rows_to_file(kind, result.columns, result.rows, opts).await
}

// ---------------------------------------------------------------------------
// SQL dump export
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlExportOptions {
    pub include_drop: bool,
    pub include_create: bool,
    pub include_data: bool,
    /// Empty means every table in the schema.
    pub tables: Vec<String>,
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

/// Parse the comma-separated table filter the dialog offers.
pub fn parse_table_filter(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

pub fn drop_statement(driver: &DbDriver, schema: &str, table: &str) -> String {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon | DbDriver::Sqlite => {
            format!(
                "DROP TABLE IF EXISTS {} CASCADE;",
                qualify(driver, schema, table)
            )
        }
        DbDriver::Turso => format!("DROP TABLE IF EXISTS {};", quote_ident(driver, table)),
        DbDriver::MySql => {
            format!("DROP TABLE IF EXISTS {};", qualify(driver, schema, table))
        }
        DbDriver::SqlServer => format!(
            "IF OBJECT_ID('{}.{}', 'U') IS NOT NULL DROP TABLE {};",
            escape_literal(driver, schema),
            escape_literal(driver, table),
            qualify(driver, schema, table)
        ),
        DbDriver::MongoDb => format!("-- db.{}.drop();", table),
        DbDriver::Redis => format!("-- DEL {}", table),
    }
}

/// `CREATE TABLE` from a column list.
///
/// Defaults are copied through verbatim: they are server-generated
/// expressions (`nextval(...)`, `CURRENT_TIMESTAMP`), not user input, and
/// quoting them would turn a default expression into a string literal.
pub fn create_table_statement(
    driver: &DbDriver,
    schema: &str,
    table: &str,
    columns: &[ColumnInfo],
) -> String {
    let mut sql = format!("CREATE TABLE {} (\n", qualify(driver, schema, table));
    let cols: Vec<String> = columns
        .iter()
        .map(|col| {
            let mut parts = vec![
                format!("  {}", quote_ident(driver, &col.name)),
                col.data_type.clone(),
            ];
            if !col.nullable {
                parts.push("NOT NULL".to_string());
            }
            if let Some(default) = &col.default_value {
                if !default.is_empty() {
                    parts.push(format!("DEFAULT {}", default));
                }
            }
            parts.join(" ")
        })
        .collect();
    sql.push_str(&cols.join(",\n"));
    sql.push_str("\n);");
    sql
}

/// Header comment block for a dump file.
pub fn dump_header(driver: &DbDriver, timestamp: &str) -> String {
    format!(
        "-- DB Hive SQL Dump\n-- Database: {:?}\n-- Export time: {}\n\n",
        driver, timestamp
    )
}

fn connection(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<Arc<dyn DatabaseDriver>, DbError> {
    // The Arc is cloned and the guard dropped here, so no caller can hold the
    // state lock across an await.
    let guard = state
        .lock()
        .map_err(|_| DbError::InternalError("Application state lock is poisoned".to_string()))?;
    guard
        .get_connection(connection_id)
        .cloned()
        .ok_or_else(|| DbError::NotFound(format!("Connection {} is not open", connection_id)))
}

pub fn driver_kind(state: &Mutex<AppState>, connection_id: &str) -> Result<DbDriver, DbError> {
    let guard = state
        .lock()
        .map_err(|_| DbError::InternalError("Application state lock is poisoned".to_string()))?;
    guard
        .get_profile(connection_id)
        .map(|p| p.driver.clone())
        .ok_or_else(|| DbError::NotFound(format!("Connection profile {} not found", connection_id)))
}

/// Export schema and/or data to a SQL file. Returns the number of tables written.
pub async fn export_to_sql(
    state: &Mutex<AppState>,
    connection_id: &str,
    file_path: &str,
    options: SqlExportOptions,
) -> Result<usize, DbError> {
    let driver = connection(state, connection_id)?;
    let kind = driver_kind(state, connection_id)?;
    let support = dump_support(&kind);

    if !support.drop && !support.create && !support.data {
        return Err(DbError::InvalidInput(format!(
            "SQL dumps are not supported for {:?}",
            kind
        )));
    }

    let file = File::create(file_path)
        .map_err(|e| DbError::InternalError(format!("Failed to create SQL file: {}", e)))?;
    let mut out = BufWriter::new(file);

    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    write_sql(&mut out, &dump_header(&kind, &stamp))?;

    let schema = options
        .schema
        .clone()
        .unwrap_or_else(|| "public".to_string());
    // What table names are qualified with in the file. SQLite has no schema
    // to name, and the shell hands over the database path or "main" there.
    let qualifier = effective_schema(&kind, &schema);
    let shown = |table: &str| {
        if qualifier.is_empty() {
            table.to_string()
        } else {
            format!("{}.{}", qualifier, table)
        }
    };
    let tables: Vec<String> = if options.tables.is_empty() {
        driver
            .get_tables(&schema)
            .await?
            .into_iter()
            .map(|t| t.name)
            .collect()
    } else {
        options.tables.clone()
    };

    for table in &tables {
        write_sql(&mut out, &format!("\n-- Table: {}\n", shown(table)))?;

        if options.include_drop {
            write_sql(
                &mut out,
                &format!("{}\n", drop_statement(&kind, &qualifier, table)),
            )?;
        }

        if options.include_create {
            if support.create {
                let table_schema = driver.get_table_schema(&schema, table).await?;
                write_sql(
                    &mut out,
                    &format!(
                        "{}\n",
                        create_table_statement(&kind, &qualifier, table, &table_schema.columns)
                    ),
                )?;
            } else {
                // Aborting here would leave a half-written file; a comment
                // keeps the data portion usable and says why.
                write_sql(
                    &mut out,
                    &format!("-- CREATE TABLE export is not supported for {:?}\n", kind),
                )?;
            }
        }

        if options.include_data && support.data {
            let sql = format!("SELECT * FROM {}", qualify(&kind, &qualifier, table));
            let result = driver.execute_query(&sql).await?;
            if result.rows.is_empty() {
                write_sql(&mut out, "-- No data in table\n")?;
            } else {
                write_sql(&mut out, &format!("\n-- Data for table {}\n", shown(table)))?;
                let cols: Vec<String> = result
                    .columns
                    .iter()
                    .map(|c| quote_ident(&kind, c))
                    .collect();
                let target = qualify(&kind, &qualifier, table);
                for row in &result.rows {
                    let values: Vec<String> =
                        row.iter().map(|v| sql_value_to_string(&kind, v)).collect();
                    write_sql(
                        &mut out,
                        &format!(
                            "INSERT INTO {} ({}) VALUES ({});\n",
                            target,
                            cols.join(", "),
                            values.join(", ")
                        ),
                    )?;
                }
            }
        }
    }

    write_sql(&mut out, "\n-- Dump completed\n")?;
    out.flush()
        .map_err(|e| DbError::InternalError(format!("Failed to flush SQL file: {}", e)))?;

    Ok(tables.len())
}

fn write_sql(out: &mut BufWriter<File>, s: &str) -> Result<(), DbError> {
    out.write_all(s.as_bytes())
        .map_err(|e| DbError::InternalError(format!("Failed to write SQL file: {}", e)))
}

// ---------------------------------------------------------------------------
// SQL dump import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlImportOptions {
    pub continue_on_error: bool,
    pub use_transaction: bool,
}

impl Default for SqlImportOptions {
    fn default() -> Self {
        Self {
            continue_on_error: false,
            use_transaction: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlImportResult {
    pub executed: usize,
    pub errors_count: usize,
    pub skipped: usize,
    pub first_error: Option<String>,
    /// The user stopped it part way through.
    pub cancelled: bool,
    /// Path of the error log, when there was anything to log.
    pub log_file: Option<String>,
}

/// `/path/dump.sql` → `/path/dump_import_errors.log`.
pub fn derive_log_path(sql_path: &str) -> String {
    let stem = sql_path.strip_suffix(".sql").unwrap_or(sql_path);
    format!("{}_import_errors.log", stem)
}

fn write_import_log(
    log_path: &str,
    sql_path: &str,
    executed: usize,
    skipped: usize,
    errors: &[String],
) -> std::io::Result<()> {
    let mut f = File::create(log_path)?;
    writeln!(f, "DB Hive SQL Import Error Log")?;
    writeln!(f, "Source file : {}", sql_path)?;
    writeln!(
        f,
        "Timestamp   : {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )?;
    writeln!(f, "Executed    : {}", executed)?;
    writeln!(f, "Skipped     : {}", skipped)?;
    writeln!(f, "Errors      : {}", errors.len())?;
    writeln!(f, "{}", "-".repeat(60))?;
    for err in errors {
        writeln!(f, "{}", err)?;
    }
    Ok(())
}

/// Split a multi-row INSERT into batches of at most `max_rows` value groups.
///
/// Large mysqldump INSERTs exceed the server's `max_allowed_packet`; MariaDB
/// ignores `SET SESSION` for that variable, so the split has to happen here.
pub fn split_insert_values(stmt: &str, max_rows: usize) -> Vec<String> {
    let upper = stmt.to_uppercase();
    let values_pos = match upper.find("VALUES") {
        Some(p) => p,
        None => return vec![stmt.to_string()],
    };

    let prefix = &stmt[..values_pos + "VALUES".len()];
    let values_str = stmt[values_pos + "VALUES".len()..]
        .trim_start()
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

/// Split `(a,b),(c,d)` into `["(a,b)", "(c,d)"]`, respecting nested
/// parentheses, single-quoted strings and backslash escapes.
pub fn parse_value_row_groups(s: &str) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_single_quote = false;
    let mut escape_next = false;
    let mut group_start: Option<usize> = None;
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
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
                        if let Some(start) = group_start.take() {
                            groups.push(s[start..=i].to_string());
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

/// Fix known cross-tool dump quirks.
///
/// A MySQL 8.0 client dumping a MariaDB server emits `REPLACE IGNORE INTO`,
/// which neither server accepts. `INSERT IGNORE` has the same skip-duplicates
/// meaning.
pub fn normalize_dump_stmt(stmt: String) -> String {
    let words: Vec<&str> = stmt.split_ascii_whitespace().collect();
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

/// Statements the server cannot run and that a dump only includes as advice.
pub fn is_client_only_statement(stmt: &str) -> bool {
    let first = stmt.split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(first.as_str(), "LOCK" | "UNLOCK")
}

/// The transaction verbs for a dialect, or None when it has none.
pub fn transaction_verbs(driver: &DbDriver) -> Option<(&'static str, &'static str, &'static str)> {
    match driver {
        DbDriver::Postgres
        | DbDriver::Supabase
        | DbDriver::Neon
        | DbDriver::Sqlite
        | DbDriver::Turso => Some(("BEGIN", "COMMIT", "ROLLBACK")),
        DbDriver::MySql => Some(("START TRANSACTION", "COMMIT", "ROLLBACK")),
        DbDriver::SqlServer => Some(("BEGIN TRANSACTION", "COMMIT", "ROLLBACK")),
        DbDriver::MongoDb | DbDriver::Redis => None,
    }
}

/// Run a `.sql` dump against a connection.
///
/// The file is streamed line by line rather than read into memory: dumps are
/// routinely gigabytes. `cancel` is polled between lines so the UI's Stop
/// button takes effect after the statement in flight, not after the file.
pub async fn import_from_sql(
    state: &Mutex<AppState>,
    connection_id: &str,
    file_path: &str,
    options: SqlImportOptions,
    cancel: Arc<AtomicBool>,
) -> Result<SqlImportResult, DbError> {
    let driver = connection(state, connection_id)?;
    let kind = driver_kind(state, connection_id)?;

    cancel.store(false, Ordering::Relaxed);

    let file = File::open(file_path)
        .map_err(|e| DbError::InternalError(format!("Failed to open SQL file: {}", e)))?;
    let reader = BufReader::new(file);

    // mysqldump output assumes these are relaxed; without them a dump with
    // circular foreign keys cannot be replayed in file order.
    if matches!(kind, DbDriver::MySql) {
        for stmt in &[
            "SET SESSION foreign_key_checks = 0",
            "SET SESSION unique_checks = 0",
            "SET SESSION sql_notes = 0",
            "SET SESSION sql_mode = ''",
            // MariaDB treats max_allowed_packet as global-only, so SET
            // SESSION is ignored. Needs SUPER; failure here is harmless.
            "SET GLOBAL max_allowed_packet = 1073741824",
        ] {
            let _ = driver.execute_query(stmt).await;
        }
    }

    let verbs = transaction_verbs(&kind);
    if options.use_transaction {
        let Some((begin, _, _)) = verbs else {
            return Err(DbError::InvalidInput(
                "Transactions are not supported for this driver".to_string(),
            ));
        };
        driver
            .execute_query(begin)
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to begin transaction: {}", e)))?;
    }

    let mut executed: usize = 0;
    let mut skipped: usize = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut stmt_index: usize = 0;
    let mut current_statement = String::new();
    // mysqldump switches the delimiter around triggers and procedures.
    let mut current_delimiter = ";".to_string();

    for line_result in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let line = line_result
            .map_err(|e| DbError::InternalError(format!("Failed to read SQL file: {}", e)))?;
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }

        // DELIMITER is a mysql client command, not SQL; sending it to the
        // server is a syntax error.
        if trimmed.to_uppercase().starts_with("DELIMITER") {
            if let Some(new_delim) = trimmed.split_whitespace().nth(1) {
                current_delimiter = new_delim.to_string();
            }
            continue;
        }

        current_statement.push_str(&line);
        current_statement.push('\n');

        if !trimmed.ends_with(current_delimiter.as_str()) {
            continue;
        }

        let stmt = current_statement.trim().to_string();
        current_statement.clear();

        let stmt = if current_delimiter != ";" {
            stmt.trim_end_matches(current_delimiter.as_str())
                .trim()
                .to_string()
        } else {
            stmt.trim_end_matches(';').trim().to_string()
        };

        if stmt.is_empty() {
            continue;
        }

        let stmt = normalize_dump_stmt(stmt);
        stmt_index += 1;

        if is_client_only_statement(&stmt) {
            skipped += 1;
            continue;
        }

        // Split oversized INSERTs before the server closes the connection on
        // us. 4 MB is comfortably under any default max_allowed_packet.
        const SPLIT_THRESHOLD: usize = 4 * 1024 * 1024;
        let sub_stmts: Vec<String> = if stmt.len() > SPLIT_THRESHOLD {
            let split = split_insert_values(&stmt, 50);
            if split.len() > 1 {
                split
            } else {
                skipped += 1;
                errors.push(format!(
                    "Statement {}: skipped: single row is {} MB, exceeds server max_allowed_packet",
                    stmt_index,
                    stmt.len() / 1024 / 1024
                ));
                continue;
            }
        } else {
            vec![stmt.clone()]
        };

        'sub: for sub_stmt in &sub_stmts {
            match driver.execute_query(sub_stmt).await {
                Ok(_) => executed += 1,
                Err(ref e) => {
                    let msg = e.to_string().to_lowercase();
                    // A broken pipe means the server dropped the connection
                    // because the packet was too large for it.
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
                            if let Some((_, _, rollback)) = verbs {
                                let _ = driver.execute_query(rollback).await;
                            }
                        }
                        return Err(DbError::QueryError(format!(
                            "Import failed at statement {}: {}",
                            stmt_index, e
                        )));
                    }
                    if oversized {
                        break 'sub;
                    }
                }
            }
        }
    }

    // A trailing statement with no delimiter still counts.
    let remaining = current_statement.trim().to_string();
    if !remaining.is_empty() && !remaining.starts_with("--") {
        let _ = driver.execute_query(&remaining).await;
    }

    if options.use_transaction {
        if let Some((_, commit, _)) = verbs {
            driver
                .execute_query(commit)
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to commit: {}", e)))?;
        }
    }

    if matches!(kind, DbDriver::MySql) {
        for stmt in &[
            "SET SESSION foreign_key_checks = 1",
            "SET SESSION unique_checks = 1",
            "SET SESSION sql_notes = 1",
        ] {
            let _ = driver.execute_query(stmt).await;
        }
    }

    let log_file = if errors.is_empty() {
        None
    } else {
        let log_path = derive_log_path(file_path);
        // A failed log write must not fail an import that otherwise worked.
        write_import_log(&log_path, file_path, executed, skipped, &errors)
            .ok()
            .map(|()| log_path)
    };

    Ok(SqlImportResult {
        executed,
        errors_count: errors.len(),
        skipped,
        first_error: errors.first().cloned(),
        cancelled: cancel.load(Ordering::Relaxed),
        log_file,
    })
}

// ---------------------------------------------------------------------------
// File preview and type inference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Known for spreadsheets; a CSV would need a full pass to count.
    pub total_rows: Option<usize>,
    pub detected_types: Vec<String>,
    /// "csv", "xlsx" or "xls".
    pub file_type: String,
    pub sheet_names: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct PreviewOptions {
    pub sheet_name: Option<String>,
    pub delimiter: Option<char>,
    pub max_rows: usize,
    pub first_row_is_header: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            sheet_name: None,
            delimiter: Some(','),
            max_rows: MAX_PREVIEW_ROWS,
            first_row_is_header: true,
        }
    }
}

/// File extension, lowercased.
pub fn file_kind(file_path: &str) -> String {
    Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Column names when the file has no header row: Column1, Column2, ...
pub fn generated_headers(width: usize) -> Vec<String> {
    (0..width).map(|i| format!("Column{}", i + 1)).collect()
}

/// The preview a file yields, for CSV/TSV/TXT and both Excel formats.
pub fn preview_import_file(
    file_path: &str,
    opts: &PreviewOptions,
) -> Result<ImportPreview, DbError> {
    match file_kind(file_path).as_str() {
        "csv" | "tsv" | "txt" => preview_csv(file_path, opts),
        "xlsx" => preview_xlsx(file_path, opts),
        "xls" => preview_xls(file_path, opts),
        other => Err(DbError::ImportError(format!(
            "Unsupported file type: {}",
            if other.is_empty() { "(none)" } else { other }
        ))),
    }
}

fn preview_csv(file_path: &str, opts: &PreviewOptions) -> Result<ImportPreview, DbError> {
    let delimiter = opts.delimiter.unwrap_or(',') as u8;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(opts.first_row_is_header)
        .flexible(true)
        .from_path(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open CSV: {}", e)))?;

    let mut headers: Vec<String> = if opts.first_row_is_header {
        reader
            .headers()
            .map_err(|e| DbError::ImportError(format!("Failed to read headers: {}", e)))?
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut rows: Vec<Vec<String>> = Vec::new();
    for (i, result) in reader.records().enumerate() {
        if i >= opts.max_rows {
            break;
        }
        match result {
            Ok(record) => rows.push(record.iter().map(|s| s.to_string()).collect()),
            // A malformed row must not lose the rows around it.
            Err(e) => rows.push(vec![format!("<unreadable row {}: {}>", i + 1, e)]),
        }
    }

    if headers.is_empty() {
        headers = generated_headers(rows.first().map(|r| r.len()).unwrap_or(0));
    }

    Ok(ImportPreview {
        detected_types: detect_types(&headers, &rows),
        columns: headers,
        rows,
        total_rows: None,
        file_type: "csv".to_string(),
        sheet_names: None,
    })
}

fn preview_xlsx(file_path: &str, opts: &PreviewOptions) -> Result<ImportPreview, DbError> {
    let mut workbook: Xlsx<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;
    let sheet_names = workbook.sheet_names().to_vec();
    let sheet = chosen_sheet(&sheet_names, opts.sheet_name.as_deref());
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|e| DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e)))?;
    Ok(preview_from_range(range.rows(), opts, "xlsx", sheet_names))
}

fn preview_xls(file_path: &str, opts: &PreviewOptions) -> Result<ImportPreview, DbError> {
    let mut workbook: Xls<_> = open_workbook(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;
    let sheet_names = workbook.sheet_names().to_vec();
    let sheet = chosen_sheet(&sheet_names, opts.sheet_name.as_deref());
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|e| DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e)))?;
    Ok(preview_from_range(range.rows(), opts, "xls", sheet_names))
}

fn chosen_sheet(sheet_names: &[String], wanted: Option<&str>) -> String {
    match wanted {
        Some(w) if sheet_names.iter().any(|s| s == w) => w.to_string(),
        _ => sheet_names.first().cloned().unwrap_or_default(),
    }
}

fn preview_from_range<'a>(
    range_rows: impl Iterator<Item = &'a [Data]>,
    opts: &PreviewOptions,
    file_type: &str,
    sheet_names: Vec<String>,
) -> ImportPreview {
    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut total = 0usize;

    for (i, row) in range_rows.enumerate() {
        let cells: Vec<String> = row.iter().map(cell_to_string).collect();
        if i == 0 && opts.first_row_is_header {
            headers = cells;
            continue;
        }
        total += 1;
        if rows.len() < opts.max_rows {
            rows.push(cells);
        }
    }

    if headers.is_empty() {
        headers = generated_headers(rows.first().map(|r| r.len()).unwrap_or(0));
    }

    ImportPreview {
        detected_types: detect_types(&headers, &rows),
        columns: headers,
        rows,
        total_rows: Some(total),
        file_type: file_type.to_string(),
        sheet_names: Some(sheet_names),
    }
}

/// Render a spreadsheet cell as the text an import would insert.
pub fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        // Excel stores every number as a float; whole numbers must not come
        // out as "42.0" and fail an integer column.
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => format!("{}", dt),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERROR: {:?}", e),
    }
}

/// Per-column type guesses from the preview rows.
pub fn detect_types(columns: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let mut samples: Vec<Vec<String>> = vec![Vec::new(); columns.len()];
    for row in rows {
        for (j, value) in row.iter().enumerate() {
            if j < samples.len() && !value.is_empty() {
                samples[j].push(value.clone());
            }
        }
    }
    samples.iter().map(|s| detect_column_type(s)).collect()
}

fn date_patterns() -> &'static [regex::Regex] {
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"^\d{4}-\d{2}-\d{2}", // YYYY-MM-DD
            r"^\d{2}/\d{2}/\d{4}", // MM/DD/YYYY
            r"^\d{2}-\d{2}-\d{4}", // DD-MM-YYYY
        ]
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
    })
}

/// Guess a column's type from up to 50 non-empty samples.
///
/// Boolean is checked first and accepts 1/0, so a column of only ones and
/// zeroes is reported BOOLEAN rather than INTEGER. The guess is only a
/// default the user overrides in the mapping step.
pub fn detect_column_type(samples: &[String]) -> String {
    if samples.is_empty() {
        return "TEXT".to_string();
    }

    let mut is_integer = true;
    let mut is_float = true;
    let mut is_boolean = true;
    let mut is_date = true;
    let mut seen = 0usize;

    for value in samples.iter().take(50) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        seen += 1;

        if is_integer && trimmed.parse::<i64>().is_err() {
            is_integer = false;
        }
        if is_float && trimmed.parse::<f64>().is_err() {
            is_float = false;
        }

        let lower = trimmed.to_lowercase();
        if is_boolean && !matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "1" | "0") {
            is_boolean = false;
        }

        if is_date && !date_patterns().iter().any(|r| r.is_match(trimmed)) {
            is_date = false;
        }
    }

    if seen == 0 {
        return "TEXT".to_string();
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

// ---------------------------------------------------------------------------
// Column mapping and the import plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMapping {
    /// Column name in the file.
    pub source_column: String,
    /// Column name in the table. Empty means "not mapped".
    pub target_column: String,
    pub target_type: Option<String>,
    /// Used when the source cell is empty.
    pub default_value: Option<String>,
    pub skip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataImportOptions {
    pub table_name: String,
    pub schema: Option<String>,
    pub column_mappings: Vec<ColumnMapping>,
    /// Data rows discarded from the top, after the header row.
    pub skip_rows: usize,
    pub create_table: bool,
    pub truncate_before: bool,
    /// Rows per INSERT statement. 1 gives one statement per row.
    pub batch_size: usize,
    pub delimiter: Option<char>,
    pub sheet_name: Option<String>,
    pub first_row_is_header: bool,
}

impl Default for DataImportOptions {
    fn default() -> Self {
        Self {
            table_name: String::new(),
            schema: None,
            column_mappings: Vec::new(),
            skip_rows: 0,
            create_table: false,
            truncate_before: false,
            batch_size: 1000,
            delimiter: Some(','),
            sheet_name: None,
            first_row_is_header: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportResult {
    pub rows_imported: usize,
    pub rows_failed: usize,
    /// First few failures, in row order.
    pub errors: Vec<String>,
    /// True when every row went in.
    pub success: bool,
}

/// Suggest a mapping for each source column.
///
/// Matching is case-insensitive on the name, which is what the UI wizard
/// did; unmatched columns come back with an empty target so the user has to
/// choose one or skip it.
pub fn auto_map_columns(
    source_columns: &[String],
    detected_types: &[String],
    target_columns: &[TableColumnInfo],
) -> Vec<ColumnMapping> {
    source_columns
        .iter()
        .enumerate()
        .map(|(i, source)| {
            let matched = target_columns
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(source));
            ColumnMapping {
                source_column: source.clone(),
                target_column: matched.map(|t| t.name.clone()).unwrap_or_default(),
                target_type: matched
                    .map(|t| t.data_type.clone())
                    .or_else(|| detected_types.get(i).cloned())
                    .or_else(|| Some("TEXT".to_string())),
                default_value: None,
                skip: matched.is_none(),
            }
        })
        .collect()
}

/// Mappings that will actually be written.
pub fn active_mappings(mappings: &[ColumnMapping]) -> Vec<&ColumnMapping> {
    mappings
        .iter()
        .filter(|m| !m.skip && !m.target_column.trim().is_empty())
        .collect()
}

/// Project one source row onto the mapped target columns.
///
/// `None` means SQL NULL. An empty cell falls back to the mapping's default
/// value when it has one, which is what "Default value if source is empty"
/// promised in the UI wizard but the handler only applied when the
/// source column was missing entirely.
pub fn map_row(
    source_columns: &[String],
    row: &[String],
    mappings: &[ColumnMapping],
) -> Vec<Option<String>> {
    active_mappings(mappings)
        .into_iter()
        .map(|m| {
            let raw = source_columns
                .iter()
                .position(|c| c == &m.source_column)
                .and_then(|i| row.get(i))
                .map(|s| s.as_str())
                .unwrap_or("");
            if raw.is_empty() {
                m.default_value.as_ref().filter(|d| !d.is_empty()).cloned()
            } else {
                Some(boolean_cell(raw, m.target_type.as_deref()))
            }
        })
        .collect()
}

/// A boolean word (`true`, `no`, `T`...) headed for a boolean-like column
/// becomes `1` or `0`, which every dialect accepts there. MySQL stores
/// booleans as TINYINT(1) and refuses the quoted string `'true'` ("Incorrect
/// integer value"); PostgreSQL accepts `'1'` for a boolean. Anything that is
/// not exactly a boolean word, and every other column type, is left alone, so
/// a text column still receives the text "true".
pub fn boolean_cell(raw: &str, target_type: Option<&str>) -> String {
    let ty = target_type.unwrap_or("").to_ascii_uppercase();
    let boolean_like = ty.contains("BOOL") || ty.starts_with("TINYINT");
    if !boolean_like {
        return raw.to_string();
    }
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" => "1".to_string(),
        "false" | "f" | "no" | "n" | "off" => "0".to_string(),
        _ => raw.to_string(),
    }
}

/// One INSERT covering `rows`, with every value escaped for `driver`.
///
/// Values are interpolated rather than bound because placeholder syntax
/// differs per driver and the drivers here take a plain SQL string. That
/// makes `escape_literal` the only thing standing between an untrusted file
/// and the database, so it is applied to every single value.
pub fn insert_statement(
    driver: &DbDriver,
    qualified_table: &str,
    target_columns: &[String],
    rows: &[Vec<Option<String>>],
) -> String {
    let cols: Vec<String> = target_columns
        .iter()
        .map(|c| quote_ident(driver, c))
        .collect();

    let groups: Vec<String> = rows
        .iter()
        .map(|row| {
            let values: Vec<String> = row
                .iter()
                .map(|v| match v {
                    Some(v) => format!("'{}'", escape_literal(driver, v)),
                    None => "NULL".to_string(),
                })
                .collect();
            format!("({})", values.join(", "))
        })
        .collect();

    format!(
        "INSERT INTO {} ({}) VALUES {}",
        qualified_table,
        cols.join(", "),
        groups.join(", ")
    )
}

/// `CREATE TABLE IF NOT EXISTS` from the mapping's target types.
///
/// The UI wizard exposed a "create table" option that was never wired to
/// anything; this is what it should have generated.
pub fn create_table_from_mappings(
    driver: &DbDriver,
    schema: &str,
    table: &str,
    mappings: &[ColumnMapping],
) -> String {
    let cols: Vec<String> = active_mappings(mappings)
        .into_iter()
        .map(|m| {
            format!(
                "  {} {}",
                quote_ident(driver, &m.target_column),
                sanitize_type(m.target_type.as_deref().unwrap_or("TEXT"))
            )
        })
        .collect();
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n)",
        qualify(driver, schema, table),
        cols.join(",\n")
    )
}

/// A type name is not an identifier and cannot be quoted, so it is restricted
/// to the shapes a type can take. Anything else falls back to TEXT rather
/// than being pasted into DDL.
pub fn sanitize_type(ty: &str) -> String {
    let t = ty.trim();
    let ok = !t.is_empty()
        && t.len() <= 40
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '(' | ')' | ',' | '_'));
    if ok {
        t.to_uppercase()
    } else {
        "TEXT".to_string()
    }
}

/// The rows read out of a file, ready to import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSource {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Exactly what an import is about to do, for the confirmation step.
///
/// Nothing is written until the user has seen this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    /// Quoted, schema-qualified target.
    pub target: String,
    /// Target column names, unquoted, in write order.
    pub target_columns: Vec<String>,
    pub row_count: usize,
    pub batch_count: usize,
    /// Runs before the inserts when the user asked to empty the table.
    pub truncate_statement: Option<String>,
    pub create_statement: Option<String>,
    /// First few INSERTs, verbatim.
    pub sample_statements: Vec<String>,
    /// Source columns the user chose not to import.
    pub skipped_columns: Vec<String>,
}

impl ImportPlan {
    /// True when the plan deletes existing rows, which needs a louder
    /// confirmation than an append.
    pub fn is_destructive(&self) -> bool {
        self.truncate_statement.is_some()
    }
}

/// Build the plan for an import. Pure: no database, no filesystem.
pub fn build_import_plan(
    driver: &DbDriver,
    source: &ImportSource,
    options: &DataImportOptions,
) -> Result<ImportPlan, DbError> {
    if options.table_name.trim().is_empty() {
        return Err(DbError::InvalidInput(
            "Choose a target table first".to_string(),
        ));
    }

    let active = active_mappings(&options.column_mappings);
    if active.is_empty() {
        return Err(DbError::InvalidInput(
            "No columns are mapped for import".to_string(),
        ));
    }
    let target_columns: Vec<String> = active.iter().map(|m| m.target_column.clone()).collect();

    let schema = options.schema.clone().unwrap_or_default();
    let target = qualify(driver, &schema, &options.table_name);

    let rows = import_rows(source, options);
    let batch = options.batch_size.max(1);
    let mapped: Vec<Vec<Option<String>>> = rows
        .iter()
        .take(batch * PLAN_SAMPLE_STATEMENTS)
        .map(|r| map_row(&source.columns, r, &options.column_mappings))
        .collect();

    let sample_statements: Vec<String> = mapped
        .chunks(batch)
        .take(PLAN_SAMPLE_STATEMENTS)
        .map(|chunk| {
            format!(
                "{};",
                insert_statement(driver, &target, &target_columns, chunk)
            )
        })
        .collect();

    let row_count = rows.len();
    Ok(ImportPlan {
        truncate_statement: options
            .truncate_before
            .then(|| format!("{};", truncate_statement(driver, &target))),
        create_statement: options.create_table.then(|| {
            format!(
                "{};",
                create_table_from_mappings(
                    driver,
                    &schema,
                    &options.table_name,
                    &options.column_mappings
                )
            )
        }),
        skipped_columns: options
            .column_mappings
            .iter()
            .filter(|m| m.skip || m.target_column.trim().is_empty())
            .map(|m| m.source_column.clone())
            .collect(),
        target,
        target_columns,
        batch_count: row_count.div_ceil(batch),
        row_count,
        sample_statements,
    })
}

/// Data rows after `skip_rows`.
pub fn import_rows<'a>(source: &'a ImportSource, options: &DataImportOptions) -> &'a [Vec<String>] {
    let skip = options.skip_rows.min(source.rows.len());
    &source.rows[skip..]
}

// ---------------------------------------------------------------------------
// Reading a file for import
// ---------------------------------------------------------------------------

/// Read every row of a file, honouring header and delimiter settings.
pub fn read_import_file(
    file_path: &str,
    options: &DataImportOptions,
) -> Result<ImportSource, DbError> {
    match file_kind(file_path).as_str() {
        "csv" | "tsv" | "txt" => read_csv_data(file_path, options),
        "xlsx" => {
            let mut workbook: Xlsx<_> = open_workbook(file_path)
                .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;
            let names = workbook.sheet_names().to_vec();
            let sheet = chosen_sheet(&names, options.sheet_name.as_deref());
            let range = workbook.worksheet_range(&sheet).map_err(|e| {
                DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))
            })?;
            Ok(source_from_range(range.rows(), options.first_row_is_header))
        }
        "xls" => {
            let mut workbook: Xls<_> = open_workbook(file_path)
                .map_err(|e| DbError::ImportError(format!("Failed to open Excel file: {}", e)))?;
            let names = workbook.sheet_names().to_vec();
            let sheet = chosen_sheet(&names, options.sheet_name.as_deref());
            let range = workbook.worksheet_range(&sheet).map_err(|e| {
                DbError::ImportError(format!("Failed to read sheet '{}': {}", sheet, e))
            })?;
            Ok(source_from_range(range.rows(), options.first_row_is_header))
        }
        other => Err(DbError::ImportError(format!(
            "Unsupported file type: {}",
            if other.is_empty() { "(none)" } else { other }
        ))),
    }
}

fn read_csv_data(file_path: &str, options: &DataImportOptions) -> Result<ImportSource, DbError> {
    let delimiter = options.delimiter.unwrap_or(',') as u8;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(options.first_row_is_header)
        .flexible(true)
        .from_path(file_path)
        .map_err(|e| DbError::ImportError(format!("Failed to open CSV: {}", e)))?;

    let mut columns: Vec<String> = if options.first_row_is_header {
        reader
            .headers()
            .map_err(|e| DbError::ImportError(format!("Failed to read headers: {}", e)))?
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut rows: Vec<Vec<String>> = Vec::new();
    for result in reader.records() {
        let record =
            result.map_err(|e| DbError::ImportError(format!("Failed to read CSV row: {}", e)))?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }

    if columns.is_empty() {
        columns = generated_headers(rows.first().map(|r| r.len()).unwrap_or(0));
    }

    Ok(ImportSource { columns, rows })
}

fn source_from_range<'a>(
    range_rows: impl Iterator<Item = &'a [Data]>,
    first_row_is_header: bool,
) -> ImportSource {
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    for (i, row) in range_rows.enumerate() {
        let cells: Vec<String> = row.iter().map(cell_to_string).collect();
        if i == 0 && first_row_is_header {
            columns = cells;
        } else {
            rows.push(cells);
        }
    }

    if columns.is_empty() {
        columns = generated_headers(rows.first().map(|r| r.len()).unwrap_or(0));
    }

    ImportSource { columns, rows }
}

// ---------------------------------------------------------------------------
// Running the import
// ---------------------------------------------------------------------------

/// Tables available as an import target.
pub async fn get_tables_for_import(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: Option<&str>,
) -> Result<Vec<String>, DbError> {
    let driver = connection(state, connection_id)?;
    let tables = driver.get_tables(schema.unwrap_or("public")).await?;
    Ok(tables.into_iter().map(|t| t.name).collect())
}

/// Columns of the target table, for the mapping step.
pub async fn get_table_columns_for_import(
    state: &Mutex<AppState>,
    connection_id: &str,
    table_name: &str,
    schema: Option<&str>,
) -> Result<Vec<TableColumnInfo>, DbError> {
    let driver = connection(state, connection_id)?;
    let table_schema = driver
        .get_table_schema(schema.unwrap_or("public"), table_name)
        .await?;
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

/// Write the rows of `source` into the table named by `options`.
///
/// Rows go in batches of `batch_size`. A failed batch is retried row by row
/// so the error report names the rows that actually failed rather than
/// writing off the whole batch, and so one bad row does not cost the other
/// 999 in its batch.
///
/// `source` arrives behind an `Arc` so a shell can keep the parsed file for a
/// retry without copying every row into the task.
pub async fn run_import(
    state: &Mutex<AppState>,
    connection_id: &str,
    source: Arc<ImportSource>,
    options: DataImportOptions,
    cancel: Arc<AtomicBool>,
) -> Result<ImportResult, DbError> {
    let driver = connection(state, connection_id)?;
    let kind = driver_kind(state, connection_id)?;
    let plan = build_import_plan(&kind, &source, &options)?;

    cancel.store(false, Ordering::Relaxed);

    if let Some(create) = &plan.create_statement {
        driver
            .execute_query(create.trim_end_matches(';'))
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to create table: {}", e)))?;
    }

    if let Some(truncate) = &plan.truncate_statement {
        driver
            .execute_query(truncate.trim_end_matches(';'))
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to truncate table: {}", e)))?;
    }

    let rows = import_rows(&source, &options);
    let batch = options.batch_size.max(1);

    let mut rows_imported = 0usize;
    let mut rows_failed = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for (batch_index, chunk) in rows.chunks(batch).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let mapped: Vec<Vec<Option<String>>> = chunk
            .iter()
            .map(|r| map_row(&source.columns, r, &options.column_mappings))
            .collect();
        let sql = insert_statement(&kind, &plan.target, &plan.target_columns, &mapped);

        match driver.execute_query(&sql).await {
            Ok(_) => rows_imported += chunk.len(),
            Err(batch_err) => {
                if chunk.len() == 1 {
                    rows_failed += 1;
                    push_error(
                        &mut errors,
                        format!("Row {}: {}", batch_index * batch + 1, batch_err),
                    );
                    continue;
                }
                // Retry singly so the report blames the right rows.
                for (i, row) in mapped.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let one = insert_statement(
                        &kind,
                        &plan.target,
                        &plan.target_columns,
                        std::slice::from_ref(row),
                    );
                    match driver.execute_query(&one).await {
                        Ok(_) => rows_imported += 1,
                        Err(e) => {
                            rows_failed += 1;
                            push_error(
                                &mut errors,
                                format!("Row {}: {}", batch_index * batch + i + 1, e),
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(ImportResult {
        success: rows_failed == 0 && !cancel.load(Ordering::Relaxed),
        rows_imported,
        rows_failed,
        errors,
    })
}

/// Keep the first ten failures. A million-row file with a wrong delimiter
/// would otherwise build a million-entry error list.
fn push_error(errors: &mut Vec<String>, message: String) {
    if errors.len() < 10 {
        errors.push(message);
    }
}

/// A preview, converted for the results grid so import and query results are
/// rendered by the same widget.
pub fn preview_to_result(preview: &ImportPreview) -> QueryResult {
    let columns: Vec<String> = preview
        .columns
        .iter()
        .zip(
            preview
                .detected_types
                .iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        )
        .map(|(name, ty)| match ty {
            Some(t) => format!("{}  ({})", name, t),
            None => name.clone(),
        })
        .collect();

    let rows = preview
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    if cell.is_empty() {
                        Value::Null
                    } else {
                        Value::String(cell.clone())
                    }
                })
                .collect()
        })
        .collect();

    QueryResult::with_data(columns, rows)
}

/// A mapping HashMap keyed by source column, for callers that need lookups.
pub fn mapping_index(mappings: &[ColumnMapping]) -> HashMap<String, &ColumnMapping> {
    mappings
        .iter()
        .map(|m| (m.source_column.clone(), m))
        .collect()
}

// ---------------------------------------------------------------------------
// Default locations
// ---------------------------------------------------------------------------

/// Where a suggested import or export path points: the Downloads folder if
/// the platform has one, else the home directory, else the working directory.
///
/// There is no native file dialog in this workspace, so shells fill path
/// fields with a suggestion built from this and let the user edit it.
pub fn suggested_directory() -> std::path::PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_postgres_and_mysql_qualify_tables_with_the_workspace_schema() {
        assert_eq!(effective_schema(&DbDriver::Sqlite, "/data/shop.sqlite"), "");
        assert_eq!(effective_schema(&DbDriver::Sqlite, "main"), "");
        assert_eq!(effective_schema(&DbDriver::Turso, "x"), "");
        assert_eq!(effective_schema(&DbDriver::SqlServer, "shop"), "");
        assert_eq!(effective_schema(&DbDriver::Postgres, ""), "public");
        assert_eq!(effective_schema(&DbDriver::Postgres, " app "), "app");
        assert_eq!(effective_schema(&DbDriver::MySql, "shop"), "shop");
        // A path that went through qualify would not run on SQLite.
        let t = qualify(
            &DbDriver::Sqlite,
            &effective_schema(&DbDriver::Sqlite, "/d/x.db"),
            "t",
        );
        assert_eq!(t, "\"t\"");
    }

    #[test]
    fn the_suggested_directory_is_never_empty() {
        assert!(!suggested_directory().as_os_str().is_empty());
    }

    fn mapping(source: &str, target: &str) -> ColumnMapping {
        ColumnMapping {
            source_column: source.to_string(),
            target_column: target.to_string(),
            target_type: Some("TEXT".to_string()),
            default_value: None,
            skip: false,
        }
    }

    fn source(columns: &[&str], rows: &[&[&str]]) -> ImportSource {
        ImportSource {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|s| s.to_string()).collect())
                .collect(),
        }
    }

    // -- CSV escaping ------------------------------------------------------

    #[test]
    fn plain_values_are_not_quoted() {
        assert_eq!(escape_csv_value("hello", ',', CsvQuoting::Minimal), "hello");
        assert_eq!(escape_csv_value("123", ',', CsvQuoting::Minimal), "123");
        assert_eq!(escape_csv_value("", ',', CsvQuoting::Minimal), "");
    }

    #[test]
    fn a_value_containing_the_delimiter_is_quoted() {
        assert_eq!(
            escape_csv_value("hello,world", ',', CsvQuoting::Minimal),
            "\"hello,world\""
        );
    }

    #[test]
    fn quoting_follows_the_configured_delimiter_not_a_hardcoded_comma() {
        // With a semicolon delimiter, a comma is ordinary data and a
        // semicolon is what must be quoted. Getting this backwards produces
        // a file that reads back with the wrong number of columns.
        assert_eq!(
            escape_csv_value("a,b", ';', CsvQuoting::Minimal),
            "a,b",
            "a comma is safe when the delimiter is a semicolon"
        );
        assert_eq!(escape_csv_value("a;b", ';', CsvQuoting::Minimal), "\"a;b\"");
        assert_eq!(
            escape_csv_value("a\tb", '\t', CsvQuoting::Minimal),
            "\"a\tb\""
        );
        assert_eq!(escape_csv_value("a|b", '|', CsvQuoting::Minimal), "\"a|b\"");
    }

    #[test]
    fn embedded_quotes_are_doubled_and_the_field_is_quoted() {
        assert_eq!(
            escape_csv_value("hello \"world\"", ',', CsvQuoting::Minimal),
            "\"hello \"\"world\"\"\""
        );
        // A lone quote still forces quoting, even with no delimiter present.
        assert_eq!(
            escape_csv_value("a\"b", ',', CsvQuoting::Minimal),
            "\"a\"\"b\""
        );
    }

    #[test]
    fn newlines_inside_a_value_are_quoted_not_stripped() {
        assert_eq!(
            escape_csv_value("hello\nworld", ',', CsvQuoting::Minimal),
            "\"hello\nworld\""
        );
        assert_eq!(
            escape_csv_value("hello\r\nworld", ',', CsvQuoting::Minimal),
            "\"hello\r\nworld\""
        );
    }

    #[test]
    fn quote_all_quotes_even_harmless_values() {
        assert_eq!(escape_csv_value("plain", ',', CsvQuoting::All), "\"plain\"");
        assert_eq!(
            escape_csv_value("a\"b", ',', CsvQuoting::All),
            "\"a\"\"b\"",
            "escaping still applies when quoting everything"
        );
    }

    #[test]
    fn a_value_that_tries_to_end_the_field_survives_a_round_trip() {
        // The nastiest CSV cell: delimiter, quotes and a newline together.
        let evil = "x\",\ny\"z";
        let line = csv_record(&[evil.to_string()], &CsvOptions::default());
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let record = rdr.records().next().unwrap().unwrap();
        assert_eq!(record.len(), 1, "must still be a single field");
        assert_eq!(&record[0], evil);
    }

    #[test]
    fn headers_are_written_only_when_asked_for() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![json!(1), json!("Alice")]];

        let with = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(with, "id,name\n1,Alice\n");

        let without = render_csv(
            &columns,
            &rows,
            &CsvOptions {
                include_headers: false,
                ..CsvOptions::default()
            },
        );
        assert_eq!(without, "1,Alice\n");
    }

    #[test]
    fn a_header_containing_the_delimiter_is_escaped_too() {
        let columns = vec!["last,first".to_string()];
        let out = render_csv(&columns, &[], &CsvOptions::default());
        assert_eq!(out, "\"last,first\"\n");
    }

    #[test]
    fn null_renders_as_the_configured_text() {
        let columns = vec!["a".to_string()];
        let rows = vec![vec![Value::Null]];
        assert_eq!(
            render_csv(&columns, &rows, &CsvOptions::default()),
            "a\n\n",
            "the default is an empty field"
        );
        let out = render_csv(
            &columns,
            &rows,
            &CsvOptions {
                null_text: "\\N".to_string(),
                ..CsvOptions::default()
            },
        );
        assert_eq!(out, "a\n\\N\n");
    }

    #[test]
    fn crlf_is_used_for_every_line_when_selected() {
        let out = render_csv(
            &["a".to_string()],
            &[vec![json!(1)], vec![json!(2)]],
            &CsvOptions {
                crlf: true,
                ..CsvOptions::default()
            },
        );
        assert_eq!(out, "a\r\n1\r\n2\r\n");
    }

    #[test]
    fn nested_json_survives_as_one_csv_field() {
        let out = render_csv(
            &["payload".to_string()],
            &[vec![json!({"k": "v,w"})]],
            &CsvOptions::default(),
        );
        assert_eq!(out, "payload\n\"{\"\"k\"\":\"\"v,w\"\"}\"\n");
    }

    #[test]
    fn row_limit_truncates_the_export() {
        let rows: Vec<Vec<Value>> = (0..10).map(|i| vec![json!(i)]).collect();
        assert_eq!(limited(&rows, Some(3)).len(), 3);
        assert_eq!(limited(&rows, Some(50)).len(), 10);
        assert_eq!(limited(&rows, None).len(), 10);
    }

    // -- JSON export -------------------------------------------------------

    #[test]
    fn json_export_keys_rows_by_column_name() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![json!(1), json!("Alice")], vec![json!(2), json!(null)]];
        let out = render_json(&columns, &rows, false).unwrap();
        let parsed: Vec<serde_json::Map<String, Value>> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["name"], json!("Alice"));
        assert_eq!(parsed[1]["name"], json!(null));
    }

    // -- Excel export ------------------------------------------------------

    #[test]
    fn xml_special_characters_cannot_break_out_of_a_cell() {
        assert_eq!(xml_escape("<Cell>&\"'"), "&lt;Cell&gt;&amp;&quot;&apos;");
        // A control byte would make Excel reject the workbook outright.
        assert_eq!(xml_escape("a\u{1}b"), "ab");
    }

    #[test]
    fn excel_keeps_numbers_numeric_and_nulls_blank() {
        let out = render_excel_xml(
            "Results",
            &["n".to_string(), "s".to_string()],
            &[vec![json!(42), Value::Null]],
        );
        assert!(
            out.contains("<Data ss:Type=\"Number\">42</Data>"),
            "{}",
            out
        );
        assert!(out.contains("<Cell/>"), "{}", out);
        assert!(out.contains("ss:Name=\"Results\""), "{}", out);
    }

    #[test]
    fn sheet_names_are_made_legal_for_excel() {
        assert_eq!(sanitize_sheet_name("a/b:c"), "a_b_c");
        assert_eq!(sanitize_sheet_name("   "), "Sheet1");
        assert_eq!(sanitize_sheet_name(&"x".repeat(50)).len(), 31);
    }

    // -- SQL escaping: the injection surface -------------------------------

    #[test]
    fn a_value_containing_a_quote_cannot_end_the_literal() {
        let evil = "'); DROP TABLE users; --";
        let out = sql_value_to_string(&DbDriver::Postgres, &json!(evil));
        assert_eq!(out, "'''); DROP TABLE users; --'");
        assert!(
            !out[1..out.len() - 1].contains('\'')
                || out[1..out.len() - 1].matches('\'').count() % 2 == 0,
            "every quote inside the literal must be doubled: {}",
            out
        );
    }

    #[test]
    fn mysql_also_escapes_backslashes() {
        // On MySQL, `\'` ends a literal early unless the backslash is
        // escaped; the standard doubling rule alone is not enough.
        let out = sql_value_to_string(&DbDriver::MySql, &json!("a\\'; DROP TABLE t; --"));
        assert_eq!(out, "'a\\\\''; DROP TABLE t; --'");
        let pg = sql_value_to_string(&DbDriver::Postgres, &json!("a\\'"));
        assert_eq!(pg, "'a\\'''");
    }

    #[test]
    fn sql_literals_keep_types_that_are_not_strings() {
        assert_eq!(
            sql_value_to_string(&DbDriver::Postgres, &Value::Null),
            "NULL"
        );
        assert_eq!(
            sql_value_to_string(&DbDriver::Postgres, &json!(true)),
            "TRUE"
        );
        assert_eq!(
            sql_value_to_string(&DbDriver::Postgres, &json!(false)),
            "FALSE"
        );
        assert_eq!(sql_value_to_string(&DbDriver::Postgres, &json!(42)), "42");
        assert_eq!(
            sql_value_to_string(&DbDriver::Postgres, &json!(-1.5)),
            "-1.5"
        );
    }

    #[test]
    fn a_json_object_is_escaped_as_a_single_literal() {
        let out = sql_value_to_string(&DbDriver::Postgres, &json!({"k": "it's"}));
        assert_eq!(out, "'{\"k\":\"it''s\"}'");
    }

    #[test]
    fn identifiers_are_quoted_per_dialect_and_embedded_quotes_escaped() {
        assert_eq!(quote_ident(&DbDriver::Postgres, "order"), "\"order\"");
        assert_eq!(quote_ident(&DbDriver::Postgres, "we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_ident(&DbDriver::MySql, "we`ird"), "`we``ird`");
        assert_eq!(quote_ident(&DbDriver::SqlServer, "we]ird"), "[we]]ird]");
    }

    #[test]
    fn a_table_name_cannot_inject_through_qualification() {
        let evil = "users\"; DROP TABLE t; --";
        let out = qualify(&DbDriver::Postgres, "public", evil);
        assert_eq!(out, "\"public\".\"users\"\"; DROP TABLE t; --\"");
    }

    #[test]
    fn turso_and_empty_schemas_produce_a_bare_table_name() {
        assert_eq!(qualify(&DbDriver::Sqlite, "", "users"), "\"users\"");
        assert_eq!(qualify(&DbDriver::Turso, "main", "users"), "\"users\"");
        assert_eq!(qualify(&DbDriver::MySql, "app", "users"), "`app`.`users`");
    }

    #[test]
    fn drop_statements_match_the_dialect() {
        assert_eq!(
            drop_statement(&DbDriver::Postgres, "public", "users"),
            "DROP TABLE IF EXISTS \"public\".\"users\" CASCADE;"
        );
        assert_eq!(
            drop_statement(&DbDriver::MySql, "app", "users"),
            "DROP TABLE IF EXISTS `app`.`users`;"
        );
        assert_eq!(
            drop_statement(&DbDriver::Turso, "main", "users"),
            "DROP TABLE IF EXISTS \"users\";"
        );
        assert!(drop_statement(&DbDriver::MongoDb, "db", "users").starts_with("--"));
    }

    #[test]
    fn a_drop_statement_escapes_the_names_in_its_sql_server_literal() {
        // SQL Server embeds the name in a string literal as well as an
        // identifier; both have to be escaped.
        let out = drop_statement(&DbDriver::SqlServer, "dbo", "it's");
        assert!(out.contains("'dbo.it''s'"), "{}", out);
        assert!(out.contains("[dbo].[it's]"), "{}", out);
    }

    #[test]
    fn create_table_renders_columns_nullability_and_defaults() {
        let columns = vec![
            ColumnInfo {
                name: "id".into(),
                data_type: "integer".into(),
                nullable: false,
                default_value: Some("nextval('s')".into()),
                is_primary_key: true,
                is_auto_increment: true,
            },
            ColumnInfo {
                name: "note".into(),
                data_type: "text".into(),
                nullable: true,
                default_value: None,
                is_primary_key: false,
                is_auto_increment: false,
            },
        ];
        let sql = create_table_statement(&DbDriver::Postgres, "public", "t", &columns);
        assert_eq!(
            sql,
            "CREATE TABLE \"public\".\"t\" (\n  \"id\" integer NOT NULL DEFAULT nextval('s'),\n  \"note\" text\n);"
        );
    }

    #[test]
    fn generated_inserts_name_their_columns_and_escape_values() {
        let sql = render_sql_inserts(
            &DbDriver::Postgres,
            "public",
            "users",
            &["id".to_string(), "name".to_string()],
            &[vec![json!(1), json!("O'Hara")]],
        );
        assert!(
            sql.contains(
                "INSERT INTO \"public\".\"users\" (\"id\", \"name\") VALUES (1, 'O''Hara');"
            ),
            "{}",
            sql
        );
    }

    // -- dump import parsing -----------------------------------------------

    #[test]
    fn value_groups_split_on_top_level_parens_only() {
        let groups = parse_value_row_groups("(1,'a'),(2,'b')");
        assert_eq!(groups, vec!["(1,'a')", "(2,'b')"]);
    }

    #[test]
    fn a_paren_or_comma_inside_a_quoted_value_does_not_split_a_row() {
        let groups = parse_value_row_groups("(1,'a),(b'),(2,'c')");
        assert_eq!(groups, vec!["(1,'a),(b')", "(2,'c')"]);
    }

    #[test]
    fn a_backslash_escaped_quote_does_not_end_the_string() {
        let groups = parse_value_row_groups(r"(1,'a\'),(b'),(2,'c')");
        assert_eq!(groups, vec![r"(1,'a\'),(b')", "(2,'c')"]);
    }

    #[test]
    fn large_inserts_split_into_batches_and_small_ones_do_not() {
        let stmt = "INSERT INTO t VALUES (1),(2),(3),(4),(5);";
        assert_eq!(split_insert_values(stmt, 5), vec![stmt.to_string()]);
        let split = split_insert_values(stmt, 2);
        assert_eq!(
            split,
            vec![
                "INSERT INTO t VALUES (1),(2)",
                "INSERT INTO t VALUES (3),(4)",
                "INSERT INTO t VALUES (5)",
            ]
        );
    }

    #[test]
    fn replace_ignore_into_is_rewritten_to_insert_ignore() {
        assert_eq!(
            normalize_dump_stmt("REPLACE IGNORE INTO `t` VALUES (1)".to_string()),
            "INSERT IGNORE INTO `t` VALUES (1)"
        );
        // Anything else is left exactly as it was.
        let plain = "INSERT INTO `t` VALUES (1)".to_string();
        assert_eq!(normalize_dump_stmt(plain.clone()), plain);
        let replace = "REPLACE INTO `t` VALUES (1)".to_string();
        assert_eq!(normalize_dump_stmt(replace.clone()), replace);
    }

    #[test]
    fn client_only_statements_are_recognised() {
        assert!(is_client_only_statement("LOCK TABLES `t` WRITE"));
        assert!(is_client_only_statement("unlock tables"));
        assert!(!is_client_only_statement("INSERT INTO t VALUES (1)"));
    }

    #[test]
    fn the_error_log_sits_next_to_the_dump() {
        assert_eq!(
            derive_log_path("/tmp/dump.sql"),
            "/tmp/dump_import_errors.log"
        );
        assert_eq!(derive_log_path("/tmp/dump"), "/tmp/dump_import_errors.log");
    }

    // -- type inference ----------------------------------------------------

    #[test]
    fn integers_floats_and_text_are_told_apart() {
        assert_eq!(detect_column_type(&["12".into(), "3".into()]), "INTEGER");
        assert_eq!(detect_column_type(&["1.5".into(), "3".into()]), "DECIMAL");
        assert_eq!(detect_column_type(&["ada".into(), "3".into()]), "TEXT");
        assert_eq!(detect_column_type(&[]), "TEXT");
    }

    #[test]
    fn booleans_win_over_integers_for_columns_of_ones_and_zeroes() {
        // Documented, deliberate: 1/0 is ambiguous and the guess is only a
        // default the user can change in the mapping step.
        assert_eq!(detect_column_type(&["1".into(), "0".into()]), "BOOLEAN");
        assert_eq!(detect_column_type(&["true".into(), "NO".into()]), "BOOLEAN");
        assert_eq!(detect_column_type(&["1".into(), "2".into()]), "INTEGER");
    }

    #[test]
    fn iso_and_slashed_dates_are_detected() {
        assert_eq!(detect_column_type(&["2026-09-08".into()]), "DATE");
        assert_eq!(detect_column_type(&["09/08/2026".into()]), "DATE");
        assert_eq!(detect_column_type(&["08-09-2026".into()]), "DATE");
        assert_eq!(detect_column_type(&["8 Sept 2026".into()]), "TEXT");
    }

    #[test]
    fn a_single_bad_sample_demotes_the_whole_column_to_text() {
        assert_eq!(
            detect_column_type(&["1".into(), "2".into(), "n/a".into()]),
            "TEXT"
        );
    }

    #[test]
    fn per_column_detection_ignores_empty_cells() {
        let types = detect_types(
            &["n".to_string(), "s".to_string()],
            &[
                vec!["1".to_string(), "".to_string()],
                vec!["".to_string(), "ada".to_string()],
                vec!["7".to_string(), "grace".to_string()],
            ],
        );
        assert_eq!(types, vec!["INTEGER", "TEXT"]);
    }

    #[test]
    fn detection_covers_every_column_even_with_ragged_rows() {
        let types = detect_types(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &[vec!["1".to_string()]],
        );
        assert_eq!(types.len(), 3);
        assert_eq!(types[2], "TEXT");
    }

    // -- header handling ---------------------------------------------------

    #[test]
    fn missing_headers_are_generated_one_based() {
        assert_eq!(generated_headers(3), vec!["Column1", "Column2", "Column3"]);
        assert!(generated_headers(0).is_empty());
    }

    #[test]
    fn a_headerless_csv_gets_generated_names_and_keeps_its_first_row() {
        let dir = std::env::temp_dir().join(format!("dbhive_csv_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("noheader.csv");
        std::fs::write(&path, "1,ada\n2,grace\n").unwrap();

        let opts = DataImportOptions {
            first_row_is_header: false,
            ..DataImportOptions::default()
        };
        let src = read_import_file(path.to_str().unwrap(), &opts).unwrap();
        assert_eq!(src.columns, vec!["Column1", "Column2"]);
        assert_eq!(src.rows.len(), 2, "the first row is data, not a header");
        assert_eq!(src.rows[0], vec!["1", "ada"]);

        let with_header =
            read_import_file(path.to_str().unwrap(), &DataImportOptions::default()).unwrap();
        assert_eq!(with_header.columns, vec!["1", "ada"]);
        assert_eq!(with_header.rows.len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_quoted_csv_field_containing_the_delimiter_reads_back_as_one_column() {
        let dir = std::env::temp_dir().join(format!("dbhive_csv2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("quoted.csv");
        std::fs::write(&path, "id,name\n1,\"Doe, Jane\"\n").unwrap();

        let src = read_import_file(path.to_str().unwrap(), &DataImportOptions::default()).unwrap();
        assert_eq!(src.columns, vec!["id", "name"]);
        assert_eq!(src.rows[0], vec!["1", "Doe, Jane"]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_semicolon_file_needs_the_semicolon_delimiter() {
        let dir = std::env::temp_dir().join(format!("dbhive_csv3_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("semi.csv");
        std::fs::write(&path, "id;name\n1;ada\n").unwrap();

        let semi = DataImportOptions {
            delimiter: Some(';'),
            ..DataImportOptions::default()
        };
        let src = read_import_file(path.to_str().unwrap(), &semi).unwrap();
        assert_eq!(src.columns, vec!["id", "name"]);
        assert_eq!(src.rows[0], vec!["1", "ada"]);

        // With the wrong delimiter it is one wide column, not an error.
        let comma =
            read_import_file(path.to_str().unwrap(), &DataImportOptions::default()).unwrap();
        assert_eq!(comma.columns, vec!["id;name"]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_extension_is_rejected_with_its_name() {
        let err = read_import_file("/tmp/x.parquet", &DataImportOptions::default()).unwrap_err();
        assert!(err.to_string().contains("parquet"), "{}", err);
    }

    // -- column mapping ----------------------------------------------------

    #[test]
    fn auto_mapping_matches_names_case_insensitively() {
        let targets = vec![
            TableColumnInfo {
                name: "ID".into(),
                data_type: "integer".into(),
                nullable: false,
                default_value: None,
                is_primary_key: true,
            },
            TableColumnInfo {
                name: "email".into(),
                data_type: "text".into(),
                nullable: true,
                default_value: None,
                is_primary_key: false,
            },
        ];
        let mappings = auto_map_columns(
            &["id".to_string(), "Email".to_string(), "extra".to_string()],
            &[
                "INTEGER".to_string(),
                "TEXT".to_string(),
                "TEXT".to_string(),
            ],
            &targets,
        );
        assert_eq!(mappings[0].target_column, "ID");
        assert_eq!(mappings[1].target_column, "email");
        assert_eq!(
            mappings[2].target_column, "",
            "an unmatched column has no target"
        );
        assert!(
            mappings[2].skip,
            "an unmatched column defaults to skipped rather than silently mapping"
        );
        assert_eq!(mappings[0].target_type.as_deref(), Some("integer"));
    }

    #[test]
    fn only_mapped_unskipped_columns_are_written() {
        let mappings = vec![
            mapping("a", "col_a"),
            ColumnMapping {
                skip: true,
                ..mapping("b", "col_b")
            },
            mapping("c", ""),
        ];
        let active = active_mappings(&mappings);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].target_column, "col_a");
    }

    #[test]
    fn mapping_reorders_source_columns_to_match_the_target() {
        let src = source(&["name", "id"], &[&["ada", "1"]]);
        let mappings = vec![mapping("id", "id"), mapping("name", "name")];
        let row = map_row(&src.columns, &src.rows[0], &mappings);
        assert_eq!(
            row,
            vec![Some("1".to_string()), Some("ada".to_string())],
            "values follow the mapping order, not the file's"
        );
    }

    #[test]
    fn boolean_words_become_1_and_0_only_for_boolean_like_columns() {
        for (raw, want) in [
            ("true", "1"),
            ("FALSE", "0"),
            (" Yes ", "1"),
            ("n", "0"),
            ("T", "1"),
        ] {
            assert_eq!(boolean_cell(raw, Some("BOOLEAN")), want, "{raw:?}");
            assert_eq!(boolean_cell(raw, Some("tinyint(1)")), want, "{raw:?}");
            assert_eq!(boolean_cell(raw, Some("bool")), want, "{raw:?}");
        }
        // Not a boolean word: left alone, even in a boolean column (the server says no).
        assert_eq!(boolean_cell("maybe", Some("BOOLEAN")), "maybe");
        assert_eq!(boolean_cell("2", Some("BOOLEAN")), "2");
        // Other column types keep the text exactly.
        for ty in [Some("TEXT"), Some("VARCHAR(20)"), Some("INTEGER"), None] {
            assert_eq!(boolean_cell("true", ty), "true", "{ty:?}");
        }
    }

    #[test]
    fn a_boolean_word_is_converted_when_a_row_is_mapped() {
        let src = source(&["flag", "label"], &[&["true", "true"]]);
        let mappings = vec![
            ColumnMapping {
                target_type: Some("BOOLEAN".to_string()),
                ..mapping("flag", "flag")
            },
            ColumnMapping {
                target_type: Some("TEXT".to_string()),
                ..mapping("label", "label")
            },
        ];
        let row = map_row(&src.columns, &src.rows[0], &mappings);
        assert_eq!(row, vec![Some("1".to_string()), Some("true".to_string())]);
    }

    #[test]
    fn an_empty_cell_becomes_null_or_the_mapping_default() {
        let src = source(&["a", "b"], &[&["", ""]]);
        let mappings = vec![
            mapping("a", "a"),
            ColumnMapping {
                default_value: Some("unknown".to_string()),
                ..mapping("b", "b")
            },
        ];
        let row = map_row(&src.columns, &src.rows[0], &mappings);
        assert_eq!(row, vec![None, Some("unknown".to_string())]);
    }

    #[test]
    fn a_mapping_for_a_column_the_file_lacks_falls_back_to_its_default() {
        let src = source(&["a"], &[&["1"]]);
        let mappings = vec![
            mapping("a", "a"),
            ColumnMapping {
                default_value: Some("0".to_string()),
                ..mapping("missing", "b")
            },
        ];
        let row = map_row(&src.columns, &src.rows[0], &mappings);
        assert_eq!(row, vec![Some("1".to_string()), Some("0".to_string())]);
    }

    #[test]
    fn a_short_row_maps_its_missing_cells_to_null() {
        let src = source(&["a", "b"], &[&["1"]]);
        let mappings = vec![mapping("a", "a"), mapping("b", "b")];
        let row = map_row(&src.columns, &src.rows[0], &mappings);
        assert_eq!(row, vec![Some("1".to_string()), None]);
    }

    // -- generated import SQL ----------------------------------------------

    #[test]
    fn an_insert_quotes_its_table_and_columns_and_escapes_its_values() {
        let sql = insert_statement(
            &DbDriver::Postgres,
            "\"public\".\"users\"",
            &["id".to_string(), "name".to_string()],
            &[vec![Some("1".into()), Some("O'Hara".into())]],
        );
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"id\", \"name\") VALUES ('1', 'O''Hara')"
        );
    }

    #[test]
    fn a_cell_full_of_sql_is_inserted_as_data_not_executed() {
        let sql = insert_statement(
            &DbDriver::Postgres,
            "\"t\"",
            &["c".to_string()],
            &[vec![Some("'); DROP TABLE users; --".into())]],
        );
        assert_eq!(
            sql,
            "INSERT INTO \"t\" (\"c\") VALUES ('''); DROP TABLE users; --')"
        );
    }

    #[test]
    fn a_mysql_backslash_cell_cannot_end_its_literal() {
        let sql = insert_statement(
            &DbDriver::MySql,
            "`t`",
            &["c".to_string()],
            &[vec![Some("\\' OR 1=1 -- ".into())]],
        );
        assert_eq!(sql, "INSERT INTO `t` (`c`) VALUES ('\\\\'' OR 1=1 -- ')");
    }

    #[test]
    fn a_batch_insert_carries_every_row_in_one_statement() {
        let sql = insert_statement(
            &DbDriver::Postgres,
            "\"t\"",
            &["a".to_string()],
            &[vec![Some("1".into())], vec![None], vec![Some("3".into())]],
        );
        assert_eq!(sql, "INSERT INTO \"t\" (\"a\") VALUES ('1'), (NULL), ('3')");
    }

    #[test]
    fn truncate_uses_delete_where_truncate_does_not_exist() {
        assert_eq!(
            truncate_statement(&DbDriver::Postgres, "\"t\""),
            "TRUNCATE TABLE \"t\""
        );
        assert_eq!(
            truncate_statement(&DbDriver::Sqlite, "\"t\""),
            "DELETE FROM \"t\""
        );
    }

    #[test]
    fn a_generated_create_table_refuses_a_type_that_is_not_a_type() {
        assert_eq!(sanitize_type("varchar(255)"), "VARCHAR(255)");
        assert_eq!(sanitize_type("double precision"), "DOUBLE PRECISION");
        assert_eq!(sanitize_type("text); DROP TABLE t; --"), "TEXT");
        assert_eq!(sanitize_type(""), "TEXT");

        let sql = create_table_from_mappings(
            &DbDriver::Postgres,
            "public",
            "t",
            &[ColumnMapping {
                target_type: Some("text); DROP TABLE users; --".into()),
                ..mapping("a", "a")
            }],
        );
        assert_eq!(
            sql,
            "CREATE TABLE IF NOT EXISTS \"public\".\"t\" (\n  \"a\" TEXT\n)"
        );
    }

    // -- the plan the user confirms ----------------------------------------

    #[test]
    fn the_plan_reports_the_target_row_count_and_batching() {
        let src = source(
            &["id", "name"],
            &[&["1", "ada"], &["2", "grace"], &["3", "x"]],
        );
        let options = DataImportOptions {
            table_name: "users".into(),
            schema: Some("public".into()),
            column_mappings: vec![mapping("id", "id"), mapping("name", "name")],
            batch_size: 2,
            ..DataImportOptions::default()
        };
        let plan = build_import_plan(&DbDriver::Postgres, &src, &options).unwrap();

        assert_eq!(plan.target, "\"public\".\"users\"");
        assert_eq!(plan.target_columns, vec!["id", "name"]);
        assert_eq!(plan.row_count, 3);
        assert_eq!(plan.batch_count, 2);
        assert!(!plan.is_destructive());
        assert!(plan.truncate_statement.is_none());
        assert!(plan.create_statement.is_none());
        assert_eq!(plan.sample_statements.len(), 2);
        assert!(
            plan.sample_statements[0].starts_with(
                "INSERT INTO \"public\".\"users\" (\"id\", \"name\") VALUES ('1', 'ada'), ('2', 'grace')"
            ),
            "{}",
            plan.sample_statements[0]
        );
    }

    #[test]
    fn skip_rows_removes_rows_from_the_top_of_the_data() {
        let src = source(&["a"], &[&["1"], &["2"], &["3"]]);
        let options = DataImportOptions {
            table_name: "t".into(),
            column_mappings: vec![mapping("a", "a")],
            skip_rows: 2,
            ..DataImportOptions::default()
        };
        let plan = build_import_plan(&DbDriver::Postgres, &src, &options).unwrap();
        assert_eq!(plan.row_count, 1);
        assert!(
            plan.sample_statements[0].contains("('3')"),
            "{}",
            plan.sample_statements[0]
        );

        // Skipping past the end is empty, not a panic.
        let options = DataImportOptions {
            skip_rows: 99,
            ..options
        };
        assert_eq!(
            build_import_plan(&DbDriver::Postgres, &src, &options)
                .unwrap()
                .row_count,
            0
        );
    }

    #[test]
    fn a_truncating_plan_is_flagged_destructive_and_shows_the_statement() {
        let src = source(&["a"], &[&["1"]]);
        let options = DataImportOptions {
            table_name: "t".into(),
            column_mappings: vec![mapping("a", "a")],
            truncate_before: true,
            create_table: true,
            ..DataImportOptions::default()
        };
        let plan = build_import_plan(&DbDriver::Postgres, &src, &options).unwrap();
        assert!(plan.is_destructive());
        assert_eq!(
            plan.truncate_statement.as_deref(),
            Some("TRUNCATE TABLE \"t\";")
        );
        assert!(plan
            .create_statement
            .unwrap()
            .starts_with("CREATE TABLE IF NOT EXISTS"));
    }

    #[test]
    fn the_plan_lists_the_columns_that_will_not_be_written() {
        let src = source(&["a", "b"], &[&["1", "2"]]);
        let options = DataImportOptions {
            table_name: "t".into(),
            column_mappings: vec![
                mapping("a", "a"),
                ColumnMapping {
                    skip: true,
                    ..mapping("b", "b")
                },
            ],
            ..DataImportOptions::default()
        };
        let plan = build_import_plan(&DbDriver::Postgres, &src, &options).unwrap();
        assert_eq!(plan.skipped_columns, vec!["b"]);
        assert_eq!(plan.target_columns, vec!["a"]);
    }

    #[test]
    fn a_plan_without_a_table_or_a_mapping_is_refused() {
        let src = source(&["a"], &[&["1"]]);
        let no_table = DataImportOptions {
            column_mappings: vec![mapping("a", "a")],
            ..DataImportOptions::default()
        };
        assert!(build_import_plan(&DbDriver::Postgres, &src, &no_table).is_err());

        let no_mapping = DataImportOptions {
            table_name: "t".into(),
            ..DataImportOptions::default()
        };
        assert!(build_import_plan(&DbDriver::Postgres, &src, &no_mapping).is_err());
    }

    #[test]
    fn an_empty_file_plans_zero_rows_rather_than_failing() {
        let src = source(&["a"], &[]);
        let options = DataImportOptions {
            table_name: "t".into(),
            column_mappings: vec![mapping("a", "a")],
            ..DataImportOptions::default()
        };
        let plan = build_import_plan(&DbDriver::Postgres, &src, &options).unwrap();
        assert_eq!(plan.row_count, 0);
        assert_eq!(plan.batch_count, 0);
        assert!(plan.sample_statements.is_empty());
    }

    // -- misc --------------------------------------------------------------

    #[test]
    fn the_table_filter_ignores_whitespace_and_empty_entries() {
        assert_eq!(
            parse_table_filter(" users , orders ,, "),
            vec!["users", "orders"]
        );
        assert!(parse_table_filter("   ").is_empty());
    }

    #[test]
    fn a_preview_becomes_a_grid_result_with_types_in_the_headers() {
        let preview = ImportPreview {
            columns: vec!["id".into(), "name".into()],
            rows: vec![vec!["1".into(), "".into()]],
            total_rows: Some(1),
            detected_types: vec!["INTEGER".into(), "TEXT".into()],
            file_type: "csv".into(),
            sheet_names: None,
        };
        let result = preview_to_result(&preview);
        assert_eq!(result.columns, vec!["id  (INTEGER)", "name  (TEXT)"]);
        assert_eq!(result.rows[0][0], json!("1"));
        assert_eq!(
            result.rows[0][1],
            Value::Null,
            "an empty cell shows as NULL in the grid, matching what will be inserted"
        );
    }

    #[test]
    fn export_formats_have_distinct_extensions() {
        let exts: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.extension()).collect();
        assert_eq!(exts, vec!["csv", "json", "sql", "xls"]);
    }

    #[test]
    fn a_row_capped_select_uses_the_dialects_own_syntax() {
        assert_eq!(
            limited_select(&DbDriver::Postgres, "public", "users", Some(10)),
            "SELECT * FROM \"public\".\"users\" LIMIT 10"
        );
        assert_eq!(
            limited_select(&DbDriver::SqlServer, "dbo", "users", Some(10)),
            "SELECT TOP 10 * FROM [dbo].[users]",
            "T-SQL has no LIMIT; using one is a syntax error, not an unlimited read"
        );
        assert_eq!(
            limited_select(&DbDriver::Postgres, "public", "users", None),
            "SELECT * FROM \"public\".\"users\""
        );
    }

    #[test]
    fn rendering_as_sql_without_a_table_name_is_refused() {
        let opts = ExportOptions {
            format: ExportFormat::Sql,
            sql_table: "  ".to_string(),
            ..ExportOptions::default()
        };
        assert!(render_export(&DbDriver::Postgres, &["a".into()], &[], &opts).is_err());
    }

    #[test]
    fn export_writes_the_rendered_bytes_to_disk() {
        let path = std::env::temp_dir().join(format!("dbhive_export_{}.csv", std::process::id()));
        let opts = ExportOptions {
            format: ExportFormat::Csv,
            path: path.to_string_lossy().to_string(),
            ..ExportOptions::default()
        };
        let body = render_export(
            &DbDriver::Postgres,
            &["id".to_string()],
            &[vec![json!(1)]],
            &opts,
        )
        .unwrap();
        write_file(&opts.path, &body).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "id\n1\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn exporting_without_a_path_says_so_instead_of_writing_somewhere() {
        let err = write_file("", "x").unwrap_err();
        assert!(err.to_string().contains("where to save"), "{}", err);
    }
}
