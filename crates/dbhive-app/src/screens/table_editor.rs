//! Create, alter and drop tables, and create databases.
//!
//! The SQL is produced by `dbhive_core::ddl`, which already knows each
//! dialect, so this screen never writes DDL by hand. It shows the generated
//! statements before running them: DDL is not undoable, and a user who
//! cannot see what is about to run has no way to catch a mistake.
//!
//! Four modes share one form, because they share one shape: pick a target,
//! edit it, read the SQL, confirm, run.
//!
//! * **Create table** builds a `TableDefinition` from an empty form.
//! * **Alter table** loads the live table, lets the same column editor edit
//!   it, and *diffs* the result: the operations are derived from what
//!   changed, never from what the form happens to contain. A form that is
//!   untouched produces no SQL at all.
//! * **Drop table** is unrecoverable, so beyond the usual confirmation it
//!   makes the user type the table's name. A misplaced click cannot do it.
//! * **Create database** is here rather than in its own screen because it is
//!   the same preview-then-confirm flow over `dbhive_core::service`.

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::drivers::DatabaseDriver;
use dbhive_core::models::{
    AlterColumnOperation, AlterTableDefinition, ColumnDefinition, ColumnType, DbDriver, DbError,
    DropTableDefinition, TableDefinition, TableSchema,
};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;

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
    fn title(self) -> &'static str {
        match self {
            Mode::CreateTable => "New table",
            Mode::AlterTable => "Alter table",
            Mode::DropTable => "Drop table",
            Mode::CreateDatabase => "New database",
        }
    }

    /// The toast shown once the statements have run.
    fn done_message(self) -> &'static str {
        match self {
            Mode::CreateTable => "Table created",
            Mode::AlterTable => "Table altered",
            Mode::DropTable => "Table dropped",
            Mode::CreateDatabase => "Database created",
        }
    }

    /// The label on the button that opens the confirmation.
    fn action_label(self) -> &'static str {
        match self {
            Mode::CreateTable => "Create table",
            Mode::AlterTable => "Apply changes",
            Mode::DropTable => "Drop table",
            Mode::CreateDatabase => "Create database",
        }
    }

    fn confirm_label(self) -> &'static str {
        match self {
            Mode::CreateTable => "Yes, create it",
            Mode::AlterTable => "Yes, apply it",
            Mode::DropTable => "Yes, drop it",
            Mode::CreateDatabase => "Yes, create it",
        }
    }
}

#[derive(Default)]
pub struct TableEditor {
    pub open: bool,
    pub mode: Mode,
    pub connection_id: Option<String>,
    pub schema: String,
    pub table_name: String,
    pub columns: Vec<EditRow>,
    pub preview: Vec<String>,
    pub error: Option<String>,
    pub run_job: Job<Result<(), DbError>>,
    pub confirming: bool,

    /// Loading the table being altered. Alter mode cannot diff against
    /// something it has not read, so the form waits for this.
    pub load_job: Job<Result<TableSchema, DbError>>,
    /// The table exactly as loaded. Kept separately from the rows so a
    /// column the user deleted is still known to have existed.
    pub loaded: Vec<ColumnRow>,
    /// The user's typed confirmation for a drop.
    pub drop_confirm: String,
    pub drop_cascade: bool,
    /// The name for a new database.
    pub database_name: String,
}

impl TableEditor {
    /// Reset everything that is per-target, keeping nothing from the last
    /// use. Every `open_*` goes through here so a stale preview or a
    /// half-typed drop confirmation cannot survive into the next target.
    fn reset(&mut self, mode: Mode, connection_id: impl Into<String>) {
        self.open = true;
        self.mode = mode;
        self.connection_id = Some(connection_id.into());
        self.error = None;
        self.preview.clear();
        self.confirming = false;
        self.columns.clear();
        self.loaded.clear();
        self.drop_confirm.clear();
        self.drop_cascade = false;
        self.database_name.clear();
        self.table_name.clear();
    }

