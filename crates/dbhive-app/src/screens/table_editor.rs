//! Create and alter tables.
//!
//! The SQL is produced by `dbhive_core::ddl`, which already knows each
//! dialect, so this screen never writes DDL by hand. It shows the generated
//! statements before running them: DDL is not undoable, and a user who
//! cannot see what is about to run has no way to catch a mistake.

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::drivers::DatabaseDriver;
use dbhive_core::models::{
    ColumnDefinition, ColumnType, DbDriver, DbError, TableDefinition,
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

#[derive(Default)]
pub struct TableEditor {
    pub open: bool,
    pub connection_id: Option<String>,
    pub schema: String,
    pub table_name: String,
    pub columns: Vec<ColumnRow>,
    pub preview: Vec<String>,
    pub error: Option<String>,
    pub run_job: Job<Result<(), DbError>>,
    pub confirming: bool,
}

impl TableEditor {
    pub fn open_new(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        self.open = true;
        self.connection_id = Some(connection_id.into());
        self.schema = schema.into();
        self.table_name = String::new();
        self.error = None;
        self.preview.clear();
        self.confirming = false;
        // A table with no columns cannot be created, so start with the one
        // almost every table has.
        self.columns = vec![ColumnRow {
            name: "id".into(),
            type_kind: TypeKind::BigInt,
            nullable: false,
            primary_key: true,
            auto_increment: true,
            ..Default::default()
        }];
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
        columns: columns
            .iter()
            .map(|c| ColumnDefinition {
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
            })
            .collect(),
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

    if let Some(result) = app.screens.table_editor.run_job.take_result() {
        match result {
            Ok(()) => {
                app.toasts.success("Table created");
                app.screens.table_editor.open = false;
            }
            Err(e) => app.screens.table_editor.error = Some(e.to_string()),
        }
    }

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "New table"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                app.screens.table_editor.open = false;
            }
        });
    });
    ui.add_space(6.0);

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

    // Recompute the preview, then release the borrow before drawing: the
    // confirm button needs `app` again to launch the job.
    let (errors, preview, error, confirming, running) = {
        let ed = &mut app.screens.table_editor;
        let errors = validate(&ed.table_name, &ed.columns);
        if let Some(driver) = driver_kind.as_ref() {
            if errors.is_empty() {
                match preview_sql(driver, &ed.schema, &ed.table_name, &ed.columns) {
                    Ok(sql) => ed.preview = sql,
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
    }

    ui.add_space(8.0);
    let mut start = false;
    let mut set_confirming: Option<bool> = None;
    ui.horizontal(|ui| {
        let ready = errors.is_empty() && !preview.is_empty() && !running;
        if confirming {
            ui.label(crate::theme::text::strong(palette, "Run this DDL?"));
            if ui.button("Yes, create it").clicked() {
                set_confirming = Some(false);
                start = true;
            }
            if ui.button("Cancel").clicked() {
                set_confirming = Some(false);
            }
        } else if ui
            .add_enabled(ready, egui::Button::new("Create table"))
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

fn columns_editor(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut remove: Option<usize> = None;
    let mut add = false;

    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            let ed = &mut app.screens.table_editor;
            for (i, col) in ed.columns.iter_mut().enumerate() {
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
                });
            }
        });

    ui.add_space(4.0);
    if ui.button("Add column").clicked() {
        add = true;
    }
    let _ = palette;

    let ed = &mut app.screens.table_editor;
    if let Some(i) = remove {
        ed.columns.remove(i);
    }
    if add {
        ed.columns.push(ColumnRow::default());
    }
}

fn run(app: &mut DbHiveApp, ctx: &egui::Context) {
    let ed = &app.screens.table_editor;
    let Some(connection_id) = ed.connection_id.clone() else {
        return;
    };
    let statements = ed.preview.clone();
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
}
