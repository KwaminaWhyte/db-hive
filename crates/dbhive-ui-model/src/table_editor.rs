//! Pure logic for the table editor: create, alter and drop tables, and
//! create databases.
//!
//! Lifted from the egui `table_editor` screen so any shell can reuse it. The
//! SQL is produced by `dbhive_core::ddl`, which already knows each dialect,
//! so nothing here writes DDL by hand.
//!
//! No UI toolkit types. Drawing and job plumbing stay in the shell.

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::{
    AlterColumnOperation, AlterTableDefinition, ColumnDefinition, ColumnType, DbDriver, DbError,
    DropTableDefinition, TableDefinition,
};

/// One row of the column editor.
#[derive(Clone)]
pub struct ColumnRow {
    pub name: String,
    pub type_kind: TypeKind,
    pub length: u32,
    pub precision: u8,
    pub scale: u8,
    pub custom: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub default: String,
}

impl Default for ColumnRow {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_kind: TypeKind::Integer,
            length: 255,
            precision: 10,
            scale: 2,
            custom: String::new(),
            nullable: true,
            primary_key: false,
            auto_increment: false,
            default: String::new(),
        }
    }
}

/// A row of the editor, plus where it came from.
///
/// The origin is what makes a rename distinguishable from "dropped one
/// column and added another with a different name". Without it the diff
/// would drop and recreate a column the user merely renamed, taking its data
/// with it. New rows have no origin; that is exactly what marks them as
/// additions.
#[derive(Clone, Default)]
pub struct EditRow {
    pub col: ColumnRow,
    pub origin: Option<ColumnRow>,
}

impl EditRow {
    pub fn new(col: ColumnRow) -> Self {
        Self { col, origin: None }
    }

    /// A row loaded from a live table: its current state and its origin
    /// start out identical, so an untouched row diffs to nothing.
    pub fn existing(col: ColumnRow) -> Self {
        Self {
            origin: Some(col.clone()),
            col,
        }
    }
}

/// The type choices offered in the picker.
///
/// A flat enum rather than `ColumnType` itself, because the parameterised
/// variants (`Varchar { length }`, `Decimal { precision, scale }`) need their
/// arguments edited separately from the choice of type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind {
    SmallInt,
    Integer,
    BigInt,
    Decimal,
    Real,
    DoublePrecision,
    Varchar,
    Char,
    Text,
    Bytea,
    Boolean,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Json,
    JsonB,
    Uuid,
    Custom,
}

impl TypeKind {
    pub const ALL: &'static [TypeKind] = &[
        TypeKind::SmallInt,
        TypeKind::Integer,
        TypeKind::BigInt,
        TypeKind::Decimal,
        TypeKind::Real,
        TypeKind::DoublePrecision,
        TypeKind::Varchar,
        TypeKind::Char,
        TypeKind::Text,
        TypeKind::Bytea,
        TypeKind::Boolean,
        TypeKind::Date,
        TypeKind::Time,
        TypeKind::Timestamp,
        TypeKind::TimestampTz,
        TypeKind::Json,
        TypeKind::JsonB,
        TypeKind::Uuid,
        TypeKind::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TypeKind::SmallInt => "SMALLINT",
            TypeKind::Integer => "INTEGER",
            TypeKind::BigInt => "BIGINT",
            TypeKind::Decimal => "DECIMAL(p,s)",
            TypeKind::Real => "REAL",
            TypeKind::DoublePrecision => "DOUBLE PRECISION",
            TypeKind::Varchar => "VARCHAR(n)",
            TypeKind::Char => "CHAR(n)",
            TypeKind::Text => "TEXT",
            TypeKind::Bytea => "BINARY",
            TypeKind::Boolean => "BOOLEAN",
            TypeKind::Date => "DATE",
            TypeKind::Time => "TIME",
            TypeKind::Timestamp => "TIMESTAMP",
            TypeKind::TimestampTz => "TIMESTAMP WITH TIME ZONE",
            TypeKind::Json => "JSON",
            TypeKind::JsonB => "JSONB",
            TypeKind::Uuid => "UUID",
            TypeKind::Custom => "Custom…",
        }
    }

    /// Whether this variant needs a length, precision/scale, or a name.
    pub fn takes_length(self) -> bool {
        matches!(self, TypeKind::Varchar | TypeKind::Char)
    }
    pub fn takes_precision(self) -> bool {
        matches!(self, TypeKind::Decimal)
    }
    pub fn takes_custom_name(self) -> bool {
        matches!(self, TypeKind::Custom)
    }

    /// Only integers can sensibly auto-increment.
    pub fn can_auto_increment(self) -> bool {
        matches!(self, TypeKind::SmallInt | TypeKind::Integer | TypeKind::BigInt)
    }
}

