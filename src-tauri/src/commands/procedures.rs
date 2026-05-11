//! Stored procedures / functions commands
//!
//! Lists, inspects, and executes server-side routines (procedures + functions)
//! for the active connection. Drivers that don't support stored routines
//! (SQLite, MongoDB) simply return an empty list so the UI can render an
//! empty state without error.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::drivers::{DatabaseDriver, QueryResult};
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

/// Metadata describing a single stored procedure or function.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcedureInfo {
    pub schema: String,
    pub name: String,
    /// "procedure" | "function"
    pub kind: String,
    /// Implementation language (PG: plpgsql/sql/c; others: None)
    pub language: Option<String>,
    pub return_type: Option<String>,
    /// Fully rendered argument signature, e.g. "(a integer, b text)"
    pub argument_signature: String,
}

fn resolve_driver(state: &AppState, connection_id: &str) -> Result<DbDriver, DbError> {
    let profile = state
        .get_profile(connection_id)
        .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?;
    Ok(profile.driver.clone())
}

fn take_connection(
    state: &State<'_, Mutex<AppState>>,
    connection_id: &str,
) -> Result<(std::sync::Arc<dyn DatabaseDriver>, DbDriver), DbError> {
    let state = state.lock().unwrap();
    let driver = resolve_driver(&state, connection_id)?;
    let conn = state
        .get_connection(connection_id)
        .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
        .clone();
    Ok((conn, driver))
}

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

/// List stored procedures / functions for a connection, optionally filtered by schema.
#[tauri::command]
pub async fn list_procedures(
    connection_id: String,
    schema: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ProcedureInfo>, DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    if driver.is_postgres_compatible() {
        // pg_get_function_arguments / pg_get_function_result render signatures
        // with proper type names and defaults, which information_schema cannot.
        // prokind: 'f' = function, 'p' = procedure, 'a' = aggregate, 'w' = window
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
        let sql = match &schema {
            Some(s) => format!(
                "{} AND n.nspname = '{}' ORDER BY n.nspname, p.proname",
                base,
                s.replace('\'', "''")
            ),
            None => format!("{} ORDER BY n.nspname, p.proname", base),
        };
        let res = conn.execute_query(&sql).await?;
        let mut out = Vec::with_capacity(res.rows.len());
        for row in res.rows {
            out.push(ProcedureInfo {
                schema: row.first().and_then(as_string).unwrap_or_default(),
                name: row.get(1).and_then(as_string).unwrap_or_default(),
                kind: row
                    .get(2)
                    .and_then(as_string)
                    .unwrap_or_else(|| "function".to_string()),
                language: row.get(3).and_then(as_string),
                return_type: row.get(4).and_then(as_string),
                argument_signature: row
                    .get(5)
                    .and_then(as_string)
                    .unwrap_or_else(|| "()".to_string()),
            });
        }
        return Ok(out);
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
            if let Some(s) = &schema {
                sql.push_str(&format!(" AND r.ROUTINE_SCHEMA = '{}'", s.replace('\'', "''")));
            }
            sql.push_str(" GROUP BY r.ROUTINE_SCHEMA, r.ROUTINE_NAME, r.ROUTINE_TYPE, r.DTD_IDENTIFIER");
            sql.push_str(" ORDER BY r.ROUTINE_SCHEMA, r.ROUTINE_NAME");
            let res = conn.execute_query(&sql).await?;
            let mut out = Vec::with_capacity(res.rows.len());
            for row in res.rows {
                let args = row.get(5).and_then(as_string).unwrap_or_default();
                out.push(ProcedureInfo {
                    schema: row.first().and_then(as_string).unwrap_or_default(),
                    name: row.get(1).and_then(as_string).unwrap_or_default(),
                    kind: row
                        .get(2)
                        .and_then(as_string)
                        .unwrap_or_else(|| "function".to_string()),
                    language: None,
                    return_type: row.get(4).and_then(as_string),
                    argument_signature: format!("({})", args),
                });
            }
            Ok(out)
        }
        DbDriver::SqlServer => {
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
            if let Some(s) = &schema {
                sql.push_str(&format!(
                    " AND SCHEMA_NAME(o.schema_id) = '{}'",
                    s.replace('\'', "''")
                ));
            }
            sql.push_str(" ORDER BY schema_name, o.name");
            let res = conn.execute_query(&sql).await?;
            let mut out = Vec::with_capacity(res.rows.len());
            for row in res.rows {
                out.push(ProcedureInfo {
                    schema: row.first().and_then(as_string).unwrap_or_default(),
                    name: row.get(1).and_then(as_string).unwrap_or_default(),
                    kind: row
                        .get(2)
                        .and_then(as_string)
                        .unwrap_or_else(|| "procedure".to_string()),
                    language: None,
                    return_type: None,
                    argument_signature: "()".to_string(),
                });
            }
            Ok(out)
        }
        // SQLite, Turso, MongoDB: no stored procedure concept.
        _ => Ok(Vec::new()),
    }
}