    pub fn open_new(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        self.reset(Mode::CreateTable, connection_id);
        self.schema = schema.into();
        // A table with no columns cannot be created, so start with the one
        // almost every table has.
        self.columns = vec![EditRow::new(ColumnRow {
            name: "id".into(),
            type_kind: TypeKind::BigInt,
            nullable: false,
            primary_key: true,
            auto_increment: true,
            ..Default::default()
        })];
    }

    /// Edit an existing table. The columns arrive from a background load.
    pub fn open_alter(
        &mut self,
        connection_id: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) {
        self.reset(Mode::AlterTable, connection_id);
        self.schema = schema.into();
        self.table_name = table.into();
    }

    /// Drop an existing table. No load is needed: dropping does not depend
    /// on the columns.
    pub fn open_drop(
        &mut self,
        connection_id: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) {
        self.reset(Mode::DropTable, connection_id);
        self.schema = schema.into();
        self.table_name = table.into();
    }

    pub fn open_create_database(&mut self, connection_id: impl Into<String>) {
        self.reset(Mode::CreateDatabase, connection_id);
        self.schema = String::new();
    }

    /// The rows as plain columns, for validation and definition building.
    fn plain_columns(&self) -> Vec<ColumnRow> {
        self.columns.iter().map(|r| r.col.clone()).collect()
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

/// Turn a live table's schema into editor rows.
///
/// The type string is mapped back onto a picker choice by
/// `schema_designer::parse_type`, which keeps `VARCHAR(50)`'s 50 and falls
/// back to a custom type carrying the original text. Anything it cannot name
/// round-trips unchanged rather than silently becoming something else.
pub fn rows_from_schema(schema: &TableSchema) -> Vec<EditRow> {
    schema
        .columns
        .iter()
        .map(|c| {
            let (kind, length, precision, scale, custom) =
                crate::screens::schema_designer::parse_type(&c.data_type);
            EditRow::existing(ColumnRow {
                name: c.name.clone(),
                type_kind: kind,
                length,
                precision,
                scale,
                custom,
                nullable: c.nullable && !c.is_primary_key,
                primary_key: c.is_primary_key,
                auto_increment: c.is_auto_increment,
                default: c.default_value.clone().unwrap_or_default(),
            })
        })
        .collect()
}

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
// Drawing
// ---------------------------------------------------------------------------

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    if !app.screens.table_editor.open {
        return;
    }
    let Some(connection_id) = app.screens.table_editor.connection_id.clone() else {
        return;
    };
    let driver_kind = app
        .state
        .lock()
        .unwrap()
        .get_profile(&connection_id)
        .map(|p| p.driver.clone());

    let mode = app.screens.table_editor.mode;

    // Drain finished work before drawing, so this frame shows the newest
    // state rather than last frame's.
    if let Some(result) = app.screens.table_editor.load_job.take_result() {
        let ed = &mut app.screens.table_editor;
        match result {
            Ok(schema) => {
                ed.columns = rows_from_schema(&schema);
                ed.loaded = ed.columns.iter().map(|r| r.col.clone()).collect();
                ed.error = None;
            }
            Err(e) => ed.error = Some(e.to_string()),
        }
    }
    if let Some(result) = app.screens.table_editor.run_job.take_result() {
        match result {
            Ok(()) => {
                app.toasts.success(mode.done_message());
                app.screens.table_editor.open = false;
            }
            Err(e) => app.screens.table_editor.error = Some(e.to_string()),
        }
    }

    // Alter mode needs the live table before it can diff anything.
    if mode == Mode::AlterTable
        && app.screens.table_editor.loaded.is_empty()
        && app.screens.table_editor.error.is_none()
        && !app.screens.table_editor.load_job.is_running()
    {
        start_load(app, ui.ctx());
    }

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, mode.title()));
        if mode != Mode::CreateTable && mode != Mode::CreateDatabase {
            ui.label(crate::theme::text::muted(
                palette,
                qualified_display(
                    &app.screens.table_editor.schema,
                    &app.screens.table_editor.table_name,
                ),
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                app.screens.table_editor.open = false;
            }
        });
    });
    ui.add_space(6.0);

    match mode {
        Mode::CreateTable => create_table_form(app, ui, palette),
        Mode::AlterTable => alter_table_form(app, ui, palette),
        Mode::DropTable => drop_table_form(app, ui, palette),
        Mode::CreateDatabase => create_database_form(app, ui, palette),
    }

    // Recompute the preview, then release the borrow before drawing: the
    // confirm button needs `app` again to launch the job.
    let (errors, preview, error, confirming, running) = {
        let ed = &mut app.screens.table_editor;
        let errors = form_errors(ed);
        if let Some(driver) = driver_kind.as_ref() {
            if errors.is_empty() {
                match build_preview(ed, driver) {
                    Ok(sql) => {
                        ed.preview = sql;
                        ed.error = None;
                    }
                    Err(e) => {
                        ed.preview.clear();
                        ed.error = Some(e.to_string());
                    }
                }
            } else {
                ed.preview.clear();
            }
        }
        (
            errors,
            ed.preview.clone(),
            ed.error.clone(),
            ed.confirming,
            ed.run_job.is_running(),
        )
    };

    ui.add_space(8.0);
    for e in &errors {
        ui.colored_label(palette.destructive, e);
    }
    if let Some(err) = error {
        ui.colored_label(palette.destructive, err);
    }

    if !preview.is_empty() {
        ui.add_space(6.0);
        ui.label(crate::theme::text::strong(palette, "SQL to run"));
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .inner_margin(egui::Margin::same(8.0))
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                for stmt in &preview {
                    ui.label(
                        egui::RichText::new(stmt)
                            .monospace()
                            .color(palette.foreground),
                    );
                }
            });
    } else if mode == Mode::AlterTable && errors.is_empty() {
        ui.add_space(6.0);
        ui.label(crate::theme::text::muted(
            palette,
            "No changes yet. Edit, add or remove a column to see the SQL.",
        ));
    }

    ui.add_space(8.0);
    let mut start = false;
    let mut set_confirming: Option<bool> = None;
    let confirmed_name = drop_confirmed(
        &app.screens.table_editor.table_name,
        &app.screens.table_editor.drop_confirm,
    );

    ui.horizontal(|ui| {
        let mut ready = errors.is_empty() && !preview.is_empty() && !running;
        if mode == Mode::DropTable {
            ready = ready && confirmed_name;
        }
        if confirming {
            ui.label(crate::theme::text::strong(palette, "Run this DDL?"));
            if mode == Mode::DropTable {
                ui.colored_label(palette.destructive, "This cannot be undone.");
            }
            if ui.button(mode.confirm_label()).clicked() {
                set_confirming = Some(false);
                start = true;
            }
            if ui.button("Cancel").clicked() {
                set_confirming = Some(false);
            }
        } else if ui
            .add_enabled(ready, egui::Button::new(mode.action_label()))
            .clicked()
        {
            set_confirming = Some(true);
        }
        if running {
            ui.add(egui::Spinner::new());
        }
    });

    if let Some(v) = set_confirming {
        app.screens.table_editor.confirming = v;
    }
    if start {
        run(app, ui.ctx());
    }
}

