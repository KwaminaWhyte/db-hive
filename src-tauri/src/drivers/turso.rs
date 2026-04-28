//! Turso (libSQL) database driver implementation
//!
//! Turso is a hosted libSQL service. libSQL is a fork of SQLite with remote
//! and replication support. Connection uses the libSQL URL (e.g.
//! `libsql://my-db.turso.io`) plus an auth token (placed in the password field).

use async_trait::async_trait;
use libsql::{params, Builder, Connection, Database, Value};

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo,
    TableSchema,
};

/// Turso (libSQL) database driver
pub struct TursoDriver {
    /// libSQL Database handle (used to obtain Connections)
    db: Database,
    /// Underlying URL for diagnostics
    url: String,
}

impl TursoDriver {
    fn build_url(host: &str) -> String {
        let h = host.trim();
        if h.starts_with("libsql://")
            || h.starts_with("https://")
            || h.starts_with("http://")
            || h.starts_with("wss://")
            || h.starts_with("ws://")
        {
            h.to_string()
        } else if h.is_empty() {
            String::new()
        } else {
            format!("libsql://{}", h)
        }
    }

    async fn conn(&self) -> Result<Connection, DbError> {
        self.db
            .connect()
            .map_err(|e| DbError::ConnectionError(format!("libsql connect failed: {}", e)))
    }

    fn value_to_json(v: &Value) -> serde_json::Value {
        match v {
            Value::Null => serde_json::Value::Null,
            Value::Integer(n) => serde_json::Value::Number((*n).into()),
            Value::Real(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Text(s) => serde_json::Value::String(s.clone()),
            Value::Blob(b) => serde_json::Value::String(format!("<BLOB {} bytes>", b.len())),
        }
    }
}

#[async_trait]
impl DatabaseDriver for TursoDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let url = Self::build_url(&opts.host);
        if url.is_empty() {
            return Err(DbError::ConnectionError(
                "Turso requires a libsql URL (e.g. libsql://my-db.turso.io)".to_string(),
            ));
        }

        let token = opts.password.unwrap_or_default();

        let db = Builder::new_remote(url.clone(), token)
            .build()
            .await
            .map_err(|e| DbError::ConnectionError(format!("Failed to build libsql client: {}", e)))?;

        // Verify connectivity early
        let conn = db
            .connect()
            .map_err(|e| DbError::ConnectionError(format!("libsql connect failed: {}", e)))?;
        conn.execute("SELECT 1", params![])
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection probe failed: {}", e)))?;

        Ok(Self { db, url })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        let conn = self.conn().await?;
        conn.execute("SELECT 1", params![])
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let conn = self.conn().await?;

        // Prepare statement to inspect column count
        let stmt = conn
            .prepare(sql)
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to prepare statement: {}", e)))?;

        let column_count = stmt.column_count();

