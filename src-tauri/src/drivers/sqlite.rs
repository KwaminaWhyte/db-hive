//! SQLite database driver implementation
//!
//! This module provides the SQLite implementation of the DatabaseDriver trait
//! using rusqlite for database operations.

use async_trait::async_trait;
use rusqlite::{Connection, OpenFlags, Row};
use std::sync::{Arc, Mutex as StdMutex};

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, IndexInfo, SchemaInfo, TableInfo, TableSchema,
};

/// SQLite database driver
///
/// Manages connections to SQLite database files and provides query execution
/// and metadata retrieval capabilities.
///
/// Note: SQLite is file-based, so many concepts like "host", "port", and "username"
/// don't apply. The `database` field in ConnectionOptions should contain the file path.
pub struct SqliteDriver {
    /// The active SQLite connection wrapped in Arc<Mutex> for thread-safety
    /// rusqlite::Connection is not Send, so we need to wrap it
    conn: Arc<StdMutex<Connection>>,

    /// Path to the database file
    db_path: String,
}

impl SqliteDriver {
    /// Convert a rusqlite Row to a Vec of JSON values
    fn row_to_json_vec(row: &Row, column_count: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let mut values = Vec::new();

        for i in 0..column_count {
            let value = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(n) => serde_json::Value::Number(n.into()),
                rusqlite::types::ValueRef::Real(f) => {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                }
                rusqlite::types::ValueRef::Text(s) => {
                    let text = std::str::from_utf8(s).unwrap_or("");
                    serde_json::Value::String(text.to_string())
                }
                rusqlite::types::ValueRef::Blob(b) => {
                    // Convert blob to base64 string
                    serde_json::Value::String(format!("<BLOB {} bytes>", b.len()))
                }
            };

            values.push(value);
        }

        Ok(values)
    }
}

