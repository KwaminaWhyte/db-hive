//! DDL (Data Definition Language) types for schema management
//!
//! This module defines the data structures for creating, modifying, and deleting
//! database objects such as tables, columns, constraints, and indexes.

use serde::{Deserialize, Serialize};

/// Column data type
///
/// Represents common database column types. Each database driver will map these
/// to their native types (e.g., `Integer` -> `INT` in MySQL, `INTEGER` in SQLite).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ColumnType {
    // Integer types
    /// Small integer (-32768 to 32767)
    SmallInt,
    /// Standard integer (-2147483648 to 2147483647)
    Integer,
    /// Large integer (-9223372036854775808 to 9223372036854775807)
    BigInt,

    // Decimal types
    /// Fixed-point decimal number
    Decimal { precision: u8, scale: u8 },
    /// Single-precision floating point
    Real,
    /// Double-precision floating point
    DoublePrecision,

    // String types
    /// Variable-length character string
    Varchar { length: u32 },
    /// Fixed-length character string
    Char { length: u32 },
    /// Unlimited text
    Text,

    // Binary types
    /// Variable-length binary data
    Bytea,

    // Boolean
    /// Boolean (true/false)
    Boolean,

    // Date/Time types
    /// Date only (no time)
    Date,
    /// Time only (no date)
    Time,
    /// Timestamp without timezone
    Timestamp,
    /// Timestamp with timezone
    TimestampTz,

    // JSON types
    /// JSON data (text-based)
    Json,
    /// JSONB data (binary, PostgreSQL-specific)
    JsonB,

    // UUID
    /// UUID (universally unique identifier)
    Uuid,

    // Array (PostgreSQL-specific)
    /// Array of another type
    Array { element_type: Box<ColumnType> },

    // Database-specific custom type
    /// Custom type name (for database-specific types)
    Custom { type_name: String },
}

/// Foreign key action on DELETE or UPDATE
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForeignKeyAction {
    /// No action (default)
    NoAction,
    /// Restrict deletion/update if referenced
    Restrict,
    /// Cascade deletion/update to referencing rows
    Cascade,
    /// Set foreign key to NULL
    SetNull,
    /// Set foreign key to default value
    SetDefault,
}

impl Default for ForeignKeyAction {
    fn default() -> Self {
        Self::NoAction
    }
}

/// Foreign key constraint definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyConstraint {
    /// Name of the constraint (optional, will be auto-generated if not provided)
    pub name: Option<String>,

    /// Columns in this table that reference another table
    pub columns: Vec<String>,

    /// Referenced table name
    pub referenced_table: String,

    /// Referenced columns in the target table
    pub referenced_columns: Vec<String>,

    /// Action on DELETE
    pub on_delete: ForeignKeyAction,

    /// Action on UPDATE
    pub on_update: ForeignKeyAction,
}

/// Unique constraint definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UniqueConstraint {
    /// Name of the constraint (optional)
    pub name: Option<String>,

    /// Columns that must be unique together
    pub columns: Vec<String>,
}

/// Check constraint definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckConstraint {
    /// Name of the constraint (optional)
    pub name: Option<String>,

    /// SQL expression for the check (e.g., "age >= 18")
    pub expression: String,
}

/// Index type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IndexType {
    /// Standard B-tree index
    BTree,
    /// Hash index
    Hash,
    /// GiST index (PostgreSQL)
    Gist,
    /// GIN index (PostgreSQL)
    Gin,
}

impl Default for IndexType {
    fn default() -> Self {
        Self::BTree
    }
}

/// Index definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDefinition {
    /// Index name
    pub name: String,

    /// Columns to index
    pub columns: Vec<String>,

    /// Whether this is a unique index
    pub unique: bool,

    /// Index type (defaults to BTree)
    pub index_type: IndexType,
}