/// Display-only qualified name. Never used in SQL, so it is not quoted.
fn qualified_display(schema: &str, table: &str) -> String {
    if schema.trim().is_empty() {
        table.to_string()
    } else {
        format!("{}.{}", schema, table)
    }
}

/// Form-level validation for the active mode.
fn form_errors(ed: &TableEditor) -> Vec<String> {
    match ed.mode {
        Mode::CreateTable => validate(&ed.table_name, &ed.plain_columns()),
        Mode::AlterTable => {
            if ed.loaded.is_empty() {
                // Nothing loaded yet: the diff has nothing to compare
                // against, so complaining about the form would be noise.
                Vec::new()
            } else {
                validate(&ed.table_name, &ed.plain_columns())
            }
        }
        Mode::DropTable => {
            if ed.table_name.trim().is_empty() {
                vec!["Table name is required".into()]
            } else {
                Vec::new()
            }
        }
        Mode::CreateDatabase => {
            match dbhive_core::service::validate_database_name(&ed.database_name) {
                Ok(()) => Vec::new(),
                // While the box is still empty this is a prompt, not a
                // complaint, so it is left to the hint text.
                Err(_) if ed.database_name.trim().is_empty() => Vec::new(),
                Err(e) => vec![e.to_string()],
            }
        }
    }
}

