---
name: database-driver
description: Implement database driver with connection handling, metadata queries, and result streaming. Use when creating or extending database drivers for PostgreSQL, MySQL, SQLite, MongoDB, or SQL Server.
allowed-tools: Read, Write, Edit, Grep
---

# Database Driver Implementation

This skill helps you implement database drivers following the project's driver interface pattern.

## When to Use

Use this skill when:
- Implementing a new database driver
- Adding metadata queries for a database
- Implementing query streaming
- Extending driver functionality
- Debugging connection issues

## Driver Interface

All drivers must implement:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    async fn connect(opts: ConnectionOptions) -> Result<Box<dyn ConnectionHandle>, DbError>
    where
        Self: Sized;

    fn driver_name(&self) -> &'static str;
    fn driver_version(&self) -> &'static str;
}

#[async_trait]
pub trait ConnectionHandle: Send + Sync {
    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError>;
    async fn execute_query_streaming(&self, sql: &str, batch_size: usize) -> Result<QueryStream, DbError>;
    async fn cancel_query(&self, query_id: &str) -> Result<(), DbError>;
    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError>;
    async fn get_schemas(&self, database: Option<&str>) -> Result<Vec<SchemaInfo>, DbError>;
    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError>;
    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError>;
    async fn ping(&self) -> Result<(), DbError>;
    async fn get_version(&self) -> Result<String, DbError>;
    async fn close(self: Box<Self>) -> Result<(), DbError>;
}
```

## PostgreSQL Driver Example

```rust
use tokio_postgres::{Client, Config, NoTls};
use async_trait::async_trait;

pub struct PostgresDriver {
    client: Client,
    _connection: tokio::task::JoinHandle<()>,
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Box<dyn ConnectionHandle>, DbError> {
        let mut config = Config::new();
        config.host(&opts.host);
        config.port(opts.port);
        config.user(&opts.username);
        config.password(&opts.password);
        if let Some(db) = &opts.database {
            config.dbname(db);
        }

        let (client, connection) = config.connect(NoTls).await?;

        // Spawn connection handler
        let handle = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Connection error: {}", e);
            }
        });

        Ok(Box::new(PostgresDriver {
            client,
            _connection: handle,
        }))
    }

    fn driver_name(&self) -> &'static str {
        "postgresql"
    }

    fn driver_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

#[async_trait]
impl ConnectionHandle for PostgresDriver {
    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let rows = self.client.query(sql, &[]).await?;

        let columns = rows
            .first()
            .map(|row| {
                row.columns()
                    .iter()
                    .map(|col| ColumnInfo {
                        name: col.name().to_string(),
                        data_type: col.type_().name().to_string(),
                        nullable: true, // PostgreSQL doesn't provide this in result
                        ..Default::default()
                    })
                    .collect()
            })
            .unwrap_or_default();

        let data = rows
            .into_iter()
            .map(|row| {
                row.columns()
                    .iter()
                    .enumerate()
                    .map(|(i, col)| {
                        let value = row.get::<_, Option<String>>(i);
                        (col.name().to_string(), value)
                    })
                    .collect()
            })
            .collect();

        Ok(QueryResult { columns, data })
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        let query = r#"
            SELECT
                datname as name,
                pg_size_pretty(pg_database_size(datname)) as size,
                (SELECT count(*) FROM pg_tables WHERE schemaname NOT IN ('pg_catalog', 'information_schema')) as tables_count
            FROM pg_database
            WHERE datistemplate = false
            ORDER BY datname
        "#;

        let rows = self.client.query(query, &[]).await?;

        let databases = rows
            .into_iter()
            .map(|row| DatabaseInfo {
                name: row.get(0),
                size: row.get(1),
                tables_count: row.get(2),
            })
            .collect();