/// Column definition for table creation/alteration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,

    /// Column data type
    pub column_type: ColumnType,

    /// Whether the column allows NULL values
    pub nullable: bool,

    /// Default value expression (SQL string, e.g., "0", "CURRENT_TIMESTAMP")
    pub default: Option<String>,

    /// Whether this column is part of the primary key
    pub primary_key: bool,

    /// Whether this column is auto-incrementing (SERIAL in PostgreSQL, AUTO_INCREMENT in MySQL)
    pub auto_increment: bool,

    /// Comment/description for the column
    pub comment: Option<String>,
}

/// Table definition for creation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDefinition {
    /// Schema name (defaults to "public" in PostgreSQL, ignored in SQLite/MySQL without USE)
    pub schema: Option<String>,

    /// Table name
    pub name: String,

    /// Column definitions
    pub columns: Vec<ColumnDefinition>,

    /// Primary key columns (if composite, or if not set on individual columns)
    pub primary_key: Option<Vec<String>>,

    /// Foreign key constraints
    pub foreign_keys: Vec<ForeignKeyConstraint>,

    /// Unique constraints
    pub unique_constraints: Vec<UniqueConstraint>,

    /// Check constraints
    pub check_constraints: Vec<CheckConstraint>,

    /// Table comment/description
    pub comment: Option<String>,

    /// If true, add "IF NOT EXISTS" clause
    pub if_not_exists: bool,
}

/// Operation for altering a table column
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AlterColumnOperation {
    /// Add a new column
    AddColumn { column: ColumnDefinition },

    /// Drop an existing column
    DropColumn { column_name: String, cascade: bool },

    /// Rename a column
    RenameColumn {
        old_name: String,
        new_name: String,
    },

    /// Change column type
    AlterType {
        column_name: String,
        new_type: ColumnType,
    },

    /// Set/drop NOT NULL constraint
    SetNotNull {
        column_name: String,
        not_null: bool,
    },

    /// Set/drop default value
    SetDefault {
        column_name: String,
        default: Option<String>,
    },
}

/// Table alteration definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlterTableDefinition {
    /// Schema name
    pub schema: Option<String>,

    /// Table name
    pub name: String,

    /// Column operations
    pub operations: Vec<AlterColumnOperation>,
}

/// Request to drop a table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropTableDefinition {
    /// Schema name
    pub schema: Option<String>,

    /// Table name
    pub name: String,

    /// If true, also drop dependent objects (CASCADE)
    pub cascade: bool,

    /// If true, add "IF EXISTS" clause
    pub if_exists: bool,
}

/// DDL operation result with generated SQL
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DdlResult {
    /// Generated SQL statement(s)
    pub sql: Vec<String>,

    /// Success message
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_definition_serialization() {
        let col = ColumnDefinition {
            name: "id".to_string(),
            column_type: ColumnType::Integer,
            nullable: false,
            default: None,
            primary_key: true,
            auto_increment: true,
            comment: Some("Primary key".to_string()),
        };

        let json = serde_json::to_string(&col).unwrap();
        assert!(json.contains("\"name\":\"id\""));
        assert!(json.contains("\"primaryKey\":true"));
    }

    #[test]
    fn test_table_definition_serialization() {
        let table = TableDefinition {
            schema: Some("public".to_string()),
            name: "users".to_string(),
            columns: vec![ColumnDefinition {
                name: "id".to_string(),
                column_type: ColumnType::Integer,
                nullable: false,
                default: None,
                primary_key: true,
                auto_increment: true,
                comment: None,
            }],
            primary_key: None,
            foreign_keys: vec![],
            unique_constraints: vec![],
            check_constraints: vec![],
            comment: None,
            if_not_exists: true,
        };

        let json = serde_json::to_string(&table).unwrap();
        assert!(json.contains("\"name\":\"users\""));
        assert!(json.contains("\"ifNotExists\":true"));
    }

    #[test]
    fn test_foreign_key_action_default() {
        let action = ForeignKeyAction::default();
        assert_eq!(action, ForeignKeyAction::NoAction);
    }
}
