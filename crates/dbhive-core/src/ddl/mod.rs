//! DDL (Data Definition Language) generators
//!
//! This module contains SQL generators for creating, altering, and dropping
//! database objects. Each database driver has its own generator to handle
//! database-specific syntax and features.

pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod sqlserver;

use crate::models::{
    ddl::{AlterTableDefinition, DdlResult, DropTableDefinition, TableDefinition},
    DbDriver, DbError,
};

/// DDL generator trait
///
/// Defines the interface for generating DDL SQL statements for different databases.
pub trait DdlGenerator {
    /// Generate CREATE TABLE statement
    fn generate_create_table(&self, table: &TableDefinition) -> Result<DdlResult, DbError>;

    /// Generate ALTER TABLE statement
    fn generate_alter_table(&self, alter: &AlterTableDefinition) -> Result<DdlResult, DbError>;

    /// Generate DROP TABLE statement
    fn generate_drop_table(&self, drop: &DropTableDefinition) -> Result<DdlResult, DbError>;
}

/// Get DDL generator for a specific database driver
pub fn get_ddl_generator(driver: &DbDriver) -> Result<Box<dyn DdlGenerator>, DbError> {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            Ok(Box::new(postgres::PostgresDdlGenerator))
        }
        DbDriver::MySql => Ok(Box::new(mysql::MySqlDdlGenerator)),
        // Turso is libSQL — same DDL surface as SQLite.
        DbDriver::Sqlite | DbDriver::Turso => Ok(Box::new(sqlite::SqliteDdlGenerator)),
        DbDriver::SqlServer => Ok(Box::new(sqlserver::SqlServerDdlGenerator)),
        DbDriver::MongoDb | DbDriver::Redis => Err(DbError::InvalidInput(
            "DDL operations not supported for this driver".to_string(),
        )),
        DbDriver::Redis => Err(DbError::InvalidInput(
            "DDL operations not supported for Redis (key-value store)".to_string(),
        )),
    }
}

/// Identifier escaping for the DDL generators.
///
/// The generators wrap every name in the dialect's quote characters but never
/// escaped the quote character itself, so a table called `foo"bar` produced
/// SQL that terminated the identifier early. Table and column names in the
/// schema designer and table editor come straight from user input, which
/// makes that a way to append arbitrary DDL, not just a formatting bug.
///
/// The fix escapes the *arguments* rather than rewriting ~78 format strings:
/// doubling the quote character composes correctly with the quotes the format
/// strings already add, which is exactly what every one of these dialects
/// specifies for an embedded quote.
pub mod escaping {
    use crate::models::{
        AlterColumnOperation, AlterTableDefinition, DropTableDefinition, TableDefinition,
    };

    /// Double `q` wherever it appears, so the name survives being quoted.
    ///
    /// SQL Server is the odd one: it brackets identifiers as `[name]` and
    /// escapes only the closing bracket.
    pub fn escape(name: &str, q: char) -> String {
        name.replace(q, &format!("{}{}", q, q))
    }

    fn esc_opt(v: &Option<String>, q: char) -> Option<String> {
        v.as_ref().map(|s| escape(s, q))
    }
    fn esc_vec(v: &[String], q: char) -> Vec<String> {
        v.iter().map(|s| escape(s, q)).collect()
    }

    /// A copy of `table` with every identifier escaped for quote char `q`.
    pub fn table(table: &TableDefinition, q: char) -> TableDefinition {
        let mut out = table.clone();
        out.name = escape(&out.name, q);
        out.schema = esc_opt(&out.schema, q);
        for c in &mut out.columns {
            c.name = escape(&c.name, q);
        }
        out.primary_key = out.primary_key.as_ref().map(|pk| esc_vec(pk, q));
        for fk in &mut out.foreign_keys {
            fk.name = esc_opt(&fk.name, q);
            fk.columns = esc_vec(&fk.columns, q);
            fk.referenced_table = escape(&fk.referenced_table, q);
            fk.referenced_columns = esc_vec(&fk.referenced_columns, q);
        }
        for u in &mut out.unique_constraints {
            u.name = esc_opt(&u.name, q);
            u.columns = esc_vec(&u.columns, q);
        }
        for c in &mut out.check_constraints {
            c.name = esc_opt(&c.name, q);
            // `expression` is raw SQL by design and is deliberately not
            // touched; escaping it would corrupt a valid CHECK.
        }
        out
    }

    pub fn alter(alter: &AlterTableDefinition, q: char) -> AlterTableDefinition {
        let mut out = alter.clone();
        out.name = escape(&out.name, q);
        out.schema = esc_opt(&out.schema, q);
        for op in &mut out.operations {
            match op {
                AlterColumnOperation::AddColumn { column } => {
                    column.name = escape(&column.name, q);
                }
                AlterColumnOperation::DropColumn { column_name, .. }
                | AlterColumnOperation::AlterType { column_name, .. }
                | AlterColumnOperation::SetNotNull { column_name, .. }
                | AlterColumnOperation::SetDefault { column_name, .. } => {
                    *column_name = escape(column_name, q);
                }
                AlterColumnOperation::RenameColumn { old_name, new_name } => {
                    *old_name = escape(old_name, q);
                    *new_name = escape(new_name, q);
                }
            }
        }
        out
    }

