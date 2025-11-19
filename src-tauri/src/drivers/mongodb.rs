//! MongoDB database driver implementation
//!
//! This driver provides connectivity to MongoDB databases using the official
//! MongoDB Rust driver. Note that MongoDB is a NoSQL database, so some SQL-specific
//! concepts like schemas don't apply directly.

use async_trait::async_trait;
use futures_util::StreamExt;
use mongodb::{
    bson::{doc, Document},
    Client, Database,
};
use serde_json::Value as JsonValue;

use super::{ConnectionOptions, DatabaseDriver, QueryResult};
use crate::models::{ColumnInfo, DatabaseInfo, DbError, IndexInfo, SchemaInfo, TableInfo, TableSchema};

/// MongoDB database driver
///
/// Provides access to MongoDB databases. Note that MongoDB doesn't have
/// traditional schemas, so collections are treated as "tables" in the public schema.
pub struct MongoDbDriver {
    client: Client,
    database: Database,
    database_name: String,
}

impl MongoDbDriver {
    /// Parse a MongoDB query/command from a string
    ///
    /// This is a simplified parser that handles basic MongoDB operations:
    /// - db.collection.find({ ... })
    /// - db.collection.findOne({ ... })
    /// - db.collection.insertOne({ ... })
    /// - db.collection.insertMany([{ ... }])
    /// - db.collection.updateOne({ filter }, { update })
    /// - db.collection.deleteOne({ ... })
    /// - db.collection.aggregate([{ ... }])
    ///
    /// Returns (collection_name, operation, document)
    fn parse_query(sql: &str) -> Result<(String, String, String), DbError> {
        let trimmed = sql.trim();

        // Basic parsing for db.collection.operation(...)
        if !trimmed.starts_with("db.") {
            return Err(DbError::QueryError(
                "MongoDB queries must start with 'db.'".to_string(),
            ));
        }

        // Extract collection and operation
        let without_db = &trimmed[3..]; // Remove "db."
        let parts: Vec<&str> = without_db.splitn(2, '.').collect();

        if parts.len() != 2 {
            return Err(DbError::QueryError(
                "Invalid MongoDB query format. Use: db.collection.operation(...)".to_string(),
            ));
        }

        let collection = parts[0].to_string();
        let rest = parts[1];

        // Extract operation and parameters
        let operation_end = rest.find('(').ok_or_else(|| {
            DbError::QueryError("Missing opening parenthesis for operation".to_string())
        })?;

        let operation = rest[..operation_end].to_string();
        let params_start = operation_end + 1;
        let params_end = rest.rfind(')').ok_or_else(|| {
            DbError::QueryError("Missing closing parenthesis for operation".to_string())
        })?;

        let params = rest[params_start..params_end].trim().to_string();

        Ok((collection, operation, params))
    }

    /// Convert BSON document to JSON value
    fn bson_to_json(doc: &Document) -> JsonValue {
        // Use bson's built-in conversion
        match serde_json::to_value(doc) {
            Ok(val) => val,
            Err(_) => JsonValue::Null,
        }
    }

    /// Convert JSON value to Vec<JsonValue> for table row
    fn json_to_row(json: &JsonValue) -> Vec<JsonValue> {
        if let JsonValue::Object(map) = json {
            map.values().cloned().collect()
        } else {
            vec![json.clone()]
        }
    }

    /// Get column names from a JSON object
    fn get_columns(json: &JsonValue) -> Vec<String> {
        if let JsonValue::Object(map) = json {
            map.keys().cloned().collect()
        } else {
            vec!["value".to_string()]
        }
    }
}

#[async_trait]
impl DatabaseDriver for MongoDbDriver {
    async fn connect(opts: ConnectionOptions) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        // Build MongoDB connection string
        // Format: mongodb://username:password@host:port/database
        let auth = if let Some(password) = &opts.password {
            format!("{}:{}@", opts.username, password)
        } else {
            format!("{}@", opts.username)
        };

        let database_name = opts.database.clone().unwrap_or_else(|| "test".to_string());

        let connection_string = format!(
            "mongodb://{}{}:{}/{}",
            auth, opts.host, opts.port, database_name
        );

        // Connect to MongoDB
        let client = Client::with_uri_str(&connection_string)
            .await
            .map_err(|e| DbError::ConnectionError(format!("Failed to connect: {}", e)))?;

        // Get the database
        let database = client.database(&database_name);

        // Test the connection by running a ping command
        database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;

