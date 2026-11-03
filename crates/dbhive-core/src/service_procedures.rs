//! Shell-agnostic stored procedure and function operations.
//!
//! These are the bodies of the `#[tauri::command]` handlers in
//! `src-tauri/src/commands/procedures.rs`, taking a plain `&Mutex<AppState>`
//! instead of Tauri's `State`, so both shells run the same code. See
//! docs/rust-ui-migration/03-conventions.md.
//!
//! Everything that can be a pure function is one: the per-driver catalogue
//! SQL, the row mapping, the argument rendering and the CALL/SELECT/EXEC
//! syntax are all testable without a database.
//!
//! Identifiers never reach SQL unquoted. The async entry points take the
//! quoting function from the live driver (`DatabaseDriver::quote_identifier`)
//! and hand it to the pure builders, so a routine called `order details` or
//! `select` works on every dialect and a schema name cannot break out of a
//! string literal.
//!
//! Drivers with no stored-routine concept (SQLite, Turso, MongoDB, Redis)
//! return an empty list rather than an error, so the UI can render an empty
//! state without pretending something went wrong.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drivers::{DatabaseDriver, QueryResult};
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Whether a routine returns a value (function) or is invoked as a statement
/// (procedure). The distinction decides `SELECT` versus `CALL`/`EXEC`, so it
/// is carried around rather than guessed at call time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutineKind {
    Procedure,
    Function,
}

impl RoutineKind {
    /// Read the catalogue's spelling. Postgres says `procedure`/`function`,
    /// MySQL's `ROUTINE_TYPE` is `PROCEDURE`/`FUNCTION`; anything else is
    /// treated as a function, which is the safer default because a `SELECT`
    /// of a procedure fails loudly instead of running something twice.
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("procedure") {
            RoutineKind::Procedure
        } else {
            RoutineKind::Function
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RoutineKind::Procedure => "procedure",
            RoutineKind::Function => "function",
        }
    }
}

/// Metadata describing a single stored procedure or function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcedureInfo {
    pub schema: String,
    pub name: String,
    pub kind: RoutineKind,
    /// Implementation language (Postgres: plpgsql/sql/c; others: none).
    pub language: Option<String>,
    pub return_type: Option<String>,
    /// Fully rendered argument signature, e.g. `(a integer, b text)`.
    pub argument_signature: String,
}

impl ProcedureInfo {
    /// `schema.name`, for display and for keying a selection. Never used in
    /// SQL, so it is deliberately unquoted.
    pub fn qualified(&self) -> String {
        if self.schema.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.schema, self.name)
        }
    }
}

/// One attempt at reading a routine's source, and the column its text lands
/// in. MySQL needs two attempts (`SHOW CREATE PROCEDURE`, then `FUNCTION`)
/// because it has no single statement covering both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionQuery {
    pub sql: String,
    /// Zero-based column holding the definition text.
    pub column: usize,
}

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Whether this driver has stored routines at all.
pub fn supports_routines(driver: &DbDriver) -> bool {
    driver.is_postgres_compatible() || matches!(driver, DbDriver::MySql | DbDriver::SqlServer)
}

/// `schema.name`, both quoted with the driver's own quoting. An empty schema
/// yields the bare name.
pub fn qualify(quote: &dyn Fn(&str) -> String, schema: &str, name: &str) -> String {
    if schema.trim().is_empty() {
        quote(name)
    } else {
        format!("{}.{}", quote(schema), quote(name))
    }
}

