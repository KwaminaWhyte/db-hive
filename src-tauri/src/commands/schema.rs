//! Schema exploration commands
//!
//! This module provides Tauri commands for exploring database schemas,
//! including listing databases, schemas, tables, and retrieving table details.

use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::models::{DatabaseInfo, DbError, SchemaInfo, TableInfo, TableSchema};
use crate::state::{AppState, MetadataCache};

/// Get list of databases for a connection
///
/// Returns all databases available on the connected database server.
/// This command requires an active connection.
///
/// # Arguments
/// * `connection_id` - UUID of the active connection
/// * `state` - Application state containing active connections
///
/// # Returns
/// * `Ok(Vec<DatabaseInfo>)` - List of databases with metadata
/// * `Err(DbError)` - If connection not found or query fails
#[tauri::command]
pub async fn get_databases(
    connection_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<DatabaseInfo>, DbError> {
    // Clone the Arc<dyn DatabaseDriver> before awaiting to avoid holding the lock across await
    let connection = {
        let state = state.lock().unwrap();
        state
            .get_connection(&connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    // Call the driver method
    connection.get_databases().await
}

/// Get list of schemas for a database
///
/// Returns all schemas/namespaces within a specific database.
/// Some databases (like MySQL) don't have separate schemas, in which case
/// this may return a single schema with the same name as the database.
///
/// # Arguments
/// * `connection_id` - UUID of the active connection
/// * `database` - Name of the database to query
/// * `state` - Application state containing active connections
///
/// # Returns
/// * `Ok(Vec<SchemaInfo>)` - List of schemas
/// * `Err(DbError)` - If connection not found or query fails
#[tauri::command]
pub async fn get_schemas(
    connection_id: String,
    database: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<SchemaInfo>, DbError> {
    // Clone the Arc<dyn DatabaseDriver> before awaiting to avoid holding the lock across await
    let connection = {
        let state = state.lock().unwrap();
        state
            .get_connection(&connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    // Call the driver method
    connection.get_schemas(&database).await
}

/// Get list of tables in a schema
///
/// Returns all tables and views within a specific schema.
/// Includes table type information (TABLE, VIEW, MATERIALIZED VIEW, etc.)
///
/// # Arguments
/// * `connection_id` - UUID of the active connection
/// * `schema` - Name of the schema to query
/// * `state` - Application state containing active connections
///
/// # Returns
/// * `Ok(Vec<TableInfo>)` - List of tables and views
/// * `Err(DbError)` - If connection not found or query fails
#[tauri::command]
pub async fn get_tables(
    connection_id: String,
    schema: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TableInfo>, DbError> {
    // Clone the Arc<dyn DatabaseDriver> before awaiting to avoid holding the lock across await
    let connection = {
        let state = state.lock().unwrap();
        state
            .get_connection(&connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    // Call the driver method
    connection.get_tables(&schema).await
}

/// Get detailed schema for a specific table
///
/// Returns complete table metadata including columns, indexes, and constraints.
/// This is used when the user wants to see the full table structure.
///
/// # Arguments
/// * `connection_id` - UUID of the active connection
/// * `schema` - Name of the schema containing the table
/// * `table` - Name of the table
/// * `state` - Application state containing active connections
///
/// # Returns
/// * `Ok(TableSchema)` - Complete table schema with columns and indexes
/// * `Err(DbError)` - If connection not found or query fails
#[tauri::command]
pub async fn get_table_schema(
    connection_id: String,
    schema: String,
    table: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<TableSchema, DbError> {
    // Clone the Arc<dyn DatabaseDriver> before awaiting to avoid holding the lock across await
    let connection = {
        let state = state.lock().unwrap();
        state
            .get_connection(&connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    // Call the driver method
    connection.get_table_schema(&schema, &table).await
}

/// Response for autocomplete metadata
///
/// Contains all metadata needed for SQL autocomplete functionality,
/// organized by schema and table for quick lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutocompleteMetadata {
    /// List of all databases
    pub databases: Vec<String>,

    /// List of all schemas
    pub schemas: Vec<String>,

    /// List of all tables with their schema
    pub tables: Vec<TableReference>,

    /// List of all columns with their table and schema
    pub columns: Vec<ColumnReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableReference {
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnReference {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub data_type: String,
}

/// Get metadata for SQL autocomplete
///
/// Returns flattened metadata suitable for autocomplete suggestions.
/// Uses cached metadata when available and not stale (> 5 minutes old).
/// Falls back to fetching fresh metadata from the database if needed.
///
/// # Arguments
/// * `connection_id` - UUID of the active connection
/// * `database` - Name of the database to get metadata for
/// * `force_refresh` - If true, bypass cache and fetch fresh metadata
/// * `state` - Application state containing active connections and cache
///
/// # Returns
/// * `Ok(AutocompleteMetadata)` - Flattened metadata for autocomplete
/// * `Err(DbError)` - If connection not found or query fails
#[tauri::command]
pub async fn get_autocomplete_metadata(
    connection_id: String,
    database: String,
    force_refresh: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<AutocompleteMetadata, DbError> {
    // Check cache first
    let cache_valid = {
        let state = state.lock().unwrap();
        if let Some(cache) = state.metadata_cache.get(&connection_id) {
            !force_refresh && !cache.is_stale()
        } else {
            false
        }
    };

    // Return cached data if valid
    if cache_valid {
        let state = state.lock().unwrap();
        let cache = state.metadata_cache.get(&connection_id).unwrap();
        return Ok(flatten_metadata_for_autocomplete(cache));
    }

    // Otherwise, fetch fresh metadata
    let connection = {
        let state = state.lock().unwrap();
        state
            .get_connection(&connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    // Fetch all metadata
    let databases = connection.get_databases().await?;
    let schemas = connection.get_schemas(&database).await?;

    // Fetch tables and columns for all schemas
    let mut all_tables = Vec::new();
    let mut all_columns = std::collections::HashMap::new();

    for schema in &schemas {
        let tables = connection.get_tables(&schema.name).await?;
        all_tables.push((schema.name.clone(), tables.clone()));

        // Fetch columns for each table
        for table in tables {
            let table_schema = connection.get_table_schema(&schema.name, &table.name).await?;
            let key = format!("{}.{}", schema.name, table.name);
            all_columns.insert(key, table_schema.columns);
        }
    }

    // Build and cache metadata
    let mut cache = MetadataCache::new();
    cache.databases = databases;
    cache.schemas.insert(database.clone(), schemas);

    for (schema_name, tables) in all_tables {
        cache.tables.insert(schema_name, tables);
    }

    cache.columns = all_columns;
    cache.touch();

    let result = flatten_metadata_for_autocomplete(&cache);

    // Store in cache
    {
        let mut state = state.lock().unwrap();
        state.metadata_cache.insert(connection_id, cache);
    }

    Ok(result)
}

/// Helper function to flatten metadata cache into autocomplete format
fn flatten_metadata_for_autocomplete(cache: &MetadataCache) -> AutocompleteMetadata {
    let mut metadata = AutocompleteMetadata {
        databases: cache.databases.iter().map(|d| d.name.clone()).collect(),
        schemas: Vec::new(),
        tables: Vec::new(),
        columns: Vec::new(),
    };

    // Flatten schemas
    for schemas in cache.schemas.values() {
        for schema in schemas {
            if !metadata.schemas.contains(&schema.name) {
                metadata.schemas.push(schema.name.clone());
            }
        }
    }

    // Flatten tables
    for (schema_name, tables) in &cache.tables {
        for table in tables {
            metadata.tables.push(TableReference {
                schema: schema_name.clone(),
                table: table.name.clone(),
            });
        }
    }

    // Flatten columns
    for (key, columns) in &cache.columns {
        // key is "schema.table"
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() == 2 {
            let schema = parts[0];
            let table = parts[1];

            for column in columns {
                metadata.columns.push(ColumnReference {
                    schema: schema.to_string(),
                    table: table.to_string(),
                    column: column.name.clone(),
                    data_type: column.data_type.clone(),
                });
            }
        }
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Mock driver for testing
    struct MockDriver;

    #[async_trait::async_trait]
    impl DatabaseDriver for MockDriver {
        async fn connect(
            _opts: crate::drivers::ConnectionOptions,
        ) -> Result<Self, DbError>
        where
            Self: Sized,
        {
            Ok(Self)
        }

        async fn test_connection(&self) -> Result<(), DbError> {
            Ok(())
        }

        async fn execute_query(&self, _sql: &str) -> Result<crate::drivers::QueryResult, DbError> {
            Ok(crate::drivers::QueryResult::empty())
        }

        async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
            Ok(vec![DatabaseInfo::new("test_db".to_string())])
        }

        async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
            Ok(vec![SchemaInfo::new(
                "public".to_string(),
                "test_db".to_string(),
            )])
        }

        async fn get_tables(&self, _schema: &str) -> Result<Vec<TableInfo>, DbError> {
            Ok(vec![TableInfo::new(
                "users".to_string(),
                "public".to_string(),
                "TABLE".to_string(),
            )])
        }

        async fn get_table_schema(
            &self,
            _schema: &str,
            _table: &str,
        ) -> Result<TableSchema, DbError> {
            let table = TableInfo::new(
                "users".to_string(),
                "public".to_string(),
                "TABLE".to_string(),
            );
            Ok(TableSchema::new(table, vec![], vec![]))
        }

        async fn close(&self) -> Result<(), DbError> {
            Ok(())
        }
    }

    fn create_test_state() -> Mutex<AppState> {
        let mut state = AppState::new();

        // Add the mock driver as an active connection
        state.add_connection("test-conn-id".to_string(), Arc::new(MockDriver));

        Mutex::new(state)
    }

    #[tokio::test]
    async fn test_get_databases() {
        let state = create_test_state();
        let result = get_databases("test-conn-id".to_string(), State::from(&state)).await;

        assert!(result.is_ok());
        let databases = result.unwrap();
        assert_eq!(databases.len(), 1);
        assert_eq!(databases[0].name, "test_db");
    }

    #[tokio::test]
    async fn test_get_databases_invalid_connection() {
        let state = create_test_state();
        let result = get_databases("invalid-id".to_string(), State::from(&state)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::ConnectionError(msg) => assert!(msg.contains("not found")),
            _ => panic!("Expected ConnectionError"),
        }
    }

    #[tokio::test]
    async fn test_get_schemas() {
        let state = create_test_state();
        let result = get_schemas(
            "test-conn-id".to_string(),
            "test_db".to_string(),
            State::from(&state),
        )
        .await;

        assert!(result.is_ok());
        let schemas = result.unwrap();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "public");
    }

    #[tokio::test]
    async fn test_get_tables() {
        let state = create_test_state();
        let result = get_tables(
            "test-conn-id".to_string(),
            "public".to_string(),
            State::from(&state),
        )
        .await;

        assert!(result.is_ok());
        let tables = result.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
    }

    #[tokio::test]
    async fn test_get_table_schema() {
        let state = create_test_state();
        let result = get_table_schema(
            "test-conn-id".to_string(),
            "public".to_string(),
            "users".to_string(),
            State::from(&state),
        )
        .await;

        assert!(result.is_ok());
        let schema = result.unwrap();
        assert_eq!(schema.table.name, "users");
    }
}
