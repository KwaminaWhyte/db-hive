//! SQLite DDL generator
//!
//! Generates SQLite-specific DDL statements for table creation,
//! alteration, and deletion.
//!
//! Note: SQLite has limited ALTER TABLE support. Many operations require
//! creating a new table and copying data.

use crate::ddl::DdlGenerator;
use crate::models::{
    ddl::{
        AlterColumnOperation, AlterTableDefinition, CheckConstraint, ColumnDefinition,
        ColumnType, DdlResult, DropTableDefinition, ForeignKeyAction, ForeignKeyConstraint,
        TableDefinition, UniqueConstraint,
    },
    DbError,
};

/// SQLite DDL generator
pub struct SqliteDdlGenerator;

impl SqliteDdlGenerator {
    /// Convert ColumnType to SQLite type string
    fn column_type_to_sql(&self, col_type: &ColumnType) -> String {
        // SQLite has dynamic typing with type affinities
        match col_type {
            ColumnType::SmallInt | ColumnType::Integer | ColumnType::BigInt => "INTEGER".to_string(),
            ColumnType::Decimal { .. } | ColumnType::Real | ColumnType::DoublePrecision => {
                "REAL".to_string()
            }
            ColumnType::Varchar { .. }
            | ColumnType::Char { .. }
            | ColumnType::Text
            | ColumnType::Uuid => "TEXT".to_string(),
            ColumnType::Bytea => "BLOB".to_string(),
            ColumnType::Boolean => "INTEGER".to_string(), // 0 or 1
            ColumnType::Date | ColumnType::Time | ColumnType::Timestamp | ColumnType::TimestampTz => {
                "TEXT".to_string() // Store as ISO 8601 strings
            }
            ColumnType::Json | ColumnType::JsonB => "TEXT".to_string(),
            ColumnType::Array { .. } => "TEXT".to_string(), // Store as JSON
            ColumnType::Custom { type_name } => type_name.clone(),
        }
    }

    /// Generate column definition SQL
    fn generate_column_sql(&self, col: &ColumnDefinition) -> Result<String, DbError> {
        let mut parts = Vec::new();

        // Column name
        parts.push(format!("\"{}\"", col.name));

        // Column type
        parts.push(self.column_type_to_sql(&col.column_type));

        // PRIMARY KEY and AUTOINCREMENT
        if col.primary_key {
            parts.push("PRIMARY KEY".to_string());
            if col.auto_increment {
                if !matches!(
                    col.column_type,
                    ColumnType::SmallInt | ColumnType::Integer | ColumnType::BigInt
                ) {
                    return Err(DbError::InvalidInput(
                        "AUTOINCREMENT is only supported for INTEGER PRIMARY KEY".to_string(),
                    ));
                }
                parts.push("AUTOINCREMENT".to_string());
            }
        }

        // NOT NULL constraint
        if !col.nullable {
            parts.push("NOT NULL".to_string());
        }

        // DEFAULT value
        if let Some(default) = &col.default {
            parts.push(format!("DEFAULT {}", default));
        }

        Ok(parts.join(" "))
    }

    /// Generate foreign key action SQL
    fn foreign_key_action_to_sql(&self, action: &ForeignKeyAction) -> &str {
        match action {
            ForeignKeyAction::NoAction => "NO ACTION",
            ForeignKeyAction::Restrict => "RESTRICT",
            ForeignKeyAction::Cascade => "CASCADE",
            ForeignKeyAction::SetNull => "SET NULL",
            ForeignKeyAction::SetDefault => "SET DEFAULT",
        }
    }

    /// Generate foreign key constraint SQL
    fn generate_foreign_key_sql(&self, fk: &ForeignKeyConstraint) -> String {
        let columns = fk
            .columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        let ref_columns = fk
            .referenced_columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "FOREIGN KEY ({}) REFERENCES \"{}\" ({}) ON DELETE {} ON UPDATE {}",
            columns,
            fk.referenced_table,
            ref_columns,
            self.foreign_key_action_to_sql(&fk.on_delete),
            self.foreign_key_action_to_sql(&fk.on_update)
        )
    }

    /// Generate unique constraint SQL
    fn generate_unique_constraint_sql(&self, unique: &UniqueConstraint) -> String {
        let columns = unique
            .columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!("UNIQUE ({})", columns)
    }

    /// Generate check constraint SQL
    fn generate_check_constraint_sql(&self, check: &CheckConstraint) -> String {
        format!("CHECK ({})", check.expression)
    }

    /// Generate primary key constraint SQL (for composite keys)
    fn generate_primary_key_sql(&self, columns: &[String]) -> String {
        let col_list = columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PRIMARY KEY ({})", col_list)
    }
}

impl DdlGenerator for SqliteDdlGenerator {
    fn generate_create_table(&self, table: &TableDefinition) -> Result<DdlResult, DbError> {
        if table.columns.is_empty() {
            return Err(DbError::InvalidInput(
                "Table must have at least one column".to_string(),
            ));
        }

        let mut sql_parts = Vec::new();

        // CREATE TABLE clause
        let if_not_exists = if table.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        };

        sql_parts.push(format!("CREATE TABLE {}\"{}\" (", if_not_exists, table.name));

        // Column definitions
        let mut table_elements = Vec::new();