/// The statements the active mode would run.
fn build_preview(ed: &TableEditor, driver: &DbDriver) -> Result<Vec<String>, DbError> {
    match ed.mode {
        Mode::CreateTable => preview_sql(driver, &ed.schema, &ed.table_name, &ed.plain_columns()),
        Mode::AlterTable => preview_alter_sql(
            driver,
            &ed.schema,
            &ed.table_name,
            &ed.loaded,
            &ed.columns,
        ),
        Mode::DropTable => preview_drop_sql(driver, &ed.schema, &ed.table_name, ed.drop_cascade),
        Mode::CreateDatabase => {
            if ed.database_name.trim().is_empty() {
                return Ok(Vec::new());
            }
            // Quoting belongs to the driver, but the preview must work
            // before anything runs, so the dialect's own rule is applied
            // through the same function the execution path uses.
            let quote = quoter(driver);
            dbhive_core::service::create_database_sql(driver, &ed.database_name, &quote)
                .map(|sql| vec![sql])
        }
    }
}

/// The dialect's identifier quoting, matching each driver's
/// `DatabaseDriver::quote_identifier`.
///
/// Used only for the preview, which has no connection to ask. The statement
/// that actually runs is built by `service::create_database` from the live
/// driver.
fn quoter(driver: &DbDriver) -> impl Fn(&str) -> String {
    let driver = driver.clone();
    move |ident: &str| match driver {
        DbDriver::MySql => format!("`{}`", ident.replace('`', "``")),
        DbDriver::SqlServer => format!("[{}]", ident.replace(']', "]]")),
        _ => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
}

fn create_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut app.screens.table_editor.table_name)
                .hint_text("table_name")
                .desired_width(240.0),
        );
        ui.label(crate::theme::text::muted(
            palette,
            format!("in {}", app.screens.table_editor.schema),
        ));
    });
    ui.add_space(8.0);
    columns_editor(app, ui, palette);
}

fn alter_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    if app.screens.table_editor.load_job.is_running() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading the table"));
        });
        return;
    }
    if app.screens.table_editor.loaded.is_empty() {
        ui.label(crate::theme::text::muted(
            palette,
            "The table's columns could not be read, so there is nothing to diff against.",
        ));
        return;
    }

    let mut drop_instead = false;
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(
            palette,
            format!("{} columns loaded", app.screens.table_editor.loaded.len()),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Drop table…")
                .on_hover_text("Delete this table and everything in it")
                .clicked()
            {
                drop_instead = true;
            }
        });
    });
    ui.add_space(6.0);
    columns_editor(app, ui, palette);

    if drop_instead {
        let ed = &app.screens.table_editor;
        let (conn, schema, table) = (
            ed.connection_id.clone().unwrap_or_default(),
            ed.schema.clone(),
            ed.table_name.clone(),
        );
        app.screens.table_editor.open_drop(conn, schema, table);
    }
}

