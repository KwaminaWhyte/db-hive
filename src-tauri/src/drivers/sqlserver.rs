//! SQL Server database driver implementation
//!
//! This module provides the SQL Server implementation of the DatabaseDriver trait
//! using tiberius for async database operations with Microsoft SQL Server.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{
    ColumnInfo, DatabaseInfo, DbError, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo,
    TableSchema,
};

/// SQL Server database driver
///
/// Manages connections to Microsoft SQL Server databases and provides query execution
/// and metadata retrieval capabilities.
pub struct SqlServerDriver {
    /// The active SQL Server client connection wrapped in a Mutex for safe concurrent access
    client: Arc<Mutex<Client<tokio_util::compat::Compat<TcpStream>>>>,
}

impl SqlServerDriver {
    /// Build SQL Server config from connection options
    fn build_config(opts: &ConnectionOptions) -> Result<Config, DbError> {
        let mut config = Config::new();

        config.host(&opts.host);
        config.port(opts.port);

        // Set authentication (SQL Server authentication only, Windows auth requires integrated-auth-gssapi feature)
        if let Some(password) = &opts.password {
            config.authentication(AuthMethod::sql_server(&opts.username, password));
        } else {
            return Err(DbError::AuthError(
                "Password is required for SQL Server authentication".to_string(),
            ));
        }

        // Set database if provided
        if let Some(database) = &opts.database {
            config.database(database);
        }

        // Set encryption level (not supported to avoid TLS issues)
        config.encryption(EncryptionLevel::NotSupported);

        // Set trust server certificate (for self-signed certificates)
        config.trust_cert();

        Ok(config)
    }

    /// Convert a tiberius Row to a Vec of JSON values
    fn row_to_json_vec(row: &tiberius::Row) -> Vec<serde_json::Value> {
        let mut values = Vec::new();

        for i in 0..row.len() {
            let value = match row.try_get::<&str, usize>(i) {
                Ok(Some(v)) => serde_json::Value::String(v.to_string()),
                Ok(None) => serde_json::Value::Null,
                Err(_) => {
                    // Try other types
                    if let Ok(Some(v)) = row.try_get::<i32, usize>(i) {
                        serde_json::Value::Number(v.into())
                    } else if let Ok(Some(v)) = row.try_get::<i64, usize>(i) {
                        serde_json::Value::Number(v.into())
                    } else if let Ok(Some(v)) = row.try_get::<f64, usize>(i) {
                        serde_json::Number::from_f64(v)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null)
                    } else if let Ok(Some(v)) = row.try_get::<bool, usize>(i) {
                        serde_json::Value::Bool(v)
                    } else if let Ok(Some(v)) = row.try_get::<uuid::Uuid, usize>(i) {
                        serde_json::Value::String(v.to_string())
                    } else if let Ok(Some(v)) = row.try_get::<chrono::NaiveDateTime, usize>(i) {
                        serde_json::Value::String(v.to_string())
                    } else {
                        // Fallback to null for unknown types
                        serde_json::Value::Null
                    }
                }
            };

            values.push(value);
        }

        values
    }
}