        // Check if we have a composite primary key
        let has_composite_pk = table.primary_key.as_ref().map_or(false, |pk| pk.len() > 1);
        let has_inline_pk = table.columns.iter().any(|c| c.primary_key);

        for col in &table.columns {
            // Don't add PRIMARY KEY to column if we have a composite key
            let mut col_def = col.clone();
            if has_composite_pk {
                col_def.primary_key = false;
            }
            table_elements.push(format!("  {}", self.generate_column_sql(&col_def)?));
        }

        // Primary key (if composite or specified as separate constraint)
        if let Some(pk_columns) = &table.primary_key {
            if !pk_columns.is_empty() && !has_inline_pk {
                table_elements.push(format!("  {}", self.generate_primary_key_sql(pk_columns)));
            }
        }

        // Foreign keys
        for fk in &table.foreign_keys {
            table_elements.push(format!("  {}", self.generate_foreign_key_sql(fk)));
        }

        // Unique constraints
        for unique in &table.unique_constraints {
            table_elements.push(format!("  {}", self.generate_unique_constraint_sql(unique)));
        }

        // Check constraints
        for check in &table.check_constraints {
            table_elements.push(format!("  {}", self.generate_check_constraint_sql(check)));
        }

        sql_parts.push(table_elements.join(",\n"));
        sql_parts.push(");".to_string());

        let full_sql = vec![sql_parts.join("\n")];

        Ok(DdlResult {
            sql: full_sql,
            message: format!("Table \"{}\" created successfully", table.name),
        })
    }

    fn generate_alter_table(&self, alter: &AlterTableDefinition) -> Result<DdlResult, DbError> {
        if alter.operations.is_empty() {
            return Err(DbError::InvalidInput(
                "Alter table must have at least one operation".to_string(),
            ));
        }

        let table_name = format!("\"{}\"", alter.name);
        let mut sql_statements = Vec::new();

        for op in &alter.operations {
            let sql = match op {
                AlterColumnOperation::AddColumn { column } => {
                    // SQLite supports ADD COLUMN
                    format!(
                        "ALTER TABLE {} ADD COLUMN {};",
                        table_name,
                        self.generate_column_sql(column)?
                    )
                }
                AlterColumnOperation::RenameColumn { old_name, new_name } => {
                    // SQLite 3.25.0+ supports RENAME COLUMN
                    format!(
                        "ALTER TABLE {} RENAME COLUMN \"{}\" TO \"{}\";",
                        table_name, old_name, new_name
                    )
                }
                AlterColumnOperation::DropColumn { column_name, .. } => {
                    // SQLite 3.35.0+ supports DROP COLUMN
                    format!("ALTER TABLE {} DROP COLUMN \"{}\";", table_name, column_name)
                }
                AlterColumnOperation::AlterType { .. }
                | AlterColumnOperation::SetNotNull { .. }
                | AlterColumnOperation::SetDefault { .. } => {
                    // These operations are NOT supported by SQLite ALTER TABLE
                    // Would require table recreation
                    return Err(DbError::InvalidInput(format!(
                        "Operation {:?} not supported by SQLite. Consider recreating the table.",
                        op
                    )));
                }
            };
            sql_statements.push(sql);
        }

        Ok(DdlResult {
            sql: sql_statements,
            message: format!("Table \"{}\" altered successfully", alter.name),
        })
    }

    fn generate_drop_table(&self, drop: &DropTableDefinition) -> Result<DdlResult, DbError> {
        let if_exists = if drop.if_exists { "IF EXISTS " } else { "" };
        // SQLite doesn't support CASCADE
        let sql = format!("DROP TABLE {}\"{}\"", if_exists, drop.name);

        Ok(DdlResult {
            sql: vec![sql],
            message: format!("Table \"{}\" dropped successfully", drop.name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_simple_table() {
        let generator = SqliteDdlGenerator;

        let table = TableDefinition {
            schema: None,
            name: "users".to_string(),
            columns: vec![
                ColumnDefinition {
                    name: "id".to_string(),
                    column_type: ColumnType::Integer,
                    nullable: false,
                    default: None,
                    primary_key: true,
                    auto_increment: true,
                    comment: None,
                },
                ColumnDefinition {
                    name: "email".to_string(),
                    column_type: ColumnType::Varchar { length: 255 },
                    nullable: false,
                    default: None,
                    primary_key: false,
                    auto_increment: false,
                    comment: None,
                },
            ],
            primary_key: None,
            foreign_keys: vec![],
            unique_constraints: vec![],
            check_constraints: vec![],
            comment: None,
            if_not_exists: true,
        };

        let result = generator.generate_create_table(&table).unwrap();
        assert!(result.sql[0].contains("CREATE TABLE IF NOT EXISTS"));
        assert!(result.sql[0].contains("\"users\""));
        assert!(result.sql[0].contains("\"id\" INTEGER PRIMARY KEY AUTOINCREMENT"));
        assert!(result.sql[0].contains("\"email\" TEXT NOT NULL"));
    }

    #[test]
    fn test_drop_table() {
        let generator = SqliteDdlGenerator;

        let drop = DropTableDefinition {
            schema: None,
            name: "users".to_string(),
            cascade: false,
            if_exists: true,
        };

        let result = generator.generate_drop_table(&drop).unwrap();
        assert!(result.sql[0].contains("DROP TABLE IF EXISTS"));
        assert!(result.sql[0].contains("\"users\""));
    }
}
