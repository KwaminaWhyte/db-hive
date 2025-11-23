//! SQL Server DDL generator
//!
//! Generates Microsoft SQL Server-specific DDL statements for table creation,
//! alteration, and deletion.

use crate::ddl::DdlGenerator;
use crate::models::{
    ddl::{
        AlterColumnOperation, AlterTableDefinition, ColumnDefinition, ColumnType, DdlResult,
        DropTableDefinition, ForeignKeyAction, ForeignKeyConstraint, TableDefinition,
        UniqueConstraint,
    },
    DbError,
};

/// SQL Server DDL generator
pub struct SqlServerDdlGenerator;

impl SqlServerDdlGenerator {
    /// Convert ColumnType to SQL Server type string
    fn column_type_to_sql(&self, col_type: &ColumnType) -> String {
        match col_type {
            ColumnType::SmallInt => "SMALLINT".to_string(),
            ColumnType::Integer => "INT".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Real => "REAL".to_string(),
            ColumnType::DoublePrecision => "FLOAT".to_string(),
            ColumnType::Varchar { length } => format!("NVARCHAR({})", length),
            ColumnType::Char { length } => format!("NCHAR({})", length),
            ColumnType::Text => "NVARCHAR(MAX)".to_string(),
            ColumnType::Bytea => "VARBINARY(MAX)".to_string(),
            ColumnType::Boolean => "BIT".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::Timestamp => "DATETIME2".to_string(),
            ColumnType::TimestampTz => "DATETIMEOFFSET".to_string(),
            ColumnType::Json | ColumnType::JsonB => "NVARCHAR(MAX)".to_string(), // Store as string
            ColumnType::Uuid => "UNIQUEIDENTIFIER".to_string(),
            ColumnType::Array { .. } => "NVARCHAR(MAX)".to_string(), // Store as JSON
            ColumnType::Custom { type_name } => type_name.clone(),
        }
    }

    /// Generate column definition SQL
    fn generate_column_sql(&self, col: &ColumnDefinition) -> Result<String, DbError> {
        let mut parts = Vec::new();

        // Column name
        parts.push(format!("[{}]", col.name));

        // Column type
        parts.push(self.column_type_to_sql(&col.column_type));

        // IDENTITY (auto-increment)
        if col.auto_increment {
            if !matches!(
                col.column_type,
                ColumnType::SmallInt | ColumnType::Integer | ColumnType::BigInt
            ) {
                return Err(DbError::InvalidInput(
                    "IDENTITY is only supported for integer types".to_string(),
                ));
            }
            parts.push("IDENTITY(1,1)".to_string());
        }

        // NOT NULL constraint
        if !col.nullable {
            parts.push("NOT NULL".to_string());
        } else {
            parts.push("NULL".to_string());
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
            ForeignKeyAction::Restrict => "NO ACTION", // SQL Server doesn't have RESTRICT
            ForeignKeyAction::Cascade => "CASCADE",
            ForeignKeyAction::SetNull => "SET NULL",
            ForeignKeyAction::SetDefault => "SET DEFAULT",
        }
    }

    /// Generate foreign key constraint SQL
    fn generate_foreign_key_sql(&self, fk: &ForeignKeyConstraint, table_name: &str) -> String {
        let constraint_name = fk.name.as_ref().map_or_else(
            || format!("FK_{}_{}",  table_name, fk.columns.join("_")),
            |name| name.clone(),
        );

        let columns = fk
            .columns
            .iter()
            .map(|c| format!("[{}]", c))
            .collect::<Vec<_>>()
            .join(", ");

        let ref_columns = fk
            .referenced_columns
            .iter()
            .map(|c| format!("[{}]", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "CONSTRAINT [{}] FOREIGN KEY ({}) REFERENCES [{}] ({}) ON DELETE {} ON UPDATE {}",
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
            || format!("UQ_{}_{}", table_name, unique.columns.join("_")),
            |name| name.clone(),
        );

        let columns = unique
            .columns
            .iter()
            .map(|c| format!("[{}]", c))
            .collect::<Vec<_>>()
            .join(", ");

        format!("CONSTRAINT [{}] UNIQUE ({})", constraint_name, columns)
    }

    /// Generate primary key constraint SQL
    fn generate_primary_key_sql(&self, columns: &[String], table_name: &str) -> String {
        let col_list = columns
            .iter()
            .map(|c| format!("[{}]", c))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "CONSTRAINT [PK_{}] PRIMARY KEY CLUSTERED ({})",
            table_name, col_list
        )
    }
}