/// Turn an editor row into the core's column type.
pub fn column_type(row: &ColumnRow) -> ColumnType {
    match row.type_kind {
        TypeKind::SmallInt => ColumnType::SmallInt,
        TypeKind::Integer => ColumnType::Integer,
        TypeKind::BigInt => ColumnType::BigInt,
        TypeKind::Decimal => ColumnType::Decimal {
            precision: row.precision,
            scale: row.scale,
        },
        TypeKind::Real => ColumnType::Real,
        TypeKind::DoublePrecision => ColumnType::DoublePrecision,
        TypeKind::Varchar => ColumnType::Varchar { length: row.length },
        TypeKind::Char => ColumnType::Char { length: row.length },
        TypeKind::Text => ColumnType::Text,
        TypeKind::Bytea => ColumnType::Bytea,
        TypeKind::Boolean => ColumnType::Boolean,
        TypeKind::Date => ColumnType::Date,
        TypeKind::Time => ColumnType::Time,
        TypeKind::Timestamp => ColumnType::Timestamp,
        TypeKind::TimestampTz => ColumnType::TimestampTz,
        TypeKind::Json => ColumnType::Json,
        TypeKind::JsonB => ColumnType::JsonB,
        TypeKind::Uuid => ColumnType::Uuid,
        TypeKind::Custom => ColumnType::Custom {
            type_name: row.custom.clone(),
        },
    }
}

/// What the screen is doing. One form, four jobs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    CreateTable,
    AlterTable,
    DropTable,
    CreateDatabase,
}

impl Mode {
    pub fn title(self) -> &'static str {
        match self {
            Mode::CreateTable => "New table",
            Mode::AlterTable => "Alter table",
            Mode::DropTable => "Drop table",
            Mode::CreateDatabase => "New database",
        }
    }

    /// The toast shown once the statements have run.
    pub fn done_message(self) -> &'static str {
        match self {
            Mode::CreateTable => "Table created",
            Mode::AlterTable => "Table altered",
            Mode::DropTable => "Table dropped",
            Mode::CreateDatabase => "Database created",
        }
    }

    /// The label on the button that opens the confirmation.
    pub fn action_label(self) -> &'static str {
        match self {
            Mode::CreateTable => "Create table",
            Mode::AlterTable => "Apply changes",
            Mode::DropTable => "Drop table",
            Mode::CreateDatabase => "Create database",
        }
    }

    pub fn confirm_label(self) -> &'static str {
        match self {
            Mode::CreateTable => "Yes, create it",
            Mode::AlterTable => "Yes, apply it",
            Mode::DropTable => "Yes, drop it",
            Mode::CreateDatabase => "Yes, create it",
        }
    }
}

/// Everything wrong with the form, as user-facing messages.
pub fn validate(name: &str, columns: &[ColumnRow]) -> Vec<String> {
    let mut errors = Vec::new();
    if name.trim().is_empty() {
        errors.push("Table name is required".into());
    }
    if columns.is_empty() {
        errors.push("A table needs at least one column".into());
    }
    if columns.iter().any(|c| c.name.trim().is_empty()) {
        errors.push("Every column needs a name".into());
    }

    let mut seen: Vec<String> = Vec::new();
    for c in columns {
        let lower = c.name.trim().to_lowercase();
        if lower.is_empty() {
            continue;
        }
        if seen.contains(&lower) {
            errors.push(format!("Duplicate column name: {}", c.name.trim()));
        }
        seen.push(lower);
    }

    for c in columns {
        if c.type_kind.takes_custom_name() && c.custom.trim().is_empty() {
            errors.push(format!("Column {} needs a type name", c.name.trim()));
        }
        if c.type_kind.takes_length() && c.length == 0 {
            errors.push(format!("Column {} needs a length above zero", c.name.trim()));
        }
        if c.type_kind.takes_precision() && c.scale > c.precision {
            errors.push(format!(
                "Column {}: scale ({}) cannot exceed precision ({})",
                c.name.trim(),
                c.scale,
                c.precision
            ));
        }
        if c.auto_increment && !c.type_kind.can_auto_increment() {
            errors.push(format!(
                "Column {} cannot auto-increment as {}",
                c.name.trim(),
                c.type_kind.label()
            ));
        }
        // A primary key column is implicitly NOT NULL everywhere; letting the
        // form claim otherwise would produce DDL the server rejects.
        if c.primary_key && c.nullable {
            errors.push(format!(
                "Column {} is part of the primary key, so it cannot be nullable",
                c.name.trim()
            ));
        }
    }
    errors
}

