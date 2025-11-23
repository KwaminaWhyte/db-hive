//! MySQL DDL generator
//!
//! Generates MySQL-specific DDL statements for table creation,
//! alteration, and deletion.

use crate::ddl::DdlGenerator;
use crate::models::{
    ddl::{
        AlterColumnOperation, AlterTableDefinition, CheckConstraint, ColumnDefinition,
        ColumnType, DdlResult, DropTableDefinition, ForeignKeyAction, ForeignKeyConstraint,
        TableDefinition, UniqueConstraint,
    },
    DbError,
};

/// MySQL DDL generator
pub struct MySqlDdlGenerator;

impl MySqlDdlGenerator {
    /// Convert ColumnType to MySQL type string
    fn column_type_to_sql(&self, col_type: &ColumnType) -> String {
        match col_type {
            ColumnType::SmallInt => "SMALLINT".to_string(),
            ColumnType::Integer => "INT".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Real => "FLOAT".to_string(),
            ColumnType::DoublePrecision => "DOUBLE".to_string(),
            ColumnType::Varchar { length } => format!("VARCHAR({})", length),
            ColumnType::Char { length } => format!("CHAR({})", length),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Bytea => "BLOB".to_string(),
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::Timestamp => "TIMESTAMP".to_string(),
            ColumnType::TimestampTz => "TIMESTAMP".to_string(), // MySQL doesn't have explicit TZ support
            ColumnType::Json => "JSON".to_string(),
            ColumnType::JsonB => "JSON".to_string(), // MySQL doesn't distinguish JSON/JSONB
            ColumnType::Uuid => "CHAR(36)".to_string(), // MySQL doesn't have native UUID
            ColumnType::Array { .. } => {
                // MySQL doesn't support arrays natively, use JSON
                "JSON".to_string()
            }
            ColumnType::Custom { type_name } => type_name.clone(),
        }
    }