        Ok(databases)
    }

    async fn get_schemas(&self, _database: Option<&str>) -> Result<Vec<SchemaInfo>, DbError> {
        let query = r#"
            SELECT
                schema_name as name,
                (SELECT count(*) FROM information_schema.tables WHERE table_schema = schema_name) as tables_count
            FROM information_schema.schemata
            WHERE schema_name NOT IN ('pg_catalog', 'information_schema')
            ORDER BY schema_name
        "#;

        let rows = self.client.query(query, &[]).await?;

        let schemas = rows
            .into_iter()
            .map(|row| SchemaInfo {
                name: row.get(0),
                tables_count: row.get::<_, i64>(1) as usize,
            })
            .collect();

        Ok(schemas)
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let query = r#"
            SELECT
                schemaname,
                tablename,
                pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
            FROM pg_tables
            WHERE schemaname = $1
            ORDER BY tablename
        "#;

        let rows = self.client.query(query, &[&schema]).await?;

        let tables = rows
            .into_iter()
            .map(|row| TableInfo {
                schema: row.get(0),
                name: row.get(1),
                size: row.get(2),
                row_count: None,
                table_type: TableType::Table,
            })
            .collect();

        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        // Columns
        let query = r#"
            SELECT
                c.column_name,
                c.data_type,
                c.is_nullable::boolean,
                c.column_default,
                CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END as is_primary,
                CASE WHEN u.column_name IS NOT NULL THEN true ELSE false END as is_unique
            FROM information_schema.columns c
            LEFT JOIN (
                SELECT ku.column_name
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage ku ON tc.constraint_name = ku.constraint_name
                WHERE tc.constraint_type = 'PRIMARY KEY'
                    AND tc.table_schema = $1
                    AND tc.table_name = $2
            ) pk ON c.column_name = pk.column_name
            LEFT JOIN (
                SELECT ku.column_name
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage ku ON tc.constraint_name = ku.constraint_name
                WHERE tc.constraint_type = 'UNIQUE'
                    AND tc.table_schema = $1
                    AND tc.table_name = $2
            ) u ON c.column_name = u.column_name
            WHERE c.table_schema = $1
                AND c.table_name = $2
            ORDER BY c.ordinal_position
        "#;

        let rows = self.client.query(query, &[&schema, &table]).await?;

        let columns = rows
            .into_iter()
            .map(|row| ColumnInfo {
                name: row.get(0),
                data_type: row.get(1),
                nullable: row.get(2),
                default_value: row.get(3),
                is_primary: row.get(4),
                is_unique: row.get(5),
                comment: None,
            })
            .collect();

        // Get indexes, foreign keys, etc. (simplified for brevity)
        let indexes = Vec::new();
        let foreign_keys = Vec::new();
        let primary_key = None;

        Ok(TableSchema {
            columns,
            indexes,
            foreign_keys,
            primary_key,
        })
    }

    async fn ping(&self) -> Result<(), DbError> {
        self.client.query("SELECT 1", &[]).await?;
        Ok(())
    }

    async fn get_version(&self) -> Result<String, DbError> {
        let row = self.client.query_one("SELECT version()", &[]).await?;
        Ok(row.get(0))
    }

    async fn close(self: Box<Self>) -> Result<(), DbError> {
        // Connection will be closed when dropped
        Ok(())
    }
}
```

## Streaming Implementation

```rust
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

pub struct QueryStream {
    receiver: mpsc::Receiver<Result<Vec<Row>, DbError>>,
    cancel_token: CancellationToken,
}

impl QueryStream {
    pub async fn next(&mut self) -> Option<Result<Vec<Row>, DbError>> {
        self.receiver.recv().await
    }

    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }
}

async fn execute_query_streaming(
    &self,
    sql: &str,
    batch_size: usize,
) -> Result<QueryStream, DbError> {
    let (tx, rx) = mpsc::channel(10);
    let cancel_token = CancellationToken::new();
    let cancel_clone = cancel_token.clone();

    let sql = sql.to_string();
    let client = self.client.clone();

    tokio::spawn(async move {
        let result = client.query_raw(&sql, &[]).await;

        let mut stream = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(Err(e.into())).await;
                return;
            }
        };

        let mut batch = Vec::new();

        loop {
            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    break;
                }
                row = stream.next() => {
                    match row {
                        Some(Ok(row)) => {
                            batch.push(row);
                            if batch.len() >= batch_size {
                                if tx.send(Ok(batch.clone())).await.is_err() {
                                    break;
                                }
                                batch.clear();
                            }
                        }
                        Some(Err(e)) => {
                            let _ = tx.send(Err(e.into())).await;
                            break;
                        }
                        None => {
                            if !batch.is_empty() {
                                let _ = tx.send(Ok(batch)).await;
                            }
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(QueryStream {
        receiver: rx,
        cancel_token,
    })
}
```

## MySQL Driver Template

```rust
use mysql_async::{Pool, Conn, prelude::*};

pub struct MySqlDriver {
    pool: Pool,
}

impl MySqlDriver {
    pub async fn connect(opts: ConnectionOptions) -> Result<Self, DbError> {
        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            opts.username,
            opts.password,
            opts.host,
            opts.port,
            opts.database.unwrap_or_default()
        );

        let pool = Pool::new(url.as_str());

        Ok(Self { pool })
    }

    async fn get_conn(&self) -> Result<Conn, DbError> {
        Ok(self.pool.get_conn().await?)
    }
}
```

## SQLite Driver Template

```rust
use rusqlite::{Connection, params};

pub struct SqliteDriver {
    conn: Connection,
}

impl SqliteDriver {
    pub fn connect(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }
}
```

## Testing Drivers

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection() {
        let opts = test_connection_options();
        let driver = PostgresDriver::connect(opts).await;
        assert!(driver.is_ok());
    }

    #[tokio::test]
    async fn test_query() {
        let driver = setup_test_driver().await;
        let result = driver.execute_query("SELECT 1 as num").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_metadata() {
        let driver = setup_test_driver().await;
        let databases = driver.get_databases().await.unwrap();
        assert!(!databases.is_empty());
    }
}
```

## Common Metadata Queries

### List Tables (Generic SQL)
```sql
SELECT table_schema, table_name
FROM information_schema.tables
WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
ORDER BY table_name;
```

### Table Columns (Generic SQL)
```sql
SELECT column_name, data_type, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = ? AND table_name = ?
ORDER BY ordinal_position;
```

## Remember

- Implement all trait methods
- Use connection pooling for performance
- Stream large result sets
- Handle database-specific quirks
- Test with real databases
- Document database version compatibility
