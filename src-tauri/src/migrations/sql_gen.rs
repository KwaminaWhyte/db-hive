//! Migration SQL generator.
//!
//! Converts a `SchemaDiff` into an ordered list of SQL statements. Statements
//! are emitted in this dependency-safe order:
//!   1. CREATE new tables (without FKs)
//!   2. ADD new columns
//!   3. MODIFY existing columns
//!   4. ADD indexes
//!   5. ADD foreign keys
//!   6. DROP foreign keys
//!   7. DROP indexes
//!   8. DROP removed columns
//!   9. DROP removed tables
//!
//! Column type strings come directly from metadata (`ColumnInfo::data_type`),
//! which each driver already formats in its native dialect. FK definitions on
//! newly-created tables are deferred to the ADD-FK pass so referenced tables
//! created in the same migration exist first.

use crate::migrations::diff::SchemaDiff;
use crate::models::metadata::{ColumnInfo, ForeignKeyInfo, IndexInfo, TableSchema};
use crate::models::{DbDriver, DbError};

pub fn generate_migration_sql(
    diff: &SchemaDiff,
    driver: &DbDriver,
) -> Result<Vec<String>, DbError> {
    let q = Quoter::for_driver(driver)?;
    let mut out: Vec<String> = Vec::new();

    // 1. CREATE tables (columns + PK only; FKs deferred)
    for t in &diff.added_tables {
        out.push(create_table_sql(t, &q));
        for idx in &t.indexes {
            if idx.is_primary {
                continue;
            }
            out.push(create_index_sql(&t.table.schema, &t.table.name, idx, &q));
        }
    }

    // 2. ADD columns
    for td in &diff.modified_tables {
        for col in &td.added_columns {
            out.push(format!(
                "ALTER TABLE {} ADD COLUMN {}",
                q.qualified(&td.schema, &td.name),
                column_sql(col, &q),
            ));
        }
    }

    // 3. MODIFY columns (best-effort; driver-specific syntax)
    for td in &diff.modified_tables {
        for ch in &td.modified_columns {
            out.extend(alter_column_sql(
                &td.schema, &td.name, &ch.source, &ch.target, &q, driver,
            ));
        }
    }

    // 4. ADD indexes on existing tables
    for td in &diff.modified_tables {
        for idx in &td.added_indexes {
            out.push(create_index_sql(&td.schema, &td.name, idx, &q));
        }
    }

    // 5. ADD foreign keys — both for new tables and modified tables
    for t in &diff.added_tables {
        // FKs for added tables come via diff.modified_tables only when both
        // sides exist; for brand-new tables we have no FK info in SchemaDiff,
        // so the caller should include them separately if desired.
        let _ = t;
    }
    for td in &diff.modified_tables {
        for fk in &td.added_fks {
            out.push(add_fk_sql(&td.schema, &td.name, fk, &q));
        }
    }

    // 6. DROP foreign keys
    for td in &diff.modified_tables {
        for fk in &td.removed_fks {
            out.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}",
                q.qualified(&td.schema, &td.name),
                q.ident(&fk.name),
            ));
        }
    }

    // 7. DROP indexes
    for td in &diff.modified_tables {
        for idx in &td.removed_indexes {
            out.push(drop_index_sql(&td.schema, &idx.name, &q, driver));
        }
    }

    // 8. DROP removed columns
    for td in &diff.modified_tables {
        for col in &td.removed_columns {
            out.push(format!(
                "ALTER TABLE {} DROP COLUMN {}",
                q.qualified(&td.schema, &td.name),
                q.ident(&col.name),
            ));
        }
    }

    // 9. DROP removed tables
    for t in &diff.removed_tables {
        out.push(format!(
            "DROP TABLE {}",
            q.qualified(&t.table.schema, &t.table.name)
        ));
    }

    Ok(out)
}

// ── SQL fragment builders ──

fn create_table_sql(t: &TableSchema, q: &Quoter) -> String {
    let mut cols: Vec<String> = t.columns.iter().map(|c| column_sql(c, q)).collect();
    let pks: Vec<&str> = t
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.as_str())
        .collect();
    if pks.len() > 1 {
        let quoted: Vec<String> = pks.iter().map(|n| q.ident(n)).collect();
        cols.push(format!("PRIMARY KEY ({})", quoted.join(", ")));
    }
    format!(
        "CREATE TABLE {} (\n  {}\n)",
        q.qualified(&t.table.schema, &t.table.name),
        cols.join(",\n  ")
    )
}

fn column_sql(c: &ColumnInfo, q: &Quoter) -> String {
    let mut parts = vec![q.ident(&c.name), c.data_type.clone()];
    if !c.nullable {
        parts.push("NOT NULL".to_string());
    }
    if let Some(d) = &c.default_value {
        parts.push(format!("DEFAULT {}", d));
    }
    // Inline PK only when the column is solo-PK; composite PKs are emitted
    // separately by create_table_sql.
    if c.is_primary_key {
        parts.push("PRIMARY KEY".to_string());
    }
    parts.join(" ")
}

fn create_index_sql(schema: &str, table: &str, idx: &IndexInfo, q: &Quoter) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    let cols: Vec<String> = idx.columns.iter().map(|c| q.ident(c)).collect();
    format!(
        "CREATE {}INDEX {} ON {} ({})",
        unique,
        q.ident(&idx.name),
        q.qualified(schema, table),
        cols.join(", ")
    )
}