    /// Generate column definition SQL
    fn generate_column_sql(&self, col: &ColumnDefinition) -> Result<String, DbError> {
        let mut parts = Vec::new();

        // Column name
        parts.push(format!("`{}`", col.name));

        // Column type
        parts.push(self.column_type_to_sql(&col.column_type));

        // NOT NULL constraint
        if !col.nullable {
            parts.push("NOT NULL".to_string());
        }

        // AUTO_INCREMENT (must come before DEFAULT)
        if col.auto_increment {
            if !matches!(
                col.column_type,
                ColumnType::SmallInt | ColumnType::Integer | ColumnType::BigInt
            ) {
                return Err(DbError::InvalidInput(
                    "AUTO_INCREMENT is only supported for integer types".to_string(),
                ));
            }
            parts.push("AUTO_INCREMENT".to_string());
        }

        // DEFAULT value
        if let Some(default) = &col.default {
            parts.push(format!("DEFAULT {}", default));
        }

        // Comment
        if let Some(comment) = &col.comment {
            parts.push(format!("COMMENT '{}'", comment.replace('\'', "''")));
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
    fn generate_foreign_key_sql(&self, fk: &ForeignKeyConstraint, table_name: &str) -> String {
        let constraint_name = fk.name.as_ref().map_or_else(
            || format!("fk_{}_{}", table_name, fk.columns.join("_")),
            |name| name.clone(),
        );

        let columns = fk
            .columns
            .iter()
            .map(|c| format!("`{}`", c))
            .collect::<Vec<_>>()
            .join(", ");

        let ref_columns = fk
            .referenced_columns
            .iter()
            .map(|c| format!("`{}`", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "CONSTRAINT `{}` FOREIGN KEY ({}) REFERENCES `{}` ({}) ON DELETE {} ON UPDATE {}",
            constraint_name,
            columns,
            fk.referenced_table,
            ref_columns,
            self.foreign_key_action_to_sql(&fk.on_delete),
            self.foreign_key_action_to_sql(&fk.on_update)
        )
    }

    /// Generate unique constraint SQL
    fn generate_unique_constraint_sql(&self, unique: &UniqueConstraint, table_name: &str) -> String {
        let constraint_name = unique.name.as_ref().map_or_else(
            || format!("unique_{}_{}", table_name, unique.columns.join("_")),
            |name| name.clone(),
        );

        let columns = unique
            .columns
            .iter()
            .map(|c| format!("`{}`", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!("CONSTRAINT `{}` UNIQUE ({})", constraint_name, columns)
    }

    /// Generate check constraint SQL (MySQL 8.0.16+)
    fn generate_check_constraint_sql(&self, check: &CheckConstraint, table_name: &str) -> String {
        let constraint_name = check
            .name
            .as_ref()
            .map_or_else(|| format!("check_{}", table_name), |name| name.clone());

        format!("CONSTRAINT `{}` CHECK ({})", constraint_name, check.expression)
    }

    /// Generate primary key constraint SQL
    fn generate_primary_key_sql(&self, columns: &[String]) -> String {
        let col_list = columns
            .iter()
            .map(|c| format!("`{}`", c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PRIMARY KEY ({})", col_list)
    }
}

impl DdlGenerator for MySqlDdlGenerator {
    fn generate_create_table(&self, table: &TableDefinition) -> Result<DdlResult, DbError> {
        if table.columns.is_empty() {
            return Err(DbError::InvalidInput(
                "Table must have at least one column".to_string(),
            ));
        }

        let mut sql_parts = Vec::new();

        // CREATE TABLE clause (MySQL ignores schema, uses database context)
        let if_not_exists = if table.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        };

        sql_parts.push(format!("CREATE TABLE {}`{}` (", if_not_exists, table.name));

        // Column definitions
        let mut table_elements = Vec::new();

        for col in &table.columns {
            table_elements.push(format!("  {}", self.generate_column_sql(col)?));
        }

        // Primary key (if specified as separate constraint)
        if let Some(pk_columns) = &table.primary_key {
            if !pk_columns.is_empty() {
                table_elements.push(format!("  {}", self.generate_primary_key_sql(pk_columns)));
            }
        } else {
            // Check for primary key in column definitions
            let pk_columns: Vec<String> = table
                .columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| c.name.clone())
                .collect();

            if !pk_columns.is_empty() {
                table_elements.push(format!("  {}", self.generate_primary_key_sql(&pk_columns)));
            }
        }

        // Foreign keys
        for fk in &table.foreign_keys {
            table_elements.push(format!("  {}", self.generate_foreign_key_sql(fk, &table.name)));
        }

        // Unique constraints
        for unique in &table.unique_constraints {
            table_elements.push(format!(
                "  {}",
                self.generate_unique_constraint_sql(unique, &table.name)
            ));
        }

        // Check constraints
        for check in &table.check_constraints {
            table_elements.push(format!(
                "  {}",
                self.generate_check_constraint_sql(check, &table.name)
            ));
        }

        sql_parts.push(table_elements.join(",\n"));
        sql_parts.push(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;".to_string());

        let full_sql = vec![sql_parts.join("\n")];

        Ok(DdlResult {
            sql: full_sql,
            message: format!("Table `{}` created successfully", table.name),
        })
    }

    fn generate_alter_table(&self, alter: &AlterTableDefinition) -> Result<DdlResult, DbError> {
        if alter.operations.is_empty() {
            return Err(DbError::InvalidInput(
                "Alter table must have at least one operation".to_string(),
            ));
        }

        let table_name = format!("`{}`", alter.name);
        let mut sql_statements = Vec::new();

        for op in &alter.operations {
            let sql = match op {
                AlterColumnOperation::AddColumn { column } => {
                    format!(
                        "ALTER TABLE {} ADD COLUMN {};",
                        table_name,
                        self.generate_column_sql(column)?
                    )
                }
                AlterColumnOperation::DropColumn { column_name, .. } => {
                    // MySQL doesn't support CASCADE in column drop
                    format!("ALTER TABLE {} DROP COLUMN `{}`;", table_name, column_name)
                }
                AlterColumnOperation::RenameColumn { old_name, new_name } => {
                    // MySQL 8.0+ supports RENAME COLUMN
                    format!(
                        "ALTER TABLE {} RENAME COLUMN `{}` TO `{}`;",
                        table_name, old_name, new_name
                    )
                }
                AlterColumnOperation::AlterType {
                    column_name,
                    new_type,
                } => {
                    format!(
                        "ALTER TABLE {} MODIFY COLUMN `{}` {};",
                        table_name,
                        column_name,
                        self.column_type_to_sql(new_type)
                    )
                }
                AlterColumnOperation::SetNotNull { column_name, not_null } => {
                    // In MySQL, we need to use MODIFY with the full column definition
                    // This is a simplified version - in production, you'd need to fetch the current column definition
                    let null_clause = if *not_null { "NOT NULL" } else { "NULL" };
                    format!(
                        "ALTER TABLE {} MODIFY COLUMN `{}` {} {};",
                        table_name,
                        column_name,
                        "/* type needed */",
                        null_clause
                    )
                }
                AlterColumnOperation::SetDefault {
                    column_name,
                    default,
                } => {
                    if let Some(default_value) = default {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN `{}` SET DEFAULT {};",
                            table_name, column_name, default_value
                        )
                    } else {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN `{}` DROP DEFAULT;",
                            table_name, column_name
                        )
                    }
                }
            };
            sql_statements.push(sql);
        }

        Ok(DdlResult {
            sql: sql_statements,
            message: format!("Table `{}` altered successfully", alter.name),
        })
    }

    fn generate_drop_table(&self, drop: &DropTableDefinition) -> Result<DdlResult, DbError> {
        let if_exists = if drop.if_exists { "IF EXISTS " } else { "" };
        // MySQL doesn't support CASCADE in DROP TABLE
        let sql = format!("DROP TABLE {}`{}`;", if_exists, drop.name);

        Ok(DdlResult {
            sql: vec![sql],
            message: format!("Table `{}` dropped successfully", drop.name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_simple_table() {
        let generator = MySqlDdlGenerator;

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
        assert!(result.sql[0].contains("`users`"));
        assert!(result.sql[0].contains("`id` INT NOT NULL AUTO_INCREMENT"));
        assert!(result.sql[0].contains("`email` VARCHAR(255) NOT NULL"));
        assert!(result.sql[0].contains("PRIMARY KEY (`id`)"));
        assert!(result.sql[0].contains("ENGINE=InnoDB"));
    }

    #[test]
    fn test_drop_table() {
        let generator = MySqlDdlGenerator;

        let drop = DropTableDefinition {
            schema: None,
            name: "users".to_string(),
            cascade: true,
            if_exists: true,
        };

        let result = generator.generate_drop_table(&drop).unwrap();
        assert!(result.sql[0].contains("DROP TABLE IF EXISTS"));
        assert!(result.sql[0].contains("`users`"));
    }
}