    pub fn drop(drop: &DropTableDefinition, q: char) -> DropTableDefinition {
        let mut out = drop.clone();
        out.name = escape(&out.name, q);
        out.schema = esc_opt(&out.schema, q);
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn doubles_the_quote_character() {
            assert_eq!(escape(r#"foo"bar"#, '"'), r#"foo""bar"#);
            assert_eq!(escape("foo`bar", '`'), "foo``bar");
            assert_eq!(escape("foo]bar", ']'), "foo]]bar");
        }

        #[test]
        fn leaves_ordinary_names_alone() {
            assert_eq!(escape("users", '"'), "users");
            assert_eq!(escape("my table", '"'), "my table");
        }

        #[test]
        fn escapes_nested_identifiers_in_a_definition() {
            use crate::models::{ColumnDefinition, ColumnType};
            let def = TableDefinition {
                schema: Some(r#"s"1"#.into()),
                name: r#"t"1"#.into(),
                columns: vec![ColumnDefinition {
                    name: r#"c"1"#.into(),
                    column_type: ColumnType::Text,
                    nullable: true,
                    default: None,
                    primary_key: false,
                    auto_increment: false,
                    comment: None,
                }],
                primary_key: Some(vec![r#"c"1"#.into()]),
                foreign_keys: Vec::new(),
                unique_constraints: Vec::new(),
                check_constraints: Vec::new(),
                comment: None,
                if_not_exists: false,
            };
            let e = table(&def, '"');
            assert_eq!(e.name, r#"t""1"#);
            assert_eq!(e.schema.as_deref(), Some(r#"s""1"#));
            assert_eq!(e.columns[0].name, r#"c""1"#);
            assert_eq!(e.primary_key.unwrap()[0], r#"c""1"#);
        }
    }
}

#[cfg(test)]
mod injection_tests {
    use super::*;
    use crate::models::{ColumnDefinition, ColumnType, DbDriver, TableDefinition};

    fn hostile_table() -> TableDefinition {
        TableDefinition {
            schema: None,
            // A name that closes the identifier and appends a statement.
            name: r#"t"; DROP TABLE users; --"#.into(),
            columns: vec![ColumnDefinition {
                name: "id`\"];--".into(),
                column_type: ColumnType::Integer,
                nullable: false,
                default: None,
                primary_key: true,
                auto_increment: false,
                comment: None,
            }],
            primary_key: None,
            foreign_keys: Vec::new(),
            unique_constraints: Vec::new(),
            check_constraints: Vec::new(),
            comment: None,
            if_not_exists: false,
        }
    }

    /// Strip quoted identifiers, so what remains is only SQL the generator
    /// actually emitted as syntax.
    ///
    /// The hostile text is *supposed* to appear in the output: it is the
    /// table's name. What must not happen is it appearing outside the
    /// quotes, where the server would execute it.
    fn outside_quotes(sql: &str, open: char, close: char) -> String {
        let mut out = String::new();
        let mut chars = sql.chars().peekable();
        while let Some(c) = chars.next() {
            if c != open {
                out.push(c);
                continue;
            }
            // Inside an identifier: consume to the closing quote, treating a
            // doubled quote as an escaped one.
            loop {
                match chars.next() {
                    None => return out,
                    Some(c) if c == close => {
                        if chars.peek() == Some(&close) {
                            chars.next(); // escaped, still inside
                        } else {
                            break;
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        out
    }

    /// A hostile identifier must stay inside its quotes in every dialect.
    #[test]
    fn identifiers_cannot_break_out_of_their_quoting() {
        for (driver, open, close) in [
            (DbDriver::Postgres, '"', '"'),
            (DbDriver::MySql, '`', '`'),
            (DbDriver::Sqlite, '"', '"'),
            (DbDriver::SqlServer, '[', ']'),
        ] {
            let generator = get_ddl_generator(&driver).unwrap();
            let sql = generator
                .generate_create_table(&hostile_table())
                .unwrap()
                .sql
                .join("\n");

            let bare = outside_quotes(&sql, open, close).to_uppercase();
            assert!(
                !bare.contains("DROP TABLE"),
                "{:?} let a DROP escape the quoting.\n  sql:  {}\n  bare: {}",
                driver,
                sql,
                bare
            );
            // The name is still fully present, escaped, inside the quotes.
            assert!(
                sql.contains("DROP TABLE users"),
                "{:?} silently mangled the name instead of escaping it:\n{}",
                driver,
                sql
            );
        }
    }
}