        if column_count > 0 {
            let columns: Vec<String> = (0..column_count)
                .map(|i| {
                    stmt.columns()
                        .get(i)
                        .map(|c| c.name().to_string())
                        .unwrap_or_else(|| format!("col_{}", i))
                })
                .collect();

            let mut rows = stmt
                .query(params![])
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to execute query: {}", e)))?;

            let mut data: Vec<Vec<serde_json::Value>> = Vec::new();
            loop {
                match rows.next().await {
                    Ok(Some(row)) => {
                        let mut values = Vec::with_capacity(column_count);
                        for i in 0..column_count {
                            let v = row
                                .get_value(i as i32)
                                .map_err(|e| {
                                    DbError::QueryError(format!("Failed to read column: {}", e))
                                })?;
                            values.push(Self::value_to_json(&v));
                        }
                        data.push(values);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        return Err(DbError::QueryError(format!("Row iteration failed: {}", e)));
                    }
                }
            }
            Ok(QueryResult::with_data(columns, data))
        } else {
            // Mutation / DDL — re-execute via execute()
            let affected = conn
                .execute(sql, params![])
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to execute statement: {}", e)))?;
            Ok(QueryResult::with_affected(affected))
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        // libSQL/SQLite presents a single attached "main" database
        Ok(vec![DatabaseInfo {
            name: "main".to_string(),
            owner: None,
            size: None,
        }])
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        Ok(vec![SchemaInfo {
            name: "main".to_string(),
            database: "main".to_string(),
        }])
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let conn = self.conn().await?;
        let sql = "SELECT name, type FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                   ORDER BY name";

        let mut rows = conn
            .query(sql, params![])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to query tables: {}", e)))?;

        let mut tables = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read table row: {}", e)))?
        {
            let name: String = row
                .get(0)
                .map_err(|e| DbError::QueryError(format!("name: {}", e)))?;
            let table_type: String = row
                .get(1)
                .map_err(|e| DbError::QueryError(format!("type: {}", e)))?;
            tables.push(TableInfo {
                name,
                schema: schema.to_string(),
                row_count: None,
                table_type: table_type.to_uppercase(),
            });
        }
        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        let conn = self.conn().await?;

        let pragma = format!("PRAGMA table_info(\"{}\")", table.replace('"', ""));
        let mut rows = conn
            .query(&pragma, params![])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed PRAGMA table_info: {}", e)))?;

        let mut columns = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::QueryError(format!("Row error: {}", e)))?
        {
            let name: String = row.get(1).map_err(|e| DbError::QueryError(e.to_string()))?;
            let data_type: String = row.get(2).map_err(|e| DbError::QueryError(e.to_string()))?;
            let not_null: i64 = row.get(3).map_err(|e| DbError::QueryError(e.to_string()))?;
            let default_value: Option<String> =
                row.get(4).ok();
            let is_pk: i64 = row.get(5).map_err(|e| DbError::QueryError(e.to_string()))?;

            let is_auto_increment =
                is_pk > 0 && data_type.to_uppercase() == "INTEGER";

            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: not_null == 0,
                default_value,
                is_primary_key: is_pk > 0,
                is_auto_increment,
            });
        }

        // Indexes
        let pragma_idx = format!("PRAGMA index_list(\"{}\")", table.replace('"', ""));
        let mut idx_rows = conn
            .query(&pragma_idx, params![])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed PRAGMA index_list: {}", e)))?;

        let mut indexes = Vec::new();
        let mut index_metas: Vec<(String, bool, bool)> = Vec::new();
        while let Some(row) = idx_rows
            .next()
            .await
            .map_err(|e| DbError::QueryError(e.to_string()))?
        {
            let name: String = row.get(1).map_err(|e| DbError::QueryError(e.to_string()))?;
            let unique: i64 = row.get(2).map_err(|e| DbError::QueryError(e.to_string()))?;
            let origin: String = row.get(3).map_err(|e| DbError::QueryError(e.to_string()))?;
            index_metas.push((name, unique != 0, origin == "pk"));
        }

        for (idx_name, is_unique, is_primary) in index_metas {
            let pragma_info = format!("PRAGMA index_info(\"{}\")", idx_name.replace('"', ""));
            let mut info_rows = conn
                .query(&pragma_info, params![])
                .await
                .map_err(|e| DbError::QueryError(e.to_string()))?;
            let mut cols = Vec::new();
            while let Some(row) = info_rows
                .next()
                .await
                .map_err(|e| DbError::QueryError(e.to_string()))?
            {
                let col: String = row.get(2).map_err(|e| DbError::QueryError(e.to_string()))?;
                cols.push(col);
            }
            indexes.push(IndexInfo {
                name: idx_name,
                columns: cols,
                is_unique,
                is_primary,
            });
        }

        let table_info = TableInfo {
            name: table.to_string(),
            schema: schema.to_string(),
            row_count: None,
            table_type: "TABLE".to_string(),
        };

        Ok(TableSchema {
            table: table_info,
            columns,
            indexes,
        })
    }

    async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let conn = self.conn().await?;
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
                params![],
            )
            .await
            .map_err(|e| DbError::QueryError(e.to_string()))?;

        let mut tables: Vec<String> = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::QueryError(e.to_string()))?
        {
            tables.push(row.get(0).map_err(|e| DbError::QueryError(e.to_string()))?);
        }

        let mut foreign_keys = Vec::new();
        for tbl in tables {
            let pragma = format!("PRAGMA foreign_key_list(\"{}\")", tbl.replace('"', ""));
            let mut fk_rows = conn
                .query(&pragma, params![])
                .await
                .map_err(|e| DbError::QueryError(e.to_string()))?;

            use std::collections::HashMap;
            let mut fk_map: HashMap<i64, (String, Vec<String>, Vec<String>, String, String)> =
                HashMap::new();

            while let Some(row) = fk_rows
                .next()
                .await
                .map_err(|e| DbError::QueryError(e.to_string()))?
            {
                let id: i64 = row.get(0).map_err(|e| DbError::QueryError(e.to_string()))?;
                let _seq: i64 = row.get(1).map_err(|e| DbError::QueryError(e.to_string()))?;
                let ref_table: String = row.get(2).map_err(|e| DbError::QueryError(e.to_string()))?;
                let from_col: String = row.get(3).map_err(|e| DbError::QueryError(e.to_string()))?;
                let to_col: String = row.get(4).map_err(|e| DbError::QueryError(e.to_string()))?;
                let on_update: String = row.get(5).map_err(|e| DbError::QueryError(e.to_string()))?;
                let on_delete: String = row.get(6).map_err(|e| DbError::QueryError(e.to_string()))?;

                fk_map
                    .entry(id)
                    .and_modify(|(_, cols, refs, _, _)| {
                        cols.push(from_col.clone());
                        refs.push(to_col.clone());
                    })
                    .or_insert((
                        ref_table,
                        vec![from_col],
                        vec![to_col],
                        on_update,
                        on_delete,
                    ));
            }

            for (id, (ref_table, cols, refs, on_update, on_delete)) in fk_map {
                foreign_keys.push(ForeignKeyInfo {
                    name: format!("{}_{}_fkey", tbl, id),
                    table: tbl.clone(),
                    schema: schema.to_string(),
                    columns: cols,
                    referenced_table: ref_table,
                    referenced_schema: schema.to_string(),
                    referenced_columns: refs,
                    on_delete: Some(on_delete),
                    on_update: Some(on_update),
                });
            }
        }

        Ok(foreign_keys)
    }

    async fn close(&self) -> Result<(), DbError> {
        // Database handle drops automatically; nothing to close explicitly.
        let _ = &self.url;
        Ok(())
    }
}
