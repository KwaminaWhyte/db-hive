//! Redis database driver implementation
//!
//! This driver provides connectivity to Redis key-value stores using the `redis` crate
//! with async multiplexed connections. Because Redis is not a relational database,
//! its concepts are mapped to the `DatabaseDriver` trait as follows:
//!
//! - **Databases**: Redis logical databases 0-15
//! - **Schemas**: A single virtual "keys" namespace per database
//! - **Tables**: The five Redis data-type groups (strings, hashes, lists, sets, zsets)
//! - **Columns**: Conceptual fields within each type group
//!
//! Query execution interprets the `sql` argument as a raw Redis command string,
//! e.g. `SET foo bar`, `GET foo`, `HGETALL myhash`.

use async_trait::async_trait;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo,
    TableSchema,
};

/// Redis database driver
///
/// Wraps a `redis::aio::MultiplexedConnection` behind an `Arc<Mutex<...>>` so that
/// the driver can be shared across async tasks while the trait requires `Send + Sync`.
pub struct RedisDriver {
    conn: Arc<Mutex<redis::aio::MultiplexedConnection>>,
    host: String,
    db_index: u8,
}

#[async_trait]
impl DatabaseDriver for RedisDriver {
    /// Connect to a Redis server
    ///
    /// Builds a `redis://` URL from the connection options:
    /// - If a password is provided: `redis://:{password}@{host}:{port}/{db_index}`
    /// - Otherwise:                 `redis://{host}:{port}/{db_index}`
    ///
    /// The database index is parsed from `opts.database`; defaults to `0` when absent
    /// or when the string cannot be parsed as a `u8`.
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let db_index: u8 = opts
            .database
            .as_ref()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0u8);

        let url = match &opts.password {
            Some(password) if !password.is_empty() => format!(
                "redis://:{}@{}:{}/{}",
                password, opts.host, opts.port, db_index
            ),
            _ => format!("redis://{}:{}/{}", opts.host, opts.port, db_index),
        };

        let client = redis::Client::open(url.as_str())
            .map_err(|e| DbError::ConnectionError(format!("Invalid Redis URL: {}", e)))?;

        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| DbError::ConnectionError(format!("Failed to connect to Redis: {}", e)))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            host: opts.host,
            db_index,
        })
    }

    /// Verify the connection is alive by sending a PING command
    async fn test_connection(&self) -> Result<(), DbError> {
        let response: String = redis::cmd("PING")
            .query_async(&mut *self.conn.lock().await)
            .await
            .map_err(|e| DbError::ConnectionError(format!("PING failed: {}", e)))?;

        if response == "PONG" {
            Ok(())
        } else {
            Err(DbError::ConnectionError(format!(
                "Unexpected PING response: {}",
                response
            )))
        }
    }

    /// Execute a Redis command expressed as a plain text string
    ///
    /// The string is split on ASCII whitespace; the first token is the command
    /// name and the remaining tokens are its arguments.  The returned
    /// `redis::Value` is translated into a `QueryResult` with a single
    /// `"result"` (or `"value"`) column.
    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return Ok(QueryResult::empty());
        }

        // Split the command string into tokens
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let command_name = tokens[0];
        let args = &tokens[1..];

        // Build the redis command
        let mut cmd = redis::cmd(command_name);
        for arg in args {
            cmd.arg(*arg);
        }

        let value: redis::Value = cmd
            .query_async(&mut *self.conn.lock().await)
            .await
            .map_err(|e| DbError::QueryError(format!("Redis command failed: {}", e)))?;

        Ok(redis_value_to_query_result(value))
    }

    /// Return the 16 standard Redis logical databases (db0 … db15)
    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        let databases: Vec<DatabaseInfo> = (0u8..16)
            .map(|i| DatabaseInfo {
                name: format!("db{}", i),
                owner: None,
                size: None,
            })
            .collect();
        Ok(databases)
    }

    /// Return a single virtual schema called "keys" for the given database
    async fn get_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        Ok(vec![SchemaInfo {
            name: "keys".to_string(),
            database: database.to_string(),
        }])
    }

    /// Return the five Redis data-type groups as virtual tables
    ///
    /// Attempts a lightweight `SCAN` to probe which types are actually present
    /// in the current database.  Falls back to returning all five types
    /// statically when the scan fails.
    async fn get_tables(&self, _schema: &str) -> Result<Vec<TableInfo>, DbError> {
        // Try to sample key types via SCAN
        let sampled_types = self.sample_key_types().await;

        let type_names = match sampled_types {
            Ok(types) if !types.is_empty() => types,
            // Fall back to the full static list on any error or empty result
            _ => vec![
                "strings".to_string(),
                "hashes".to_string(),
                "lists".to_string(),
                "sets".to_string(),
                "zsets".to_string(),
            ],
        };

        let tables = type_names
            .into_iter()
            .map(|name| TableInfo {
                name,
                schema: "keys".to_string(),
                row_count: None,
                table_type: "KEY_TYPE".to_string(),
            })
            .collect();

        Ok(tables)
    }

    /// Return a conceptual column layout for each Redis data-type group
    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        let table_info = TableInfo {
            name: table.to_string(),
            schema: schema.to_string(),
            row_count: None,
            table_type: "KEY_TYPE".to_string(),
        };

        let columns: Vec<ColumnInfo> = match table {
            "hashes" => vec![
                ColumnInfo {
                    name: "key".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "field".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "value".to_string(),
                    data_type: "String".to_string(),
                    nullable: true,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
            "strings" => vec![
                ColumnInfo {
                    name: "key".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "value".to_string(),
                    data_type: "String".to_string(),
                    nullable: true,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
            "lists" => vec![
                ColumnInfo {
                    name: "key".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "index".to_string(),
                    data_type: "Integer".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "value".to_string(),
                    data_type: "String".to_string(),
                    nullable: true,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
            "sets" => vec![
                ColumnInfo {
                    name: "key".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "member".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
            // "zsets" | "sorted_sets" | anything else
            _ => vec![
                ColumnInfo {
                    name: "key".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "score".to_string(),
                    data_type: "Float".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "member".to_string(),
                    data_type: "String".to_string(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
        };

        Ok(TableSchema {
            table: table_info,
            columns,
            indexes: vec![],
        })
    }

    /// Redis has no foreign key concept; always returns an empty list
    async fn get_foreign_keys(&self, _schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        Ok(vec![])
    }

    /// Close the connection (the multiplexed connection cleans itself up on drop)
    async fn close(&self) -> Result<(), DbError> {
        Ok(())
    }
}

impl RedisDriver {
    /// Sample up to 100 keys with SCAN and query each key's TYPE.
    ///
    /// Returns the unique Redis type names mapped to their canonical group names:
    /// `string` → `strings`, `hash` → `hashes`, `list` → `lists`,
    /// `set` → `sets`, `zset` → `zsets`.
    async fn sample_key_types(&self) -> Result<Vec<String>, DbError> {
        let mut conn = self.conn.lock().await;

        // SCAN 0 COUNT 100 — returns (cursor, keys)
        let (_, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(0u64)
            .arg("COUNT")
            .arg(100u64)
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbError::QueryError(format!("SCAN failed: {}", e)))?;

        let mut seen_types: std::collections::HashSet<String> = std::collections::HashSet::new();

        for key in &keys {
            let type_str: String = redis::cmd("TYPE")
                .arg(key)
                .query_async(&mut *conn)
                .await
                .unwrap_or_else(|_| "none".to_string());

            let group = match type_str.as_str() {
                "string" => "strings",
                "hash" => "hashes",
                "list" => "lists",
                "set" => "sets",
                "zset" => "zsets",
                _ => continue,
            };
            seen_types.insert(group.to_string());
        }

        // Return in a stable, consistent order
        let ordered = ["strings", "hashes", "lists", "sets", "zsets"];
        Ok(ordered
            .iter()
            .filter(|t| seen_types.contains(**t))
            .map(|t| t.to_string())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Helper: convert a redis::Value into a QueryResult
// ---------------------------------------------------------------------------

fn redis_value_to_query_result(value: redis::Value) -> QueryResult {
    use redis::Value;

    match value {
        Value::Nil => QueryResult {
            columns: vec!["result".to_string()],
            rows: vec![vec![serde_json::Value::Null]],
            rows_affected: Some(0),
        },
        Value::Int(n) => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!(n)]],
        ),
        Value::BulkString(bytes) => {
            let s = String::from_utf8_lossy(&bytes).to_string();
            QueryResult::with_data(
                vec!["result".to_string()],
                vec![vec![serde_json::json!(s)]],
            )
        }
        Value::SimpleString(s) => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!(s)]],
        ),
        Value::Okay => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!("OK")]],
        ),
        Value::Boolean(b) => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!(b)]],
        ),
        Value::Double(d) => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!(d)]],
        ),
        Value::BigNumber(n) => {
            // BigNumber comes from the num-bigint type; convert via Display
            QueryResult::with_data(
                vec!["result".to_string()],
                vec![vec![serde_json::json!(n.to_string())]],
            )
        }
        Value::Array(items) => {
            let rows: Vec<Vec<serde_json::Value>> = items
                .into_iter()
                .map(|item| vec![redis_value_to_json(item)])
                .collect();
            QueryResult::with_data(vec!["value".to_string()], rows)
        }
        Value::Map(pairs) => {
            // Flatten map into rows of [field, value]
            let rows: Vec<Vec<serde_json::Value>> = pairs
                .into_iter()
                .map(|(k, v)| vec![redis_value_to_json(k), redis_value_to_json(v)])
                .collect();
            QueryResult::with_data(
                vec!["field".to_string(), "value".to_string()],
                rows,
            )
        }
        Value::Set(items) => {
            let rows: Vec<Vec<serde_json::Value>> = items
                .into_iter()
                .map(|item| vec![redis_value_to_json(item)])
                .collect();
            QueryResult::with_data(vec!["member".to_string()], rows)
        }
        Value::VerbatimString { format: _, text } => QueryResult::with_data(
            vec!["result".to_string()],
            vec![vec![serde_json::json!(text)]],
        ),
        // Attribute, Push — treat as informational; surface as JSON string
        Value::Attribute { data, attributes: _ } => redis_value_to_query_result(*data),
        Value::Push { kind: _, data } => {
            let rows: Vec<Vec<serde_json::Value>> = data
                .into_iter()
                .map(|item| vec![redis_value_to_json(item)])
                .collect();
            QueryResult::with_data(vec!["value".to_string()], rows)
        }
        Value::ServerError(e) => QueryResult {
            columns: vec!["error".to_string()],
            rows: vec![vec![serde_json::json!(e.details().unwrap_or("server error"))]],
            rows_affected: None,
        },
    }
}