/// The catalogue query listing routines, or `None` when the driver has none.
///
/// Every projection returns the same six columns in the same order:
/// schema, name, kind, language, return type, argument signature. That is
/// what lets [`map_rows`] be one function rather than three.
pub fn list_routines_sql(
    driver: &DbDriver,
    schema: Option<&str>,
    escape: &dyn Fn(&str) -> String,
) -> Option<String> {
    if driver.is_postgres_compatible() {
        // pg_get_function_arguments / pg_get_function_result render signatures
        // with proper type names and defaults, which information_schema
        // cannot. prokind: 'f' function, 'p' procedure, 'a' aggregate,
        // 'w' window.
        let base = r#"
            SELECT n.nspname AS schema,
                   p.proname AS name,
                   CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END AS kind,
                   l.lanname AS language,
                   CASE WHEN p.prokind = 'p' THEN NULL
                        ELSE pg_get_function_result(p.oid) END AS return_type,
                   '(' || pg_get_function_arguments(p.oid) || ')' AS argument_signature
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            LEFT JOIN pg_language l ON l.oid = p.prolang
            WHERE p.prokind IN ('f', 'p')
              AND n.nspname NOT IN ('pg_catalog', 'information_schema')
        "#;
        return Some(match schema {
            Some(s) if !s.trim().is_empty() => format!(
                "{} AND n.nspname = '{}' ORDER BY n.nspname, p.proname",
                base,
                escape(s)
            ),
            _ => format!("{} ORDER BY n.nspname, p.proname", base),
        });
    }

    match driver {
        DbDriver::MySql => {
            let mut sql = String::from(
                r#"SELECT r.ROUTINE_SCHEMA, r.ROUTINE_NAME, LOWER(r.ROUTINE_TYPE), NULL,
                          r.DTD_IDENTIFIER,
                          COALESCE(GROUP_CONCAT(
                              CONCAT(
                                  CASE WHEN p.PARAMETER_MODE IS NOT NULL
                                       THEN CONCAT(p.PARAMETER_MODE, ' ') ELSE '' END,
                                  COALESCE(p.PARAMETER_NAME, ''),
                                  ' ',
                                  p.DTD_IDENTIFIER
                              )
                              ORDER BY p.ORDINAL_POSITION SEPARATOR ', '
                          ), '') AS args
                   FROM information_schema.ROUTINES r
                   LEFT JOIN information_schema.PARAMETERS p
                       ON p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                      AND p.SPECIFIC_NAME = r.ROUTINE_NAME
                      AND p.ORDINAL_POSITION > 0
                   WHERE r.ROUTINE_SCHEMA NOT IN ('mysql','sys','performance_schema','information_schema')"#,
            );
            if let Some(s) = schema.filter(|s| !s.trim().is_empty()) {
                sql.push_str(&format!(" AND r.ROUTINE_SCHEMA = '{}'", escape(s)));
            }
            sql.push_str(
                " GROUP BY r.ROUTINE_SCHEMA, r.ROUTINE_NAME, r.ROUTINE_TYPE, r.DTD_IDENTIFIER",
            );
            sql.push_str(" ORDER BY r.ROUTINE_SCHEMA, r.ROUTINE_NAME");
            Some(sql)
        }
        DbDriver::SqlServer => {
            // 'P' procedure, 'FN' scalar function, 'IF'/'TF' table-valued.
            let mut sql = String::from(
                r#"SELECT SCHEMA_NAME(o.schema_id) AS schema_name,
                          o.name,
                          CASE WHEN o.type = 'P' THEN 'procedure' ELSE 'function' END,
                          NULL,
                          NULL,
                          NULL
                   FROM sys.objects o
                   WHERE o.type IN ('P','FN','IF','TF')"#,
            );
            if let Some(s) = schema.filter(|s| !s.trim().is_empty()) {
                sql.push_str(&format!(" AND SCHEMA_NAME(o.schema_id) = '{}'", escape(s)));
            }
            sql.push_str(" ORDER BY schema_name, o.name");
            Some(sql)
        }
        // SQLite, Turso, MongoDB, Redis: no stored routine concept.
        _ => None,
    }
}

/// Read a cell as text. NULL and the empty string both become `None`, since
/// neither carries information for any of the fields read here.
fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        other => Some(other.to_string()),
    }
}

/// Ensure a signature reads as an argument list.
///
/// Postgres already renders the parentheses; MySQL hands back a bare
/// comma-separated list and SQL Server hands back nothing.
pub fn parenthesise(raw: Option<String>) -> String {
    match raw {
        Some(s) if s.trim_start().starts_with('(') => s.trim().to_string(),
        Some(s) => format!("({})", s.trim()),
        None => "()".to_string(),
    }
}

