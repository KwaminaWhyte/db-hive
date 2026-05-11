//! Turso (libSQL) database driver implementation
//!
//! Connects to Turso / libSQL remote databases over HTTP using the official
//! `libsql` crate. Metadata queries reuse SQLite's `sqlite_master` and
//! `pragma_*` surface because libSQL is a SQLite superset.

use async_trait::async_trait;
use libsql::{Builder, Connection, Value};

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo, TableSchema,
};

pub struct TursoDriver {
    conn: Connection,
    url: String,
}

impl TursoDriver {
    fn value_to_json(v: Value) -> serde_json::Value {
        match v {
            Value::Null => serde_json::Value::Null,
            Value::Integer(n) => serde_json::Value::Number(n.into()),
            Value::Real(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Text(s) => serde_json::Value::String(s),
            Value::Blob(b) => serde_json::Value::String(format!("<BLOB {} bytes>", b.len())),
        }
    }

    async fn collect_rows(&self, sql: &str) -> Result<Vec<Vec<Value>>, DbError> {
        let mut rows = self
            .conn
            .query(sql, ())
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to execute query: {}", e)))?;

        let col_count = rows.column_count();
        let mut out = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch row: {}", e)))?
        {
            let mut vals = Vec::with_capacity(col_count as usize);
            for i in 0..col_count {
                let v = row
                    .get_value(i)
                    .map_err(|e| DbError::QueryError(format!("Failed to read column: {}", e)))?;
                vals.push(v);
            }
            out.push(vals);
        }
        Ok(out)
    }

    fn is_select(sql: &str) -> bool {
        let trimmed = sql.trim_start().to_ascii_lowercase();
        trimmed.starts_with("select")
            || trimmed.starts_with("pragma")
            || trimmed.starts_with("with")
            || trimmed.starts_with("explain")
    }
}

#[async_trait]
impl DatabaseDriver for TursoDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        if opts.host.trim().is_empty() {
            return Err(DbError::ConnectionError(
                "Turso requires a database URL in the host field (e.g. libsql://...turso.io)"
                    .to_string(),
            ));
        }
        let auth_token = opts.password.unwrap_or_default();

        let db = Builder::new_remote(opts.host.clone(), auth_token)
            .build()
            .await
            .map_err(|e| DbError::ConnectionError(format!("Failed to build Turso database: {}", e)))?;

        let conn = db
            .connect()
            .map_err(|e| DbError::ConnectionError(format!("Failed to open Turso connection: {}", e)))?;

        Ok(Self { conn, url: opts.host })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        self.conn
            .query("SELECT 1", ())
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        if Self::is_select(sql) {
            let mut rows = self
                .conn
                .query(sql, ())
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to execute query: {}", e)))?;

            let col_count = rows.column_count();
            let mut columns = Vec::with_capacity(col_count as usize);
            for i in 0..col_count {
                columns.push(
                    rows.column_name(i)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("column_{}", i)),
                );
            }

            let mut data = Vec::new();
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to fetch row: {}", e)))?
            {
                let mut json_row = Vec::with_capacity(col_count as usize);
                for i in 0..col_count {
                    let v = row.get_value(i).map_err(|e| {
                        DbError::QueryError(format!("Failed to read column: {}", e))
                    })?;
                    json_row.push(Self::value_to_json(v));
                }
                data.push(json_row);
            }