/// A column as the DDL layer wants it.
///
/// A primary key column is forced NOT NULL here rather than trusting the
/// form, because every server rejects a nullable primary key.
pub fn column_definition(c: &ColumnRow) -> ColumnDefinition {
    ColumnDefinition {
        name: c.name.trim().to_string(),
        column_type: column_type(c),
        nullable: c.nullable && !c.primary_key,
        default: {
            let d = c.default.trim();
            if d.is_empty() {
                None
            } else {
                Some(d.to_string())
            }
        },
        primary_key: c.primary_key,
        auto_increment: c.auto_increment,
        comment: None,
    }
}

/// Build the core definition from the form.
pub fn build_definition(schema: &str, name: &str, columns: &[ColumnRow]) -> TableDefinition {
    let primary: Vec<String> = columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.trim().to_string())
        .collect();

    TableDefinition {
        schema: if schema.is_empty() {
            None
        } else {
            Some(schema.to_string())
        },
        name: name.trim().to_string(),
        columns: columns.iter().map(column_definition).collect(),
        primary_key: if primary.is_empty() {
            None
        } else {
            Some(primary)
        },
        foreign_keys: Vec::new(),
        unique_constraints: Vec::new(),
        check_constraints: Vec::new(),
        comment: None,
        // Deliberately false: a silent no-op when the table already exists
        // hides a real mistake. Let the server say so.
        if_not_exists: false,
    }
}

/// Generate the CREATE TABLE statements for this driver.
pub fn preview_sql(
    driver: &DbDriver,
    schema: &str,
    name: &str,
    columns: &[ColumnRow],
) -> Result<Vec<String>, DbError> {
    let generator = get_ddl_generator(driver)?;
    let def = build_definition(schema, name, columns);
    Ok(generator.generate_create_table(&def)?.sql)
}

// ---------------------------------------------------------------------------
// Alter: loading, diffing, generating
// ---------------------------------------------------------------------------

fn trimmed_default(c: &ColumnRow) -> Option<String> {
    let d = c.default.trim();
    if d.is_empty() {
        None
    } else {
        Some(d.to_string())
    }
}

/// Effective nullability: a primary key column is never nullable.
fn effective_nullable(c: &ColumnRow) -> bool {
    c.nullable && !c.primary_key
}

/// What changed between the table as loaded and the table as edited.
///
/// Order matters and is deliberate:
///
/// 1. **Drops first**, so a column can be renamed onto a name that a dropped
///    column was using without colliding.
/// 2. **Renames before the other changes to the same column**, and every
///    later operation on that column uses its *new* name, because that is
///    what the table is called by the time they run.
/// 3. **Adds last**, so they are not affected by anything above.
///
/// An untouched form yields an empty list, which is what stops the screen
/// offering to run a no-op `ALTER TABLE`.
///
/// Column drops are never generated with CASCADE: a column another object
/// depends on should make the server refuse, not quietly take the dependent
/// view with it.
pub fn diff_columns(loaded: &[ColumnRow], rows: &[EditRow]) -> Vec<AlterColumnOperation> {
    let mut ops = Vec::new();

    let kept: Vec<String> = rows
        .iter()
        .filter_map(|r| r.origin.as_ref().map(|o| o.name.trim().to_string()))
        .collect();

    for original in loaded {
        let name = original.name.trim();
        if !kept.iter().any(|k| k == name) {
            ops.push(AlterColumnOperation::DropColumn {
                column_name: name.to_string(),
                cascade: false,
            });
        }
    }

    for row in rows {
        let Some(origin) = row.origin.as_ref() else {
            continue;
        };
        let old_name = origin.name.trim().to_string();
        let new_name = row.col.name.trim().to_string();
        // An empty name is a half-typed edit, not an instruction to rename a
        // column to nothing.
        let name = if new_name.is_empty() || new_name == old_name {
            old_name.clone()
        } else {
            ops.push(AlterColumnOperation::RenameColumn {
                old_name: old_name.clone(),
                new_name: new_name.clone(),
            });
            new_name
        };

        if column_type(&row.col) != column_type(origin) {
            ops.push(AlterColumnOperation::AlterType {
                column_name: name.clone(),
                new_type: column_type(&row.col),
            });
        }
        if effective_nullable(&row.col) != effective_nullable(origin) {
            ops.push(AlterColumnOperation::SetNotNull {
                column_name: name.clone(),
                not_null: !effective_nullable(&row.col),
            });
        }
        if trimmed_default(&row.col) != trimmed_default(origin) {
            ops.push(AlterColumnOperation::SetDefault {
                column_name: name,
                default: trimmed_default(&row.col),
            });
        }
    }

    for row in rows {
        if row.origin.is_none() && !row.col.name.trim().is_empty() {
            ops.push(AlterColumnOperation::AddColumn {
                column: column_definition(&row.col),
            });
        }
    }

    ops
}