/// Map the six-column catalogue projection onto [`ProcedureInfo`].
pub fn map_rows(result: &QueryResult) -> Vec<ProcedureInfo> {
    result
        .rows
        .iter()
        .map(|row| ProcedureInfo {
            schema: row.first().and_then(as_string).unwrap_or_default(),
            name: row.get(1).and_then(as_string).unwrap_or_default(),
            kind: row
                .get(2)
                .and_then(as_string)
                .map(|k| RoutineKind::parse(&k))
                .unwrap_or(RoutineKind::Function),
            language: row.get(3).and_then(as_string),
            return_type: row.get(4).and_then(as_string),
            argument_signature: parenthesise(row.get(5).and_then(as_string)),
        })
        .collect()
}

/// Statements to try, in order, when reading a routine's source.
///
/// `kind` orders MySQL's two attempts; when it is unknown both are tried,
/// procedure first, and the first one that answers wins.
pub fn definition_queries(
    driver: &DbDriver,
    schema: &str,
    name: &str,
    kind: Option<RoutineKind>,
    quote: &dyn Fn(&str) -> String,
    escape: &dyn Fn(&str) -> String,
) -> Result<Vec<DefinitionQuery>, DbError> {
    if driver.is_postgres_compatible() {
        // pg_get_functiondef takes a regprocedure; with overloads present the
        // lowest oid is picked, which keeps this a single round trip.
        return Ok(vec![DefinitionQuery {
            sql: format!(
                r#"SELECT pg_get_functiondef(p.oid)
               FROM pg_proc p
               JOIN pg_namespace n ON n.oid = p.pronamespace
               WHERE n.nspname = '{}' AND p.proname = '{}'
               ORDER BY p.oid
               LIMIT 1"#,
                escape(schema),
                escape(name),
            ),
            column: 0,
        }]);
    }

    match driver {
        DbDriver::MySql => {
            let qualified = qualify(quote, schema, name);
            // SHOW CREATE ... columns: Routine, sql_mode, Create statement.
            let procedure = DefinitionQuery {
                sql: format!("SHOW CREATE PROCEDURE {}", qualified),
                column: 2,
            };
            let function = DefinitionQuery {
                sql: format!("SHOW CREATE FUNCTION {}", qualified),
                column: 2,
            };
            Ok(match kind {
                Some(RoutineKind::Function) => vec![function, procedure],
                _ => vec![procedure, function],
            })
        }
        DbDriver::SqlServer => Ok(vec![DefinitionQuery {
            // OBJECT_ID takes the quoted name as a string, so the identifier
            // is quoted first and then escaped as a literal.
            sql: format!(
                "SELECT OBJECT_DEFINITION(OBJECT_ID('{}'))",
                escape(&qualify(quote, schema, name))
            ),
            column: 0,
        }]),
        _ => Err(DbError::InvalidInput(
            "Routine definitions are not available for this driver".to_string(),
        )),
    }
}

/// Render a JSON value as a SQL literal for an inline argument list.
///
/// Arrays and objects are passed as JSON text and left for the server to
/// cast; guessing an array syntax per dialect would be wrong more often than
/// it was right.
pub fn render_arg(value: &Value, escape: &dyn Fn(&str) -> String) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", escape(s)),
        other => format!("'{}'", escape(&other.to_string())),
    }
}