            Ok(QueryResult::with_data(columns, data))
        } else {
            let affected = self
                .conn
                .execute(sql, ())
                .await
                .map_err(|e| DbError::QueryError(format!("Failed to execute statement: {}", e)))?;
            Ok(QueryResult::with_affected(affected))
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        Ok(vec![DatabaseInfo {
            name: "main".to_string(),
            owner: None,
            size: None,
        }])
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        Ok(vec![SchemaInfo {
            name: "main".to_string(),
            database: self.url.clone(),
        }])
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let rows = self
            .collect_rows(
                "SELECT name, type FROM sqlite_master \
                 WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
            )
            .await?;

        let mut tables = Vec::with_capacity(rows.len());
        for row in rows {
            let name = match &row[0] {
                Value::Text(s) => s.clone(),
                _ => continue,
            };
            let table_type = match &row[1] {
                Value::Text(s) => s.to_uppercase(),
                _ => "TABLE".to_string(),
            };

            let row_count = if table_type == "TABLE" {
                let count_sql = format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\""));
                self.collect_rows(&count_sql)
                    .await
                    .ok()
                    .and_then(|r| r.into_iter().next())
                    .and_then(|r| r.into_iter().next())
                    .and_then(|v| match v {
                        Value::Integer(n) => Some(n as u64),
                        _ => None,
                    })
            } else {
                None
            };

            tables.push(TableInfo {
                name,
                schema: schema.to_string(),
                row_count,
                table_type,
            });
        }
        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        let escaped = table.replace('"', "\"\"");

        let col_rows = self
            .collect_rows(&format!("PRAGMA table_info(\"{}\")", escaped))
            .await?;

        let mut columns = Vec::with_capacity(col_rows.len());
        for row in col_rows {
            // PRAGMA table_info columns: cid, name, type, notnull, dflt_value, pk
            let name = match &row[1] {
                Value::Text(s) => s.clone(),
                _ => continue,
            };
            let data_type = match &row[2] {
                Value::Text(s) => s.clone(),
                _ => String::new(),
            };
            let not_null = matches!(&row[3], Value::Integer(n) if *n != 0);
            let default_value = match &row[4] {
                Value::Text(s) => Some(s.clone()),
                Value::Integer(n) => Some(n.to_string()),
                Value::Real(f) => Some(f.to_string()),
                _ => None,
            };
            let pk = matches!(&row[5], Value::Integer(n) if *n > 0);
            let is_auto_increment = pk && data_type.to_uppercase() == "INTEGER";

            columns.push(ColumnInfo {
                name,
                data_type,
                nullable: !not_null,
                default_value,
                is_primary_key: pk,
                is_auto_increment,
            });
        }

        let idx_list = self
            .collect_rows(&format!("PRAGMA index_list(\"{}\")", escaped))
            .await?;

        let mut indexes = Vec::new();
        for row in idx_list {
            // index_list columns: seq, name, unique, origin, partial
            let idx_name = match &row[1] {
                Value::Text(s) => s.clone(),
                _ => continue,
            };
            let is_unique = matches!(&row[2], Value::Integer(n) if *n != 0);
            let is_primary = matches!(&row[3], Value::Text(s) if s == "pk");

            let idx_escaped = idx_name.replace('"', "\"\"");
            let col_info = self
                .collect_rows(&format!("PRAGMA index_info(\"{}\")", idx_escaped))
                .await?;

            let mut index_columns = Vec::new();
            for c in col_info {
                // index_info columns: seqno, cid, name
                if let Some(Value::Text(s)) = c.get(2) {
                    index_columns.push(s.clone());
                }
            }

            indexes.push(IndexInfo {
                name: idx_name,
                columns: index_columns,
                is_unique,
                is_primary,
            });
        }

        let row_count = self
            .collect_rows(&format!("SELECT COUNT(*) FROM \"{}\"", escaped))
            .await
            .ok()
            .and_then(|r| r.into_iter().next())
            .and_then(|r| r.into_iter().next())
            .and_then(|v| match v {
                Value::Integer(n) => Some(n as u64),
                _ => None,
            });

        Ok(TableSchema {
            table: TableInfo {
                name: table.to_string(),
                schema: schema.to_string(),
                row_count,
                table_type: "TABLE".to_string(),
            },
            columns,
            indexes,
        })
    }

    async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let table_rows = self
            .collect_rows("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .await?;

        let mut foreign_keys = Vec::new();
        use std::collections::HashMap;

        for trow in table_rows {
            let table_name = match trow.into_iter().next() {
                Some(Value::Text(s)) => s,
                _ => continue,
            };
            let escaped = table_name.replace('"', "\"\"");
            let fk_rows = self
                .collect_rows(&format!("PRAGMA foreign_key_list(\"{}\")", escaped))
                .await?;

            // foreign_key_list columns: id, seq, table, from, to, on_update, on_delete, match
            let mut fk_map: HashMap<i64, (String, Vec<String>, Vec<String>, String, String)> =
                HashMap::new();

            for row in fk_rows {
                let id = match row.get(0) {
                    Some(Value::Integer(n)) => *n,
                    _ => continue,
                };
                let ref_table = match row.get(2) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => continue,
                };
                let from_col = match row.get(3) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => continue,
                };
                let to_col = match row.get(4) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => continue,
                };
                let on_update = match row.get(5) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => String::new(),
                };
                let on_delete = match row.get(6) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => String::new(),
                };

                fk_map
                    .entry(id)
                    .and_modify(|(_, cols, ref_cols, _, _)| {
                        cols.push(from_col.clone());
                        ref_cols.push(to_col.clone());
                    })
                    .or_insert((
                        ref_table,
                        vec![from_col],
                        vec![to_col],
                        on_update,
                        on_delete,
                    ));
            }

            for (id, (ref_table, cols, ref_cols, on_update, on_delete)) in fk_map {
                foreign_keys.push(ForeignKeyInfo {
                    name: format!("{}_{}_fkey", table_name, id),
                    table: table_name.clone(),
                    schema: schema.to_string(),
                    columns: cols,
                    referenced_table: ref_table,
                    referenced_schema: schema.to_string(),
                    referenced_columns: ref_cols,
                    on_delete: Some(on_delete),
                    on_update: Some(on_update),
                });
            }
        }

        Ok(foreign_keys)
    }

    async fn close(&self) -> Result<(), DbError> {
        Ok(())
    }
}