/// Generate the ALTER TABLE statements, or nothing when nothing changed.
pub fn preview_alter_sql(
    driver: &DbDriver,
    schema: &str,
    name: &str,
    loaded: &[ColumnRow],
    rows: &[EditRow],
) -> Result<Vec<String>, DbError> {
    let operations = diff_columns(loaded, rows);
    if operations.is_empty() {
        return Ok(Vec::new());
    }
    let generator = get_ddl_generator(driver)?;
    let alter = AlterTableDefinition {
        schema: if schema.is_empty() {
            None
        } else {
            Some(schema.to_string())
        },
        name: name.trim().to_string(),
        operations,
    };
    Ok(generator.generate_alter_table(&alter)?.sql)
}

// ---------------------------------------------------------------------------
// Drop
// ---------------------------------------------------------------------------

/// Whether the typed confirmation matches the table being dropped.
///
/// The match is exact after trimming, not case-insensitive: identifiers are
/// case-sensitive on several of the servers this talks to, and the point of
/// the box is to make the user look at the name they are about to destroy.
pub fn drop_confirmed(table: &str, typed: &str) -> bool {
    let table = table.trim();
    !table.is_empty() && typed.trim() == table
}

/// Generate the DROP TABLE statement.
///
/// `IF EXISTS` is deliberately off: a drop that silently does nothing
/// because the name was wrong is worse than an error saying so.
pub fn preview_drop_sql(
    driver: &DbDriver,
    schema: &str,
    name: &str,
    cascade: bool,
) -> Result<Vec<String>, DbError> {
    let generator = get_ddl_generator(driver)?;
    let drop = DropTableDefinition {
        schema: if schema.is_empty() {
            None
        } else {
            Some(schema.to_string())
        },
        name: name.trim().to_string(),
        cascade,
        if_exists: false,
    };
    Ok(generator.generate_drop_table(&drop)?.sql)
}

// ---------------------------------------------------------------------------
// Form helpers
// ---------------------------------------------------------------------------

/// Display-only qualified name. Never used in SQL, so it is not quoted.
pub fn qualified_display(schema: &str, table: &str) -> String {
    if schema.trim().is_empty() {
        table.to_string()
    } else {
        format!("{}.{}", schema, table)
    }
}

/// Form-level validation for a drop: only the table name is needed.
pub fn drop_table_errors(table_name: &str) -> Vec<String> {
    if table_name.trim().is_empty() {
        vec!["Table name is required".into()]
    } else {
        Vec::new()
    }
}

/// Form-level validation for a new database name, via the core's rule.
pub fn database_name_errors(database_name: &str) -> Vec<String> {
    match dbhive_core::service::validate_database_name(database_name) {
        Ok(()) => Vec::new(),
        // While the box is still empty this is a prompt, not a
        // complaint, so it is left to the hint text.
        Err(_) if database_name.trim().is_empty() => Vec::new(),
        Err(e) => vec![e.to_string()],
    }
}

/// The CREATE DATABASE statement the preview shows, or nothing while the
/// name is still empty.
pub fn preview_create_database_sql(
    driver: &DbDriver,
    database_name: &str,
) -> Result<Vec<String>, DbError> {
    if database_name.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Quoting belongs to the driver, but the preview must work
    // before anything runs, so the dialect's own rule is applied
    // through the same function the execution path uses.
    let quote = quoter(driver);
    dbhive_core::service::create_database_sql(driver, database_name, &quote).map(|sql| vec![sql])
}