impl DdlGenerator for SqlServerDdlGenerator {
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
            .map_or("dbo".to_string(), |s| s.clone());

        sql_parts.push(format!("CREATE TABLE [{}].[{}] (", schema_prefix, table.name));

        // Column definitions
        let mut table_elements = Vec::new();

        for col in &table.columns {
            table_elements.push(format!("  {}", self.generate_column_sql(col)?));
        }

        // Primary key
        if let Some(pk_columns) = &table.primary_key {
            if !pk_columns.is_empty() {
                table_elements.push(format!(
                    "  {}",
                    self.generate_primary_key_sql(pk_columns, &table.name)
                ));
            }
        } else {
            let pk_columns: Vec<String> = table
                .columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| c.name.clone())
                .collect();

            if !pk_columns.is_empty() {
                table_elements.push(format!(
                    "  {}",
                    self.generate_primary_key_sql(&pk_columns, &table.name)
                ));
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

        sql_parts.push(table_elements.join(",\n"));
        sql_parts.push(");".to_string());

        let full_sql = vec![sql_parts.join("\n")];

        Ok(DdlResult {
            sql: full_sql,
            message: format!("Table [{}] created successfully", table.name),
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
            .map_or("dbo".to_string(), |s| s.clone());
        let table_name = format!("[{}].[{}]", schema_prefix, alter.name);
        let mut sql_statements = Vec::new();

        for op in &alter.operations {
            let sql = match op {
                AlterColumnOperation::AddColumn { column } => {
                    format!(
                        "ALTER TABLE {} ADD {};",
                        table_name,
                        self.generate_column_sql(column)?
                    )
                }
                AlterColumnOperation::DropColumn { column_name, .. } => {
                    format!("ALTER TABLE {} DROP COLUMN [{}];", table_name, column_name)
                }
                AlterColumnOperation::AlterType {
                    column_name,
                    new_type,
                } => {
                    format!(
                        "ALTER TABLE {} ALTER COLUMN [{}] {};",
                        table_name,
                        column_name,
                        self.column_type_to_sql(new_type)
                    )
                }
                AlterColumnOperation::SetNotNull { column_name, not_null } => {
                    let null_clause = if *not_null { "NOT NULL" } else { "NULL" };
                    format!(
                        "ALTER TABLE {} ALTER COLUMN [{}] {} {};",
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
                            "ALTER TABLE {} ADD DEFAULT {} FOR [{}];",
                            table_name, default_value, column_name
                        )
                    } else {
                        format!(
                            "ALTER TABLE {} DROP CONSTRAINT [DF_{}_{}];",
                            table_name, alter.name, column_name
                        )
                    }
                }
                AlterColumnOperation::RenameColumn { old_name, new_name } => {
                    format!(
                        "EXEC sp_rename '[{}].[{}]', '{}', 'COLUMN';",
                        schema_prefix, old_name, new_name
                    )
                }
            };
            sql_statements.push(sql);
        }

        Ok(DdlResult {
            sql: sql_statements,
            message: format!("Table [{}] altered successfully", alter.name),
        })
    }

    fn generate_drop_table(&self, drop: &DropTableDefinition) -> Result<DdlResult, DbError> {
        let schema_prefix = drop
            .schema
            .as_ref()
            .map_or("dbo".to_string(), |s| s.clone());

        let if_exists = if drop.if_exists {
            format!("IF OBJECT_ID('[{}].[{}]', 'U') IS NOT NULL ", schema_prefix, drop.name)
        } else {
            String::new()
        };

        let sql = format!("{}DROP TABLE [{}].[{}];", if_exists, schema_prefix, drop.name);

        Ok(DdlResult {
            sql: vec![sql],
            message: format!("Table [{}] dropped successfully", drop.name),
        })
    }
}