        Ok(Self {
            client,
            database,
            database_name,
        })
    }

    async fn test_connection(&self) -> Result<(), DbError> {
        self.database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| DbError::ConnectionError(format!("Connection test failed: {}", e)))?;
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<QueryResult, DbError> {
        // Parse the MongoDB query
        let (collection_name, operation, params) = Self::parse_query(sql)?;

        let collection = self.database.collection::<Document>(&collection_name);

        match operation.as_str() {
            "find" => {
                // Parse filter document
                let filter: Document = if params.is_empty() || params == "{}" {
                    doc! {}
                } else {
                    serde_json::from_str(&params)
                        .map_err(|e| DbError::QueryError(format!("Invalid filter: {}", e)))?
                };

                // Execute find query
                let mut cursor = collection
                    .find(filter)
                    .await
                    .map_err(|e| DbError::QueryError(format!("Find failed: {}", e)))?;

                let mut rows = Vec::new();
                let mut columns = Vec::new();

                // Fetch documents
                while let Some(result) = cursor.next().await {
                    let doc = result
                        .map_err(|e| DbError::QueryError(format!("Cursor error: {}", e)))?;

                    let json = Self::bson_to_json(&doc);

                    // Get columns from first document
                    if columns.is_empty() {
                        columns = Self::get_columns(&json);
                    }

                    rows.push(Self::json_to_row(&json));
                }

                Ok(QueryResult::with_data(columns, rows))
            }
            "findOne" => {
                // Parse filter document
                let filter: Document = if params.is_empty() || params == "{}" {
                    doc! {}
                } else {
                    serde_json::from_str(&params)
                        .map_err(|e| DbError::QueryError(format!("Invalid filter: {}", e)))?
                };

                // Execute findOne query
                let doc = collection
                    .find_one(filter)
                    .await
                    .map_err(|e| DbError::QueryError(format!("FindOne failed: {}", e)))?;

                if let Some(doc) = doc {
                    let json = Self::bson_to_json(&doc);
                    let columns = Self::get_columns(&json);
                    let row = Self::json_to_row(&json);
                    Ok(QueryResult::with_data(columns, vec![row]))
                } else {
                    Ok(QueryResult::empty())
                }
            }
            "insertOne" => {
                // Parse document to insert
                let document: Document = serde_json::from_str(&params)
                    .map_err(|e| DbError::QueryError(format!("Invalid document: {}", e)))?;

                // Execute insert
                collection
                    .insert_one(document)
                    .await
                    .map_err(|e| DbError::QueryError(format!("Insert failed: {}", e)))?;

                Ok(QueryResult::with_affected(1))
            }
            "insertMany" => {
                // Parse array of documents
                let documents: Vec<Document> = serde_json::from_str(&params)
                    .map_err(|e| DbError::QueryError(format!("Invalid documents array: {}", e)))?;

                let count = documents.len() as u64;

                // Execute insert many
                collection
                    .insert_many(documents)
                    .await
                    .map_err(|e| DbError::QueryError(format!("Insert many failed: {}", e)))?;

                Ok(QueryResult::with_affected(count))
            }
            "deleteOne" | "deleteMany" => {
                // Parse filter document
                let filter: Document = serde_json::from_str(&params)
                    .map_err(|e| DbError::QueryError(format!("Invalid filter: {}", e)))?;

                // Execute delete
                let result = if operation == "deleteOne" {
                    collection
                        .delete_one(filter)
                        .await
                } else {
                    collection
                        .delete_many(filter)
                        .await
                }
                .map_err(|e| DbError::QueryError(format!("Delete failed: {}", e)))?;

                Ok(QueryResult::with_affected(result.deleted_count))
            }
            "updateOne" | "updateMany" => {
                // Parse filter and update (params should be "filter, update")
                let parts: Vec<&str> = params.splitn(2, ',').collect();
                if parts.len() != 2 {
                    return Err(DbError::QueryError(
                        "Update requires filter and update parameters".to_string(),
                    ));
                }

                let filter: Document = serde_json::from_str(parts[0].trim())
                    .map_err(|e| DbError::QueryError(format!("Invalid filter: {}", e)))?;

                let update: Document = serde_json::from_str(parts[1].trim())
                    .map_err(|e| DbError::QueryError(format!("Invalid update: {}", e)))?;

                // Execute update
                let result = if operation == "updateOne" {
                    collection
                        .update_one(filter, update)
                        .await
                } else {
                    collection
                        .update_many(filter, update)
                        .await
                }
                .map_err(|e| DbError::QueryError(format!("Update failed: {}", e)))?;

                Ok(QueryResult::with_affected(result.modified_count))
            }
            "aggregate" => {
                // Parse pipeline array
                let pipeline: Vec<Document> = serde_json::from_str(&params)
                    .map_err(|e| DbError::QueryError(format!("Invalid pipeline: {}", e)))?;

                // Execute aggregation
                let mut cursor = collection
                    .aggregate(pipeline)
                    .await
                    .map_err(|e| DbError::QueryError(format!("Aggregation failed: {}", e)))?;

                let mut rows = Vec::new();
                let mut columns = Vec::new();

                // Fetch results
                while let Some(result) = cursor.next().await {
                    let doc = result
                        .map_err(|e| DbError::QueryError(format!("Cursor error: {}", e)))?;

                    let json = Self::bson_to_json(&doc);

                    if columns.is_empty() {
                        columns = Self::get_columns(&json);
                    }

                    rows.push(Self::json_to_row(&json));
                }

                Ok(QueryResult::with_data(columns, rows))
            }
            _ => Err(DbError::QueryError(format!(
                "Unsupported MongoDB operation: {}. Supported: find, findOne, insertOne, insertMany, updateOne, updateMany, deleteOne, deleteMany, aggregate",
                operation
            ))),
        }
    }

    async fn get_databases(&self) -> Result<Vec<DatabaseInfo>, DbError> {
        // List all databases
        let db_names = self
            .client
            .list_database_names()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to list databases: {}", e)))?;

        Ok(db_names
            .into_iter()
            .map(|name| DatabaseInfo {
                name,
                owner: None,
                size: None,
            })
            .collect())
    }

    async fn get_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>, DbError> {
        // MongoDB doesn't have schemas, return a single "public" schema
        Ok(vec![SchemaInfo {
            name: "public".to_string(),
            database: self.database_name.clone(),
        }])
    }

    async fn get_tables(&self, _schema: &str) -> Result<Vec<TableInfo>, DbError> {
        // Get list of collections (treated as tables)
        let collection_names = self
            .database
            .list_collection_names()
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to list collections: {}", e)))?;

        Ok(collection_names
            .into_iter()
            .map(|name| TableInfo {
                name,
                schema: "public".to_string(),
                table_type: "COLLECTION".to_string(),
                row_count: None,
            })
            .collect())
    }

    async fn get_table_schema(&self, _schema: &str, table: &str) -> Result<TableSchema, DbError> {
        // For MongoDB, we'll sample a document to infer the schema
        let collection = self.database.collection::<Document>(table);

        let sample_doc = collection
            .find_one(doc! {})
            .await
            .map_err(|e| DbError::QueryError(format!("Failed to sample collection: {}", e)))?;

        let columns = if let Some(doc) = sample_doc {
            doc.iter()
                .map(|(key, value)| ColumnInfo {
                    name: key.clone(),
                    data_type: match value {
                        mongodb::bson::Bson::Double(_) => "Double",
                        mongodb::bson::Bson::String(_) => "String",
                        mongodb::bson::Bson::Array(_) => "Array",
                        mongodb::bson::Bson::Document(_) => "Document",
                        mongodb::bson::Bson::Boolean(_) => "Boolean",
                        mongodb::bson::Bson::Int32(_) => "Int32",
                        mongodb::bson::Bson::Int64(_) => "Int64",
                        mongodb::bson::Bson::ObjectId(_) => "ObjectId",
                        mongodb::bson::Bson::DateTime(_) => "DateTime",
                        _ => "Mixed",
                    }
                    .to_string(),
                    nullable: true, // MongoDB fields are always nullable
                    default_value: None,
                    is_primary_key: key == "_id",
                })
                .collect()
        } else {
            // Empty collection, return minimal schema
            vec![ColumnInfo {
                name: "_id".to_string(),
                data_type: "ObjectId".to_string(),
                nullable: false,
                default_value: None,
                is_primary_key: true,
            }]
        };

        // MongoDB always has an _id index
        let indexes = vec![IndexInfo {
            name: "_id_".to_string(),
            columns: vec!["_id".to_string()],
            is_unique: true,
            is_primary: true,
        }];

        Ok(TableSchema {
            table: TableInfo {
                name: table.to_string(),
                schema: "public".to_string(),
                table_type: "COLLECTION".to_string(),
                row_count: None,
            },
            columns,
            indexes,
        })
    }

    async fn close(&self) -> Result<(), DbError> {
        // MongoDB driver handles cleanup automatically
        Ok(())
    }
}