#[async_trait]
impl DatabaseDriver for SqlServerDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let config = Self::build_config(&opts)?;

        // Create TCP connection
        let tcp = TcpStream::connect(config.get_addr())
            .await
            .map_err(|e| {
                DbError::ConnectionError(format!("Failed to connect to SQL Server: {}", e))
            })?;

        // Wrap in compat for tiberius
        tcp.set_nodelay(true).map_err(|e| {
            DbError::ConnectionError(format!("Failed to set TCP nodelay: {}", e))
        })?;

        let tcp_compat = tcp.compat_write();

        // Connect to SQL Server
        let client = Client::connect(config, tcp_compat)
            .await
            .map_err(|e| {
                DbError::ConnectionError(format!("SQL Server connection failed: {}", e))
            })?;

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        // SQL Server uses SELECT 1 for connection testing
        let mut client = self.client.lock().await;
        let _ = client
            .query("SELECT 1", &[])
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;

        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        let mut client = self.client.lock().await;

        // Execute query
        let mut stream = client
            .query(sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Query execution failed: {}", e)))?;

        // Get column names
        let columns = stream
            .columns()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get columns: {}", e)))?
            .unwrap_or(&[]);

        let column_names: Vec<String> = columns
            .iter()
            .map(|col| col.name().to_string())
            .collect();

        // Collect rows
        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read query results: {}", e)))?;

        let mut rows = Vec::new();
        for row in row_stream {
            rows.push(Self::row_to_json_vec(&row));
        }

        // For DML statements, get rows affected
        let rows_affected = if column_names.is_empty() {
            Some(rows.len() as u64)
        } else {
            None
        };

        Ok(QueryResult {
            columns: column_names,
            rows,
            rows_affected,
        })
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        let sql = "SELECT name FROM sys.databases WHERE name NOT IN ('master', 'tempdb', 'model', 'msdb') ORDER BY name";

        let mut client = self.client.lock().await;

        let stream = client
            .query(sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get databases: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read databases: {}", e)))?;

        let mut databases = Vec::new();
        for row in row_stream {
            let name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse database name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Database name is null".to_string()))?;

            databases.push(DatabaseInfo {
                name: name.to_string(),
                owner: None,
                size: None,
            });
        }

        Ok(databases)
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        // SQL Server stores schemas in sys.schemas
        let sql = "SELECT name FROM sys.schemas WHERE name NOT IN ('sys', 'INFORMATION_SCHEMA', 'guest', 'db_owner', 'db_accessadmin', 'db_securityadmin', 'db_ddladmin', 'db_backupoperator', 'db_datareader', 'db_datawriter', 'db_denydatareader', 'db_denydatawriter') ORDER BY name";

        let mut client = self.client.lock().await;

        let stream = client
            .query(sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get schemas: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read schemas: {}", e)))?;

        let mut schemas = Vec::new();
        for row in row_stream {
            let name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse schema name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Schema name is null".to_string()))?;

            schemas.push(SchemaInfo {
                name: name.to_string(),
                database: _database.to_string(),
            });
        }

        Ok(schemas)
    }

    async fn get_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let sql = format!(
            "SELECT t.name, t.type_desc
             FROM sys.tables t
             INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
             WHERE s.name = '{}'
             ORDER BY t.name",
            schema
        );

        let mut client = self.client.lock().await;

        let stream = client
            .query(&sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get tables: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read tables: {}", e)))?;

        let mut tables = Vec::new();
        for row in row_stream {
            let name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse table name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Table name is null".to_string()))?;

            let type_desc: &str = row
                .try_get(1)
                .map_err(|e| DbError::QueryError(format!("Failed to parse table type: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Table type is null".to_string()))?;

            tables.push(TableInfo {
                name: name.to_string(),
                schema: schema.to_string(),
                row_count: None,
                table_type: type_desc.to_string(),
            });
        }

        Ok(tables)
    }

    async fn get_table_schema(&self, schema: &str, table: &str) -> Result<TableSchema, DbError> {
        // Get columns
        let columns_sql = format!(
            "SELECT
                c.name,
                t.name AS data_type,
                c.is_nullable,
                dc.definition AS default_value,
                CASE WHEN pk.column_id IS NOT NULL THEN 1 ELSE 0 END AS is_primary_key,
                c.is_identity AS is_auto_increment
             FROM sys.columns c
             INNER JOIN sys.types t ON c.user_type_id = t.user_type_id
             LEFT JOIN sys.default_constraints dc ON c.default_object_id = dc.object_id
             LEFT JOIN (
                 SELECT ic.object_id, ic.column_id
                 FROM sys.index_columns ic
                 INNER JOIN sys.indexes i ON ic.object_id = i.object_id AND ic.index_id = i.index_id
                 WHERE i.is_primary_key = 1
             ) pk ON c.object_id = pk.object_id AND c.column_id = pk.column_id
             WHERE c.object_id = OBJECT_ID('{}.{}')
             ORDER BY c.column_id",
            schema, table
        );

        let mut client = self.client.lock().await;

        let stream = client
            .query(&columns_sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get columns: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read columns: {}", e)))?;

        let mut columns = Vec::new();
        for row in row_stream {
            let name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse column name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Column name is null".to_string()))?;

            let data_type: &str = row
                .try_get(1)
                .map_err(|e| DbError::QueryError(format!("Failed to parse data type: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Data type is null".to_string()))?;

            let is_nullable: bool = row
                .try_get(2)
                .map_err(|e| DbError::QueryError(format!("Failed to parse nullable: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Nullable is null".to_string()))?;

            let default_value: Option<&str> = row
                .try_get(3)
                .map_err(|e| DbError::QueryError(format!("Failed to parse default value: {}", e)))?;

            let is_primary_key: i32 = row
                .try_get(4)
                .map_err(|e| DbError::QueryError(format!("Failed to parse is_primary_key: {}", e)))?
                .ok_or_else(|| DbError::QueryError("is_primary_key is null".to_string()))?;

            let is_auto_increment: bool = row
                .try_get(5)
                .map_err(|e| DbError::QueryError(format!("Failed to parse is_identity: {}", e)))?
                .ok_or_else(|| DbError::QueryError("is_identity is null".to_string()))?;

            columns.push(ColumnInfo {
                name: name.to_string(),
                data_type: data_type.to_string(),
                nullable: is_nullable,
                default_value: default_value.map(|s| s.to_string()),
                is_primary_key: is_primary_key == 1,
                is_auto_increment,
            });
        }

        // Get indexes - Note: STRING_AGG requires SQL Server 2017+
        // For compatibility, we'll use a simpler query
        let indexes_sql = format!(
            "SELECT DISTINCT
                i.name,
                i.is_unique,
                i.is_primary_key
             FROM sys.indexes i
             WHERE i.object_id = OBJECT_ID('{}.{}')
             AND i.name IS NOT NULL
             ORDER BY i.name",
            schema, table
        );

        let stream = client
            .query(&indexes_sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get indexes: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read indexes: {}", e)))?;

        let mut indexes = Vec::new();
        for row in row_stream {
            let name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse index name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Index name is null".to_string()))?;

            let is_unique: bool = row
                .try_get(1)
                .map_err(|e| DbError::QueryError(format!("Failed to parse is_unique: {}", e)))?
                .ok_or_else(|| DbError::QueryError("is_unique is null".to_string()))?;

            let is_primary: bool = row
                .try_get(2)
                .map_err(|e| DbError::QueryError(format!("Failed to parse is_primary_key: {}", e)))?
                .ok_or_else(|| DbError::QueryError("is_primary_key is null".to_string()))?;

            // For now, we'll leave columns empty as getting them requires a more complex query
            indexes.push(IndexInfo {
                name: name.to_string(),
                columns: Vec::new(),
                is_unique,
                is_primary,
            });
        }

        Ok(TableSchema {
            table: TableInfo {
                name: table.to_string(),
                schema: schema.to_string(),
                row_count: None,
                table_type: "TABLE".to_string(),
            },
            columns,
            indexes,
        })
    }

    async fn get_foreign_keys(&self, schema: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let sql = format!(
            "SELECT
                fk.name AS constraint_name,
                OBJECT_NAME(fk.parent_object_id) AS table_name,
                s1.name AS schema_name,
                COL_NAME(fkc.parent_object_id, fkc.parent_column_id) AS column_name,
                OBJECT_NAME(fk.referenced_object_id) AS referenced_table_name,
                s2.name AS referenced_schema_name,
                COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id) AS referenced_column_name,
                fk.delete_referential_action_desc,
                fk.update_referential_action_desc
             FROM sys.foreign_keys fk
             INNER JOIN sys.foreign_key_columns fkc ON fk.object_id = fkc.constraint_object_id
             INNER JOIN sys.schemas s1 ON fk.schema_id = s1.schema_id
             INNER JOIN sys.schemas s2 ON OBJECT_SCHEMA_NAME(fk.referenced_object_id) = s2.name
             WHERE s1.name = '{}'
             ORDER BY fk.name, fkc.constraint_column_id",
            schema
        );

        let mut client = self.client.lock().await;

        let stream = client
            .query(&sql, &[])
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to get foreign keys: {}", e)))?;

        let row_stream = stream
            .into_first_result()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to read foreign keys: {}", e)))?;

        // Group by constraint name
        let mut fk_map: std::collections::HashMap<String, ForeignKeyInfo> =
            std::collections::HashMap::new();

        for row in row_stream {
            let constraint_name: &str = row
                .try_get(0)
                .map_err(|e| DbError::QueryError(format!("Failed to parse constraint name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Constraint name is null".to_string()))?;

            let table_name: &str = row
                .try_get(1)
                .map_err(|e| DbError::QueryError(format!("Failed to parse table name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Table name is null".to_string()))?;

            let schema_name: &str = row
                .try_get(2)
                .map_err(|e| DbError::QueryError(format!("Failed to parse schema name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Schema name is null".to_string()))?;

            let column_name: &str = row
                .try_get(3)
                .map_err(|e| DbError::QueryError(format!("Failed to parse column name: {}", e)))?
                .ok_or_else(|| DbError::QueryError("Column name is null".to_string()))?;

            let referenced_table_name: &str = row
                .try_get(4)
                .map_err(|e| {
                    DbError::QueryError(format!("Failed to parse referenced table name: {}", e))
                })?
                .ok_or_else(|| DbError::QueryError("Referenced table name is null".to_string()))?;

            let referenced_schema_name: &str = row
                .try_get(5)
                .map_err(|e| {
                    DbError::QueryError(format!("Failed to parse referenced schema name: {}", e))
                })?
                .ok_or_else(|| DbError::QueryError("Referenced schema name is null".to_string()))?;

            let referenced_column_name: &str = row
                .try_get(6)
                .map_err(|e| {
                    DbError::QueryError(format!("Failed to parse referenced column name: {}", e))
                })?
                .ok_or_else(|| DbError::QueryError("Referenced column name is null".to_string()))?;

            let on_delete: Option<&str> = row
                .try_get(7)
                .map_err(|e| DbError::QueryError(format!("Failed to parse on_delete: {}", e)))?;

            let on_update: Option<&str> = row
                .try_get(8)
                .map_err(|e| DbError::QueryError(format!("Failed to parse on_update: {}", e)))?;

            // Get or create ForeignKeyInfo
            let fk_info = fk_map
                .entry(constraint_name.to_string())
                .or_insert_with(|| ForeignKeyInfo {
                    name: constraint_name.to_string(),
                    table: table_name.to_string(),
                    schema: schema_name.to_string(),
                    columns: Vec::new(),
                    referenced_table: referenced_table_name.to_string(),
                    referenced_schema: referenced_schema_name.to_string(),
                    referenced_columns: Vec::new(),
                    on_delete: on_delete.map(|s| s.to_string()),
                    on_update: on_update.map(|s| s.to_string()),
                });

            // Add columns
            fk_info.columns.push(column_name.to_string());
            fk_info
                .referenced_columns
                .push(referenced_column_name.to_string());
        }

        Ok(fk_map.into_values().collect())
    }

    async fn close(&self) -> Result<(), DbError> {
        // Tiberius client doesn't need explicit close
        // Connection will be closed when dropped
        Ok(())
    }
}