/// The statement that invokes a routine.
///
/// A function returns a value, so it is selected; a procedure is a statement
/// and is called. T-SQL spells the call `EXEC` and takes its arguments
/// without parentheses.
pub fn call_sql(
    driver: &DbDriver,
    kind: RoutineKind,
    schema: &str,
    name: &str,
    args: &[Value],
    quote: &dyn Fn(&str) -> String,
    escape: &dyn Fn(&str) -> String,
) -> Result<String, DbError> {
    if !supports_routines(driver) {
        return Err(DbError::InvalidInput(
            "Executing routines is not supported for this driver".to_string(),
        ));
    }

    let rendered: Vec<String> = args.iter().map(|a| render_arg(a, escape)).collect();
    let arg_list = rendered.join(", ");
    let target = qualify(quote, schema, name);

    Ok(match (driver, kind) {
        (DbDriver::SqlServer, RoutineKind::Procedure) => {
            if arg_list.is_empty() {
                format!("EXEC {}", target)
            } else {
                format!("EXEC {} {}", target, arg_list)
            }
        }
        (_, RoutineKind::Procedure) => format!("CALL {}({})", target, arg_list),
        (_, RoutineKind::Function) => format!("SELECT {}({})", target, arg_list),
    })
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Clone the live driver and the profile's dialect out of state.
///
/// The guard is dropped before returning, so no caller can hold it across an
/// await point.
fn take_connection(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<(Arc<dyn DatabaseDriver>, DbDriver), DbError> {
    let guard = state.lock().unwrap();
    let kind = guard
        .get_profile(connection_id)
        .ok_or_else(|| {
            DbError::NotFound(format!(
                "Connection profile for '{}' not found",
                connection_id
            ))
        })?
        .driver
        .clone();
    let conn = guard
        .get_connection(connection_id)
        .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))?
        .clone();
    Ok((conn, kind))
}

/// List stored procedures and functions, optionally limited to one schema.
pub async fn list_procedures(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: Option<&str>,
) -> Result<Vec<ProcedureInfo>, DbError> {
    let (conn, kind) = take_connection(state, connection_id)?;
    let escape = |s: &str| conn.escape_string_literal(s);

    let Some(sql) = list_routines_sql(&kind, schema, &escape) else {
        return Ok(Vec::new());
    };
    let result = conn.execute_query(&sql).await?;
    Ok(map_rows(&result))
}

/// Read a routine's `CREATE` statement.
pub async fn get_procedure_definition(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: &str,
    name: &str,
    kind: Option<RoutineKind>,
) -> Result<String, DbError> {
    let (conn, driver) = take_connection(state, connection_id)?;
    let quote = |s: &str| conn.quote_identifier(s);
    let escape = |s: &str| conn.escape_string_literal(s);

    let queries = definition_queries(&driver, schema, name, kind, &quote, &escape)?;
    let mut last_error: Option<DbError> = None;

    for query in &queries {
        match conn.execute_query(&query.sql).await {
            Ok(result) => {
                if let Some(text) = result
                    .rows
                    .first()
                    .and_then(|row| row.get(query.column))
                    .and_then(as_string)
                {
                    return Ok(text);
                }
            }
            // MySQL answers "PROCEDURE does not exist" for a function, so a
            // failed attempt is only fatal once every candidate has failed.
            Err(e) => last_error = Some(e),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        DbError::NotFound(format!(
            "No definition found for {}",
            if schema.is_empty() {
                name.to_string()
            } else {
                format!("{}.{}", schema, name)
            }
        ))
    }))
}

/// Build the statement that would invoke a routine, without running it.
///
/// Used to show the user what is about to execute. Takes the lock only long
/// enough to clone the driver.
pub fn preview_call(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: &str,
    name: &str,
    kind: RoutineKind,
    args: &[Value],
) -> Result<String, DbError> {
    let (conn, driver) = take_connection(state, connection_id)?;
    let quote = |s: &str| conn.quote_identifier(s);
    let escape = |s: &str| conn.escape_string_literal(s);
    call_sql(&driver, kind, schema, name, args, &quote, &escape)
}

