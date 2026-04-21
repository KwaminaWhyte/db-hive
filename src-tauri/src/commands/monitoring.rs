//! Monitoring commands
//!
//! Tauri commands that surface database-side session/activity metrics by
//! querying server system views (pg_stat_activity, SHOW PROCESSLIST, etc).
//! Drivers that do not expose session metadata return InvalidInput so the
//! frontend can render a graceful "not supported" message.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::drivers::DatabaseDriver;
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

/// A snapshot of a single active session/query on the database server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveQuery {
    pub pid: i64,
    pub user: Option<String>,
    pub database: Option<String>,
    pub client_addr: Option<String>,
    pub query_start: Option<String>,
    pub state: Option<String>,
    pub query_text: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Aggregate server-side stats for charting.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub numeric_connections: i64,
    pub active_connections: i64,
    pub cache_hit_ratio: Option<f64>,
    pub transactions_per_sec: Option<f64>,
    pub deadlocks: Option<i64>,
}

/// Resolve the driver type for an active connection via its profile.
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

fn not_supported(driver: &DbDriver) -> DbError {
    DbError::InvalidInput(format!(
        "Not supported for this driver ({:?})",
        driver
    ))
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
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

fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("...");
        out
    }
}

/// Retrieve the list of active queries / sessions from the database server.
#[tauri::command]
pub async fn get_active_queries(
    connection_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ActiveQuery>, DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    if driver.is_postgres_compatible() {
        // pg_stat_activity hides other users' query text unless the caller is
        // a superuser or has the pg_read_all_stats role; callers may see NULL
        // query_text for sessions they don't own.
        let sql = r#"
            SELECT pid,
                   usename,
                   datname,
                   client_addr::text,
                   to_char(query_start, 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS query_start,
                   state,
                   query,
                   CASE WHEN query_start IS NOT NULL
                        THEN (EXTRACT(EPOCH FROM (now() - query_start)) * 1000)::bigint
                        ELSE NULL END AS duration_ms
            FROM pg_stat_activity
            WHERE pid <> pg_backend_pid()
            ORDER BY query_start NULLS LAST
        "#;
        let res = conn.execute_query(sql).await?;
        let mut out = Vec::with_capacity(res.rows.len());
        for row in res.rows {
            out.push(ActiveQuery {
                pid: row.first().and_then(as_i64).unwrap_or(0),
                user: row.get(1).and_then(as_string),
                database: row.get(2).and_then(as_string),
                client_addr: row.get(3).and_then(as_string),
                query_start: row.get(4).and_then(as_string),
                state: row.get(5).and_then(as_string),
                query_text: row
                    .get(6)
                    .and_then(as_string)
                    .map(|q| truncate(q, 500)),
                duration_ms: row.get(7).and_then(as_i64),
            });
        }
        return Ok(out);
    }

    match driver {
        DbDriver::MySql => {
            let sql = "SELECT ID, USER, DB, HOST, NULL AS query_start, COMMAND, INFO, TIME*1000 AS duration_ms FROM information_schema.PROCESSLIST";
            let res = conn.execute_query(sql).await?;
            let mut out = Vec::with_capacity(res.rows.len());
            for row in res.rows {
                out.push(ActiveQuery {
                    pid: row.first().and_then(as_i64).unwrap_or(0),
                    user: row.get(1).and_then(as_string),
                    database: row.get(2).and_then(as_string),
                    client_addr: row.get(3).and_then(as_string),
                    query_start: row.get(4).and_then(as_string),
                    state: row.get(5).and_then(as_string),
                    query_text: row
                        .get(6)
                        .and_then(as_string)
                        .map(|q| truncate(q, 500)),
                    duration_ms: row.get(7).and_then(as_i64),
                });
            }
            Ok(out)
        }
        _ => Err(not_supported(&driver)),
    }
}

/// Cancel / kill a running query by session pid.
#[tauri::command]
pub async fn kill_query(
    connection_id: String,
    pid: i64,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    if driver.is_postgres_compatible() {
        let sql = format!("SELECT pg_cancel_backend({})", pid);
        conn.execute_query(&sql).await?;
        return Ok(());
    }

    match driver {
        DbDriver::MySql => {
            let sql = format!("KILL QUERY {}", pid);
            conn.execute_query(&sql).await?;
            Ok(())
        }
        _ => Err(not_supported(&driver)),
    }
}

/// Retrieve aggregate server metrics for charting.
#[tauri::command]
pub async fn get_server_stats(
    connection_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ServerStats, DbError> {
    let (conn, driver) = take_connection(&state, &connection_id)?;

    if driver.is_postgres_compatible() {
        // Aggregate across all databases; individual databases are aggregated
        // so the chart represents server-wide load, not just the current db.
        let sql = r#"
            SELECT
                (SELECT count(*) FROM pg_stat_activity)::bigint AS total_conns,
                (SELECT count(*) FROM pg_stat_activity WHERE state = 'active')::bigint AS active_conns,
                COALESCE(SUM(blks_hit)::float8 / NULLIF(SUM(blks_hit + blks_read), 0), 0) AS cache_hit_ratio,
                COALESCE(SUM(xact_commit + xact_rollback), 0)::bigint AS xact_total,
                COALESCE(SUM(deadlocks), 0)::bigint AS deadlocks
            FROM pg_stat_database
        "#;
        let res = conn.execute_query(sql).await?;
        let row = res
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| DbError::QueryError("pg_stat_database returned no rows".to_string()))?;
        let xact_total = row.get(3).and_then(as_i64).unwrap_or(0);
        return Ok(ServerStats {
            numeric_connections: row.first().and_then(as_i64).unwrap_or(0),
            active_connections: row.get(1).and_then(as_i64).unwrap_or(0),
            cache_hit_ratio: row.get(2).and_then(as_f64),
            // xact_total is a cumulative counter; the frontend computes a
            // per-second rate by diffing against the previous sample.
            transactions_per_sec: Some(xact_total as f64),
            deadlocks: Some(row.get(4).and_then(as_i64).unwrap_or(0)),
        });
    }

    match driver {
        DbDriver::MySql => {
            let sql = "SHOW GLOBAL STATUS WHERE Variable_name IN ('Threads_connected','Threads_running','Com_commit','Com_rollback','Innodb_deadlocks')";
            let res = conn.execute_query(sql).await?;
            let mut threads_connected = 0i64;
            let mut threads_running = 0i64;
            let mut commits = 0i64;
            let mut rollbacks = 0i64;
            let mut deadlocks = 0i64;
            for row in res.rows {
                let name = row.first().and_then(as_string).unwrap_or_default();
                let val = row.get(1).and_then(as_i64).unwrap_or(0);
                match name.as_str() {
                    "Threads_connected" => threads_connected = val,
                    "Threads_running" => threads_running = val,
                    "Com_commit" => commits = val,
                    "Com_rollback" => rollbacks = val,
                    "Innodb_deadlocks" => deadlocks = val,
                    _ => {}
                }
            }
            Ok(ServerStats {
                numeric_connections: threads_connected,
                active_connections: threads_running,
                cache_hit_ratio: None,
                transactions_per_sec: Some((commits + rollbacks) as f64),
                deadlocks: Some(deadlocks),
            })
        }
        _ => Err(not_supported(&driver)),
    }
}