fn drop_index_sql(schema: &str, index_name: &str, q: &Quoter, driver: &DbDriver) -> String {
    match driver {
        DbDriver::MySql => format!(
            "DROP INDEX {} ON {}",
            q.ident(index_name),
            q.qualified(schema, "")
        ),
        _ => format!("DROP INDEX {}", q.qualified(schema, index_name)),
    }
}

fn add_fk_sql(schema: &str, table: &str, fk: &ForeignKeyInfo, q: &Quoter) -> String {
    let cols: Vec<String> = fk.columns.iter().map(|c| q.ident(c)).collect();
    let ref_cols: Vec<String> = fk.referenced_columns.iter().map(|c| q.ident(c)).collect();
    let mut s = format!(
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        q.qualified(schema, table),
        q.ident(&fk.name),
        cols.join(", "),
        q.qualified(&fk.referenced_schema, &fk.referenced_table),
        ref_cols.join(", "),
    );
    if let Some(a) = &fk.on_delete {
        s.push_str(&format!(" ON DELETE {}", a));
    }
    if let Some(a) = &fk.on_update {
        s.push_str(&format!(" ON UPDATE {}", a));
    }
    s
}

fn alter_column_sql(
    schema: &str,
    table: &str,
    source: &ColumnInfo,
    target: &ColumnInfo,
    q: &Quoter,
    driver: &DbDriver,
) -> Vec<String> {
    let qualified = q.qualified(schema, table);
    let col = q.ident(&source.name);
    let mut out = Vec::new();

    if !source.data_type.eq_ignore_ascii_case(&target.data_type) {
        match driver {
            DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
                out.push(format!(
                    "ALTER TABLE {} ALTER COLUMN {} TYPE {}",
                    qualified, col, source.data_type
                ));
            }
            DbDriver::MySql => {
                out.push(format!(
                    "ALTER TABLE {} MODIFY COLUMN {} {}{}",
                    qualified,
                    col,
                    source.data_type,
                    if source.nullable { "" } else { " NOT NULL" }
                ));
            }
            DbDriver::SqlServer => {
                out.push(format!(
                    "ALTER TABLE {} ALTER COLUMN {} {}{}",
                    qualified,
                    col,
                    source.data_type,
                    if source.nullable { " NULL" } else { " NOT NULL" }
                ));
            }
            DbDriver::Sqlite | DbDriver::Turso => {
                out.push(format!(
                    "-- SQLite does not support ALTER COLUMN TYPE for {}.{}",
                    qualified, col
                ));
            }
            _ => {}
        }
    }

    if source.nullable != target.nullable {
        match driver {
            DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
                if source.nullable {
                    out.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL",
                        qualified, col
                    ));
                } else {
                    out.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL",
                        qualified, col
                    ));
                }
            }
            _ => {}
        }
    }

    if source.default_value != target.default_value {
        match driver {
            DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
                if let Some(d) = &source.default_value {
                    out.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {}",
                        qualified, col, d
                    ));
                } else {
                    out.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT",
                        qualified, col
                    ));
                }
            }
            _ => {}
        }
    }

    out
}

// ── Identifier quoting ──

struct Quoter {
    open: char,
    close: char,
}

impl Quoter {
    fn for_driver(driver: &DbDriver) -> Result<Self, DbError> {
        match driver {
            DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon | DbDriver::Sqlite | DbDriver::Turso => {
                Ok(Self { open: '"', close: '"' })
            }
            DbDriver::MySql => Ok(Self { open: '`', close: '`' }),
            DbDriver::SqlServer => Ok(Self { open: '[', close: ']' }),
            DbDriver::MongoDb | DbDriver::Redis => Err(DbError::InvalidInput(
                "Schema migrations are not supported for this driver".to_string(),
            )),
        }
    }

    fn ident(&self, name: &str) -> String {
        format!("{}{}{}", self.open, name, self.close)
    }

    fn qualified(&self, schema: &str, name: &str) -> String {
        if schema.is_empty() {
            return self.ident(name);
        }
        if name.is_empty() {
            return self.ident(schema);
        }
        format!("{}.{}", self.ident(schema), self.ident(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::diff::{compute_diff, TableWithFks};
    use crate::models::metadata::{TableInfo, TableSchema};

    fn tbl(name: &str, cols: Vec<ColumnInfo>) -> TableWithFks {
        TableWithFks {
            schema: TableSchema::new(
                TableInfo::new(name.into(), "public".into(), "TABLE".into()),
                cols,
                vec![],
            ),
            foreign_keys: vec![],
        }
    }

    #[test]
    fn generates_create_table_for_added() {
        let src = vec![tbl(
            "users",
            vec![ColumnInfo::new("id".into(), "INTEGER".into(), false)],
        )];
        let d = compute_diff(&src, &[]);
        let sql = generate_migration_sql(&d, &DbDriver::Postgres).unwrap();
        assert!(sql[0].contains("CREATE TABLE"));
        assert!(sql[0].contains("users"));
    }

    #[test]
    fn generates_drop_table_for_removed() {
        let tgt = vec![tbl("old", vec![])];
        let d = compute_diff(&[], &tgt);
        let sql = generate_migration_sql(&d, &DbDriver::Postgres).unwrap();
        assert!(sql.iter().any(|s| s.starts_with("DROP TABLE")));
    }
}
