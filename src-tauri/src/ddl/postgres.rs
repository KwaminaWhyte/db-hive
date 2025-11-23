//! PostgreSQL DDL generator
//!
//! Generates PostgreSQL-specific DDL statements for table creation,
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

/// PostgreSQL DDL generator
pub struct PostgresDdlGenerator;

impl PostgresDdlGenerator {
    /// Convert ColumnType to PostgreSQL type string
    fn column_type_to_sql(&self, col_type: &ColumnType) -> String {
        match col_type {
            ColumnType::SmallInt => "SMALLINT".to_string(),
            ColumnType::Integer => "INTEGER".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Real => "REAL".to_string(),
            ColumnType::DoublePrecision => "DOUBLE PRECISION".to_string(),
            ColumnType::Varchar { length } => format!("VARCHAR({})", length),
            ColumnType::Char { length } => format!("CHAR({})", length),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Bytea => "BYTEA".to_string(),
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::Timestamp => "TIMESTAMP".to_string(),
            ColumnType::TimestampTz => "TIMESTAMPTZ".to_string(),
            ColumnType::Json => "JSON".to_string(),
            ColumnType::JsonB => "JSONB".to_string(),
            ColumnType::Uuid => "UUID".to_string(),
            ColumnType::Array { element_type } => {
                format!("{}[]", self.column_type_to_sql(element_type))
            }
            ColumnType::Custom { type_name } => type_name.clone(),
        }
    }

    /// Generate column definition SQL
    fn generate_column_sql(&self, col: &ColumnDefinition) -> Result<String, DbError> {
        let mut parts = Vec::new();

        // Column name
        parts.push(format!("\"{}\"", col.name));

        // Column type (handle auto-increment specially)
        if col.auto_increment {
            let serial_type = match col.column_type {
                ColumnType::SmallInt => "SMALLSERIAL",
                ColumnType::Integer => "SERIAL",
                ColumnType::BigInt => "BIGSERIAL",
                _ => {
                    return Err(DbError::InvalidInput(
                        "Auto-increment is only supported for integer types".to_string(),
                    ))
                }
            };
            parts.push(serial_type.to_string());
        } else {
            parts.push(self.column_type_to_sql(&col.column_type));
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
    fn generate_foreign_key_sql(&self, fk: &ForeignKeyConstraint, table_name: &str) -> String {
        let constraint_name = fk.name.as_ref().map_or_else(
            || {
                format!(
                    "fk_{}_{}",
                    table_name,
                    fk.columns.join("_")
                )
            },
            |name| name.clone(),
        );

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
            "CONSTRAINT \"{}\" FOREIGN KEY ({}) REFERENCES \"{}\" ({}) ON DELETE {} ON UPDATE {}",
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
            || {
                format!(
                    "unique_{}_{}",
                    table_name,
                    unique.columns.join("_")
                )
            },
            |name| name.clone(),
        );

        let columns = unique
            .columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!("CONSTRAINT \"{}\" UNIQUE ({})", constraint_name, columns)
    }

    /// Generate check constraint SQL
    fn generate_check_constraint_sql(&self, check: &CheckConstraint, table_name: &str) -> String {
        let constraint_name = check.name.as_ref().map_or_else(
            || format!("check_{}", table_name),
            |name| name.clone(),
        );

        format!(
            "CONSTRAINT \"{}\" CHECK ({})",
            constraint_name, check.expression
        )
    }

    /// Generate primary key constraint SQL
    fn generate_primary_key_sql(&self, columns: &[String]) -> String {
        let col_list = columns
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PRIMARY KEY ({})", col_list)
    }
}

impl DdlGenerator for PostgresDdlGenerator {
    fn generate_create_table(&self, table: &TableDefinition) -> Result<DdlResult, DbError> {
        if table.columns.is_empty() {
            return Err(DbError::InvalidInput(
                "Table must have at least one column".to_string(),
            ));
        }

        let mut sql_parts = Vec::new();

        // CREATE TABLE clause
        let schema_prefix = table
            .schema
            .as_ref()
            .map_or(String::new(), |s| format!("\"{}\".", s));

        let if_not_exists = if table.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        };

        sql_parts.push(format!(
            "CREATE TABLE {}{}\"{}\" (",
            if_not_exists, schema_prefix, table.name
        ));

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
            table_elements.push(format!(
                "  {}",
                self.generate_foreign_key_sql(fk, &table.name)
            ));
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
        sql_parts.push(");".to_string());

        let mut full_sql = vec![sql_parts.join("\n")];

        // Add table comment if provided
        if let Some(comment) = &table.comment {
            full_sql.push(format!(
                "COMMENT ON TABLE {}\"{}\" IS '{}';",
                schema_prefix,
                table.name,
                comment.replace('\'', "''")
            ));
        }

        // Add column comments
        for col in &table.columns {
            if let Some(comment) = &col.comment {
                full_sql.push(format!(
                    "COMMENT ON COLUMN {}\"{}\".\"{}\". IS '{}';",
                    schema_prefix,
                    table.name,
                    col.name,
                    comment.replace('\'', "''")
                ));
            }
        }

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