fn drop_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let name = app.screens.table_editor.table_name.clone();
    ui.colored_label(
        palette.destructive,
        format!(
            "Dropping {} deletes the table and every row in it. This cannot be undone.",
            qualified_display(&app.screens.table_editor.schema, &name)
        ),
    );
    ui.add_space(6.0);

    ui.checkbox(
        &mut app.screens.table_editor.drop_cascade,
        "Also drop dependent objects (CASCADE)",
    );
    ui.add_space(6.0);

    ui.label(crate::theme::text::strong(
        palette,
        format!("Type {} to confirm", name),
    ));
    let matched = drop_confirmed(&name, &app.screens.table_editor.drop_confirm);
    ui.add(
        egui::TextEdit::singleline(&mut app.screens.table_editor.drop_confirm)
            .hint_text(name.clone())
            .desired_width(240.0),
    );
    if !matched && !app.screens.table_editor.drop_confirm.is_empty() {
        ui.label(crate::theme::text::muted(
            palette,
            "The name has to match exactly, including case.",
        ));
    }
}

fn create_database_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut app.screens.table_editor.database_name)
                .hint_text("database_name")
                .desired_width(240.0),
        );
    });
    ui.add_space(4.0);
    ui.label(crate::theme::text::muted(
        palette,
        "The new database is created on the server this connection is already on. \
         Switch to it from the database picker once it exists.",
    ));
}

fn columns_editor(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut remove: Option<usize> = None;
    let mut add = false;

    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            let ed = &mut app.screens.table_editor;
            for (i, row) in ed.columns.iter_mut().enumerate() {
                let col = &mut row.col;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut col.name)
                            .hint_text("column")
                            .desired_width(140.0),
                    );

                    egui::ComboBox::from_id_salt(("coltype", i))
                        .selected_text(col.type_kind.label())
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for kind in TypeKind::ALL {
                                ui.selectable_value(&mut col.type_kind, *kind, kind.label());
                            }
                        });

                    if col.type_kind.takes_length() {
                        ui.add(egui::DragValue::new(&mut col.length).range(1..=65535));
                    }
                    if col.type_kind.takes_precision() {
                        ui.add(egui::DragValue::new(&mut col.precision).range(1..=38));
                        ui.add(egui::DragValue::new(&mut col.scale).range(0..=38));
                    }
                    if col.type_kind.takes_custom_name() {
                        ui.add(
                            egui::TextEdit::singleline(&mut col.custom)
                                .hint_text("type name")
                                .desired_width(120.0),
                        );
                    }

                    if ui.checkbox(&mut col.primary_key, "PK").changed() && col.primary_key {
                        // Primary keys are NOT NULL; keep the form honest
                        // rather than generating DDL the server will reject.
                        col.nullable = false;
                    }
                    ui.add_enabled(!col.primary_key, egui::Checkbox::new(&mut col.nullable, "Null"));
                    ui.add_enabled(
                        col.type_kind.can_auto_increment(),
                        egui::Checkbox::new(&mut col.auto_increment, "Auto"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut col.default)
                            .hint_text("default")
                            .desired_width(100.0),
                    );
                    if ui.small_button("✕").on_hover_text("Remove column").clicked() {
                        remove = Some(i);
                    }
                    // An existing column that has been renamed says so, since
                    // the SQL will carry a RENAME the user should expect.
                    if let Some(origin) = row.origin.as_ref() {
                        if origin.name.trim() != row.col.name.trim()
                            && !row.col.name.trim().is_empty()
                        {
                            ui.label(crate::theme::text::muted(
                                palette,
                                format!("was {}", origin.name.trim()),
                            ));
                        }
                    }
                });
            }
        });

    ui.add_space(4.0);
    if ui.button("Add column").clicked() {
        add = true;
    }

    let ed = &mut app.screens.table_editor;
    if let Some(i) = remove {
        ed.columns.remove(i);
    }
    if add {
        ed.columns.push(EditRow::new(ColumnRow::default()));
    }
}

fn start_load(app: &mut DbHiveApp, ctx: &egui::Context) {
    let ed = &app.screens.table_editor;
    let (Some(connection_id), schema, table) = (
        ed.connection_id.clone(),
        ed.schema.clone(),
        ed.table_name.clone(),
    ) else {
        return;
    };
    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.table_editor.error = Some("Connection is no longer open".into());
        return;
    };
    let handle = app.rt.handle().clone();
    app.screens
        .table_editor
        .load_job
        .start(&handle, ctx, async move {
            driver.get_table_schema(&schema, &table).await
        });
}

