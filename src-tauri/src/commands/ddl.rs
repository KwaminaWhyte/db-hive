//! DDL (Data Definition Language) commands
//!
//! Tauri commands for creating, altering, and dropping database objects.

use crate::ddl::get_ddl_generator;
use crate::models::{
    ddl::{AlterTableDefinition, DdlResult, DropTableDefinition, TableDefinition},
    DbError,
};
use crate::state::AppState;
use std::sync::Mutex;
use tauri::State;

/// Preview CREATE TABLE SQL without executing it
///
/// Generates the SQL statement(s) for creating a table based on the provided
/// table definition, but does not execute them. This allows users to review
/// the generated SQL before applying changes.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `table` - Table definition with columns, constraints, etc.
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the generated SQL and a success message.
#[tauri::command]
pub async fn preview_create_table(
    connection_id: String,
    table: TableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    let state_guard = state.lock().unwrap();

    // Verify connection exists
    if !state_guard.connections.contains_key(&connection_id) {
        return Err(DbError::NotFound(format!("Connection '{}' not found", connection_id)));
    }

    // Get the connection profile to determine the database driver
    let profile = state_guard
        .connection_profiles
        .get(&connection_id)
        .ok_or_else(|| DbError::NotFound(format!("Connection profile for '{}' not found", connection_id)))?;

    // Get the appropriate DDL generator for this database
    let generator = get_ddl_generator(&profile.driver)?;

    // Generate SQL
    generator.generate_create_table(&table)
}

/// Create a new table
///
/// Generates and executes SQL statement(s) to create a new table with the
/// specified structure, including columns, primary keys, foreign keys,
/// unique constraints, and check constraints.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `table` - Table definition with columns, constraints, etc.
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the executed SQL and a success message.
#[tauri::command]
pub async fn create_table(
    connection_id: String,
    table: TableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    // First preview to get the SQL
    let preview_result = preview_create_table(connection_id.clone(), table, state.clone()).await?;

    // Execute each SQL statement
    let driver = {
        let state_guard = state.lock().unwrap();
        let driver = state_guard
            .connections
            .get(&connection_id)
            .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))?
            .clone();
        driver
    };

    for sql in &preview_result.sql {
        driver.execute_query(sql).await?;
    }

    Ok(preview_result)
}

/// Preview ALTER TABLE SQL without executing it
///
/// Generates the SQL statement(s) for altering a table based on the provided
/// alteration operations, but does not execute them.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `alter` - Table alteration definition with operations
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the generated SQL and a success message.
#[tauri::command]
pub async fn preview_alter_table(
    connection_id: String,
    alter: AlterTableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    let state_guard = state.lock().unwrap();

    // Verify connection exists
    if !state_guard.connections.contains_key(&connection_id) {
        return Err(DbError::NotFound(format!("Connection '{}' not found", connection_id)));
    }

    // Get the connection profile to determine the database driver
    let profile = state_guard
        .connection_profiles
        .get(&connection_id)
        .ok_or_else(|| DbError::NotFound(format!("Connection profile for '{}' not found", connection_id)))?;

    let generator = get_ddl_generator(&profile.driver)?;

    generator.generate_alter_table(&alter)
}

/// Alter an existing table
///
/// Generates and executes SQL statement(s) to modify an existing table structure.
/// Supported operations include adding/dropping columns, renaming columns,
/// changing column types, and modifying constraints.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `alter` - Table alteration definition with operations
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the executed SQL and a success message.
#[tauri::command]
pub async fn alter_table(
    connection_id: String,
    alter: AlterTableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    let preview_result = preview_alter_table(connection_id.clone(), alter, state.clone()).await?;

    let driver = {
        let state_guard = state.lock().unwrap();
        let driver = state_guard
            .connections
            .get(&connection_id)
            .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))?
            .clone();
        driver
    };

    for sql in &preview_result.sql {
        driver.execute_query(sql).await?;
    }

    Ok(preview_result)
}

/// Preview DROP TABLE SQL without executing it
///
/// Generates the SQL statement for dropping a table, but does not execute it.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `drop` - Table drop definition
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the generated SQL and a success message.
#[tauri::command]
pub async fn preview_drop_table(
    connection_id: String,
    drop: DropTableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    let state_guard = state.lock().unwrap();

    // Verify connection exists
    if !state_guard.connections.contains_key(&connection_id) {
        return Err(DbError::NotFound(format!("Connection '{}' not found", connection_id)));
    }

    // Get the connection profile to determine the database driver
    let profile = state_guard
        .connection_profiles
        .get(&connection_id)
        .ok_or_else(|| DbError::NotFound(format!("Connection profile for '{}' not found", connection_id)))?;

    let generator = get_ddl_generator(&profile.driver)?;

    generator.generate_drop_table(&drop)
}

/// Drop a table
///
/// Generates and executes SQL statement to drop a table from the database.
/// Can optionally cascade to drop dependent objects.
///
/// # Arguments
///
/// * `connection_id` - ID of the active connection
/// * `drop` - Table drop definition
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `DdlResult` with the executed SQL and a success message.
#[tauri::command]
pub async fn drop_table(
    connection_id: String,
    drop: DropTableDefinition,
    state: State<'_, Mutex<AppState>>,
) -> Result<DdlResult, DbError> {
    let preview_result = preview_drop_table(connection_id.clone(), drop, state.clone()).await?;

    let driver = {
        let state_guard = state.lock().unwrap();
        let driver = state_guard
            .connections
            .get(&connection_id)
            .ok_or_else(|| DbError::NotFound(format!("Connection '{}' not found", connection_id)))?
            .clone();
        driver
    };

    for sql in &preview_result.sql {
        driver.execute_query(sql).await?;
    }

    Ok(preview_result)
}