/// Execute a routine with the given positional arguments.
pub async fn execute_procedure(
    state: &Mutex<AppState>,
    connection_id: &str,
    schema: &str,
    name: &str,
    kind: RoutineKind,
    args: &[Value],
) -> Result<QueryResult, DbError> {
    let sql = preview_call(state, connection_id, schema, name, kind, args)?;
    let (conn, _) = take_connection(state, connection_id)?;
    conn.execute_query(&sql).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard SQL quoting, as Postgres and SQLite use.
    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
    /// MySQL backtick quoting, to prove nothing assumes double quotes.
    fn q_mysql(ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
    /// T-SQL bracket quoting.
    fn q_mssql(ident: &str) -> String {
        format!("[{}]", ident.replace(']', "]]"))
    }
    fn esc(v: &str) -> String {
        v.replace('\'', "''")
    }
    /// MySQL also treats backslash as an escape inside literals.
    fn esc_mysql(v: &str) -> String {
        v.replace('\\', "\\\\").replace('\'', "''")
    }

    fn row(cells: &[Option<&str>]) -> Vec<Value> {
        cells
            .iter()
            .map(|c| match c {
                Some(s) => Value::String((*s).to_string()),
                None => Value::Null,
            })
            .collect()
    }

    // ── kind ────────────────────────────────────────────────────────────

    #[test]
    fn routine_kind_reads_either_spelling() {
        assert_eq!(RoutineKind::parse("procedure"), RoutineKind::Procedure);
        assert_eq!(RoutineKind::parse("PROCEDURE"), RoutineKind::Procedure);
        assert_eq!(RoutineKind::parse("function"), RoutineKind::Function);
        // Unknown falls back to function: SELECTing a procedure errors, which
        // is safer than CALLing a function twice.
        assert_eq!(RoutineKind::parse("aggregate"), RoutineKind::Function);
    }

    // ── listing ─────────────────────────────────────────────────────────

    #[test]
    fn drivers_without_routines_have_no_catalogue_query() {
        for driver in [
            DbDriver::Sqlite,
            DbDriver::Turso,
            DbDriver::MongoDb,
            DbDriver::Redis,
        ] {
            assert!(!supports_routines(&driver), "{:?}", driver);
            assert!(
                list_routines_sql(&driver, None, &esc).is_none(),
                "{:?}",
                driver
            );
        }
    }

    #[test]
    fn each_dialect_queries_its_own_catalogue() {
        let pg = list_routines_sql(&DbDriver::Postgres, None, &esc).unwrap();
        assert!(pg.contains("pg_proc"), "{}", pg);

        let my = list_routines_sql(&DbDriver::MySql, None, &esc).unwrap();
        assert!(my.contains("information_schema.ROUTINES"), "{}", my);

        let ms = list_routines_sql(&DbDriver::SqlServer, None, &esc).unwrap();
        assert!(ms.contains("sys.objects"), "{}", ms);

        assert_ne!(pg, my, "two dialects produced identical SQL");
    }

    #[test]
    fn supabase_and_neon_use_the_postgres_catalogue() {
        for driver in [DbDriver::Supabase, DbDriver::Neon] {
            let sql = list_routines_sql(&driver, None, &esc).unwrap();
            assert!(sql.contains("pg_proc"), "{:?}", driver);
        }
    }

    #[test]
    fn a_schema_filter_is_escaped_not_interpolated_raw() {
        let sql = list_routines_sql(&DbDriver::Postgres, Some("o'brien"), &esc).unwrap();
        assert!(sql.contains("n.nspname = 'o''brien'"), "{}", sql);
        // The quote never survives unescaped, which is what would end the
        // literal and let the rest of the name run as SQL.
        assert!(!sql.contains("'o'brien'"), "{}", sql);
    }

    #[test]
    fn an_empty_schema_filter_is_ignored() {
        let sql = list_routines_sql(&DbDriver::MySql, Some("   "), &esc).unwrap();
        assert!(!sql.contains("AND r.ROUTINE_SCHEMA = "), "{}", sql);
    }

    // ── row mapping ─────────────────────────────────────────────────────

    #[test]
    fn rows_map_onto_procedure_info() {
        let result = QueryResult::with_data(
            vec![
                "schema".into(),
                "name".into(),
                "kind".into(),
                "language".into(),
                "return_type".into(),
                "args".into(),
            ],
            vec![row(&[
                Some("public"),
                Some("add"),
                Some("function"),
                Some("plpgsql"),
                Some("integer"),
                Some("(a integer, b integer)"),
            ])],
        );
        let mapped = map_rows(&result);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].name, "add");
        assert_eq!(mapped[0].kind, RoutineKind::Function);
        assert_eq!(mapped[0].language.as_deref(), Some("plpgsql"));
        assert_eq!(mapped[0].argument_signature, "(a integer, b integer)");
        assert_eq!(mapped[0].qualified(), "public.add");
    }

    #[test]
    fn a_bare_argument_list_gains_its_parentheses() {
        // MySQL's GROUP_CONCAT hands back a bare list.
        assert_eq!(
            parenthesise(Some("IN a int, OUT b int".to_string())),
            "(IN a int, OUT b int)"
        );
        // Postgres already parenthesises; it must not be doubled.
        assert_eq!(parenthesise(Some("(a integer)".to_string())), "(a integer)");
        // SQL Server reports nothing at all.
        assert_eq!(parenthesise(None), "()");
    }

    #[test]
    fn missing_cells_do_not_panic() {
        let result = QueryResult::with_data(vec!["schema".into()], vec![row(&[Some("public")])]);
        let mapped = map_rows(&result);
        assert_eq!(mapped[0].schema, "public");
        assert_eq!(mapped[0].name, "");
        assert_eq!(mapped[0].argument_signature, "()");
        assert_eq!(mapped[0].kind, RoutineKind::Function);
    }

    // ── definitions ─────────────────────────────────────────────────────

    #[test]
    fn postgres_reads_the_definition_in_one_query() {
        let queries =
            definition_queries(&DbDriver::Postgres, "public", "add", None, &q, &esc).unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].column, 0);
        assert!(queries[0].sql.contains("pg_get_functiondef"));
        assert!(queries[0].sql.contains("n.nspname = 'public'"));
    }

    #[test]
    fn mysql_tries_the_known_kind_first() {
        let as_function = definition_queries(
            &DbDriver::MySql,
            "app",
            "total",
            Some(RoutineKind::Function),
            &q_mysql,
            &esc_mysql,
        )
        .unwrap();
        assert!(as_function[0].sql.starts_with("SHOW CREATE FUNCTION"));
        assert!(as_function[1].sql.starts_with("SHOW CREATE PROCEDURE"));
        // The create statement is the third column of SHOW CREATE.
        assert!(as_function.iter().all(|q| q.column == 2));

        let unknown =
            definition_queries(&DbDriver::MySql, "app", "total", None, &q_mysql, &esc).unwrap();
        assert!(unknown[0].sql.starts_with("SHOW CREATE PROCEDURE"));
    }

    #[test]
    fn definition_identifiers_are_quoted_per_dialect() {
        let my = definition_queries(
            &DbDriver::MySql,
            "my db",
            "we`ird",
            None,
            &q_mysql,
            &esc_mysql,
        )
        .unwrap();
        assert!(my[0].sql.contains("`my db`.`we``ird`"), "{}", my[0].sql);

        let ms = definition_queries(&DbDriver::SqlServer, "dbo", "usp_run", None, &q_mssql, &esc)
            .unwrap();
        assert!(ms[0].sql.contains("[dbo].[usp_run]"), "{}", ms[0].sql);
    }

    #[test]
    fn a_driver_without_routines_reports_rather_than_guessing() {
        assert!(definition_queries(&DbDriver::Sqlite, "", "x", None, &q, &esc).is_err());
    }

    // ── argument binding ────────────────────────────────────────────────

    #[test]
    fn arguments_render_as_typed_literals() {
        assert_eq!(render_arg(&Value::Null, &esc), "NULL");
        assert_eq!(render_arg(&Value::Bool(true), &esc), "TRUE");
        assert_eq!(render_arg(&Value::Bool(false), &esc), "FALSE");
        assert_eq!(render_arg(&serde_json::json!(42), &esc), "42");
        assert_eq!(render_arg(&serde_json::json!(1.5), &esc), "1.5");
        assert_eq!(render_arg(&serde_json::json!("hi"), &esc), "'hi'");
    }

    #[test]
    fn a_quote_in_an_argument_cannot_end_the_literal() {
        assert_eq!(
            render_arg(&serde_json::json!("o'brien"), &esc),
            "'o''brien'"
        );
        // MySQL also honours backslash escapes, so a lone backslash before a
        // quote would otherwise smuggle the quote through.
        assert_eq!(
            render_arg(&serde_json::json!("a\\'; DROP"), &esc_mysql),
            "'a\\\\''; DROP'"
        );
    }

    #[test]
    fn structured_arguments_go_over_as_json_text() {
        let v = serde_json::json!({"a": 1});
        assert_eq!(render_arg(&v, &esc), "'{\"a\":1}'");
        let arr = serde_json::json!([1, 2]);
        assert_eq!(render_arg(&arr, &esc), "'[1,2]'");
    }

    // ── call syntax ─────────────────────────────────────────────────────

    #[test]
    fn postgres_selects_functions_and_calls_procedures() {
        let args = vec![serde_json::json!(1), serde_json::json!("x")];
        let f = call_sql(
            &DbDriver::Postgres,
            RoutineKind::Function,
            "public",
            "add",
            &args,
            &q,
            &esc,
        )
        .unwrap();
        assert_eq!(f, "SELECT \"public\".\"add\"(1, 'x')");

        let p = call_sql(
            &DbDriver::Postgres,
            RoutineKind::Procedure,
            "public",
            "refresh",
            &[],
            &q,
            &esc,
        )
        .unwrap();
        assert_eq!(p, "CALL \"public\".\"refresh\"()");
    }

    #[test]
    fn mysql_uses_backticks_and_call() {
        let sql = call_sql(
            &DbDriver::MySql,
            RoutineKind::Procedure,
            "app",
            "do thing",
            &[serde_json::json!(7)],
            &q_mysql,
            &esc_mysql,
        )
        .unwrap();
        assert_eq!(sql, "CALL `app`.`do thing`(7)");
    }

    #[test]
    fn sqlserver_execs_procedures_without_parentheses() {
        let sql = call_sql(
            &DbDriver::SqlServer,
            RoutineKind::Procedure,
            "dbo",
            "usp_run",
            &[serde_json::json!(1), serde_json::json!("a")],
            &q_mssql,
            &esc,
        )
        .unwrap();
        assert_eq!(sql, "EXEC [dbo].[usp_run] 1, 'a'");

        // No arguments means no trailing space to confuse the parser.
        let bare = call_sql(
            &DbDriver::SqlServer,
            RoutineKind::Procedure,
            "dbo",
            "usp_run",
            &[],
            &q_mssql,
            &esc,
        )
        .unwrap();
        assert_eq!(bare, "EXEC [dbo].[usp_run]");

        // Functions are still selected, with parentheses.
        let f = call_sql(
            &DbDriver::SqlServer,
            RoutineKind::Function,
            "dbo",
            "fn_total",
            &[],
            &q_mssql,
            &esc,
        )
        .unwrap();
        assert_eq!(f, "SELECT [dbo].[fn_total]()");
    }

    #[test]
    fn an_empty_schema_yields_an_unqualified_call() {
        let sql = call_sql(
            &DbDriver::MySql,
            RoutineKind::Procedure,
            "",
            "cleanup",
            &[],
            &q_mysql,
            &esc_mysql,
        )
        .unwrap();
        assert_eq!(sql, "CALL `cleanup`()");
    }

    #[test]
    fn a_routine_name_needing_quotes_is_quoted() {
        let sql = call_sql(
            &DbDriver::Postgres,
            RoutineKind::Function,
            "public",
            "select",
            &[],
            &q,
            &esc,
        )
        .unwrap();
        assert_eq!(sql, "SELECT \"public\".\"select\"()");

        // An embedded quote is doubled rather than closing the identifier.
        let nasty = call_sql(
            &DbDriver::Postgres,
            RoutineKind::Function,
            "public",
            "we\"ird",
            &[],
            &q,
            &esc,
        )
        .unwrap();
        assert_eq!(nasty, "SELECT \"public\".\"we\"\"ird\"()");
    }

    #[test]
    fn drivers_without_routines_refuse_to_build_a_call() {
        for driver in [DbDriver::Sqlite, DbDriver::MongoDb, DbDriver::Redis] {
            assert!(
                call_sql(&driver, RoutineKind::Function, "", "f", &[], &q, &esc).is_err(),
                "{:?}",
                driver
            );
        }
    }
}
