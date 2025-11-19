//! Database driver implementations
//!
//! This module provides the unified `DatabaseDriver` trait and implementations
//! for various database systems. Each driver handles connection management,
//! query execution, and metadata retrieval specific to its database type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, IndexInfo, SchemaInfo, TableInfo, TableSchema,
};

pub mod mongodb;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

/// Connection options for establishing a database connection
///
/// Contains all the necessary information to connect to a database,
/// extracted from a ConnectionProfile.
#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    /// Database server hostname or IP address
    pub host: String,

    /// Database server port
    pub port: u16,

    /// Username for authentication
    pub username: String,

    /// Password for authentication
    pub password: Option<String>,

    /// Default database to connect to
    pub database: Option<String>,

    /// Connection timeout in seconds
    pub timeout: Option<u64>,
}

/// Result of a query execution
///
/// Contains the columns, rows, and affected row count from a query.
/// For SELECT queries, rows will contain data.
/// For INSERT/UPDATE/DELETE queries, rows_affected will contain the count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    /// Column names in the result set
    pub columns: Vec<String>,

    /// Rows of data, each row is a vector of JSON values
    pub rows: Vec<Vec<serde_json::Value>>,

    /// Number of rows affected (for INSERT/UPDATE/DELETE)
    pub rows_affected: Option<u64>,
}

impl QueryResult {
    /// Create a new empty QueryResult
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: None,
        }
    }

    /// Create a QueryResult for a data-returning query
    pub fn with_data(columns: Vec<String>, rows: Vec<Vec<serde_json::Value>>) -> Self {
        Self {
            columns,
            rows,
            rows_affected: None,
        }
    }

    /// Create a QueryResult for a command (INSERT/UPDATE/DELETE)
    pub fn with_affected(rows_affected: u64) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: Some(rows_affected),
        }
    }
}

/// Database driver trait
///
/// All database drivers must implement this trait to provide a unified
/// interface for connecting to and interacting with different database types.
///
/// # Thread Safety
///
/// Implementations must be both `Send` and `Sync` to enable safe use across
/// async tasks and threads.
///
/// # Example
///
/// ```rust,ignore
/// use db_hive::drivers::{DatabaseDriver, ConnectionOptions};
///
/// async fn connect_to_db() -> Result<Box<dyn DatabaseDriver>, DbError> {
///     let opts = ConnectionOptions {
///         host: "localhost".to_string(),
///         port: 5432,
///         username: "postgres".to_string(),
///         password: Some("password".to_string()),
///         database: Some("mydb".to_string()),
///         timeout: Some(30),
///     };
///
///     PostgresDriver::connect(opts).await
/// }
/// ```
#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Establish a connection to the database
    ///
    /// # Arguments
    ///
    /// * `opts` - Connection options including host, port, credentials, etc.
    ///
    /// # Returns
    ///
    /// Returns the connected driver instance or an error if connection fails.
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized;

    /// Test if the connection is still alive
    ///
    /// # Returns
    ///
    /// Returns Ok(()) if the connection is alive, or an error if it's dead.
    async fn test_connection(&self) -> Result<(), DbError>;

    /// Execute a SQL query
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query string to execute
    ///
    /// # Returns
    ///
    /// Returns query results including columns, rows, and affected row count.
    ///
    /// # Notes
    ///
    /// This method handles both data-returning queries (SELECT) and
    /// commands (INSERT/UPDATE/DELETE). The QueryResult will be populated
    /// accordingly.
    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError>;

    /// Get list of databases
    ///
    /// # Returns
    ///
    /// Returns a list of all databases/catalogs on the server.
    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError>;

    /// Get list of schemas in a database
    ///
    /// # Arguments
    ///
    /// * `database` - Database name to query schemas from
    ///
    /// # Returns
    ///
    /// Returns a list of schemas/namespaces in the specified database.
    async fn get_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>, DbError>;

    /// Get list of tables in a schema
    ///
    /// # Arguments
    ///
    /// * `schema` - Schema name to query tables from
    ///
    /// # Returns
    ///
    /// Returns a list of tables and views in the specified schema.
    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError>;

    /// Get detailed schema information for a table
    ///
    /// # Arguments
    ///
    /// * `schema` - Schema name containing the table
    /// * `table` - Table name to query
    ///
    /// # Returns
    ///
    /// Returns complete table schema including columns and indexes.
    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError>;

    /// Close the database connection
    ///
    /// # Returns
    ///
    /// Returns Ok(()) if the connection was closed successfully.
    ///
    /// # Notes
    ///
    /// Some drivers may not need explicit cleanup and can implement this as a no-op.
    async fn close(&self) -> Result<(), DbError>;
}