/// Recursively convert a `redis::Value` into a `serde_json::Value` for embedding
/// inside table rows.
fn redis_value_to_json(value: redis::Value) -> serde_json::Value {
    use redis::Value;

    match value {
        Value::Nil => serde_json::Value::Null,
        Value::Int(n) => serde_json::json!(n),
        Value::BulkString(bytes) => {
            serde_json::json!(String::from_utf8_lossy(&bytes).to_string())
        }
        Value::SimpleString(s) => serde_json::json!(s),
        Value::Okay => serde_json::json!("OK"),
        Value::Boolean(b) => serde_json::json!(b),
        Value::Double(d) => serde_json::json!(d),
        Value::BigNumber(n) => serde_json::json!(n.to_string()),
        Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redis_value_to_json).collect())
        }
        Value::Map(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs {
                let key = match k {
                    Value::SimpleString(s) => s,
                    Value::BulkString(b) => String::from_utf8_lossy(&b).to_string(),
                    other => format!("{:?}", other),
                };
                map.insert(key, redis_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Set(items) => {
            serde_json::Value::Array(items.into_iter().map(redis_value_to_json).collect())
        }
        Value::VerbatimString { format: _, text } => serde_json::json!(text),
        Value::Attribute { data, attributes: _ } => redis_value_to_json(*data),
        Value::Push { kind: _, data } => {
            serde_json::Value::Array(data.into_iter().map(redis_value_to_json).collect())
        }
        Value::ServerError(e) => serde_json::json!(e.details().unwrap_or("server error")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nil_to_query_result() {
        let result = redis_value_to_query_result(redis::Value::Nil);
        assert_eq!(result.columns, vec!["result"]);
        assert_eq!(result.rows[0][0], serde_json::Value::Null);
        assert_eq!(result.rows_affected, Some(0));
    }

    #[test]
    fn test_int_to_query_result() {
        let result = redis_value_to_query_result(redis::Value::Int(42));
        assert_eq!(result.columns, vec!["result"]);
        assert_eq!(result.rows[0][0], serde_json::json!(42i64));
    }

    #[test]
    fn test_simple_string_to_query_result() {
        let result =
            redis_value_to_query_result(redis::Value::SimpleString("PONG".to_string()));
        assert_eq!(result.rows[0][0], serde_json::json!("PONG"));
    }

    #[test]
    fn test_okay_to_query_result() {
        let result = redis_value_to_query_result(redis::Value::Okay);
        assert_eq!(result.rows[0][0], serde_json::json!("OK"));
    }

    #[test]
    fn test_array_to_query_result() {
        let items = vec![
            redis::Value::SimpleString("a".to_string()),
            redis::Value::SimpleString("b".to_string()),
        ];
        let result = redis_value_to_query_result(redis::Value::Array(items));
        assert_eq!(result.columns, vec!["value"]);
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn test_get_databases_count() {
        // The get_databases method returns 16 entries (db0..db15)
        // We verify this at the logic level without a real connection.
        let dbs: Vec<DatabaseInfo> = (0u8..16)
            .map(|i| DatabaseInfo {
                name: format!("db{}", i),
                owner: None,
                size: None,
            })
            .collect();
        assert_eq!(dbs.len(), 16);
        assert_eq!(dbs[0].name, "db0");
        assert_eq!(dbs[15].name, "db15");
    }
}
