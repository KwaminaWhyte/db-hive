//! MySQL Database Driver Implementation
//!
//! This module provides a MySQL driver that implements the DatabaseDriver trait
//! for connecting to and interacting with MySQL databases using the mysql_async crate.

use async_trait::async_trait;
use mysql_async::prelude::*;
use mysql_async::{Conn, OptsBuilder, Pool};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::drivers::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo,
    TableSchema,
};

pub struct MysqlDriver {
    pool: Arc<Pool>,
    conn: Arc<TokioMutex<Conn>>,
    current_database: String,
}

impl MysqlDriver {
    fn map_mysql_error(err: mysql_async::Error) -> DbError {
        DbError::QueryError(err.to_string())
    }
}

#[async_trait]
impl DatabaseDriver for MysqlDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let host = &opts.host;
        let port = opts.port;
        let user = &opts.username;
        let password = opts.password.as_deref().unwrap_or("");
        let database = opts.database.as_deref().unwrap_or("mysql");

        let opts_builder = OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(user))
            .pass(Some(password))
            .db_name(Some(database));

        let pool = Pool::new(opts_builder.clone());
        let conn = pool.get_conn().await.map_err(Self::map_mysql_error)?;

        Ok(Self {
            pool: Arc::new(pool),
            conn: Arc::new(TokioMutex::new(conn)),
            current_database: database.to_string(),
        })
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let mut conn = self.conn.lock().await;

        // Execute query
        let mut result = conn.query_iter(sql).await.map_err(Self::map_mysql_error)?;

        // Check if this is a SELECT query (has result set)
        if let Some(columns) = result.columns() {
            let column_names: Vec<String> = columns
                .iter()
                .map(|col| col.name_str().to_string())
                .collect();

            let mut rows_data = Vec::new();

            // Collect rows from the stream
            while let Some(row) = result.next().await.map_err(Self::map_mysql_error)? {
                let mut values = Vec::new();
                for i in 0..column_names.len() {
                    let value: mysql_async::Value = row.get(i).unwrap_or(mysql_async::Value::NULL);
                    values.push(Self::mysql_value_to_json(value));
                }
                rows_data.push(values);
            }

            Ok(QueryResult::with_data(column_names, rows_data))
        } else {
            // DML query (INSERT, UPDATE, DELETE, etc.)
            let affected_rows = result.affected_rows();
            Ok(QueryResult::with_affected(affected_rows))
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        let mut conn = self.conn.lock().await;

        let databases: Vec<String> = conn
            .query("SHOW DATABASES")
            .await
            .map_err(Self::map_mysql_error)?;

        Ok(databases
            .into_iter()
            .map(|name| DatabaseInfo {
                name,
                owner: None,
                size: None,
            })
            .collect())
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        // MySQL doesn't have schemas like PostgreSQL
        // Databases are essentially schemas in MySQL
        // Return the current database as the only schema
        Ok(vec![SchemaInfo {
            name: self.current_database.clone(),
            database: self.current_database.clone(),
        }])
    }

    async fn get_tables(&self, _schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let mut conn = self.conn.lock().await;

        // Use parameterized query to prevent SQL injection
        let query = r#"
            SELECT TABLE_NAME, TABLE_TYPE, TABLE_ROWS, TABLE_COMMENT
            FROM information_schema.TABLES
            WHERE TABLE_SCHEMA = ?
            ORDER BY TABLE_NAME
        "#;

        let rows: Vec<(String, String, Option<u64>, Option<String>)> = conn
            .exec(query, (&self.current_database,))
            .await
            .map_err(Self::map_mysql_error)?;

        Ok(rows
            .into_iter()
            .map(|(name, table_type, row_count, _comment)| TableInfo {
                schema: self.current_database.clone(),
                name,
                table_type,
                row_count,
            })
            .collect())
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        let mut conn = self.conn.lock().await;
        conn.ping().await.map_err(Self::map_mysql_error)?;
        Ok(())
    }

    async fn get_table_schema(
        &self,
        _schema: &str,
        table_name: &str,
    ) -> Result<TableSchema, DbError> {
        let mut conn = self.conn.lock().await;

        // Get column information using parameterized query
        let column_query = r#"
            SELECT
                COLUMN_NAME,
                COLUMN_TYPE,
                IS_NULLABLE,
                COLUMN_DEFAULT,
                COLUMN_KEY,
                EXTRA
            FROM information_schema.COLUMNS
            WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
            ORDER BY ORDINAL_POSITION
        "#;

        let column_rows: Vec<(String, String, String, Option<String>, String, String)> = conn
            .exec(column_query, (&self.current_database, table_name))
            .await
            .map_err(Self::map_mysql_error)?;

        let columns: Vec<ColumnInfo> = column_rows
            .into_iter()
            .map(
                |(name, column_type, is_nullable, default_value, column_key, extra)| {
                    let is_auto_increment = extra.to_lowercase().contains("auto_increment");

                    ColumnInfo {
                        name,
                        data_type: column_type,
                        nullable: is_nullable == "YES",
                        default_value,
                        is_primary_key: column_key == "PRI",
                        is_auto_increment,
                    }
                },
            )
            .collect();

        // Get index information using parameterized query
        let index_query = r#"
            SELECT
                INDEX_NAME,
                NON_UNIQUE,
                COLUMN_NAME
            FROM information_schema.STATISTICS
            WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
            ORDER BY INDEX_NAME, SEQ_IN_INDEX
        "#;

        let index_rows: Vec<(String, i64, String)> = conn
            .exec(index_query, (&self.current_database, table_name))
            .await
            .map_err(Self::map_mysql_error)?;

        // Group columns by index name
        let mut indexes_map: std::collections::HashMap<String, (bool, Vec<String>)> =
            std::collections::HashMap::new();

        for (index_name, non_unique, column_name) in index_rows {
            let is_unique = non_unique == 0;
            indexes_map
                .entry(index_name)
                .or_insert((is_unique, Vec::new()))
                .1
                .push(column_name);
        }

        let indexes: Vec<IndexInfo> = indexes_map
            .into_iter()
            .map(|(name, (is_unique, columns))| {
                let is_primary = name == "PRIMARY";
                IndexInfo {
                    name,
                    columns,
                    is_unique,
                    is_primary,
                }
            })
            .collect();

        Ok(TableSchema {
            table: TableInfo {
                name: table_name.to_string(),
                schema: self.current_database.clone(),
                row_count: None,
                table_type: "TABLE".to_string(),
            },
            columns,
            indexes,
        })
    }

    async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let query = r#"
            SELECT
                kcu.CONSTRAINT_NAME as name,
                kcu.TABLE_NAME as `table`,
                kcu.TABLE_SCHEMA as `schema`,
                kcu.COLUMN_NAME as column_name,
                kcu.REFERENCED_TABLE_NAME as referenced_table,
                kcu.REFERENCED_TABLE_SCHEMA as referenced_schema,
                kcu.REFERENCED_COLUMN_NAME as referenced_column,
                rc.UPDATE_RULE as on_update,
                rc.DELETE_RULE as on_delete
            FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
            JOIN INFORMATION_SCHEMA.REFERENTIAL_CONSTRAINTS rc
                ON kcu.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
                AND kcu.TABLE_SCHEMA = rc.CONSTRAINT_SCHEMA
            WHERE kcu.REFERENCED_TABLE_NAME IS NOT NULL
                AND kcu.TABLE_SCHEMA = ?
            ORDER BY kcu.TABLE_NAME, kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
        "#;

        let mut conn =
            self.pool.get_conn().await.map_err(|e| {
                DbError::ConnectionError(format!("Failed to get connection: {}", e))
            })?;

        let rows: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )> = conn
            .exec(query, (schema,))
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch foreign keys: {}", e)))?;

        // Group foreign keys by constraint name (for composite foreign keys)
        use std::collections::HashMap;
        let mut fk_map: HashMap<String, (ForeignKeyInfo, Vec<String>, Vec<String>)> =
            HashMap::new();

        for (
            fk_name,
            table,
            schema,
            column,
            ref_table,
            ref_schema,
            ref_column,
            on_update,
            on_delete,
        ) in rows
        {
            fk_map
                .entry(fk_name.clone())
                .and_modify(|(_, cols, ref_cols)| {
                    cols.push(column.clone());
                    ref_cols.push(ref_column.clone());
                })
                .or_insert_with(|| {
                    let fk = ForeignKeyInfo {
                        name: fk_name,
                        table,
                        schema,
                        columns: vec![],
                        referenced_table: ref_table,
                        referenced_schema: ref_schema,
                        referenced_columns: vec![],
                        on_delete: Some(on_delete),
                        on_update: Some(on_update),
                    };
                    (fk, vec![column], vec![ref_column])
                });
        }

        // Convert HashMap to Vec, filling in the columns
        let foreign_keys: Vec<ForeignKeyInfo> = fk_map
            .into_iter()
            .map(|(_, (mut fk, cols, ref_cols))| {
                fk.columns = cols;
                fk.referenced_columns = ref_cols;
                fk
            })
            .collect();

        Ok(foreign_keys)
    }

    async fn close(&self) -> Result<(), DbError> {
        // MySQL connection pool will clean up automatically on drop
        Ok(())
    }
}

