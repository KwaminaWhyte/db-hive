//! Schema migration Tauri commands.
//!
//! Three commands: `compute_schema_diff`, `generate_migration`, and
//! `apply_migration`. The first two are pure/advisory; the third executes
//! statements against the live connection inside an optional transaction.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::drivers::DatabaseDriver;
use crate::migrations::diff::TableWithFks;
use crate::migrations::{compute_diff, generate_migration_sql, SchemaDiff};
use crate::models::DbError;
use crate::state::AppState;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub executed: u32,
    pub succeeded: u32,
    pub failed_statement: Option<String>,
    pub error: Option<String>,
}

fn driver_of(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<Arc<dyn DatabaseDriver>, DbError> {
    let guard = state.lock().unwrap();
    guard
        .connections
        .get(connection_id)
        .cloned()
        .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))
}

fn profile_driver(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<crate::models::DbDriver, DbError> {
    let guard = state.lock().unwrap();
    let profile = guard
        .connection_profiles
        .get(connection_id)
        .ok_or_else(|| {
            DbError::NotFound(format!(
                "Connection profile for '{}' not found",
                connection_id
            ))
        })?;
    Ok(profile.driver.clone())
}

async fn collect_tables(
    driver: &Arc<dyn DatabaseDriver>,
    schema: &str,
) -> Result<Vec<TableWithFks>, DbError> {
    let tables = driver.get_tables(schema).await?;
    let fks = driver.get_foreign_keys(schema).await.unwrap_or_default();

    let mut out = Vec::with_capacity(tables.len());
    for t in tables {
        if t.is_view() {
            continue;
        }
        let ts = driver.get_table_schema(schema, &t.name).await?;
        let table_fks: Vec<_> = fks
            .iter()
            .filter(|f| f.table == t.name && f.schema == schema)
            .cloned()
            .collect();
        out.push(TableWithFks {
            schema: ts,
            foreign_keys: table_fks,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn compute_schema_diff(
    source_connection_id: String,
    target_connection_id: String,
    schema: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<SchemaDiff, DbError> {
    let src_driver = driver_of(&state, &source_connection_id)?;
    let tgt_driver = driver_of(&state, &target_connection_id)?;

    let src = collect_tables(&src_driver, &schema).await?;
    let tgt = collect_tables(&tgt_driver, &schema).await?;

    Ok(compute_diff(&src, &tgt))
}

#[tauri::command]
pub async fn generate_migration(
    diff: SchemaDiff,
    target_connection_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, DbError> {
    let driver = profile_driver(&state, &target_connection_id)?;
    generate_migration_sql(&diff, &driver)
}

#[tauri::command]
pub async fn apply_migration(
    connection_id: String,
    statements: Vec<String>,
    use_transaction: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<ApplyResult, DbError> {
    let driver = driver_of(&state, &connection_id)?;

    let mut executed: u32 = 0;
    let mut succeeded: u32 = 0;

    if use_transaction {
        if let Err(e) = driver.execute_query("BEGIN").await {
            return Ok(ApplyResult {
                executed: 0,
                succeeded: 0,
                failed_statement: Some("BEGIN".to_string()),
                error: Some(e.to_string()),
            });
        }
    }

    for stmt in &statements {
        // Skip comment-only no-op markers emitted by the generator.
        if stmt.trim_start().starts_with("--") {
            continue;
        }
        executed += 1;
        match driver.execute_query(stmt).await {
            Ok(_) => succeeded += 1,
            Err(e) => {
                if use_transaction {
                    let _ = driver.execute_query("ROLLBACK").await;
                }
                return Ok(ApplyResult {
                    executed,
                    succeeded,
                    failed_statement: Some(stmt.clone()),
                    error: Some(e.to_string()),
                });
            }
        }
    }

    if use_transaction {
        if let Err(e) = driver.execute_query("COMMIT").await {
            return Ok(ApplyResult {
                executed,
                succeeded,
                failed_statement: Some("COMMIT".to_string()),
                error: Some(e.to_string()),
            });
        }
    }

    Ok(ApplyResult {
        executed,
        succeeded,
        failed_statement: None,
        error: None,
    })
}
