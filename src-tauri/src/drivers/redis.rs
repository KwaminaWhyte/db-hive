//! Redis (key-value) database driver
//!
//! Redis is not SQL — this driver adapts the SQL-shaped trait to Redis by
//! treating each command string as a "query". `execute_query` parses the
//! command line and runs it against the connection. Metadata methods return
//! Redis-specific approximations so the rest of the UI keeps working.

use async_trait::async_trait;
use redis::{aio::MultiplexedConnection, AsyncCommands, Client, Value as RValue};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, SchemaInfo, TableInfo, TableSchema,
};

/// Maximum keys returned by `get_tables` (treats each key as a "table" row).
const MAX_KEYS_LISTED: usize = 1000;

pub struct RedisDriver {
    /// Multiplexed connection — clones share the same underlying socket.
    conn: Arc<AsyncMutex<MultiplexedConnection>>,
    /// Selected logical DB index (0..15)
    db_index: i64,
}

impl RedisDriver {
    fn build_url(opts: &ConnectionOptions) -> String {
        let host = if opts.host.trim().is_empty() {
            "127.0.0.1"
        } else {
            opts.host.trim()
        };
        let port = if opts.port == 0 { 6379 } else { opts.port };

        let auth = match (opts.username.as_str(), opts.password.as_deref()) {
            ("", Some(pw)) if !pw.is_empty() => format!(":{}@", pw),
            (user, Some(pw)) if !user.is_empty() && !pw.is_empty() => format!("{}:{}@", user, pw),
            _ => String::new(),
        };

        // db index parsed from `database` field if numeric, else 0
        let db = opts
            .database
            .as_deref()
            .and_then(|d| d.parse::<u32>().ok())
            .unwrap_or(0);

        format!("redis://{}{}:{}/{}", auth, host, port, db)
    }

    fn rvalue_to_json(v: &RValue) -> serde_json::Value {
        use serde_json::Value as J;
        match v {
            RValue::Nil => J::Null,
            RValue::Int(n) => J::Number((*n).into()),
            RValue::SimpleString(s) => J::String(s.clone()),
            RValue::Okay => J::String("OK".to_string()),
            RValue::BulkString(bytes) => match std::str::from_utf8(bytes) {
                Ok(s) => J::String(s.to_string()),
                Err(_) => J::String(format!("<{} bytes>", bytes.len())),
            },
            RValue::Array(items) => {
                J::Array(items.iter().map(Self::rvalue_to_json).collect())
            }
            RValue::Map(pairs) => {
                let mut obj = serde_json::Map::new();
                for (k, val) in pairs {
                    let key = match Self::rvalue_to_json(k) {
                        J::String(s) => s,
                        other => other.to_string(),
                    };
                    obj.insert(key, Self::rvalue_to_json(val));
                }
                J::Object(obj)
            }
            RValue::Set(items) => J::Array(items.iter().map(Self::rvalue_to_json).collect()),
            RValue::Double(f) => serde_json::Number::from_f64(*f)
                .map(J::Number)
                .unwrap_or(J::Null),
            RValue::Boolean(b) => J::Bool(*b),
            RValue::VerbatimString { format: _, text } => J::String(text.clone()),
            RValue::BigNumber(n) => J::String(n.to_string()),
            RValue::Push { kind, data } => {
                let mut arr = vec![J::String(format!("{:?}", kind))];
                arr.extend(data.iter().map(Self::rvalue_to_json));
                J::Array(arr)
            }
            RValue::ServerError(e) => J::String(format!("error: {:?}", e)),
            RValue::Attribute { data, .. } => Self::rvalue_to_json(data),
        }
    }

    /// Tokenize a Redis command line into argv. Supports double-quoted strings
    /// with `\"` and `\\` escapes — sufficient for typical DBA use.
    fn tokenize(input: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = String::new();
        let mut in_quote = false;
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            if in_quote {
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        buf.push(next);
                    }
                } else if c == '"' {
                    in_quote = false;
                } else {
                    buf.push(c);
                }
            } else if c == '"' {
                in_quote = true;
            } else if c.is_whitespace() {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            out.push(buf);
        }
        out
    }
}

