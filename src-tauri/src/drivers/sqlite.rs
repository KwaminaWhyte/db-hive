//! SQLite database driver implementation
//!
//! This module provides the SQLite implementation of the DatabaseDriver trait
//! using rusqlite for database operations.

use async_trait::async_trait;
use rusqlite::{Connection, OpenFlags, Row};
use std::sync::{Arc, Mutex as StdMutex};

use super::{ConnectionOptions, DatabaseDriver, QueryResult, MAX_RESULT_ROWS};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo, TableSchema,
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

                // Enforce the row cap inside the step loop so an unbounded
                // SELECT never materializes the full result set (PERF-03).
                // One extra row past the cap lets the caller flag truncation;
                // dropping `query_rows` resets the statement.
                if rows_data.len() > MAX_RESULT_ROWS {
                    break;
                }
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

        let raw: Vec<(String, String)> = table_iter
            .collect::<Result<_, _>>()
            .map_err(|e| DbError::QueryError(format!("Failed to read table row: {}", e)))?;

        // Row counts used to be one `SELECT COUNT(*)` per table inside this
        // loop (N+1 prepares + scans). Collapse them into a single UNION ALL
        // query so the whole schema is counted in one prepared statement.
        let countable: Vec<&String> = raw
            .iter()
            .filter(|(_, t)| t.to_uppercase() == "TABLE")
            .map(|(n, _)| n)
            .collect();

        let mut counts: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        if !countable.is_empty() {
            // `"x""y"` escapes a double quote inside an identifier;
            // `'x''y'` escapes a single quote inside a string literal.
            let union = countable
                .iter()
                .map(|name| {
                    let ident = name.replace('"', "\"\"");
                    let lit = name.replace('\'', "''");
                    format!("SELECT '{}' AS n, (SELECT COUNT(*) FROM \"{}\") AS c", lit, ident)
                })
                .collect::<Vec<_>>()
                .join(" UNION ALL ");

            let mut count_stmt = conn
                .prepare(&union)
                .map_err(|e| DbError::QueryError(format!("Failed to prepare counts: {}", e)))?;
            let count_rows = count_stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| DbError::QueryError(format!("Failed to count rows: {}", e)))?;
            for r in count_rows {
                let (n, c) =
                    r.map_err(|e| DbError::QueryError(format!("Failed to read count: {}", e)))?;
                counts.insert(n, c);
            }
        }

        let tables = raw
            .into_iter()
            .map(|(name, table_type)| {
                let row_count = counts.get(&name).map(|c| *c as u64);
                TableInfo {
                    schema: schema.to_string(),
                    table_type: table_type.to_uppercase(),
                    row_count,
                    name,
                }
            })
            .collect();

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

                // SQLite auto-increment: INTEGER PRIMARY KEY or has AUTOINCREMENT in type
                let is_auto_increment = (is_primary_key > 0 && data_type.to_uppercase() == "INTEGER")
                    || data_type.to_uppercase().contains("AUTOINCREMENT");

                Ok(ColumnInfo {
                    name,
                    data_type,
                    nullable: not_null == 0,
                    default_value,
                    is_primary_key: is_primary_key > 0,
                    is_auto_increment,
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

    async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| DbError::InternalError(format!("Failed to lock connection: {}", e)))?;

        // This used to be N+1: one `SELECT name FROM sqlite_master` followed
        // by a `PRAGMA foreign_key_list("<table>")` prepared per table. The
        // `pragma_foreign_key_list` table-valued function (SQLite >= 3.16,
        // available in the bundled build) lets us join it against
        // sqlite_master and pull every FK for the whole schema in one query.
        // Rows are ordered so composite-key columns arrive contiguously and
        // in `seq` order.
        let query = r#"
            SELECT m.name AS tbl, f.id AS id, f.seq AS seq,
                   f."table" AS ref_table, f."from" AS from_col,
                   f."to" AS to_col, f.on_update AS on_update,
                   f.on_delete AS on_delete
            FROM sqlite_master m
            JOIN pragma_foreign_key_list(m.name) f
            WHERE m.type = 'table'
            ORDER BY m.name, f.id, f.seq
        "#;

        let mut stmt = conn
            .prepare(query)
            .map_err(|e| DbError::QueryError(format!("Failed to prepare foreign key query: {}", e)))?;

        let fk_iter = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?, // tbl (table owning the FK)
                    row.get::<_, i64>(1)?,    // id (FK identifier, per-table)
                    row.get::<_, i64>(2)?,    // seq (column position in composite FK)
                    row.get::<_, String>(3)?, // ref_table (referenced table)
                    row.get::<_, String>(4)?, // from_col (column in this table)
                    row.get::<_, String>(5)?, // to_col (column in referenced table)
                    row.get::<_, String>(6)?, // on_update
                    row.get::<_, String>(7)?, // on_delete
                ))
            })
            .map_err(|e| DbError::QueryError(format!("Failed to query foreign keys: {}", e)))?;

        let mut foreign_keys: Vec<ForeignKeyInfo> = Vec::new();
        // (table, id) of the FK currently being accumulated; rows are ordered
        // so all columns of one FK are contiguous.
        let mut current: Option<(String, i64)> = None;

        for row in fk_iter {
            let (tbl, id, _seq, ref_table, from_col, to_col, on_update, on_delete) = row
                .map_err(|e| DbError::QueryError(format!("Failed to read foreign key data: {}", e)))?;

            let key = (tbl.clone(), id);
            if current.as_ref() == Some(&key) {
                // Additional column of the composite FK in progress.
                let fk = foreign_keys
                    .last_mut()
                    .expect("current is Some so a FK has been pushed");
                fk.columns.push(from_col);
                fk.referenced_columns.push(to_col);
            } else {
                foreign_keys.push(ForeignKeyInfo {
                    name: format!("{}_{}_fkey", tbl, id),
                    table: tbl,
                    schema: schema.to_string(),
                    columns: vec![from_col],
                    referenced_table: ref_table,
                    referenced_schema: schema.to_string(), // SQLite has no schemas like PostgreSQL
                    referenced_columns: vec![to_col],
                    on_delete: Some(on_delete),
                    on_update: Some(on_update),
                });
                current = Some(key);
            }
        }

        Ok(foreign_keys)
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
            require_tls: false,
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
            require_tls: false,
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