impl MysqlDriver {
    fn mysql_value_to_json(value: mysql_async::Value) -> serde_json::Value {
        use mysql_async::Value;
        match value {
            Value::NULL => serde_json::Value::Null,
            Value::Bytes(b) => {
                // Try to convert bytes to UTF-8 string
                match String::from_utf8(b.clone()) {
                    Ok(s) => serde_json::Value::String(s),
                    Err(_) => {
                        // If not valid UTF-8, encode as base64 or hex
                        serde_json::Value::String(format!("0x{}", hex::encode(b)))
                    }
                }
            }
            Value::Int(i) => serde_json::json!(i),
            Value::UInt(u) => serde_json::json!(u),
            Value::Float(f) => serde_json::Number::from_f64(f as f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Double(d) => serde_json::Number::from_f64(d)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Date(year, month, day, hour, min, sec, _micro) => {
                // Format as ISO 8601 datetime
                serde_json::Value::String(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    year, month, day, hour, min, sec
                ))
            }
            Value::Time(is_negative, days, hours, minutes, seconds, _micro) => {
                let sign = if is_negative { "-" } else { "" };
                let total_hours = days * 24 + hours as u32;
                serde_json::Value::String(format!(
                    "{}{:02}:{:02}:{:02}",
                    sign, total_hours, minutes, seconds
                ))
            }
        }
    }
}
