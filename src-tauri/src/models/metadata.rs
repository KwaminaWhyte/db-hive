//! Database metadata types
//!
//! This module defines the data structures for representing database metadata,
//! including databases, schemas, tables, columns, and indexes. These types are
//! used for the schema browser functionality.

use serde::{Deserialize, Serialize};

/// Database information
///
/// Represents a database/catalog within a database server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseInfo {
    /// Database name
    pub name: String,

    /// Database owner/creator (if available)
    pub owner: Option<String>,

    /// Database size in bytes (if available)
    pub size: Option<u64>,
}

impl DatabaseInfo {
    /// Create a new DatabaseInfo with just a name
    pub fn new(name: String) -> Self {
        Self {
            name,
            owner: None,
            size: None,
        }
    }

    /// Create a DatabaseInfo with all fields
    pub fn with_details(name: String, owner: Option<String>, size: Option<u64>) -> Self {
        Self { name, owner, size }
    }
}

/// Schema information
///
/// Represents a schema/namespace within a database.
/// Some databases (like MySQL) don't have a separate schema concept,
/// in which case the schema name may be the same as the database name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaInfo {
    /// Schema name
    pub name: String,

    /// Parent database name
    pub database: String,
}

impl SchemaInfo {
    /// Create a new SchemaInfo
    pub fn new(name: String, database: String) -> Self {
        Self { name, database }
    }
}

/// Table information
///
/// Represents a table or view within a schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    /// Table name
    pub name: String,

    /// Parent schema name
    pub schema: String,

    /// Approximate row count (if available)
    /// May be None for views or if statistics are not available
    pub row_count: Option<u64>,

    /// Table type: "TABLE", "VIEW", "MATERIALIZED VIEW", etc.
    pub table_type: String,
}

impl TableInfo {
    /// Create a new TableInfo with basic information
    pub fn new(name: String, schema: String, table_type: String) -> Self {
        Self {
            name,
            schema,
            row_count: None,
            table_type,
        }
    }

    /// Create a TableInfo with row count
    pub fn with_row_count(
        name: String,
        schema: String,
        table_type: String,
        row_count: Option<u64>,
    ) -> Self {
        Self {
            name,
            schema,
            row_count,
            table_type,
        }
    }

    /// Check if this is a view
    pub fn is_view(&self) -> bool {
        self.table_type.to_uppercase().contains("VIEW")
    }
}

/// Column information
///
/// Represents a column within a table, including its data type and constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    /// Column name
    pub name: String,

    /// Data type (e.g., "VARCHAR(255)", "INTEGER", "TIMESTAMP")
    pub data_type: String,

    /// Whether the column accepts NULL values
    pub nullable: bool,

    /// Default value expression (if any)
    pub default_value: Option<String>,

    /// Whether this column is part of the primary key
    pub is_primary_key: bool,
}

impl ColumnInfo {
    /// Create a new ColumnInfo with required fields
    pub fn new(name: String, data_type: String, nullable: bool) -> Self {
        Self {
            name,
            data_type,
            nullable,
            default_value: None,
            is_primary_key: false,
        }
    }

    /// Create a ColumnInfo with all fields
    pub fn with_details(
        name: String,
        data_type: String,
        nullable: bool,
        default_value: Option<String>,
        is_primary_key: bool,
    ) -> Self {
        Self {
            name,
            data_type,
            nullable,
            default_value,
            is_primary_key,
        }
    }
}

/// Index information
///
/// Represents an index on a table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    /// Index name
    pub name: String,

    /// Columns included in the index (in order)
    pub columns: Vec<String>,

    /// Whether this is a unique index
    pub is_unique: bool,

    /// Whether this is the primary key index
    pub is_primary: bool,
}

impl IndexInfo {
    /// Create a new IndexInfo
    pub fn new(name: String, columns: Vec<String>, is_unique: bool, is_primary: bool) -> Self {
        Self {
            name,
            columns,
            is_unique,
            is_primary,
        }
    }
}

/// Complete table schema
///
/// Contains all metadata about a table, including columns and indexes.
/// This is used when displaying detailed table information in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    /// Table metadata
    pub table: TableInfo,

    /// Column definitions
    pub columns: Vec<ColumnInfo>,

    /// Indexes defined on the table
    pub indexes: Vec<IndexInfo>,
}

impl TableSchema {
    /// Create a new TableSchema
    pub fn new(table: TableInfo, columns: Vec<ColumnInfo>, indexes: Vec<IndexInfo>) -> Self {
        Self {
            table,
            columns,
            indexes,
        }
    }

    /// Get primary key columns
    pub fn primary_key_columns(&self) -> Vec<&ColumnInfo> {
        self.columns
            .iter()
            .filter(|col| col.is_primary_key)
            .collect()
    }

    /// Get the primary key index (if exists)
    pub fn primary_key_index(&self) -> Option<&IndexInfo> {
        self.indexes.iter().find(|idx| idx.is_primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_info_creation() {
        let db = DatabaseInfo::new("test_db".to_string());
        assert_eq!(db.name, "test_db");
        assert!(db.owner.is_none());
        assert!(db.size.is_none());

        let db_with_details =
            DatabaseInfo::with_details("test_db".to_string(), Some("admin".to_string()), Some(1024));
        assert_eq!(db_with_details.owner, Some("admin".to_string()));
        assert_eq!(db_with_details.size, Some(1024));
    }

    #[test]
    fn test_table_info_is_view() {
        let table = TableInfo::new("users".to_string(), "public".to_string(), "TABLE".to_string());
        assert!(!table.is_view());

        let view = TableInfo::new("user_view".to_string(), "public".to_string(), "VIEW".to_string());
        assert!(view.is_view());

        let mat_view = TableInfo::new(
            "user_mat_view".to_string(),
            "public".to_string(),
            "MATERIALIZED VIEW".to_string(),
        );
        assert!(mat_view.is_view());
    }

    #[test]
    fn test_column_info_creation() {
        let col = ColumnInfo::new("id".to_string(), "INTEGER".to_string(), false);
        assert_eq!(col.name, "id");
        assert!(!col.nullable);
        assert!(!col.is_primary_key);

        let pk_col = ColumnInfo::with_details(
            "id".to_string(),
            "INTEGER".to_string(),
            false,
            None,
            true,
        );
        assert!(pk_col.is_primary_key);
    }

    #[test]
    fn test_table_schema_primary_key() {
        let table = TableInfo::new("users".to_string(), "public".to_string(), "TABLE".to_string());

        let columns = vec![
            ColumnInfo::with_details("id".to_string(), "INTEGER".to_string(), false, None, true),
            ColumnInfo::with_details(
                "name".to_string(),
                "VARCHAR(255)".to_string(),
                false,
                None,
                false,
            ),
        ];

        let indexes = vec![IndexInfo::new(
            "users_pkey".to_string(),
            vec!["id".to_string()],
            true,
            true,
        )];

        let schema = TableSchema::new(table, columns, indexes);

        let pk_cols = schema.primary_key_columns();
        assert_eq!(pk_cols.len(), 1);
        assert_eq!(pk_cols[0].name, "id");

        let pk_idx = schema.primary_key_index();
        assert!(pk_idx.is_some());
        assert_eq!(pk_idx.unwrap().name, "users_pkey");
    }

    #[test]
    fn test_serialization() {
        let db = DatabaseInfo::new("test_db".to_string());
        let json = serde_json::to_string(&db).unwrap();
        let deserialized: DatabaseInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(db, deserialized);
    }
}