/// Retrieve the CREATE statement / body of a procedure or function.
#[tauri::command]
pub async fn get_procedure_definition(
    connection_id: String,
    schema: String,
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    if driver.is_postgres_compatible() {
        // pg_get_functiondef requires a regprocedure; when overloads exist we
        // pick the lowest oid to keep this call simple.
        let sql = format!(
            r#"SELECT pg_get_functiondef(p.oid)
               FROM pg_proc p
               JOIN pg_namespace n ON n.oid = p.pronamespace
               WHERE n.nspname = '{}' AND p.proname = '{}'
               ORDER BY p.oid
               LIMIT 1"#,
            schema.replace('\'', "''"),
            name.replace('\'', "''"),
        );
        let res = conn.execute_query(&sql).await?;
        let row = res
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| DbError::NotFound(format!("{}.{} not found", schema, name)))?;
        return row
            .into_iter()
            .next()
            .and_then(|v| as_string(&v))
            .ok_or_else(|| DbError::QueryError("Empty function definition".to_string()));
    }

    match driver {
        DbDriver::MySql => {
            // Try SHOW CREATE PROCEDURE first, fall back to FUNCTION.
            let qualified = format!("`{}`.`{}`", schema.replace('`', "``"), name.replace('`', "``"));
            let proc_sql = format!("SHOW CREATE PROCEDURE {}", qualified);
            if let Ok(res) = conn.execute_query(&proc_sql).await {
                if let Some(row) = res.rows.into_iter().next() {
                    // SHOW CREATE PROCEDURE columns: Procedure, sql_mode, Create Procedure, ...
                    if let Some(def) = row.get(2).and_then(as_string) {
                        return Ok(def);
                    }
                }
            }
            let func_sql = format!("SHOW CREATE FUNCTION {}", qualified);
            let res = conn.execute_query(&func_sql).await?;
            let row = res
                .rows
                .into_iter()
                .next()
                .ok_or_else(|| DbError::NotFound(format!("{}.{} not found", schema, name)))?;
            row.get(2)
                .and_then(as_string)
                .ok_or_else(|| DbError::QueryError("Empty routine definition".to_string()))
        }
        _ => Err(DbError::QueryError(
            "Routine definition not supported for this driver".to_string(),
        )),
    }
}

/// Execute a procedure or function with the given positional arguments.
#[tauri::command]
pub async fn execute_procedure(
    connection_id: String,
    schema: String,
    name: String,
    args: Vec<Value>,
    state: State<'_, Mutex<AppState>>,
) -> Result<QueryResult, DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    let rendered_args: Vec<String> = args.iter().map(render_arg).collect();
    let arg_list = rendered_args.join(", ");

    let sql = if driver.is_postgres_compatible() {
        // PG: plain functions are invoked via SELECT; procedures use CALL.
        // We default to SELECT here because list_procedures surfaces both and
        // CALL on a function errors with a clear message the user can act on.
        format!("SELECT \"{}\".\"{}\"({})", schema, name, arg_list)
    } else {
        match driver {
            DbDriver::MySql => format!("CALL `{}`.`{}`({})", schema, name, arg_list),
            DbDriver::SqlServer => format!("EXEC [{}].[{}] {}", schema, name, arg_list),
            _ => {
                return Err(DbError::QueryError(
                    "Executing routines is not supported for this driver".to_string(),
                ));
            }
        }
    };

    conn.execute_query(&sql).await
}

/// Render a JSON value as a SQL literal suitable for inline argument lists.
fn render_arg(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        // Arrays/objects: pass as a JSON string literal; the server casts as needed.
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}
