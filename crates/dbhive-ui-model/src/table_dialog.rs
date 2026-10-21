//! The create-table wizard and the edit-table dialog.
//!
//! `table_editor` has the column row, the type list, validation and the diff
//! that turns edited rows into `ALTER` operations. This file is the state a
//! dialog drives on top of it, with the two things that module never had:
//!
//! * a four-step create wizard that also carries unique constraints and
//!   foreign keys, and
//! * an edit dialog with soft-delete and restore, a live count of pending
//!   operations, and the data-safety rule that a primary key column can never
//!   be dropped.
//!
//! Every edit is `apply(action, i, j, text)` and everything drawn comes back
//! from `view`, so the whole flow runs in a test with no window. SQL comes
//! only from `dbhive_core::ddl`, which escapes identifiers; nothing here
//! writes DDL by hand.

use std::collections::HashMap;

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::{
    AlterTableDefinition, DbDriver, ForeignKeyAction, ForeignKeyConstraint, TableSchema,
    UniqueConstraint,
};

use crate::schema_designer::{fk_action_label, parse_type, FK_ACTIONS};
use crate::table_editor::{
    build_definition, diff_columns, qualified_display, validate as validate_columns, ColumnRow,
    EditRow, TypeKind,
};

pub use crate::builder_editor::Change;

// ---------------------------------------------------------------------------
// Type presets
// ---------------------------------------------------------------------------

/// The nine types the wizard offers, as the React dialog did. Anything more
/// exotic is a job for the SQL editor.
pub const PRESETS: [TypeKind; 9] = [
    TypeKind::Integer,
    TypeKind::BigInt,
    TypeKind::Varchar,
    TypeKind::Text,
    TypeKind::Boolean,
    TypeKind::Date,
    TypeKind::Timestamp,
    TypeKind::Decimal,
    TypeKind::Json,
];

pub fn preset_labels() -> Vec<String> {
    PRESETS
        .iter()
        .map(|k| match k {
            TypeKind::Varchar => "VARCHAR".to_string(),
            TypeKind::Decimal => "DECIMAL".to_string(),
            other => other.label().to_string(),
        })
        .collect()
}

fn preset_index(kind: TypeKind) -> i32 {
    PRESETS.iter().position(|k| *k == kind).map_or(-1, |i| i as i32)
}

/// A fresh row of this preset, with the usual defaults for its parameters.
fn column_of(kind: TypeKind) -> ColumnRow {
    ColumnRow {
        type_kind: kind,
        length: 255,
        precision: 10,
        scale: 2,
        ..ColumnRow::default()
    }
}

/// What the small parameter box holds for a type, if it has one.
fn param_text(c: &ColumnRow) -> String {
    match c.type_kind {
        TypeKind::Varchar | TypeKind::Char => c.length.to_string(),
        TypeKind::Decimal => format!("{},{}", c.precision, c.scale),
        _ => String::new(),
    }
}

fn param_hint(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Varchar | TypeKind::Char => "Length",
        TypeKind::Decimal => "Precision,scale",
        _ => "",
    }
}

