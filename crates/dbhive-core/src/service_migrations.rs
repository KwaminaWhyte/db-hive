//! Shell-agnostic schema-migration operations.
//!
//! Lifted verbatim from the bodies of the `#[tauri::command]` handlers in
//! `src-tauri/src/commands/migrations.rs`, with `State<'_, Mutex<AppState>>`
//! replaced by a plain `&Mutex<AppState>` so both shells run the same code.
//!
//! Three operations, matching the three-step wizard the UI drives:
//!
//! * [`compute_schema_diff`] reads both live connections and produces a
//!   structured diff. Advisory; touches nothing.
//! * [`generate_migration`] turns that diff into SQL for the *target's*
//!   dialect. Pure once the target's driver kind has been read from state.
//! * [`apply_migration`] executes statements against the target, optionally
//!   inside a transaction. This is the only destructive one.
//!
//! `apply_migration` reports failure through `Ok(ApplyResult)` rather than
//! `Err`: a migration that dies on statement 7 of 12 is a partial outcome the
//! caller has to show counts for, not a plain error. `Err` is reserved for
//! never-started conditions (no such connection).

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::drivers::DatabaseDriver;
use crate::migrations::diff::TableWithFks;
use crate::migrations::{compute_diff, generate_migration_sql, SchemaDiff};
use crate::models::{DbDriver, DbError};
use crate::state::AppState;

/// Outcome of [`apply_migration`].
///
/// `executed` counts statements actually sent to the server, so it excludes
/// the comment-only markers the generator emits for changes it cannot express
/// as DDL. `succeeded` counts the ones that came back clean.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub executed: u32,
    pub succeeded: u32,
    pub failed_statement: Option<String>,
    pub error: Option<String>,
}

impl ApplyResult {
    /// Whether the run finished without a failing statement.
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// Clone the live driver for a connection out of state.
///
/// The `Arc` is cloned and the guard dropped immediately: every caller here
/// awaits, and a `std::sync::MutexGuard` cannot be held across an await.
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

/// The driver *kind* of a saved profile, which decides the SQL dialect.
///
/// Read from the profile rather than the live connection because generating
/// SQL must work for a target whose connection is only about to be opened.
fn profile_driver(state: &Mutex<AppState>, connection_id: &str) -> Result<DbDriver, DbError> {
    let guard = state.lock().unwrap();
    let profile = guard.connection_profiles.get(connection_id).ok_or_else(|| {
        DbError::NotFound(format!(
            "Connection profile for '{}' not found",
            connection_id
        ))
    })?;
    Ok(profile.driver.clone())
}

/// Read every base table in `schema`, with its columns, indexes and FKs.
///
/// Views are skipped: the diff only knows how to emit table DDL, and a view
/// reported as an "added table" would generate a CREATE TABLE that silently
/// diverges from the source.
async fn collect_tables(
    driver: &Arc<dyn DatabaseDriver>,
    schema: &str,
) -> Result<Vec<TableWithFks>, DbError> {
    let tables = driver.get_tables(schema).await?;
    // Foreign keys are fetched once for the whole schema and partitioned
    // below; asking per table would be N round trips. Drivers that do not
    // implement it (SQLite variants, Redis) return an error rather than an
    // empty list, and a missing FK list must not fail the whole diff.
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

/// Diff `schema` as it exists on the source against the target.
///
/// Both connections must already be open. Applying the generated SQL to the
/// target should make it look like the source.
pub async fn compute_schema_diff(
    state: &Mutex<AppState>,
    source_connection_id: &str,
    target_connection_id: &str,
    schema: &str,
) -> Result<SchemaDiff, DbError> {
    let src_driver = driver_of(state, source_connection_id)?;
    let tgt_driver = driver_of(state, target_connection_id)?;

    let src = collect_tables(&src_driver, schema).await?;
    let tgt = collect_tables(&tgt_driver, schema).await?;

    Ok(compute_diff(&src, &tgt))
}

/// Render a diff as statements in the target's dialect.
///
/// Synchronous: unlike the Tauri command it was lifted from, nothing here
/// does IO, so there is nothing to await.
pub fn generate_migration(
    state: &Mutex<AppState>,
    diff: &SchemaDiff,
    target_connection_id: &str,
) -> Result<Vec<String>, DbError> {
    let driver = profile_driver(state, target_connection_id)?;
    generate_migration_sql(diff, &driver)
}

/// A statement the generator emitted as a comment because it could not
/// express the change as DDL. Sending it would be a no-op at best.
pub fn is_noop_statement(statement: &str) -> bool {
    statement.trim_start().starts_with("--")
}

/// Execute `statements` against `connection_id`.
///
/// With `use_transaction` the run is wrapped in BEGIN/COMMIT and rolled back
/// on the first failure, so the target is left untouched. Without it, earlier
/// statements stay applied and the returned counts are the only record of how
/// far it got.
pub async fn apply_migration(
    state: &Mutex<AppState>,
    connection_id: &str,
    statements: &[String],
    use_transaction: bool,
) -> Result<ApplyResult, DbError> {
    let driver = driver_of(state, connection_id)?;

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

    for stmt in statements {
        if is_noop_statement(stmt) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_markers_are_noops() {
        assert!(is_noop_statement("-- cannot drop column on this dialect"));
        assert!(is_noop_statement("   \n-- indented marker"));
        assert!(!is_noop_statement("ALTER TABLE t ADD COLUMN a int"));
        // A trailing comment is not a comment-only statement.
        assert!(!is_noop_statement("DROP TABLE t -- careful"));
    }

    #[test]
    fn apply_result_success_is_absence_of_error() {
        let ok = ApplyResult {
            executed: 3,
            succeeded: 3,
            ..Default::default()
        };
        assert!(ok.is_success());
        let bad = ApplyResult {
            executed: 3,
            succeeded: 2,
            failed_statement: Some("DROP TABLE t".into()),
            error: Some("boom".into()),
        };
        assert!(!bad.is_success());
    }

    #[test]
    fn unknown_connection_is_not_found() {
        let state = Mutex::new(AppState::new());
        // `Arc<dyn DatabaseDriver>` is not Debug, so the Ok side cannot be
        // unwrapped; match instead.
        match driver_of(&state, "nope") {
            Err(DbError::NotFound(_)) => {}
            _ => panic!("expected NotFound"),
        }
        let err = profile_driver(&state, "nope").unwrap_err();
        assert!(matches!(err, DbError::NotFound(_)));
    }
}