#[async_trait]
impl DatabaseDriver for RedisDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let url = Self::build_url(&opts);
        let client = Client::open(url.clone())
            .map_err(|e| DbError::ConnectionError(format!("Invalid Redis URL: {}", e)))?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| DbError::ConnectionError(format!("Redis connection failed: {}", e)))?;

        // Sanity ping
        let _: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| DbError::ConnectionError(format!("PING failed: {}", e)))?;

        let db_index = opts
            .database
            .as_deref()
            .and_then(|d| d.parse::<i64>().ok())
            .unwrap_or(0);

        Ok(Self {
            conn: Arc::new(AsyncMutex::new(conn)),
            db_index,
        })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        let mut conn = self.conn.lock().await;
        let _: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbError::ConnectionError(format!("PING failed: {}", e)))?;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let argv = Self::tokenize(sql);
        if argv.is_empty() {
            return Err(DbError::InvalidInput("Empty Redis command".to_string()));
        }

        let mut cmd = redis::cmd(&argv[0].to_uppercase());
        for arg in &argv[1..] {
            cmd.arg(arg.as_str());
        }

        let mut conn = self.conn.lock().await;
        let value: RValue = cmd
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbError::QueryError(format!("Redis command failed: {}", e)))?;

        // Render result as a single-column table for SELECT-like output.
        match &value {
            RValue::Array(items) | RValue::Set(items) => {
                let columns = vec!["value".to_string()];
                let rows = items
                    .iter()
                    .map(|v| vec![Self::rvalue_to_json(v)])
                    .collect();
                Ok(QueryResult::with_data(columns, rows))
            }
            RValue::Map(pairs) => {
                let columns = vec!["field".to_string(), "value".to_string()];
                let rows = pairs
                    .iter()
                    .map(|(k, v)| vec![Self::rvalue_to_json(k), Self::rvalue_to_json(v)])
                    .collect();
                Ok(QueryResult::with_data(columns, rows))
            }
            other => {
                let columns = vec!["result".to_string()];
                let rows = vec![vec![Self::rvalue_to_json(other)]];
                Ok(QueryResult::with_data(columns, rows))
            }
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        // Redis exposes 16 logical DBs by default (0..15). Report them all so
        // the UI can show them, but don't try to ask the server for sizes
        // across DBs (would require SELECT + DBSIZE per DB).
        Ok((0..16)
            .map(|i| DatabaseInfo {
                name: format!("db{}", i),
                owner: None,
                size: None,
            })
            .collect())
    }

    async fn get_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        Ok(vec![SchemaInfo {
            name: "default".to_string(),
            database: database.to_string(),
        }])
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        // Use SCAN to avoid blocking the server with KEYS *.
        let mut conn = self.conn.lock().await;
        let mut cursor: u64 = 0;
        let mut keys: Vec<String> = Vec::new();
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("COUNT")
                .arg(200)
                .query_async(&mut *conn)
                .await
                .map_err(|e| DbError::QueryError(format!("SCAN failed: {}", e)))?;
            keys.extend(batch);
            cursor = next;
            if cursor == 0 || keys.len() >= MAX_KEYS_LISTED {
                break;
            }
        }
        keys.truncate(MAX_KEYS_LISTED);
        keys.sort();

        Ok(keys
            .into_iter()
            .map(|k| TableInfo {
                name: k,
                schema: schema.to_string(),
                row_count: Some(1),
                table_type: "KEY".to_string(),
            })
            .collect())
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        let mut conn = self.conn.lock().await;
        let key_type: String = redis::cmd("TYPE")
            .arg(table)
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbError::QueryError(format!("TYPE failed: {}", e)))?;

        let ttl: i64 = conn
            .ttl(table)
            .await
            .map_err(|e| DbError::QueryError(format!("TTL failed: {}", e)))?;

        let columns = vec![
            ColumnInfo {
                name: "key".to_string(),
                data_type: "STRING".to_string(),
                nullable: false,
                default_value: None,
                is_primary_key: true,
                is_auto_increment: false,
            },
            ColumnInfo {
                name: "type".to_string(),
                data_type: key_type.to_uppercase(),
                nullable: false,
                default_value: None,
                is_primary_key: false,
                is_auto_increment: false,
            },
            ColumnInfo {
                name: "ttl".to_string(),
                data_type: format!("INT ({})", ttl),
                nullable: true,
                default_value: None,
                is_primary_key: false,
                is_auto_increment: false,
            },
        ];

        Ok(TableSchema {
            table: TableInfo {
                name: table.to_string(),
                schema: schema.to_string(),
                row_count: Some(1),
                table_type: key_type.to_uppercase(),
            },
            columns,
            indexes: Vec::new(),
        })
    }

    async fn get_foreign_keys(&self, _schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        Ok(Vec::new())
    }

    async fn close(&self) -> Result<(), DbError> {
        // MultiplexedConnection drops cleanly when last clone is released.
        let _ = self.db_index;
        Ok(())
    }
}