fn run(app: &mut DbHiveApp, ctx: &egui::Context) {
    let ed = &app.screens.table_editor;
    let Some(connection_id) = ed.connection_id.clone() else {
        return;
    };
    let mode = ed.mode;
    let statements = ed.preview.clone();
    let database_name = ed.database_name.clone();
    let handle = app.rt.handle().clone();

    if mode == Mode::CreateDatabase {
        // Creating a database goes through the service so the statement that
        // runs is the one the core builds from the live driver's quoting,
        // not the preview's reconstruction of it.
        let state = app.state.clone();
        app.screens
            .table_editor
            .run_job
            .start(&handle, ctx, async move {
                dbhive_core::service::create_database(&state, &connection_id, &database_name)
                    .await
                    .map(|_| ())
            });
        return;
    }

    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.table_editor.error = Some("Connection is no longer open".into());
        return;
    };
    app.screens
        .table_editor
        .run_job
        .start(&handle, ctx, async move {
            // Statements run in order and stop at the first failure, so a
            // broken index does not leave later statements applied on top.
            for stmt in statements {
                driver.execute_query(&stmt).await?;
            }
            Ok(())
        });
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

    // ── modes ────────────────────────────────────────────────────────────

    #[test]
    fn opening_a_mode_clears_the_previous_one() {
        let mut ed = TableEditor::default();
        ed.open_new("conn", "public");
        ed.preview = vec!["CREATE TABLE ...".into()];
        ed.confirming = true;

        ed.open_drop("conn", "public", "users");
        assert_eq!(ed.mode, Mode::DropTable);
        assert!(ed.preview.is_empty(), "a stale preview survived");
        assert!(!ed.confirming, "a stale confirmation survived");
        assert_eq!(ed.table_name, "users");

        // A drop confirmation must never carry over to the next table.
        ed.drop_confirm = "users".into();
        ed.open_drop("conn", "public", "orders");
        assert!(!drop_confirmed(&ed.table_name, &ed.drop_confirm));
    }

    #[test]
    fn create_table_starts_with_an_id_column() {
        let mut ed = TableEditor::default();
        ed.open_new("conn", "public");
        assert_eq!(ed.mode, Mode::CreateTable);
        assert_eq!(ed.columns.len(), 1);
        assert_eq!(ed.columns[0].col.name, "id");
        // It is new, not loaded: it must diff as an addition, never a change.
        assert!(ed.columns[0].origin.is_none());
    }

    #[test]
    fn alter_starts_empty_and_waits_for_the_load() {
        let mut ed = TableEditor::default();
        ed.open_alter("conn", "public", "users");
        assert_eq!(ed.mode, Mode::AlterTable);
        assert!(ed.columns.is_empty());
        assert!(ed.loaded.is_empty());
        // With nothing loaded there is nothing to complain about yet.
        assert!(form_errors(&ed).is_empty());
    }

    #[test]
    fn a_database_name_is_validated_by_the_core() {
        let mut ed = TableEditor::default();
        ed.open_create_database("conn");
        // Empty is a prompt, not an error.
        assert!(form_errors(&ed).is_empty());
        ed.database_name = "1bad".into();
        assert!(!form_errors(&ed).is_empty());
        ed.database_name = "analytics".into();
        assert!(form_errors(&ed).is_empty());
        assert_eq!(
            build_preview(&ed, &DbDriver::Postgres).unwrap(),
            vec!["CREATE DATABASE \"analytics\"".to_string()]
        );
    }

    #[test]
    fn create_database_is_refused_where_it_makes_no_sense() {
        let mut ed = TableEditor::default();
        ed.open_create_database("conn");
        ed.database_name = "analytics".into();
        assert!(build_preview(&ed, &DbDriver::Sqlite).is_err());
        assert!(build_preview(&ed, &DbDriver::MongoDb).is_err());
    }
}