/// Apply typed parameter text to a row. Returns what is wrong with it, if
/// anything; the row keeps its previous value in that case.
fn apply_param(c: &mut ColumnRow, text: &str) -> Result<(), String> {
    let t = text.trim();
    match c.type_kind {
        TypeKind::Varchar | TypeKind::Char => match t.parse::<u32>() {
            Ok(n) if n > 0 => {
                c.length = n;
                Ok(())
            }
            _ => Err("length must be a whole number above zero".to_string()),
        },
        TypeKind::Decimal => {
            let mut parts = t.split(',').map(|p| p.trim());
            let p = parts.next().and_then(|p| p.parse::<u8>().ok());
            let s = match parts.next() {
                None => Some(0),
                Some(s) => s.parse::<u8>().ok(),
            };
            match (p, s, parts.next()) {
                (Some(p), Some(s), None) if p > 0 && p <= 38 && s <= p => {
                    c.precision = p;
                    c.scale = s;
                    Ok(())
                }
                _ => Err("use precision,scale such as 10,2 (scale cannot exceed precision)".to_string()),
            }
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A toggle chip: a column name and whether it is picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipView {
    pub label: String,
    pub on: bool,
}

pub fn action_labels() -> Vec<String> {
    FK_ACTIONS.iter().map(|a| fk_action_label(a).to_string()).collect()
}

fn action_index(a: &ForeignKeyAction) -> i32 {
    FK_ACTIONS.iter().position(|x| x == a).unwrap_or(0) as i32
}

/// What the dialog knows about a table's columns for a foreign key target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefColumns {
    Loading,
    Loaded(Vec<String>),
    Failed(String),
}

// ---------------------------------------------------------------------------
// Create wizard
// ---------------------------------------------------------------------------

pub const CREATE_STEPS: [&str; 4] = ["Basics", "Columns", "Constraints", "Preview"];

#[derive(Debug, Clone)]
pub struct WizardColumn {
    pub row: ColumnRow,
    /// The parameter box as typed, so a half-typed value is not rewritten
    /// under the caret.
    pub param: String,
    pub param_error: String,
}

impl WizardColumn {
    fn new(kind: TypeKind) -> Self {
        let row = column_of(kind);
        Self {
            param: param_text(&row),
            row,
            param_error: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UniqueDraft {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FkDraft {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}

impl Default for FkDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            columns: Vec::new(),
            ref_table: String::new(),
            ref_columns: Vec::new(),
            on_delete: ForeignKeyAction::NoAction,
            on_update: ForeignKeyAction::NoAction,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateWizard {
    pub step: usize,
    /// What generated SQL puts in front of the table; empty on SQLite.
    pub schema: String,
    pub name: String,
    pub description: String,
    pub if_not_exists: bool,
    pub columns: Vec<WizardColumn>,
    pub uniques: Vec<UniqueDraft>,
    pub foreign_keys: Vec<FkDraft>,
    /// Columns of candidate foreign key targets, filled in by the shell as
    /// tables are picked.
    pub ref_columns: HashMap<String, RefColumns>,
}

impl CreateWizard {
    /// A new wizard that starts with the column almost every table has.
    pub fn new(schema: &str) -> Self {
        let mut id = WizardColumn::new(TypeKind::BigInt);
        id.row.name = "id".into();
        id.row.primary_key = true;
        id.row.nullable = false;
        id.row.auto_increment = true;
        Self {
            step: 0,
            schema: schema.to_string(),
            name: String::new(),
            description: String::new(),
            if_not_exists: false,
            columns: vec![id],
            uniques: Vec::new(),
            foreign_keys: Vec::new(),
            ref_columns: HashMap::new(),
        }
    }

    fn rows(&self) -> Vec<ColumnRow> {
        self.columns.iter().map(|c| c.row.clone()).collect()
    }

    /// Named columns, in order: what a constraint can pick from.
    fn column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|c| c.row.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect()
    }

    /// Foreign key targets whose columns have not been asked for yet.
    pub fn missing_ref_tables(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for fk in &self.foreign_keys {
            if !fk.ref_table.is_empty()
                && !self.ref_columns.contains_key(&fk.ref_table)
                && !out.contains(&fk.ref_table)
            {
                out.push(fk.ref_table.clone());
            }
        }
        out
    }

    // -- Validation, per step ---------------------------------------------------

    pub fn step_errors(&self, step: usize) -> Vec<String> {
        match step {
            0 => {
                let mut e = Vec::new();
                if self.name.trim().is_empty() {
                    e.push("Table name is required".to_string());
                }
                e
            }
            1 => {
                let mut e: Vec<String> = validate_columns("x", &self.rows())
                    .into_iter()
                    // The name belongs to step one; the stand-in above only
                    // satisfies the shared validator.
                    .filter(|m| !m.starts_with("Table name"))
                    .collect();
                for c in &self.columns {
                    if !c.param_error.is_empty() {
                        e.push(format!("Column {}: {}", c.row.name.trim(), c.param_error));
                    }
                }
                e
            }
            2 => {
                let names = self.column_names();
                let mut e = Vec::new();
                for (i, u) in self.uniques.iter().enumerate() {
                    if u.columns.is_empty() {
                        e.push(format!("Unique constraint {} needs at least one column", i + 1));
                    }
                }
                for (i, fk) in self.foreign_keys.iter().enumerate() {
                    let n = i + 1;
                    if fk.columns.is_empty() {
                        e.push(format!("Foreign key {n} needs at least one column"));
                    }
                    if fk.ref_table.is_empty() {
                        e.push(format!("Foreign key {n} needs a table to reference"));
                        continue;
                    }
                    match self.ref_columns.get(&fk.ref_table) {
                        Some(RefColumns::Loading) | None => {
                            e.push(format!("Foreign key {n}: still loading {}", fk.ref_table))
                        }
                        Some(RefColumns::Failed(m)) => e.push(format!(
                            "Foreign key {n}: could not read {}: {m}",
                            fk.ref_table
                        )),
                        Some(RefColumns::Loaded(_)) => {
                            if fk.ref_columns.is_empty() {
                                e.push(format!("Foreign key {n} needs a column to reference"));
                            } else if fk.ref_columns.len() != fk.columns.len() {
                                e.push(format!(
                                    "Foreign key {n}: {} column(s) cannot reference {}",
                                    fk.columns.len(),
                                    fk.ref_columns.len()
                                ));
                            }
                        }
                    }
                    if fk.columns.iter().any(|c| !names.contains(c)) {
                        e.push(format!("Foreign key {n} uses a column that no longer exists"));
                    }
                }
                e
            }
            _ => Vec::new(),
        }
    }

    /// Every error from the steps that feed the preview.
    pub fn all_errors(&self) -> Vec<String> {
        (0..3).flat_map(|s| self.step_errors(s)).collect()
    }

    pub fn can_advance(&self) -> bool {
        self.step < CREATE_STEPS.len() - 1 && self.step_errors(self.step).is_empty()
    }

    // -- SQL ---------------------------------------------------------------------

    /// The `CREATE TABLE` statements, or why they cannot be built yet.
    pub fn statements(&self, driver: &DbDriver) -> Result<Vec<String>, String> {
        let errors = self.all_errors();
        if let Some(first) = errors.first() {
            return Err(first.clone());
        }
        let generator = get_ddl_generator(driver).map_err(|e| e.to_string())?;
        let mut def = build_definition(&self.schema, &self.name, &self.rows());
        def.if_not_exists = self.if_not_exists;
        let d = self.description.trim();
        def.comment = (!d.is_empty()).then(|| d.to_string());
        def.unique_constraints = self
            .uniques
            .iter()
            .map(|u| UniqueConstraint {
                name: (!u.name.trim().is_empty()).then(|| u.name.trim().to_string()),
                columns: u.columns.clone(),
            })
            .collect();
        def.foreign_keys = self
            .foreign_keys
            .iter()
            .map(|f| ForeignKeyConstraint {
                name: (!f.name.trim().is_empty()).then(|| f.name.trim().to_string()),
                columns: f.columns.clone(),
                referenced_table: f.ref_table.clone(),
                referenced_columns: f.ref_columns.clone(),
                on_delete: f.on_delete.clone(),
                on_update: f.on_update.clone(),
            })
            .collect();
        generator
            .generate_create_table(&def)
            .map(|r| r.sql)
            .map_err(|e| e.to_string())
    }

    // -- Actions -------------------------------------------------------------------

    fn rename_in_constraints(&mut self, old: &str, new: &str) {
        if old.is_empty() || old == new {
            return;
        }
        let swap = |list: &mut Vec<String>| {
            for c in list.iter_mut() {
                if c == old {
                    *c = new.to_string();
                }
            }
        };
        for u in &mut self.uniques {
            swap(&mut u.columns);
        }
        for f in &mut self.foreign_keys {
            swap(&mut f.columns);
        }
    }

    fn forget_in_constraints(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        for u in &mut self.uniques {
            u.columns.retain(|c| c != name);
        }
        for f in &mut self.foreign_keys {
            f.columns.retain(|c| c != name);
        }
    }

    /// `tables` are the names a foreign key may point at.
    pub fn apply(&mut self, action: &str, i: i32, j: i32, text: &str, tables: &[String]) -> Change {
        use Change::{None as Nothing, Structure, Text};
        let idx = usize::try_from(i).ok();
        let col_idx = idx.filter(|k| *k < self.columns.len());
        let names = self.column_names();

        match action {
            "next" => {
                if self.can_advance() {
                    self.step += 1;
                    Structure
                } else {
                    Nothing
                }
            }
            "back" => {
                if self.step > 0 {
                    self.step -= 1;
                    Structure
                } else {
                    Nothing
                }
            }
            "step" => match idx.filter(|k| *k < CREATE_STEPS.len()) {
                // Going back is always allowed; going forward needs every
                // step in between to be valid.
                Some(k) if k <= self.step => {
                    self.step = k;
                    Structure
                }
                Some(k) if (self.step..k).all(|s| self.step_errors(s).is_empty()) => {
                    self.step = k;
                    Structure
                }
                _ => Nothing,
            },

            "name" => {
                self.name = text.to_string();
                Text
            }
            "desc" => {
                self.description = text.to_string();
                Text
            }
            "ine" => {
                self.if_not_exists = !self.if_not_exists;
                Structure
            }

            "col-add" => {
                self.columns.push(WizardColumn::new(TypeKind::Integer));
                Structure
            }
            "col-remove" => match col_idx {
                Some(k) => {
                    let gone = self.columns.remove(k).row.name.trim().to_string();
                    // A name another column still uses stays in the constraints.
                    if !self.column_names().contains(&gone) {
                        self.forget_in_constraints(&gone);
                    }
                    Structure
                }
                None => Nothing,
            },
            "col-name" => match col_idx {
                Some(k) => {
                    let old = self.columns[k].row.name.trim().to_string();
                    self.columns[k].row.name = text.to_string();
                    self.rename_in_constraints(&old, text.trim());
                    Text
                }
                None => Nothing,
            },
            "col-type" => {
                let Some(kind) = usize::try_from(j).ok().and_then(|p| PRESETS.get(p)).copied() else {
                    return Nothing;
                };
                match col_idx {
                    Some(k) => {
                        let c = &mut self.columns[k];
                        c.row.type_kind = kind;
                        c.row.length = 255;
                        c.row.precision = 10;
                        c.row.scale = 2;
                        c.param = param_text(&c.row);
                        c.param_error.clear();
                        if !kind.can_auto_increment() {
                            c.row.auto_increment = false;
                        }
                        Structure
                    }
                    None => Nothing,
                }
            }
            "col-param" => match col_idx {
                Some(k) => {
                    let c = &mut self.columns[k];
                    c.param = text.to_string();
                    c.param_error = apply_param(&mut c.row, text).err().unwrap_or_default();
                    Text
                }
                None => Nothing,
            },
            "col-pk" => match col_idx {
                Some(k) => {
                    let r = &mut self.columns[k].row;
                    r.primary_key = !r.primary_key;
                    // A primary key column is NOT NULL everywhere.
                    if r.primary_key {
                        r.nullable = false;
                    }
                    Structure
                }
                None => Nothing,
            },
            "col-auto" => match col_idx {
                Some(k) => {
                    let r = &mut self.columns[k].row;
                    if !r.auto_increment && !r.type_kind.can_auto_increment() {
                        return Nothing;
                    }
                    r.auto_increment = !r.auto_increment;
                    Structure
                }
                None => Nothing,
            },
            "col-null" => match col_idx {
                Some(k) => {
                    let r = &mut self.columns[k].row;
                    if r.primary_key {
                        return Nothing;
                    }
                    r.nullable = !r.nullable;
                    Structure
                }
                None => Nothing,
            },

            "uq-add" => {
                self.uniques.push(UniqueDraft::default());
                Structure
            }
            "uq-remove" => match idx.filter(|k| *k < self.uniques.len()) {
                Some(k) => {
                    self.uniques.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "uq-name" => match idx.and_then(|k| self.uniques.get_mut(k)) {
                Some(u) => {
                    u.name = text.to_string();
                    Text
                }
                None => Nothing,
            },
            "uq-col" => {
                let Some(col) = usize::try_from(j).ok().and_then(|p| names.get(p)).cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.uniques.get_mut(k)) {
                    Some(u) => {
                        toggle(&mut u.columns, &col);
                        Structure
                    }
                    None => Nothing,
                }
            }

            "fk-add" => {
                self.foreign_keys.push(FkDraft::default());
                Structure
            }
            "fk-remove" => match idx.filter(|k| *k < self.foreign_keys.len()) {
                Some(k) => {
                    self.foreign_keys.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "fk-name" => match idx.and_then(|k| self.foreign_keys.get_mut(k)) {
                Some(f) => {
                    f.name = text.to_string();
                    Text
                }
                None => Nothing,
            },
            "fk-col" => {
                let Some(col) = usize::try_from(j).ok().and_then(|p| names.get(p)).cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.foreign_keys.get_mut(k)) {
                    Some(f) => {
                        toggle(&mut f.columns, &col);
                        Structure
                    }
                    None => Nothing,
                }
            }
            "fk-table" => {
                let Some(t) = usize::try_from(j).ok().and_then(|p| tables.get(p)).cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.foreign_keys.get_mut(k)) {
                    Some(f) => {
                        if f.ref_table != t {
                            f.ref_table = t;
                            f.ref_columns.clear();
                        }
                        Structure
                    }
                    None => Nothing,
                }
            }
            "fk-rcol" => {
                let Some(f) = idx.and_then(|k| self.foreign_keys.get(k)) else {
                    return Nothing;
                };
                let Some(RefColumns::Loaded(list)) = self.ref_columns.get(&f.ref_table) else {
                    return Nothing;
                };
                let Some(col) = usize::try_from(j).ok().and_then(|p| list.get(p)).cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.foreign_keys.get_mut(k)) {
                    Some(f) => {
                        toggle(&mut f.ref_columns, &col);
                        Structure
                    }
                    None => Nothing,
                }
            }
            "fk-del" | "fk-upd" => {
                let Some(a) = usize::try_from(j).ok().and_then(|p| FK_ACTIONS.get(p)).cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.foreign_keys.get_mut(k)) {
                    Some(f) => {
                        if action == "fk-del" {
                            f.on_delete = a;
                        } else {
                            f.on_update = a;
                        }
                        Structure
                    }
                    None => Nothing,
                }
            }
            _ => Nothing,
        }
    }
}

fn toggle(list: &mut Vec<String>, name: &str) {
    if let Some(p) = list.iter().position(|c| c == name) {
        list.remove(p);
    } else {
        list.push(name.to_string());
    }
}

// ---------------------------------------------------------------------------
// Edit dialog
// ---------------------------------------------------------------------------

pub const EDIT_STEPS: [&str; 2] = ["Columns", "Preview"];

#[derive(Debug, Clone)]
pub struct AlterRow {
    pub row: EditRow,
    /// Marked for dropping; still listed so it can be restored.
    pub removed: bool,
    /// The type as the database reported it. New columns show their preset.
    pub type_label: String,
}

impl AlterRow {
    fn is_new(&self) -> bool {
        self.row.origin.is_none()
    }

    /// A primary key column of the live table. Never droppable.
    fn is_primary_key(&self) -> bool {
        self.row.origin.as_ref().is_some_and(|o| o.primary_key)
    }
}

#[derive(Debug, Clone)]
pub struct AlterEditor {
    pub step: usize,
    pub schema: String,
    pub table: String,
    pub loaded: Vec<ColumnRow>,
    pub rows: Vec<AlterRow>,
}

/// Turn a live table's schema into editor rows. A type the wizard cannot name
/// round-trips as a custom type carrying the database's own text, so loading
/// and saving an untouched row never rewrites its type.
pub fn rows_from_schema(schema: &TableSchema) -> Vec<(EditRow, String)> {
    schema
        .columns
        .iter()
        .map(|c| {
            let (kind, length, precision, scale, custom) = parse_type(&c.data_type);
            let row = ColumnRow {
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
            };
            (EditRow::existing(row), c.data_type.clone())
        })
        .collect()
}

impl AlterEditor {
    pub fn from_schema(schema_name: &str, table: &str, schema: &TableSchema) -> Self {
        let rows: Vec<(EditRow, String)> = rows_from_schema(schema);
        Self {
            step: 0,
            schema: schema_name.to_string(),
            table: table.to_string(),
            loaded: rows.iter().map(|(r, _)| r.col.clone()).collect(),
            rows: rows
                .into_iter()
                .map(|(row, type_label)| AlterRow {
                    row,
                    removed: false,
                    type_label,
                })
                .collect(),
        }
    }

    /// The rows that survive, in the form the diff wants.
    fn effective(&self) -> Vec<EditRow> {
        self.rows.iter().filter(|r| !r.removed).map(|r| r.row.clone()).collect()
    }

    /// Primary key columns of the live table that the edit would drop.
    pub fn dropped_primary_keys(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.removed && r.is_primary_key())
            .filter_map(|r| r.row.origin.as_ref().map(|o| o.name.trim().to_string()))
            .collect()
    }

    pub fn errors(&self) -> Vec<String> {
        let mut e = Vec::new();
        for pk in self.dropped_primary_keys() {
            e.push(format!(
                "{pk} is part of the primary key and cannot be dropped"
            ));
        }
        let rows: Vec<ColumnRow> = self.effective().into_iter().map(|r| r.col).collect();
        e.extend(
            validate_columns(&self.table, &rows)
                .into_iter()
                .filter(|m| !m.starts_with("Table name")),
        );
        e
    }

    /// How many `ALTER` operations the edit would run.
    pub fn pending_count(&self) -> usize {
        diff_columns(&self.loaded, &self.effective()).len()
    }

    /// The `ALTER TABLE` statements. Refuses a primary key drop and any
    /// invalid column before asking the generator, so unsafe SQL is never
    /// built, let alone shown.
    pub fn statements(&self, driver: &DbDriver) -> Result<Vec<String>, String> {
        if let Some(first) = self.errors().first() {
            return Err(first.clone());
        }
        let operations = diff_columns(&self.loaded, &self.effective());
        if operations.is_empty() {
            return Ok(Vec::new());
        }
        let generator = get_ddl_generator(driver).map_err(|e| e.to_string())?;
        let alter = AlterTableDefinition {
            schema: (!self.schema.is_empty()).then(|| self.schema.clone()),
            name: self.table.trim().to_string(),
            operations,
        };
        generator
            .generate_alter_table(&alter)
            .map(|r| r.sql)
            .map_err(|e| e.to_string())
    }

    pub fn apply(&mut self, action: &str, i: i32, j: i32, text: &str) -> Change {
        use Change::{None as Nothing, Structure, Text};
        let idx = usize::try_from(i).ok().filter(|k| *k < self.rows.len());

        match action {
            "next" => {
                if self.step == 0 {
                    self.step = 1;
                    Structure
                } else {
                    Nothing
                }
            }
            "back" => {
                if self.step == 1 {
                    self.step = 0;
                    Structure
                } else {
                    Nothing
                }
            }
            "step" => match usize::try_from(i).ok().filter(|k| *k < EDIT_STEPS.len()) {
                Some(k) if k != self.step => {
                    self.step = k;
                    Structure
                }
                _ => Nothing,
            },
            "col-add" => {
                let kind = TypeKind::Integer;
                self.rows.push(AlterRow {
                    row: EditRow::new(column_of(kind)),
                    removed: false,
                    type_label: kind.label().to_string(),
                });
                Structure
            }
            // A column that was never in the table is simply discarded; one
            // that is gets marked, so it can be restored.
            "col-remove" => match idx {
                Some(k) => {
                    if self.rows[k].is_new() {
                        self.rows.remove(k);
                        Structure
                    } else if self.rows[k].is_primary_key() {
                        Nothing
                    } else {
                        self.rows[k].removed = true;
                        Structure
                    }
                }
                None => Nothing,
            },
            "col-restore" => match idx {
                Some(k) if self.rows[k].removed => {
                    self.rows[k].removed = false;
                    Structure
                }
                _ => Nothing,
            },
            "col-name" => match idx {
                Some(k) if !self.rows[k].removed => {
                    self.rows[k].row.col.name = text.to_string();
                    Text
                }
                _ => Nothing,
            },
            "col-null" => match idx {
                Some(k) if !self.rows[k].removed => {
                    let c = &mut self.rows[k].row.col;
                    if c.primary_key {
                        return Nothing;
                    }
                    c.nullable = !c.nullable;
                    Structure
                }
                _ => Nothing,
            },
            // Only a column that is not yet in the table can pick its type;
            // changing a live column's type is out of scope here.
            "col-type" => {
                let Some(kind) = usize::try_from(j).ok().and_then(|p| PRESETS.get(p)).copied() else {
                    return Nothing;
                };
                match idx {
                    Some(k) if self.rows[k].is_new() => {
                        let keep = self.rows[k].row.col.clone();
                        let mut c = column_of(kind);
                        c.name = keep.name;
                        c.nullable = keep.nullable;
                        self.rows[k].row.col = c;
                        self.rows[k].type_label = kind.label().to_string();
                        Structure
                    }
                    _ => Nothing,
                }
            }
            _ => Nothing,
        }
    }
}

// ---------------------------------------------------------------------------
// Running the statements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementStatus {
    Done,
    Failed(String),
    /// Not run because an earlier statement failed.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementReport {
    pub sql: String,
    pub status: StatementStatus,
}

/// Pair statements with how each one went. `results` holds one entry per
/// statement that was actually run, in order; the rest are skipped.
pub fn report(statements: &[String], results: &[Result<(), String>]) -> Vec<StatementReport> {
    statements
        .iter()
        .enumerate()
        .map(|(i, sql)| StatementReport {
            sql: sql.clone(),
            status: match results.get(i) {
                Some(Ok(())) => StatementStatus::Done,
                Some(Err(e)) => StatementStatus::Failed(e.clone()),
                None => StatementStatus::Skipped,
            },
        })
        .collect()
}

pub fn report_ok(report: &[StatementReport]) -> bool {
    !report.is_empty() && report.iter().all(|r| r.status == StatementStatus::Done)
}

pub fn report_summary(report: &[StatementReport]) -> String {
    let done = report.iter().filter(|r| r.status == StatementStatus::Done).count();
    let failed = report
        .iter()
        .filter(|r| matches!(r.status, StatementStatus::Failed(_)))
        .count();
    let skipped = report.len() - done - failed;
    if failed == 0 && skipped == 0 {
        format!("{done} of {} statements ran", report.len())
    } else {
        format!(
            "{done} of {} statements ran; {failed} failed, {skipped} skipped",
            report.len()
        )
    }
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardColumnView {
    pub name: String,
    pub preset_index: i32,
    pub preset_label: String,
    pub show_param: bool,
    pub param: String,
    pub param_hint: String,
    pub param_error: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub auto_enabled: bool,
    pub nullable: bool,
    pub nullable_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueView {
    pub name: String,
    pub chips: Vec<ChipView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkView {
    pub name: String,
    pub chips: Vec<ChipView>,
    pub ref_table: String,
    pub ref_table_index: i32,
    pub ref_chips: Vec<ChipView>,
    /// "Loading columns..." or the load error, else empty.
    pub ref_note: String,
    pub on_delete_index: i32,
    pub on_delete: String,
    pub on_update_index: i32,
    pub on_update: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRowView {
    pub name: String,
    pub type_label: String,
    pub nullable: bool,
    pub nullable_enabled: bool,
    pub primary_key: bool,
    pub is_new: bool,
    pub removed: bool,
    /// "renamed", "modified", "new", "will drop".
    pub badges: Vec<String>,
    pub can_remove: bool,
    /// Why the remove button is off, for a primary key.
    pub remove_note: String,
    pub name_enabled: bool,
    pub type_pickable: bool,
    pub preset_index: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogView {
    /// 0 create, 1 edit.
    pub mode: i32,
    pub title: String,
    pub subtitle: String,
    pub steps: Vec<String>,
    pub step: i32,
    /// Errors for the step on screen.
    pub step_errors: Vec<String>,
    pub can_next: bool,
    pub name: String,
    pub description: String,
    pub if_not_exists: bool,
    pub columns: Vec<WizardColumnView>,
    pub preset_labels: Vec<String>,
    pub uniques: Vec<UniqueView>,
    pub foreign_keys: Vec<FkView>,
    pub fk_tables: Vec<String>,
    pub action_labels: Vec<String>,
    pub edit_rows: Vec<EditRowView>,
    pub pending: usize,
    /// The statements, one per line, blank when they cannot be built.
    pub sql: String,
    /// Why they cannot be built, when that is the reason.
    pub sql_error: String,
    pub statement_count: usize,
}

fn chips(all: &[String], on: &[String]) -> Vec<ChipView> {
    all.iter()
        .map(|n| ChipView {
            label: n.clone(),
            on: on.contains(n),
        })
        .collect()
}

impl CreateWizard {
    pub fn view(&self, driver: &DbDriver, tables: &[String]) -> DialogView {
        let names = self.column_names();
        let columns = self
            .columns
            .iter()
            .map(|c| WizardColumnView {
                name: c.row.name.clone(),
                preset_index: preset_index(c.row.type_kind),
                preset_label: preset_labels()
                    .get(preset_index(c.row.type_kind).max(0) as usize)
                    .cloned()
                    .unwrap_or_default(),
                show_param: matches!(c.row.type_kind, TypeKind::Varchar | TypeKind::Decimal),
                param: c.param.clone(),
                param_hint: param_hint(c.row.type_kind).to_string(),
                param_error: !c.param_error.is_empty(),
                primary_key: c.row.primary_key,
                auto_increment: c.row.auto_increment,
                auto_enabled: c.row.type_kind.can_auto_increment(),
                nullable: c.row.nullable && !c.row.primary_key,
                nullable_enabled: !c.row.primary_key,
            })
            .collect();

        let uniques = self
            .uniques
            .iter()
            .map(|u| UniqueView {
                name: u.name.clone(),
                chips: chips(&names, &u.columns),
            })
            .collect();

        let foreign_keys = self
            .foreign_keys
            .iter()
            .map(|f| {
                let (ref_chips, ref_note) = match self.ref_columns.get(&f.ref_table) {
                    _ if f.ref_table.is_empty() => (Vec::new(), String::new()),
                    Some(RefColumns::Loaded(list)) => (chips(list, &f.ref_columns), String::new()),
                    Some(RefColumns::Failed(m)) => (Vec::new(), format!("Could not read columns: {m}")),
                    _ => (Vec::new(), "Loading columns...".to_string()),
                };
                FkView {
                    name: f.name.clone(),
                    chips: chips(&names, &f.columns),
                    ref_table: if f.ref_table.is_empty() {
                        "Choose a table".to_string()
                    } else {
                        f.ref_table.clone()
                    },
                    ref_table_index: tables.iter().position(|t| *t == f.ref_table).map_or(-1, |p| p as i32),
                    ref_chips,
                    ref_note,
                    on_delete_index: action_index(&f.on_delete),
                    on_delete: fk_action_label(&f.on_delete).to_string(),
                    on_update_index: action_index(&f.on_update),
                    on_update: fk_action_label(&f.on_update).to_string(),
                }
            })
            .collect();

        let (sql, sql_error, statement_count) = if self.step == CREATE_STEPS.len() - 1 {
            match self.statements(driver) {
                Ok(s) => (s.join("\n\n"), String::new(), s.len()),
                Err(e) => (String::new(), e, 0),
            }
        } else {
            (String::new(), String::new(), 0)
        };

        DialogView {
            mode: 0,
            title: "New table".to_string(),
            subtitle: if self.schema.is_empty() {
                String::new()
            } else {
                format!("in {}", self.schema)
            },
            steps: CREATE_STEPS.iter().map(|s| s.to_string()).collect(),
            step: self.step as i32,
            step_errors: self.step_errors(self.step),
            can_next: self.can_advance(),
            name: self.name.clone(),
            description: self.description.clone(),
            if_not_exists: self.if_not_exists,
            columns,
            preset_labels: preset_labels(),
            uniques,
            foreign_keys,
            fk_tables: tables.to_vec(),
            action_labels: action_labels(),
            edit_rows: Vec::new(),
            pending: 0,
            sql,
            sql_error,
            statement_count,
        }
    }
}

impl AlterEditor {
    pub fn view(&self, driver: &DbDriver) -> DialogView {
        let edit_rows = self
            .rows
            .iter()
            .map(|r| {
                let origin = r.row.origin.as_ref();
                let mut badges = Vec::new();
                if r.removed {
                    badges.push("will drop".to_string());
                } else if let Some(o) = origin {
                    let new_name = r.row.col.name.trim();
                    if !new_name.is_empty() && new_name != o.name.trim() {
                        badges.push("renamed".to_string());
                    }
                    if r.row.col.nullable != o.nullable && !r.row.col.primary_key {
                        badges.push("modified".to_string());
                    }
                } else {
                    badges.push("new".to_string());
                }
                let pk = r.is_primary_key();
                EditRowView {
                    name: r.row.col.name.clone(),
                    type_label: r.type_label.clone(),
                    nullable: r.row.col.nullable && !r.row.col.primary_key,
                    nullable_enabled: !r.removed && !r.row.col.primary_key,
                    primary_key: pk,
                    is_new: r.is_new(),
                    removed: r.removed,
                    badges,
                    can_remove: !pk,
                    remove_note: if pk {
                        "Primary key columns cannot be dropped".to_string()
                    } else {
                        String::new()
                    },
                    name_enabled: !r.removed,
                    type_pickable: r.is_new() && !r.removed,
                    preset_index: preset_index(r.row.col.type_kind),
                }
            })
            .collect();

        let (sql, sql_error, statement_count) = if self.step == 1 {
            match self.statements(driver) {
                Ok(s) => (s.join("\n\n"), String::new(), s.len()),
                Err(e) => (String::new(), e, 0),
            }
        } else {
            (String::new(), String::new(), 0)
        };

        DialogView {
            mode: 1,
            title: "Edit table".to_string(),
            subtitle: qualified_display(&self.schema, &self.table),
            steps: EDIT_STEPS.iter().map(|s| s.to_string()).collect(),
            step: self.step as i32,
            step_errors: if self.step == 0 { self.errors() } else { Vec::new() },
            can_next: self.step == 0 && self.errors().is_empty(),
            name: String::new(),
            description: String::new(),
            if_not_exists: false,
            columns: Vec::new(),
            preset_labels: preset_labels(),
            uniques: Vec::new(),
            foreign_keys: Vec::new(),
            fk_tables: Vec::new(),
            action_labels: Vec::new(),
            edit_rows,
            pending: self.pending_count(),
            sql,
            sql_error,
            statement_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{ColumnInfo, TableInfo};

    fn pg() -> DbDriver {
        DbDriver::Postgres
    }

    fn named(w: &mut CreateWizard, name: &str) {
        w.apply("name", 0, 0, name, &[]);
    }

    fn live_table() -> TableSchema {
        let mut id = ColumnInfo::new("id".into(), "integer".into(), false);
        id.is_primary_key = true;
        id.is_auto_increment = true;
        let name = ColumnInfo::new("name".into(), "varchar(100)".into(), true);
        let age = ColumnInfo::new("age".into(), "integer".into(), true);
        TableSchema::new(
            TableInfo::new("people".into(), "public".into(), "TABLE".into()),
            vec![id, name, age],
            vec![],
        )
    }

    fn editor() -> AlterEditor {
        AlterEditor::from_schema("public", "people", &live_table())
    }

    // -- presets ---------------------------------------------------------------

    #[test]
    fn there_are_nine_presets_and_every_one_has_a_label() {
        assert_eq!(PRESETS.len(), 9);
        let labels = preset_labels();
        assert_eq!(labels.len(), 9);
        assert!(labels.iter().all(|l| !l.is_empty()));
        assert_eq!(labels[2], "VARCHAR");
    }

    #[test]
    fn the_type_parameter_box_parses_lengths_and_precisions() {
        let mut c = column_of(TypeKind::Varchar);
        assert!(apply_param(&mut c, " 80 ").is_ok());
        assert_eq!(c.length, 80);
        assert!(apply_param(&mut c, "0").is_err());
        assert!(apply_param(&mut c, "abc").is_err());
        assert_eq!(c.length, 80, "a bad value must not overwrite a good one");

        let mut d = column_of(TypeKind::Decimal);
        assert!(apply_param(&mut d, "12,4").is_ok());
        assert_eq!((d.precision, d.scale), (12, 4));
        assert!(apply_param(&mut d, "8").is_ok());
        assert_eq!((d.precision, d.scale), (8, 0));
        assert!(apply_param(&mut d, "4,6").is_err());
        assert!(apply_param(&mut d, "1,2,3").is_err());
        assert!(apply_param(&mut d, "39,2").is_err());
    }

    // -- create wizard -----------------------------------------------------------

    #[test]
    fn a_new_wizard_starts_with_an_id_key_and_needs_a_name() {
        let w = CreateWizard::new("public");
        assert_eq!(w.columns.len(), 1);
        assert!(w.columns[0].row.primary_key && !w.columns[0].row.nullable);
        assert_eq!(w.step_errors(0), vec!["Table name is required"]);
        assert!(!w.can_advance());
    }

    #[test]
    fn steps_advance_only_when_valid_and_going_back_is_free() {
        let mut w = CreateWizard::new("public");
        assert_eq!(w.apply("next", 0, 0, "", &[]), Change::None);
        named(&mut w, "pets");
        assert_eq!(w.apply("next", 0, 0, "", &[]), Change::Structure);
        assert_eq!(w.step, 1);
        // Jump straight to the preview: fine, every step between is valid.
        assert_eq!(w.apply("step", 3, 0, "", &[]), Change::Structure);
        assert_eq!(w.step, 3);
        assert_eq!(w.apply("next", 0, 0, "", &[]), Change::None);
        assert_eq!(w.apply("step", 0, 0, "", &[]), Change::Structure);
        assert_eq!(w.step, 0);

        // A broken column blocks jumping past it.
        w.apply("col-name", 0, 0, "", &[]);
        assert_eq!(w.apply("step", 2, 0, "", &[]), Change::None);
        assert_eq!(w.step, 0);
    }

    #[test]
    fn column_edits_keep_the_row_consistent() {
        let mut w = CreateWizard::new("public");
        w.apply("col-add", 0, 0, "", &[]);
        w.apply("col-name", 1, 0, "title", &[]);
        w.apply("col-type", 1, 2, "", &[]); // VARCHAR
        assert_eq!(w.columns[1].param, "255");
        w.apply("col-param", 1, 0, "40", &[]);
        assert_eq!(w.columns[1].row.length, 40);

        // Auto-increment only on integers.
        assert_eq!(w.apply("col-auto", 1, 0, "", &[]), Change::None);
        w.apply("col-type", 1, 0, "", &[]); // INTEGER
        assert_eq!(w.apply("col-auto", 1, 0, "", &[]), Change::Structure);
        assert!(w.columns[1].row.auto_increment);
        // Switching to a non-integer type drops it again.
        w.apply("col-type", 1, 3, "", &[]); // TEXT
        assert!(!w.columns[1].row.auto_increment);

        // A primary key is never nullable and the toggle is refused.
        w.apply("col-pk", 1, 0, "", &[]);
        assert!(!w.columns[1].row.nullable);
        assert_eq!(w.apply("col-null", 1, 0, "", &[]), Change::None);
        w.apply("col-pk", 1, 0, "", &[]);
        assert_eq!(w.apply("col-null", 1, 0, "", &[]), Change::Structure);
    }

    #[test]
    fn a_bad_parameter_is_reported_against_its_column() {
        let mut w = CreateWizard::new("public");
        named(&mut w, "t");
        w.apply("col-add", 0, 0, "", &[]);
        w.apply("col-name", 1, 0, "code", &[]);
        w.apply("col-type", 1, 2, "", &[]);
        w.apply("col-param", 1, 0, "lots", &[]);
        let errs = w.step_errors(1);
        assert!(errs.iter().any(|e| e.contains("Column code") && e.contains("length")), "{errs:?}");
        assert!(w.statements(&pg()).is_err());
        w.apply("col-param", 1, 0, "12", &[]);
        assert!(w.step_errors(1).is_empty());
    }

    #[test]
    fn duplicate_and_empty_column_names_are_refused() {
        let mut w = CreateWizard::new("public");
        w.apply("col-add", 0, 0, "", &[]);
        assert!(w.step_errors(1).iter().any(|e| e.contains("Every column needs a name")));
        w.apply("col-name", 1, 0, "ID", &[]);
        assert!(w.step_errors(1).iter().any(|e| e.contains("Duplicate column")));
    }

    #[test]
    fn create_sql_carries_options_uniques_and_foreign_keys() {
        let mut w = CreateWizard::new("public");
        named(&mut w, "pets");
        w.apply("desc", 0, 0, "All the pets", &[]);
        w.apply("ine", 0, 0, "", &[]);
        w.apply("col-add", 0, 0, "", &[]);
        w.apply("col-name", 1, 0, "email", &[]);
        w.apply("col-type", 1, 2, "", &[]);
        w.apply("col-add", 0, 0, "", &[]);
        w.apply("col-name", 2, 0, "owner_id", &[]);

        w.apply("uq-add", 0, 0, "", &[]);
        w.apply("uq-name", 0, 0, "uq_email", &[]);
        w.apply("uq-col", 0, 1, "", &[]); // email

        let tables = vec!["owners".to_string()];
        w.apply("fk-add", 0, 0, "", &tables);
        w.apply("fk-col", 0, 2, "", &tables); // owner_id
        w.apply("fk-table", 0, 0, "", &tables);
        assert_eq!(w.missing_ref_tables(), vec!["owners".to_string()]);
        w.ref_columns.insert("owners".into(), RefColumns::Loaded(vec!["id".into(), "name".into()]));
        w.apply("fk-rcol", 0, 0, "", &tables);
        w.apply("fk-del", 0, 2, "", &tables); // CASCADE
        w.apply("fk-upd", 0, 3, "", &tables); // SET NULL

        assert!(w.all_errors().is_empty(), "{:?}", w.all_errors());
        let sql = w.statements(&pg()).unwrap().join("\n");
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"public\".\"pets\""), "{sql}");
        assert!(sql.contains("CONSTRAINT \"uq_email\" UNIQUE (\"email\")"), "{sql}");
        assert!(sql.contains("FOREIGN KEY (\"owner_id\") REFERENCES \"owners\" (\"id\") ON DELETE CASCADE ON UPDATE SET NULL"), "{sql}");
        assert!(sql.contains("COMMENT ON TABLE \"public\".\"pets\" IS 'All the pets'"), "{sql}");
        assert!(sql.contains("VARCHAR(255)"), "{sql}");
    }

    #[test]
    fn a_foreign_key_must_be_complete_before_the_preview() {
        let mut w = CreateWizard::new("public");
        named(&mut w, "pets");
        let tables = vec!["owners".to_string()];
        w.apply("fk-add", 0, 0, "", &tables);
        let e = w.step_errors(2);
        assert!(e.iter().any(|m| m.contains("at least one column")));
        assert!(e.iter().any(|m| m.contains("table to reference")));

        w.apply("fk-col", 0, 0, "", &tables);
        w.apply("fk-table", 0, 0, "", &tables);
        assert!(w.step_errors(2).iter().any(|m| m.contains("still loading")));
        w.ref_columns.insert("owners".into(), RefColumns::Failed("no such table".into()));
        assert!(w.step_errors(2).iter().any(|m| m.contains("could not read owners")));
        w.ref_columns.insert("owners".into(), RefColumns::Loaded(vec!["id".into()]));
        assert!(w.step_errors(2).iter().any(|m| m.contains("needs a column to reference")));
        w.apply("fk-rcol", 0, 0, "", &tables);
        assert!(w.step_errors(2).is_empty());

        // An unchanged table keeps its picked columns; a different one clears them.
        w.apply("fk-table", 0, 0, "", &tables);
        assert_eq!(w.foreign_keys[0].ref_columns, vec!["id".to_string()]);
    }

    #[test]
    fn renaming_a_column_follows_into_its_constraints_and_removing_one_leaves_them() {
        let mut w = CreateWizard::new("public");
        w.apply("col-add", 0, 0, "", &[]);
        w.apply("col-name", 1, 0, "a", &[]);
        w.apply("uq-add", 0, 0, "", &[]);
        w.apply("uq-col", 0, 1, "", &[]);
        assert_eq!(w.uniques[0].columns, vec!["a"]);

        w.apply("col-name", 1, 0, "ab", &[]);
        w.apply("col-name", 1, 0, "abc", &[]);
        assert_eq!(w.uniques[0].columns, vec!["abc"]);

        w.apply("col-remove", 1, 0, "", &[]);
        assert!(w.uniques[0].columns.is_empty());
        assert!(w.step_errors(2).iter().any(|m| m.contains("Unique constraint 1")));
    }

    #[test]
    fn constraint_chips_only_offer_named_columns() {
        let mut w = CreateWizard::new("public");
        w.apply("col-add", 0, 0, "", &[]); // unnamed
        w.apply("uq-add", 0, 0, "", &[]);
        let v = w.view(&pg(), &[]);
        assert_eq!(v.uniques[0].chips.len(), 1);
        assert_eq!(v.uniques[0].chips[0].label, "id");
        // Picking the unnamed one's index is out of range and ignored.
        assert_eq!(w.apply("uq-col", 0, 1, "", &[]), Change::None);
    }

    #[test]
    fn the_preview_step_shows_sql_or_the_reason_there_is_none() {
        let mut w = CreateWizard::new("public");
        w.step = 3;
        let v = w.view(&pg(), &[]);
        assert!(v.sql.is_empty());
        assert_eq!(v.sql_error, "Table name is required");

        named(&mut w, "t");
        let v = w.view(&pg(), &[]);
        assert!(v.sql.contains("CREATE TABLE \"public\".\"t\""), "{}", v.sql);
        assert_eq!(v.statement_count, 1);
        assert!(v.sql_error.is_empty());
    }

    #[test]
    fn create_sql_is_per_dialect_and_escapes_hostile_names() {
        let mut w = CreateWizard::new("");
        named(&mut w, "t\"; DROP TABLE users; --");
        let sql = w.statements(&DbDriver::Sqlite).unwrap().join("\n");
        assert!(sql.contains("\"t\"\"; DROP TABLE users; --\""), "{sql}");

        let mut m = CreateWizard::new("shop");
        named(&mut m, "orders");
        let sql = m.statements(&DbDriver::MySql).unwrap().join("\n");
        // The MySQL generator leaves the schema out: the table lands in the
        // connection's own database.
        assert!(sql.contains("CREATE TABLE `orders`"), "{sql}");
    }

    #[test]
    fn a_driver_without_ddl_reports_rather_than_guessing() {
        let mut w = CreateWizard::new("");
        named(&mut w, "t");
        assert!(w.statements(&DbDriver::MongoDb).is_err());
    }

    // -- edit dialog -------------------------------------------------------------

    #[test]
    fn an_untouched_table_has_nothing_pending() {
        let e = editor();
        assert_eq!(e.pending_count(), 0);
        assert_eq!(e.statements(&pg()), Ok(Vec::new()));
        let v = e.view(&pg());
        assert_eq!(v.pending, 0);
        assert_eq!(v.edit_rows.len(), 3);
        assert!(v.edit_rows[0].primary_key && !v.edit_rows[0].can_remove);
        assert_eq!(v.edit_rows[0].remove_note, "Primary key columns cannot be dropped");
        assert!(v.edit_rows.iter().all(|r| r.badges.is_empty()));
    }

    #[test]
    fn a_primary_key_column_cannot_be_dropped() {
        let mut e = editor();
        assert_eq!(e.apply("col-remove", 0, 0, "").clone(), Change::None);
        assert!(!e.rows[0].removed);
        assert_eq!(e.pending_count(), 0);
    }

    #[test]
    fn the_drop_guard_holds_even_if_a_row_is_marked_by_other_means() {
        let mut e = editor();
        e.rows[0].removed = true; // bypass the action on purpose
        assert_eq!(e.dropped_primary_keys(), vec!["id".to_string()]);
        let err = e.statements(&pg()).unwrap_err();
        assert!(err.contains("primary key") && err.contains("cannot be dropped"), "{err}");
        assert!(e.view(&pg()).sql.is_empty());
    }

    #[test]
    fn dropping_a_column_is_soft_and_restorable() {
        let mut e = editor();
        assert_eq!(e.apply("col-remove", 2, 0, ""), Change::Structure);
        let v = e.view(&pg());
        assert!(v.edit_rows[2].removed);
        assert_eq!(v.edit_rows[2].badges, vec!["will drop"]);
        assert_eq!(v.pending, 1);
        assert!(!v.edit_rows[2].nullable_enabled);
        assert!(!v.edit_rows[2].name_enabled);

        e.step = 1;
        let sql = e.view(&pg()).sql;
        assert!(sql.contains("DROP COLUMN \"age\""), "{sql}");
        assert!(!sql.contains("CASCADE"), "a drop must never cascade silently: {sql}");

        assert_eq!(e.apply("col-restore", 2, 0, ""), Change::Structure);
        assert_eq!(e.pending_count(), 0);
        assert_eq!(e.apply("col-restore", 2, 0, ""), Change::None);
    }

    #[test]
    fn a_new_column_is_discarded_not_marked() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "");
        assert_eq!(e.rows.len(), 4);
        assert!(e.view(&pg()).edit_rows[3].badges.contains(&"new".to_string()));
        e.apply("col-remove", 3, 0, "");
        assert_eq!(e.rows.len(), 3);
    }

    #[test]
    fn adding_renaming_and_toggling_count_as_pending_operations() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "");
        e.apply("col-name", 3, 0, "email");
        e.apply("col-type", 3, 2, ""); // VARCHAR
        e.apply("col-name", 1, 0, "full_name");
        e.apply("col-null", 2, 0, ""); // age: NOT NULL

        let v = e.view(&pg());
        assert_eq!(v.pending, 3);
        assert_eq!(v.edit_rows[1].badges, vec!["renamed"]);
        assert_eq!(v.edit_rows[2].badges, vec!["modified"]);
        assert_eq!(v.edit_rows[3].badges, vec!["new"]);

        e.step = 1;
        let sql = e.view(&pg()).sql;
        assert!(sql.contains("RENAME COLUMN \"name\" TO \"full_name\""), "{sql}");
        assert!(sql.contains("ADD COLUMN \"email\" VARCHAR(255)"), "{sql}");
        assert!(sql.contains("\"age\" SET NOT NULL"), "{sql}");
        assert_eq!(e.view(&pg()).statement_count, 3);
    }

    #[test]
    fn an_unnamed_new_column_is_an_error_not_an_add() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "");
        assert!(e.errors().iter().any(|m| m.contains("Every column needs a name")));
        assert!(!e.view(&pg()).can_next);
    }

    #[test]
    fn a_live_columns_type_cannot_be_changed_here() {
        let mut e = editor();
        assert_eq!(e.apply("col-type", 1, 3, ""), Change::None);
        e.apply("col-add", 0, 0, "");
        assert_eq!(e.apply("col-type", 3, 3, ""), Change::Structure);
        assert_eq!(e.rows[3].type_label, "TEXT");
    }

    #[test]
    fn the_key_columns_nullability_cannot_be_toggled() {
        let mut e = editor();
        assert_eq!(e.apply("col-null", 0, 0, ""), Change::None);
        assert!(!e.view(&pg()).edit_rows[0].nullable_enabled);
    }

    #[test]
    fn sqlite_says_what_it_cannot_do_instead_of_producing_bad_sql() {
        let mut e = editor();
        e.apply("col-null", 2, 0, "");
        let err = e.statements(&DbDriver::Sqlite).unwrap_err();
        assert!(err.contains("not supported by SQLite"), "{err}");
        e.step = 1;
        let v = e.view(&DbDriver::Sqlite);
        assert!(v.sql.is_empty() && v.sql_error.contains("SQLite"));
    }

    #[test]
    fn dropping_every_column_is_refused() {
        let mut e = editor();
        e.apply("col-remove", 1, 0, "");
        e.apply("col-remove", 2, 0, "");
        // The key stays, so the table still has a column.
        assert!(e.errors().is_empty());
        let mut no_key = AlterEditor::from_schema(
            "",
            "t",
            &TableSchema::new(
                TableInfo::new("t".into(), "".into(), "TABLE".into()),
                vec![ColumnInfo::new("a".into(), "text".into(), true)],
                vec![],
            ),
        );
        no_key.apply("col-remove", 0, 0, "");
        assert!(no_key.errors().iter().any(|m| m.contains("at least one column")));
    }

    #[test]
    fn edit_steps_move_between_columns_and_preview() {
        let mut e = editor();
        assert_eq!(e.apply("back", 0, 0, ""), Change::None);
        assert_eq!(e.apply("next", 0, 0, ""), Change::Structure);
        assert_eq!(e.step, 1);
        assert_eq!(e.apply("next", 0, 0, ""), Change::None);
        assert_eq!(e.apply("back", 0, 0, ""), Change::Structure);
        assert_eq!(e.apply("step", 1, 0, ""), Change::Structure);
        assert_eq!(e.apply("step", 1, 0, ""), Change::None);
    }

    #[test]
    fn rows_from_schema_round_trip_types_without_rewriting_them() {
        let rows = rows_from_schema(&live_table());
        assert_eq!(rows[1].0.col.type_kind, TypeKind::Varchar);
        assert_eq!(rows[1].0.col.length, 100);
        assert_eq!(rows[1].1, "varchar(100)");
        assert!(rows[0].0.col.primary_key && !rows[0].0.col.nullable);
        let e = editor();
        assert_eq!(e.pending_count(), 0);
    }

    // -- reports -----------------------------------------------------------------

    #[test]
    fn a_report_names_each_statements_fate() {
        let stmts: Vec<String> = ["A;", "B;", "C;"].iter().map(|s| s.to_string()).collect();
        let r = report(&stmts, &[Ok(()), Err("boom".into())]);
        assert_eq!(r[0].status, StatementStatus::Done);
        assert_eq!(r[1].status, StatementStatus::Failed("boom".into()));
        assert_eq!(r[2].status, StatementStatus::Skipped);
        assert!(!report_ok(&r));
        assert_eq!(report_summary(&r), "1 of 3 statements ran; 1 failed, 1 skipped");

        let ok = report(&stmts, &[Ok(()), Ok(()), Ok(())]);
        assert!(report_ok(&ok));
        assert_eq!(report_summary(&ok), "3 of 3 statements ran");
        assert!(!report_ok(&[]));
    }
}