        let schema_prefix = alter
            .schema
            .as_ref()
            .map_or(String::new(), |s| format!("\"{}\".", s));

        let table_name = format!("{}\"{}\"", schema_prefix, alter.name);
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
                AlterColumnOperation::DropColumn { column_name, cascade } => {
                    let cascade_clause = if *cascade { " CASCADE" } else { "" };
                    format!(
                        "ALTER TABLE {} DROP COLUMN \"{}\"{};",
                        table_name, column_name, cascade_clause
                    )
                }
                AlterColumnOperation::RenameColumn { old_name, new_name } => {
                    format!(
                        "ALTER TABLE {} RENAME COLUMN \"{}\" TO \"{}\";",
                        table_name, old_name, new_name
                    )
                }
                AlterColumnOperation::AlterType { column_name, new_type } => {
                    format!(
                        "ALTER TABLE {} ALTER COLUMN \"{}\" TYPE {};",
                        table_name,
                        column_name,
                        self.column_type_to_sql(new_type)
                    )
                }
                AlterColumnOperation::SetNotNull { column_name, not_null } => {
                    let action = if *not_null { "SET NOT NULL" } else { "DROP NOT NULL" };
                    format!(
                        "ALTER TABLE {} ALTER COLUMN \"{}\" {};",
                        table_name, column_name, action
                    )
                }
                AlterColumnOperation::SetDefault { column_name, default } => {
                    if let Some(default_value) = default {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN \"{}\" SET DEFAULT {};",
                            table_name, column_name, default_value
                        )
                    } else {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN \"{}\" DROP DEFAULT;",
                            table_name, column_name
                        )
                    }
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
        let schema_prefix = drop
            .schema
            .as_ref()
            .map_or(String::new(), |s| format!("\"{}\".", s));

        let if_exists = if drop.if_exists { "IF EXISTS " } else { "" };
        let cascade = if drop.cascade { " CASCADE" } else { "" };

        let sql = format!(
            "DROP TABLE {}{}\"{}\"{}; ",
            if_exists, schema_prefix, drop.name, cascade
        );

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
        let generator = PostgresDdlGenerator;

        let table = TableDefinition {
            schema: Some("public".to_string()),
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
        assert!(result.sql[0].contains("\"public\".\"users\""));
        assert!(result.sql[0].contains("\"id\" SERIAL NOT NULL"));
        assert!(result.sql[0].contains("\"email\" VARCHAR(255) NOT NULL"));
        assert!(result.sql[0].contains("PRIMARY KEY (\"id\")"));
    }

    #[test]
    fn test_create_table_with_foreign_key() {
        let generator = PostgresDdlGenerator;

        let table = TableDefinition {
            schema: None,
            name: "posts".to_string(),
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
                    name: "user_id".to_string(),
                    column_type: ColumnType::Integer,
                    nullable: false,
                    default: None,
                    primary_key: false,
                    auto_increment: false,
                    comment: None,
                },
            ],
            primary_key: None,
            foreign_keys: vec![ForeignKeyConstraint {
                name: None,
                columns: vec!["user_id".to_string()],
                referenced_table: "users".to_string(),
                referenced_columns: vec!["id".to_string()],
                on_delete: ForeignKeyAction::Cascade,
                on_update: ForeignKeyAction::NoAction,
            }],
            unique_constraints: vec![],
            check_constraints: vec![],
            comment: None,
            if_not_exists: false,
        };

        let result = generator.generate_create_table(&table).unwrap();
        assert!(result.sql[0].contains("FOREIGN KEY"));
        assert!(result.sql[0].contains("REFERENCES \"users\""));
        assert!(result.sql[0].contains("ON DELETE CASCADE"));
    }

    #[test]
    fn test_alter_table_add_column() {
        let generator = PostgresDdlGenerator;

        let alter = AlterTableDefinition {
            schema: Some("public".to_string()),
            name: "users".to_string(),
            operations: vec![AlterColumnOperation::AddColumn {
                column: ColumnDefinition {
                    name: "created_at".to_string(),
                    column_type: ColumnType::Timestamp,
                    nullable: false,
                    default: Some("CURRENT_TIMESTAMP".to_string()),
                    primary_key: false,
                    auto_increment: false,
                    comment: None,
                },
            }],
        };

        let result = generator.generate_alter_table(&alter).unwrap();
        assert!(result.sql[0].contains("ALTER TABLE"));
        assert!(result.sql[0].contains("ADD COLUMN"));
        assert!(result.sql[0].contains("\"created_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"));
    }

    #[test]
    fn test_drop_table() {
        let generator = PostgresDdlGenerator;

        let drop = DropTableDefinition {
            schema: Some("public".to_string()),
            name: "users".to_string(),
            cascade: true,
            if_exists: true,
        };

        let result = generator.generate_drop_table(&drop).unwrap();
        assert!(result.sql[0].contains("DROP TABLE IF EXISTS"));
        assert!(result.sql[0].contains("\"public\".\"users\""));
        assert!(result.sql[0].contains("CASCADE"));
    }
}