/// The dialect's identifier quoting, matching each driver's
/// `DatabaseDriver::quote_identifier`.
///
/// Used only for the preview, which has no connection to ask. The statement
/// that actually runs is built by `service::create_database` from the live
/// driver.
pub fn quoter(driver: &DbDriver) -> impl Fn(&str) -> String {
    let driver = driver.clone();
    move |ident: &str| match driver {
        DbDriver::MySql => format!("`{}`", ident.replace('`', "``")),
        DbDriver::SqlServer => format!("[{}]", ident.replace(']', "]]")),
        _ => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str) -> ColumnRow {
        ColumnRow {
            name: name.into(),
            ..Default::default()
        }
    }

    fn rows(cols: &[ColumnRow]) -> Vec<EditRow> {
        cols.iter().cloned().map(EditRow::existing).collect()
    }

    #[test]
    fn a_nameless_table_is_rejected() {
        assert!(validate("", &[col("id")])
            .iter()
            .any(|e| e.contains("Table name")));
    }

    #[test]
    fn a_table_needs_columns() {
        assert!(validate("t", &[])
            .iter()
            .any(|e| e.contains("at least one column")));
    }

    #[test]
    fn duplicate_columns_are_rejected_case_insensitively() {
        let cols = vec![col("id"), col("ID")];
        assert!(validate("t", &cols)
            .iter()
            .any(|e| e.contains("Duplicate column")));
    }

    #[test]
    fn a_nullable_primary_key_is_rejected() {
        let mut c = col("id");
        c.primary_key = true;
        c.nullable = true;
        assert!(validate("t", &[c]).iter().any(|e| e.contains("nullable")));
    }

    #[test]
    fn auto_increment_only_on_integers() {
        let mut c = col("id");
        c.type_kind = TypeKind::Text;
        c.auto_increment = true;
        assert!(validate("t", &[c]).iter().any(|e| e.contains("auto-increment")));

        let mut ok = col("id");
        ok.type_kind = TypeKind::BigInt;
        ok.auto_increment = true;
        ok.nullable = true;
        assert!(!validate("t", &[ok])
            .iter()
            .any(|e| e.contains("auto-increment")));
    }

    #[test]
    fn scale_cannot_exceed_precision() {
        let mut c = col("amount");
        c.type_kind = TypeKind::Decimal;
        c.precision = 4;
        c.scale = 6;
        assert!(validate("t", &[c]).iter().any(|e| e.contains("scale")));
    }

    #[test]
    fn a_custom_type_needs_a_name() {
        let mut c = col("weird");
        c.type_kind = TypeKind::Custom;
        assert!(validate("t", &[c]).iter().any(|e| e.contains("type name")));
    }

    #[test]
    fn a_valid_table_has_no_errors() {
        let mut id = col("id");
        id.type_kind = TypeKind::BigInt;
        id.primary_key = true;
        id.nullable = false;
        let mut name = col("name");
        name.type_kind = TypeKind::Varchar;
        name.length = 100;
        assert!(validate("users", &[id, name]).is_empty());
    }

    #[test]
    fn definition_marks_primary_keys_not_null() {
        let mut id = col("id");
        id.primary_key = true;
        id.nullable = true; // the form should not be able to win this
        let def = build_definition("public", "users", &[id]);
        assert!(!def.columns[0].nullable);
        assert_eq!(def.primary_key, Some(vec!["id".to_string()]));
    }

    #[test]
    fn definition_trims_names_and_drops_empty_defaults() {
        let mut c = col("  spaced  ");
        c.default = "   ".into();
        let def = build_definition("", "  users  ", &[c]);
        assert_eq!(def.name, "users");
        assert_eq!(def.columns[0].name, "spaced");
        assert_eq!(def.columns[0].default, None);
        assert_eq!(def.schema, None, "an empty schema should not be emitted");
    }

    #[test]
    fn generated_sql_matches_the_dialect() {
        let mut id = col("id");
        id.type_kind = TypeKind::BigInt;
        id.primary_key = true;
        id.nullable = false;
        let mut name = col("name");
        name.type_kind = TypeKind::Varchar;
        name.length = 50;

        let pg = preview_sql(&DbDriver::Postgres, "public", "users", &[id.clone(), name.clone()])
            .unwrap()
            .join("\n");
        assert!(pg.contains("users"), "{}", pg);
        assert!(pg.to_uppercase().contains("CREATE TABLE"), "{}", pg);
        assert!(pg.contains("VARCHAR(50)"), "{}", pg);

        // A different dialect must not produce byte-identical SQL by accident.
        let my = preview_sql(&DbDriver::MySql, "", "users", &[id, name])
            .unwrap()
            .join("\n");
        assert!(my.to_uppercase().contains("CREATE TABLE"), "{}", my);
    }

    #[test]
    fn column_type_carries_its_parameters() {
        let mut c = col("amount");
        c.type_kind = TypeKind::Decimal;
        c.precision = 12;
        c.scale = 4;
        assert_eq!(
            column_type(&c),
            ColumnType::Decimal {
                precision: 12,
                scale: 4
            }
        );
    }

    // ── alter: diffing ───────────────────────────────────────────────────

    #[test]
    fn an_untouched_form_produces_no_operations() {
        let loaded = vec![col("id"), col("name")];
        let ops = diff_columns(&loaded, &rows(&loaded));
        assert!(ops.is_empty(), "{:?}", ops.len());
    }

    #[test]
    fn a_new_row_becomes_an_add() {
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        let mut added = col("email");
        added.type_kind = TypeKind::Varchar;
        added.length = 120;
        edited.push(EditRow::new(added));

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            AlterColumnOperation::AddColumn { column } => {
                assert_eq!(column.name, "email");
                assert_eq!(column.column_type, ColumnType::Varchar { length: 120 });
            }
            other => panic!("expected an add, got {:?}", other),
        }
    }

    #[test]
    fn a_half_typed_new_row_is_not_an_add() {
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        edited.push(EditRow::new(col("   ")));
        assert!(diff_columns(&loaded, &edited).is_empty());
    }

    #[test]
    fn a_removed_row_becomes_a_drop() {
        let loaded = vec![col("id"), col("legacy")];
        let mut edited = rows(&loaded);
        edited.remove(1);

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            AlterColumnOperation::DropColumn {
                column_name,
                cascade,
            } => {
                assert_eq!(column_name, "legacy");
                // Never cascading: dependents should make the server refuse.
                assert!(!cascade);
            }
            other => panic!("expected a drop, got {:?}", other),
        }
    }

    #[test]
    fn a_renamed_column_is_renamed_not_recreated() {
        let loaded = vec![col("old_name")];
        let mut edited = rows(&loaded);
        edited[0].col.name = "new_name".into();

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            AlterColumnOperation::RenameColumn { old_name, new_name } => {
                assert_eq!(old_name, "old_name");
                assert_eq!(new_name, "new_name");
            }
            other => panic!("expected a rename, got {:?}", other),
        }
    }

    #[test]
    fn clearing_a_name_is_treated_as_unfinished_not_as_a_rename() {
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        edited[0].col.name = "  ".into();
        assert!(diff_columns(&loaded, &edited).is_empty());
    }

    #[test]
    fn a_type_change_becomes_an_alter_type() {
        let loaded = vec![col("amount")];
        let mut edited = rows(&loaded);
        edited[0].col.type_kind = TypeKind::Decimal;
        edited[0].col.precision = 12;
        edited[0].col.scale = 2;

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            AlterColumnOperation::AlterType {
                column_name,
                new_type,
            } => {
                assert_eq!(column_name, "amount");
                assert_eq!(
                    *new_type,
                    ColumnType::Decimal {
                        precision: 12,
                        scale: 2
                    }
                );
            }
            other => panic!("expected a type change, got {:?}", other),
        }
    }

    #[test]
    fn nullability_and_defaults_each_produce_one_operation() {
        let mut loaded_col = col("name");
        loaded_col.nullable = true;
        loaded_col.default = String::new();
        let loaded = vec![loaded_col];

        let mut edited = rows(&loaded);
        edited[0].col.nullable = false;
        edited[0].col.default = "'unknown'".into();

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 2);
        assert!(matches!(
            ops[0],
            AlterColumnOperation::SetNotNull { not_null: true, .. }
        ));
        match &ops[1] {
            AlterColumnOperation::SetDefault { default, .. } => {
                assert_eq!(default.as_deref(), Some("'unknown'"))
            }
            other => panic!("expected a default change, got {:?}", other),
        }
    }

    #[test]
    fn clearing_a_default_drops_it() {
        let mut c = col("created");
        c.default = "CURRENT_TIMESTAMP".into();
        let loaded = vec![c];
        let mut edited = rows(&loaded);
        edited[0].col.default = "  ".into();

        let ops = diff_columns(&loaded, &edited);
        assert!(matches!(
            &ops[0],
            AlterColumnOperation::SetDefault { default: None, .. }
        ));
    }

    #[test]
    fn making_a_column_a_primary_key_makes_it_not_null() {
        let mut c = col("id");
        c.nullable = true;
        let loaded = vec![c];
        let mut edited = rows(&loaded);
        edited[0].col.primary_key = true;
        edited[0].col.nullable = false;

        let ops = diff_columns(&loaded, &edited);
        assert!(matches!(
            ops[0],
            AlterColumnOperation::SetNotNull { not_null: true, .. }
        ));
    }

    #[test]
    fn later_operations_use_the_new_name_after_a_rename() {
        let loaded = vec![col("old")];
        let mut edited = rows(&loaded);
        edited[0].col.name = "new".into();
        edited[0].col.type_kind = TypeKind::Text;

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], AlterColumnOperation::RenameColumn { .. }));
        match &ops[1] {
            // By the time this runs the column is called "new"; using the old
            // name would fail.
            AlterColumnOperation::AlterType { column_name, .. } => assert_eq!(column_name, "new"),
            other => panic!("expected a type change, got {:?}", other),
        }
    }

    #[test]
    fn drops_come_before_renames_so_a_freed_name_can_be_reused() {
        let loaded = vec![col("a"), col("b")];
        let mut edited = rows(&loaded);
        // Drop "a", then rename "b" onto the name "a" just freed.
        edited.remove(0);
        edited[0].col.name = "a".into();

        let ops = diff_columns(&loaded, &edited);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], AlterColumnOperation::DropColumn { .. }));
        assert!(matches!(ops[1], AlterColumnOperation::RenameColumn { .. }));
    }

    #[test]
    fn adds_come_last() {
        let loaded = vec![col("gone"), col("kept")];
        let mut edited = rows(&loaded);
        edited.remove(0);
        edited[0].col.type_kind = TypeKind::Text;
        edited.push(EditRow::new(col("fresh")));

        let ops = diff_columns(&loaded, &edited);
        assert!(matches!(ops[0], AlterColumnOperation::DropColumn { .. }));
        assert!(matches!(ops[1], AlterColumnOperation::AlterType { .. }));
        assert!(matches!(
            ops[ops.len() - 1],
            AlterColumnOperation::AddColumn { .. }
        ));
    }

    // ── alter: SQL ───────────────────────────────────────────────────────

    #[test]
    fn no_changes_generate_no_alter_statement() {
        let loaded = vec![col("id")];
        let sql = preview_alter_sql(
            &DbDriver::Postgres,
            "public",
            "users",
            &loaded,
            &rows(&loaded),
        )
        .unwrap();
        assert!(sql.is_empty(), "a no-op should not offer to run anything");
    }

    #[test]
    fn alter_sql_is_generated_per_dialect() {
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        let mut added = col("email");
        added.type_kind = TypeKind::Varchar;
        added.length = 120;
        edited.push(EditRow::new(added));

        let pg = preview_alter_sql(&DbDriver::Postgres, "public", "users", &loaded, &edited)
            .unwrap()
            .join("\n");
        assert!(pg.contains("ALTER TABLE"), "{}", pg);
        assert!(pg.contains("ADD COLUMN"), "{}", pg);
        assert!(pg.contains("\"public\".\"users\""), "{}", pg);
        assert!(pg.contains("\"email\""), "{}", pg);

        let my = preview_alter_sql(&DbDriver::MySql, "app", "users", &loaded, &edited)
            .unwrap()
            .join("\n");
        assert!(my.contains('`'), "MySQL should quote with backticks: {}", my);
        assert_ne!(pg, my, "two dialects produced identical SQL");
    }

    #[test]
    fn sqlite_reports_the_alterations_it_cannot_express() {
        // SQLite cannot change a column's type; the generator says so rather
        // than emitting SQL that fails at the server.
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        edited[0].col.type_kind = TypeKind::Text;
        assert!(preview_alter_sql(&DbDriver::Sqlite, "", "users", &loaded, &edited).is_err());

        // What it can express still works.
        let mut adds = rows(&loaded);
        adds.push(EditRow::new(col("extra")));
        let sql = preview_alter_sql(&DbDriver::Sqlite, "", "users", &loaded, &adds).unwrap();
        assert!(sql[0].contains("ADD COLUMN"), "{}", sql[0]);
    }

    #[test]
    fn a_driver_without_ddl_reports_rather_than_guessing() {
        let loaded = vec![col("id")];
        let mut edited = rows(&loaded);
        edited.push(EditRow::new(col("extra")));
        assert!(preview_alter_sql(&DbDriver::Redis, "", "t", &loaded, &edited).is_err());
        assert!(preview_drop_sql(&DbDriver::Redis, "", "t", false).is_err());
    }

    // ── drop ─────────────────────────────────────────────────────────────

    #[test]
    fn a_drop_needs_the_exact_name_typed() {
        assert!(drop_confirmed("users", "users"));
        // Trimming is a courtesy; the characters still have to match.
        assert!(drop_confirmed("users", "  users  "));
        assert!(!drop_confirmed("users", "user"));
        assert!(!drop_confirmed("users", ""));
        // Case matters: identifiers are case-sensitive on several servers,
        // and the box exists to make the user read the name.
        assert!(!drop_confirmed("Users", "users"));
    }

    #[test]
    fn a_drop_with_no_table_can_never_be_confirmed() {
        assert!(!drop_confirmed("", ""));
        assert!(!drop_confirmed("   ", "   "));
    }

    #[test]
    fn drop_sql_quotes_and_qualifies_the_table() {
        let sql = preview_drop_sql(&DbDriver::Postgres, "public", "users", false)
            .unwrap()
            .join("\n");
        assert!(sql.contains("DROP TABLE"), "{}", sql);
        assert!(sql.contains("\"public\".\"users\""), "{}", sql);
        // IF EXISTS would turn a mistyped name into a silent no-op.
        assert!(!sql.contains("IF EXISTS"), "{}", sql);
        assert!(!sql.contains("CASCADE"), "{}", sql);
    }

    #[test]
    fn drop_sql_cascades_only_when_asked() {
        let sql = preview_drop_sql(&DbDriver::Postgres, "public", "users", true)
            .unwrap()
            .join("\n");
        assert!(sql.contains("CASCADE"), "{}", sql);
    }

    #[test]
    fn drop_sql_is_generated_per_dialect() {
        let my = preview_drop_sql(&DbDriver::MySql, "app", "users", false)
            .unwrap()
            .join("\n");
        assert!(my.contains('`'), "{}", my);
        let ms = preview_drop_sql(&DbDriver::SqlServer, "dbo", "users", false)
            .unwrap()
            .join("\n");
        assert!(ms.contains('['), "{}", ms);
    }

    #[test]
    fn an_unqualified_drop_omits_the_schema() {
        let sql = preview_drop_sql(&DbDriver::Sqlite, "", "users", false)
            .unwrap()
            .join("\n");
        assert!(sql.contains("\"users\""), "{}", sql);
        assert!(!sql.contains('.'), "{}", sql);
    }

    // ── create database ──────────────────────────────────────────────────

    #[test]
    fn the_preview_quoter_matches_each_driver() {
        assert_eq!(quoter(&DbDriver::Postgres)("my-db"), "\"my-db\"");
        assert_eq!(quoter(&DbDriver::MySql)("my-db"), "`my-db`");
        assert_eq!(quoter(&DbDriver::SqlServer)("my-db"), "[my-db]");
        // An embedded quote is doubled rather than closing the identifier.
        assert_eq!(quoter(&DbDriver::Postgres)("a\"b"), "\"a\"\"b\"");
        assert_eq!(quoter(&DbDriver::MySql)("a`b"), "`a``b`");
        assert_eq!(quoter(&DbDriver::SqlServer)("a]b"), "[a]]b]");
    }

    #[test]
    fn database_name_errors_treat_empty_as_a_prompt() {
        assert!(database_name_errors("").is_empty());
        assert!(database_name_errors("   ").is_empty());
        assert!(!database_name_errors("1bad").is_empty());
        assert!(database_name_errors("analytics").is_empty());
    }

    #[test]
    fn create_database_preview_is_empty_until_named_and_refused_where_unsupported() {
        assert!(preview_create_database_sql(&DbDriver::Postgres, "  ")
            .unwrap()
            .is_empty());
        assert_eq!(
            preview_create_database_sql(&DbDriver::Postgres, "analytics").unwrap(),
            vec!["CREATE DATABASE \"analytics\"".to_string()]
        );
        assert!(preview_create_database_sql(&DbDriver::Sqlite, "analytics").is_err());
    }

    #[test]
    fn a_drop_without_a_table_name_is_an_error() {
        assert!(!drop_table_errors("  ").is_empty());
        assert!(drop_table_errors("users").is_empty());
    }
}