#[async_trait]
impl DatabaseDriver for SqliteDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        // For SQLite, the database field contains the file path
        let db_path = opts.database.ok_or_else(|| {
            DbError::ConnectionError("SQLite requires a database file path".to_string())
        })?;

        // Open the database file in read-write mode, creating it if it doesn't exist
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|e| DbError::ConnectionError(format!("Failed to open SQLite database: {}", e)))?;

        // Enable foreign keys (disabled by default in SQLite)
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| DbError::ConnectionError(format!("Failed to enable foreign keys: {}", e)))?;

        Ok(Self {
            conn: Arc::new(StdMutex::new(conn)),
            db_path,
        })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT 1", [], |_| Ok(()))
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let conn = self.conn.lock().unwrap();

        // Try to prepare the statement to determine if it returns rows
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| DbError::QueryError(format!("Failed to prepare statement: {}", e)))?;

        let column_count = stmt.column_count();

        if column_count > 0 {
            // This is a SELECT query
            let column_names: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|s| s.to_string())
                .collect();

            let mut rows_data = Vec::new();

            let mut query_rows = stmt
                .query([])
                .map_err(|e| DbError::QueryError(format!("Failed to execute query: {}", e)))?;

            while let Some(row) = query_rows
                .next()
                .map_err(|e| DbError::QueryError(format!("Failed to fetch row: {}", e)))?
            {
                let row_values = Self::row_to_json_vec(row, column_count)
                    .map_err(|e| DbError::QueryError(format!("Failed to convert row: {}", e)))?;
                rows_data.push(row_values);
            }

            Ok(QueryResult::with_data(column_names, rows_data))
        } else {
            // This is an INSERT/UPDATE/DELETE/CREATE/etc.
            drop(stmt); // Drop statement before executing
            let rows_affected = conn
                .execute(sql, [])
                .map_err(|e| DbError::QueryError(format!("Failed to execute statement: {}", e)))?;

            Ok(QueryResult::with_affected(rows_affected as u64))
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        // SQLite doesn't have multiple databases in the same way as PostgreSQL
        // We return the current database file as the only database
        Ok(vec![DatabaseInfo {
            name: std::path::Path::new(&self.db_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("database.db")
                .to_string(),
            owner: None,
            size: std::fs::metadata(&self.db_path)
                .ok()
                .map(|m| m.len()),
        }])
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        // SQLite has a simple schema model - typically just "main"
        // Attached databases would show up here too, but for now we just return main
        Ok(vec![SchemaInfo {
            name: "main".to_string(),
            database: std::path::Path::new(&self.db_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("database.db")
                .to_string(),
        }])
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let conn = self.conn.lock().unwrap();

        // Get tables from sqlite_master
        let query = r#"
            SELECT
                name,
                type
            FROM sqlite_master
            WHERE type IN ('table', 'view')
                AND name NOT LIKE 'sqlite_%'
            ORDER BY name
        "#;

        let mut stmt = conn
            .prepare(query)
            .map_err(|e| DbError::QueryError(format!("Failed to fetch tables: {}", e)))?;

        let table_iter = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let table_type: String = row.get(1)?;
                Ok((name, table_type))
            })
            .map_err(|e| DbError::QueryError(format!("Failed to query tables: {}", e)))?;

        let mut tables = Vec::new();

        for table_result in table_iter {
            let (name, table_type) = table_result
                .map_err(|e| DbError::QueryError(format!("Failed to read table row: {}", e)))?;

            // Get row count for tables (not views)
            let row_count = if table_type.to_uppercase() == "TABLE" {
                conn.query_row(&format!("SELECT COUNT(*) FROM \"{}\"", name), [], |row| {
                    row.get::<_, i64>(0)
                })
                .ok()
            } else {
                None
            };

            tables.push(TableInfo {
                name,
                schema: schema.to_string(),
                row_count: row_count.map(|c| c as u64),
                table_type: table_type.to_uppercase(),
            });
        }

        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        let conn = self.conn.lock().unwrap();

        // Get column information using PRAGMA table_info
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{}\")", table))
            .map_err(|e| DbError::QueryError(format!("Failed to get table info: {}", e)))?;

        let column_iter = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?; // name
                let data_type: String = row.get(2)?; // type
                let not_null: i32 = row.get(3)?; // notnull
                let default_value: Option<String> = row.get(4)?; // dflt_value
                let is_primary_key: i32 = row.get(5)?; // pk

                Ok(ColumnInfo {
                    name,
                    data_type,
                    nullable: not_null == 0,
                    default_value,
                    is_primary_key: is_primary_key > 0,
                })
            })
            .map_err(|e| DbError::QueryError(format!("Failed to query columns: {}", e)))?;

        let mut columns = Vec::new();
        for col in column_iter {
            columns.push(col.map_err(|e| DbError::QueryError(format!("Failed to read column: {}", e)))?);
        }

        // Get index information using PRAGMA index_list
        let mut index_stmt = conn
            .prepare(&format!("PRAGMA index_list(\"{}\")", table))
            .map_err(|e| DbError::QueryError(format!("Failed to get index list: {}", e)))?;

        let index_list_iter = index_stmt
            .query_map([], |row| {
                let index_name: String = row.get(1)?;
                let is_unique: i32 = row.get(2)?;
                let origin: String = row.get(3)?; // "c" = CREATE INDEX, "u" = UNIQUE, "pk" = PRIMARY KEY
                Ok((index_name, is_unique > 0, origin == "pk"))
            })
            .map_err(|e| DbError::QueryError(format!("Failed to query indexes: {}", e)))?;

        let mut indexes = Vec::new();

        for index_result in index_list_iter {
            let (index_name, is_unique, is_primary) = index_result
                .map_err(|e| DbError::QueryError(format!("Failed to read index: {}", e)))?;

            // Get columns for this index using PRAGMA index_info
            let mut col_stmt = conn
                .prepare(&format!("PRAGMA index_info(\"{}\")", index_name))
                .map_err(|e| DbError::QueryError(format!("Failed to get index info: {}", e)))?;

            let index_columns: Result<Vec<String>, rusqlite::Error> = col_stmt
                .query_map([], |row| row.get::<_, String>(2))
                .map_err(|e| DbError::QueryError(format!("Failed to query index columns: {}", e)))?
                .collect();

            let index_columns = index_columns
                .map_err(|e| DbError::QueryError(format!("Failed to read index columns: {}", e)))?;

            indexes.push(IndexInfo {
                name: index_name,
                columns: index_columns,
                is_unique,
                is_primary,
            });
        }

        // Get row count
        let row_count = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table), [], |row| {
                row.get::<_, i64>(0)
            })
            .ok()
            .map(|c| c as u64);

        let table_info = TableInfo {
            name: table.to_string(),
            schema: schema.to_string(),
            row_count,
            table_type: "TABLE".to_string(),
        };

        Ok(TableSchema {
            table: table_info,
            columns,
            indexes,
        })
    }

    async fn close(&self) -> Result<(), DbError> {
        // Connection will be automatically closed when dropped
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sqlite_connect_and_query() {
        // Create a temporary database file
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_db.sqlite");

        let opts = ConnectionOptions {
            host: String::new(), // Not used for SQLite
            port: 0,             // Not used for SQLite
            username: String::new(), // Not used for SQLite
            password: None,
            database: Some(db_path.to_str().unwrap().to_string()),
            timeout: None,
        };

        let driver = SqliteDriver::connect(opts).await.unwrap();

        // Test connection
        assert!(driver.test_connection().await.is_ok());

        // Create a test table
        let create_result = driver
            .execute_query("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)")
            .await
            .unwrap();
        assert_eq!(create_result.rows_affected, Some(0));

        // Insert data
        let insert_result = driver
            .execute_query("INSERT INTO users (name, age) VALUES ('Alice', 30)")
            .await
            .unwrap();
        assert_eq!(insert_result.rows_affected, Some(1));

        // Query data
        let query_result = driver
            .execute_query("SELECT * FROM users")
            .await
            .unwrap();

        assert_eq!(query_result.columns.len(), 3);
        assert_eq!(query_result.rows.len(), 1);

        // Cleanup
        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn test_sqlite_metadata() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_metadata.sqlite");

        let opts = ConnectionOptions {
            host: String::new(),
            port: 0,
            username: String::new(),
            password: None,
            database: Some(db_path.to_str().unwrap().to_string()),
            timeout: None,
        };

        let driver = SqliteDriver::connect(opts).await.unwrap();

        // Create test table
        driver
            .execute_query(
                "CREATE TABLE products (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    price REAL,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .await
            .unwrap();

        driver
            .execute_query("CREATE INDEX idx_name ON products(name)")
            .await
            .unwrap();

        // Get tables
        let tables = driver.get_tables("main").await.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "products");

        // Get table schema
        let schema = driver.get_table_schema("main", "products").await.unwrap();
        assert_eq!(schema.columns.len(), 4);
        assert_eq!(schema.columns[0].name, "id");
        assert!(schema.columns[0].is_primary_key);
        assert_eq!(schema.columns[1].name, "name");
        assert!(!schema.columns[1].nullable);

        // Check indexes
        assert!(!schema.indexes.is_empty());

        // Cleanup
        std::fs::remove_file(db_path).ok();
    }
}
