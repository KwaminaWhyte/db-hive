//! PostgreSQL database driver implementation
//!
//! This module provides the PostgreSQL implementation of the DatabaseDriver trait
//! using tokio-postgres for async database operations.

use async_trait::async_trait;
use tokio_postgres::{Client, NoTls};

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, IndexInfo, SchemaInfo, TableInfo, TableSchema,
};

/// PostgreSQL database driver
///
/// Manages connections to PostgreSQL databases and provides query execution
/// and metadata retrieval capabilities.
pub struct PostgresDriver {
    /// The active PostgreSQL client connection
    client: Client,
}

impl PostgresDriver {
    /// Build PostgreSQL connection string from options
    fn build_connection_string(opts: &ConnectionOptions) -> String {
        let mut parts = vec![
            format!("host={}", opts.host),
            format!("port={}", opts.port),
            format!("user={}", opts.username),
        ];

        if let Some(password) = &opts.password {
            parts.push(format!("password={}", password));
        }

        if let Some(database) = &opts.database {
            parts.push(format!("dbname={}", database));
        }

        if let Some(timeout) = opts.timeout {
            parts.push(format!("connect_timeout={}", timeout));
        }

        parts.join(" ")
    }

    /// Convert a postgres::Row to a Vec of JSON values
    fn row_to_json_vec(row: &tokio_postgres::Row) -> Vec<serde_json::Value> {
        let mut values = Vec::new();

        for i in 0..row.len() {
            // Get column type
            let column = &row.columns()[i];
            let type_name = column.type_().name();

            // Try to convert the value based on its type
            let value = match type_name {
                "bool" => row
                    .try_get::<_, Option<bool>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::Bool(v))
                    .unwrap_or(serde_json::Value::Null),
                "int2" | "int4" => row
                    .try_get::<_, Option<i32>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::Number(v.into()))
                    .unwrap_or(serde_json::Value::Null),
                "int8" => row
                    .try_get::<_, Option<i64>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::Number(v.into()))
                    .unwrap_or(serde_json::Value::Null),
                "float4" => row
                    .try_get::<_, Option<f32>>(i)
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::Number::from_f64(v as f64))
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                "float8" => row
                    .try_get::<_, Option<f64>>(i)
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::Number::from_f64(v))
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                "text" | "varchar" | "bpchar" | "name" | "char" => row
                    .try_get::<_, Option<String>>(i)
                    .ok()
                    .flatten()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
                "uuid" => row
                    .try_get::<_, Option<uuid::Uuid>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::String(v.to_string()))
                    .unwrap_or(serde_json::Value::Null),
                "timestamp" | "timestamptz" => row
                    .try_get::<_, Option<chrono::NaiveDateTime>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::String(v.to_string()))
                    .or_else(|| {
                        // Try as DateTime<Utc> for timestamptz
                        row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(i)
                            .ok()
                            .flatten()
                            .map(|v| serde_json::Value::String(v.to_rfc3339()))
                    })
                    .unwrap_or(serde_json::Value::Null),
                "date" => row
                    .try_get::<_, Option<chrono::NaiveDate>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::String(v.to_string()))
                    .unwrap_or(serde_json::Value::Null),
                "time" | "timetz" => row
                    .try_get::<_, Option<chrono::NaiveTime>>(i)
                    .ok()
                    .flatten()
                    .map(|v| serde_json::Value::String(v.to_string()))
                    .unwrap_or(serde_json::Value::Null),
                "numeric" | "decimal" => {
                    // Try as f64 first for numeric types
                    row.try_get::<_, Option<f64>>(i)
                        .ok()
                        .flatten()
                        .and_then(|v| serde_json::Number::from_f64(v))
                        .map(serde_json::Value::Number)
                        .or_else(|| {
                            // Fallback to string for very large or precise decimals
                            row.try_get::<_, Option<String>>(i)
                                .ok()
                                .flatten()
                                .map(serde_json::Value::String)
                        })
                        .unwrap_or(serde_json::Value::Null)
                },
                "json" | "jsonb" => {
                    // Get JSON as string and parse it
                    row.try_get::<_, Option<String>>(i)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(serde_json::Value::Null)
                },
                // For unknown types, try to get as string
                _ => row
                    .try_get::<_, Option<String>>(i)
                    .ok()
                    .flatten()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            };

            values.push(value);
        }

        values
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let connection_string = Self::build_connection_string(&opts);

        // Parse connection string and connect
        let (client, connection) = tokio_postgres::connect(&connection_string, NoTls)
            .await
            .map_err(|e| DbError::ConnectionError(format!("Failed to connect: {}", e)))?;

        // Spawn the connection handler in the background
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {}", e);
            }
        });

        Ok(Self { client })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        self.client
            .query("SELECT 1", &[])
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;

        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        // Check if SQL contains multiple statements (for transactions)
        // Simple heuristic: contains semicolons not in quotes
        let has_multiple_statements = sql.matches(';').count() > 1;

        if has_multiple_statements {
            // Use batch_execute for multi-statement SQL (transactions)
            self.client
                .batch_execute(sql)
                .await
                .map_err(|e| DbError::QueryError(format!("Transaction execution failed: {}", e)))?;

            // Return empty result for batch execution (no way to get affected rows count for all statements)
            return Ok(QueryResult::empty());
        }

        // Try to execute as a query first (for SELECT statements)
        match self.client.query(sql, &[]).await {
            Ok(rows) => {
                // Extract column names from the statement, even if there are no rows
                let columns: Vec<String> = if rows.is_empty() {
                    // For empty result sets, we need to execute the query to get column metadata
                    match self.client.prepare(sql).await {
                        Ok(statement) => statement
                            .columns()
                            .iter()
                            .map(|col| col.name().to_string())
                            .collect(),
                        Err(_) => vec![], // If preparation fails, return empty columns
                    }
                } else {
                    rows[0]
                        .columns()
                        .iter()
                        .map(|col| col.name().to_string())
                        .collect()
                };

                // Convert rows to JSON
                let data: Vec<Vec<serde_json::Value>> =
                    rows.iter().map(|row| Self::row_to_json_vec(row)).collect();

                Ok(QueryResult::with_data(columns, data))
            }
            Err(query_err) => {
                // If query failed, try as an execute (for INSERT/UPDATE/DELETE)
                match self.client.execute(sql, &[]).await {
                    Ok(rows_affected) => Ok(QueryResult::with_affected(rows_affected)),
                    Err(_execute_err) => {
                        // Both query and execute failed, return the query error with full details
                        Err(DbError::QueryError(format!("{}", query_err)))
                    }
                }
            }
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        let query = r#"
            SELECT
                datname as name,
                pg_catalog.pg_get_userbyid(datdba) as owner,
                pg_database_size(datname) as size
            FROM pg_database
            WHERE datistemplate = false
            ORDER BY datname
        "#;

        let rows = self
            .client
            .query(query, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch databases: {}", e)))?;

        let databases: Vec<DatabaseInfo> = rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let owner: Option<String> = row.get(1);
                let size: Option<i64> = row.get(2);

                DatabaseInfo {
                    name,
                    owner,
                    size: size.map(|s| s as u64),
                }
            })
            .collect();

        Ok(databases)
    }

    async fn get_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        let query = r#"
            SELECT
                schema_name as name
            FROM information_schema.schemata
            WHERE catalog_name = $1
                AND schema_name NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
            ORDER BY schema_name
        "#;

        let rows = self
            .client
            .query(query, &[&database])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch schemas: {}", e)))?;

        let schemas: Vec<SchemaInfo> = rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                SchemaInfo {
                    name,
                    database: database.to_string(),
                }
            })
            .collect();

        Ok(schemas)
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let query = r#"
            SELECT
                schemaname as schema,
                tablename as name,
                'TABLE' as table_type
            FROM pg_tables
            WHERE schemaname = $1
            UNION ALL
            SELECT
                schemaname as schema,
                viewname as name,
                'VIEW' as table_type
            FROM pg_views
            WHERE schemaname = $1
            UNION ALL
            SELECT
                schemaname as schema,
                matviewname as name,
                'MATERIALIZED VIEW' as table_type
            FROM pg_matviews
            WHERE schemaname = $1
            ORDER BY name
        "#;

        let rows = self
            .client
            .query(query, &[&schema])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch tables: {}", e)))?;

        let mut tables: Vec<TableInfo> = rows
            .iter()
            .map(|row| {
                let table_schema: String = row.get(0);
                let name: String = row.get(1);
                let table_type: String = row.get(2);

                TableInfo {
                    name,
                    schema: table_schema,
                    row_count: None,
                    table_type,
                }
            })
            .collect();

        // Fetch row counts for tables (not views) using pg_class.reltuples
        for table in tables.iter_mut() {
            if table.table_type == "TABLE" {
                let count_query = "SELECT reltuples::bigint FROM pg_class WHERE relname = $1 AND relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = $2)";

                if let Ok(row) = self.client.query_one(count_query, &[&table.name, &schema]).await {
                    table.row_count = row.get::<_, Option<i64>>(0).map(|v| v.max(0) as u64);
                }
            }
        }

        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        // Get column information
        let column_query = r#"
            SELECT
                c.column_name as name,
                c.data_type,
                c.is_nullable = 'YES' as nullable,
                c.column_default,
                CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END as is_primary_key
            FROM information_schema.columns c
            LEFT JOIN (
                SELECT ku.column_name
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage ku
                    ON tc.constraint_name = ku.constraint_name
                    AND tc.table_schema = ku.table_schema
                WHERE tc.constraint_type = 'PRIMARY KEY'
                    AND tc.table_schema = $1
                    AND tc.table_name = $2
            ) pk ON c.column_name = pk.column_name
            WHERE c.table_schema = $1
                AND c.table_name = $2
            ORDER BY c.ordinal_position
        "#;

        let column_rows = self
            .client
            .query(column_query, &[&schema, &table])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch columns: {}", e)))?;

        let columns: Vec<ColumnInfo> = column_rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let data_type: String = row.get(1);
                let nullable: bool = row.get(2);
                let default_value: Option<String> = row.get(3);
                let is_primary_key: bool = row.get(4);

                // Check if column is auto-increment (serial types or nextval in default)
                let is_auto_increment = data_type.to_lowercase().contains("serial")
                    || default_value
                        .as_ref()
                        .map(|dv| dv.to_lowercase().contains("nextval"))
                        .unwrap_or(false);

                ColumnInfo {
                    name,
                    data_type,
                    nullable,
                    default_value,
                    is_primary_key,
                    is_auto_increment,
                }
            })
            .collect();

        // Get index information
        let index_query = r#"
            SELECT
                i.relname as index_name,
                array_agg(a.attname ORDER BY array_position(ix.indkey, a.attnum)) as columns,
                ix.indisunique as is_unique,
                ix.indisprimary as is_primary
            FROM pg_index ix
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            CROSS JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS u(attnum, ord)
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = u.attnum
            WHERE n.nspname = $1
                AND t.relname = $2
            GROUP BY i.relname, ix.indisunique, ix.indisprimary
            ORDER BY i.relname
        "#;

        let index_rows = self
            .client
            .query(index_query, &[&schema, &table])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to fetch indexes: {}", e)))?;

        let indexes: Vec<IndexInfo> = index_rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let columns: Vec<String> = row.get(1);
                let is_unique: bool = row.get(2);
                let is_primary: bool = row.get(3);

                IndexInfo {
                    name,
                    columns,
                    is_unique,
                    is_primary,
                }
            })
            .collect();

        // Create TableInfo
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

    async fn close(&self) -> Result<(), DbError> {
        // Connection will be automatically closed when the client is dropped
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_string_builder() {
        let opts = ConnectionOptions {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: Some("secret".to_string()),
            database: Some("testdb".to_string()),
            timeout: Some(30),
        };

        let conn_str = PostgresDriver::build_connection_string(&opts);

        assert!(conn_str.contains("host=localhost"));
        assert!(conn_str.contains("port=5432"));
        assert!(conn_str.contains("user=postgres"));
        assert!(conn_str.contains("password=secret"));
        assert!(conn_str.contains("dbname=testdb"));
        assert!(conn_str.contains("connect_timeout=30"));
    }

    #[test]
    fn test_connection_string_without_optional_fields() {
        let opts = ConnectionOptions {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: None,
            database: None,
            timeout: None,
        };

        let conn_str = PostgresDriver::build_connection_string(&opts);

        assert!(conn_str.contains("host=localhost"));
        assert!(conn_str.contains("port=5432"));
        assert!(conn_str.contains("user=postgres"));
        assert!(!conn_str.contains("password="));
        assert!(!conn_str.contains("dbname="));
        assert!(!conn_str.contains("connect_timeout="));
    }
}
