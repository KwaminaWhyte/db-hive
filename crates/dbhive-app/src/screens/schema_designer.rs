//! Visual schema designer.
//!
//! Replaces the reactflow-based `VisualSchemaDesigner`. The user lays tables
//! out on a canvas, edits their columns, and draws foreign keys by dragging
//! from one column's connector onto another column. The design is then
//! turned into `TableDefinition`s and handed to `dbhive_core::ddl`, which
//! knows every dialect; no SQL is ever built here by concatenation.
//!
//! Drawing follows `er_diagram`: one `to_screen` transform, pan and zoom by
//! hand, manual positions pinned over the computed layout. The difference is
//! that this screen *creates* edges, so it carries a small drag state machine
//! (`Drag`) rather than hit-testing afresh every frame.
//!
//! Nothing is applied without the user seeing the exact statements first and
//! confirming. DDL is not undoable.

use std::collections::HashMap;

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::{
    ColumnDefinition, DbDriver, DbError, ForeignKeyAction, ForeignKeyConstraint, ForeignKeyInfo,
    TableDefinition, TableSchema, UniqueConstraint,
};

use crate::app::DbHiveApp;
use crate::screens::table_editor::{column_type, ColumnRow, TypeKind};
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::graph_layout::{self, Edge, LayoutOptions, Node};

// ─── model ────────────────────────────────────────────────────────────────

/// One column being designed.
///
/// The shared part is a `ColumnRow`, the same row the table editor edits, so
/// the type picker, its parameters and the per-column validation are one
/// implementation rather than two that drift.
#[derive(Clone)]
pub struct DesignColumn {
    pub base: ColumnRow,
    pub unique: bool,
    /// Referenced table, by name. Empty when the column is not a foreign key.
    pub ref_table: String,
    /// Referenced column, by name.
    pub ref_column: String,
    pub on_delete: ForeignKeyAction,
}

impl Default for DesignColumn {
    fn default() -> Self {
        Self {
            base: ColumnRow::default(),
            unique: false,
            ref_table: String::new(),
            ref_column: String::new(),
            on_delete: ForeignKeyAction::NoAction,
        }
    }
}

impl DesignColumn {
    fn has_reference(&self) -> bool {
        !self.ref_table.trim().is_empty() || !self.ref_column.trim().is_empty()
    }
}

/// One table being designed.
#[derive(Clone)]
pub struct DesignTable {
    /// Stable identity. Names are edited freely and start out empty, so they
    /// cannot key positions or selection.
    pub id: u64,
    pub name: String,
    pub schema: String,
    pub columns: Vec<DesignColumn>,
}

/// A resolved foreign key: both ends exist in the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relationship {
    pub from: usize,
    pub from_column: usize,
    pub to: usize,
    pub to_column: usize,
}

/// Why a declared reference cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefProblem {
    /// A table was named but no column, or the other way round.
    Incomplete,
    UnknownTable(String),
    UnknownColumn { table: String, column: String },
    IncompatibleTypes { from: String, to: String },
    /// SQL requires the referenced column to be a primary key or unique.
    TargetNotUnique { table: String, column: String },
}

impl RefProblem {
    pub fn message(&self) -> String {
        match self {
            RefProblem::Incomplete => {
                "a foreign key needs both a referenced table and column".into()
            }
            RefProblem::UnknownTable(t) => format!("references unknown table \"{}\"", t),
            RefProblem::UnknownColumn { table, column } => {
                format!("references \"{}\" which table \"{}\" does not have", column, table)
            }
            RefProblem::IncompatibleTypes { from, to } => {
                format!("type {} cannot reference {}", from, to)
            }
            RefProblem::TargetNotUnique { table, column } => format!(
                "\"{}.{}\" is neither a primary key nor unique, so it cannot be referenced",
                table, column
            ),
        }
    }
}

/// Broad type groups. Two columns can only be joined by a foreign key when
/// they land in the same group: Postgres will not let an `integer` reference
/// a `text`, and finding that out from the server is a slow way to learn it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFamily {
    Integer,
    Numeric,
    Text,
    Binary,
    Boolean,
    Date,
    Time,
    Timestamp,
    Json,
    Uuid,
    /// A driver-specific type named by the user. Comparable only by name.
    Custom,
}

pub fn family(kind: TypeKind) -> TypeFamily {
    match kind {
        TypeKind::SmallInt | TypeKind::Integer | TypeKind::BigInt => TypeFamily::Integer,
        TypeKind::Decimal | TypeKind::Real | TypeKind::DoublePrecision => TypeFamily::Numeric,
        TypeKind::Varchar | TypeKind::Char | TypeKind::Text => TypeFamily::Text,
        TypeKind::Bytea => TypeFamily::Binary,
        TypeKind::Boolean => TypeFamily::Boolean,
        TypeKind::Date => TypeFamily::Date,
        TypeKind::Time => TypeFamily::Time,
        TypeKind::Timestamp | TypeKind::TimestampTz => TypeFamily::Timestamp,
        TypeKind::Json | TypeKind::JsonB => TypeFamily::Json,
        TypeKind::Uuid => TypeFamily::Uuid,
        TypeKind::Custom => TypeFamily::Custom,
    }
}

/// Whether a foreign key from `from` to `to` is type-wise plausible.
///
/// Widths are deliberately ignored: an `integer` referencing a `bigint` is
/// normal and legal. Custom types are only compatible with an identically
/// named custom type, since nothing here knows what they are.
pub fn types_compatible(from: &DesignColumn, to: &DesignColumn) -> bool {
    let (a, b) = (family(from.base.type_kind), family(to.base.type_kind));
    if a == TypeFamily::Custom || b == TypeFamily::Custom {
        return a == b
            && from.base.custom.trim().eq_ignore_ascii_case(to.base.custom.trim())
            && !from.base.custom.trim().is_empty();
    }
    a == b
}

/// A human label for a table, since a fresh one has no name yet.
pub fn display_name(t: &DesignTable) -> String {
    if t.name.trim().is_empty() {
        "untitled".to_string()
    } else {
        t.name.trim().to_string()
    }
}

/// The type as it will read in the generated SQL, for error messages.
fn type_label(c: &DesignColumn) -> String {
    if c.base.type_kind == TypeKind::Custom {
        let name = c.base.custom.trim();
        if name.is_empty() {
            return "custom".into();
        }
        return name.to_string();
    }
    c.base.type_kind.label().to_string()
}

fn find_table(tables: &[DesignTable], name: &str) -> Option<usize> {
    let needle = name.trim();
    tables
        .iter()
        .position(|t| t.name.trim().eq_ignore_ascii_case(needle) && !t.name.trim().is_empty())
}

fn find_column(t: &DesignTable, name: &str) -> Option<usize> {
    let needle = name.trim();
    t.columns
        .iter()
        .position(|c| c.base.name.trim().eq_ignore_ascii_case(needle) && !needle.is_empty())
}

/// Resolve the foreign key declared on one column.
///
/// `Ok(None)` means the column simply is not a foreign key.
pub fn check_reference(
    tables: &[DesignTable],
    table: usize,
    column: usize,
) -> Result<Option<(usize, usize)>, RefProblem> {
    let col = &tables[table].columns[column];
    if !col.has_reference() {
        return Ok(None);
    }
    let (rt, rc) = (col.ref_table.trim(), col.ref_column.trim());
    if rt.is_empty() || rc.is_empty() {
        return Err(RefProblem::Incomplete);
    }

    let target = find_table(tables, rt).ok_or_else(|| RefProblem::UnknownTable(rt.to_string()))?;
    let target_col = find_column(&tables[target], rc).ok_or_else(|| RefProblem::UnknownColumn {
        table: display_name(&tables[target]),
        column: rc.to_string(),
    })?;

    let other = &tables[target].columns[target_col];
    if !(other.base.primary_key || other.unique) {
        return Err(RefProblem::TargetNotUnique {
            table: display_name(&tables[target]),
            column: other.base.name.trim().to_string(),
        });
    }
    if !types_compatible(col, other) {
        return Err(RefProblem::IncompatibleTypes {
            from: type_label(col),
            to: type_label(other),
        });
    }
    Ok(Some((target, target_col)))
}

/// Every foreign key that resolves. Broken ones are reported by `validate`
/// and simply not drawn.
pub fn relationships(tables: &[DesignTable]) -> Vec<Relationship> {
    let mut out = Vec::new();
    for (ti, t) in tables.iter().enumerate() {
        for ci in 0..t.columns.len() {
            if let Ok(Some((to, to_column))) = check_reference(tables, ti, ci) {
                out.push(Relationship {
                    from: ti,
                    from_column: ci,
                    to,
                    to_column,
                });
            }
        }
    }
    out
}

/// Order the tables so every table is created after the ones it references.
///
/// `Err` names the tables caught in a reference cycle. The generators emit
/// foreign keys inline in `CREATE TABLE`, so a cycle cannot be created in one
/// pass at all; it has to be reported rather than sorted around. A table
/// referencing itself is fine and is not a cycle.
pub fn creation_order(tables: &[DesignTable]) -> Result<Vec<usize>, Vec<String>> {
    let n = tables.len();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for r in relationships(tables) {
        if r.from != r.to && !deps[r.from].contains(&r.to) {
            deps[r.from].push(r.to);
        }
    }

    let mut done = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    while order.len() < n {
        let mut progressed = false;
        for i in 0..n {
            if done[i] {
                continue;
            }
            if deps[i].iter().all(|d| done[*d]) {
                done[i] = true;
                order.push(i);
                progressed = true;
            }
        }
        if !progressed {
            let stuck: Vec<String> = (0..n)
                .filter(|i| !done[*i])
                .map(|i| display_name(&tables[i]))
                .collect();
            return Err(stuck);
        }
    }
    Ok(order)
}

/// Turn one designed table into the core's definition.
///
/// `default_schema` is used when the table does not name its own, which is
/// the common case: the whole design usually targets one schema.
pub fn build_definition(default_schema: &str, t: &DesignTable) -> TableDefinition {
    let schema = if !t.schema.trim().is_empty() {
        Some(t.schema.trim().to_string())
    } else if !default_schema.trim().is_empty() {
        Some(default_schema.trim().to_string())
    } else {
        None
    };

    let primary: Vec<String> = t
        .columns
        .iter()
        .filter(|c| c.base.primary_key)
        .map(|c| c.base.name.trim().to_string())
        .collect();

    let columns: Vec<ColumnDefinition> = t
        .columns
        .iter()
        .map(|c| ColumnDefinition {
            name: c.base.name.trim().to_string(),
            column_type: column_type(&c.base),
            // A primary key is NOT NULL everywhere; the form must not be able
            // to claim otherwise or the server rejects the DDL.
            nullable: c.base.nullable && !c.base.primary_key,
            default: {
                let d = c.base.default.trim();
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_string())
                }
            },
            primary_key: c.base.primary_key,
            auto_increment: c.base.auto_increment,
            comment: None,
        })
        .collect();

    let foreign_keys: Vec<ForeignKeyConstraint> = t
        .columns
        .iter()
        .filter(|c| !c.ref_table.trim().is_empty() && !c.ref_column.trim().is_empty())
        .map(|c| ForeignKeyConstraint {
            // Unnamed: each generator has its own naming convention and will
            // produce one that fits the dialect.
            name: None,
            columns: vec![c.base.name.trim().to_string()],
            referenced_table: c.ref_table.trim().to_string(),
            referenced_columns: vec![c.ref_column.trim().to_string()],
            on_delete: c.on_delete.clone(),
            on_update: ForeignKeyAction::NoAction,
        })
        .collect();

    // A primary key is already unique; emitting a second constraint for it
    // would create a redundant index on every apply.
    let unique_constraints: Vec<UniqueConstraint> = t
        .columns
        .iter()
        .filter(|c| c.unique && !c.base.primary_key)
        .map(|c| UniqueConstraint {
            name: None,
            columns: vec![c.base.name.trim().to_string()],
        })
        .collect();

    TableDefinition {
        schema,
        name: t.name.trim().to_string(),
        columns,
        primary_key: if primary.is_empty() {
            None
        } else {
            Some(primary)
        },
        foreign_keys,
        unique_constraints,
        check_constraints: Vec::new(),
        comment: None,
        // Deliberately false, as in the table editor: a silent no-op when the
        // table already exists hides a real mistake.
        if_not_exists: false,
    }
}

/// Everything wrong with the design, as user-facing messages.
pub fn validate(tables: &[DesignTable]) -> Vec<String> {
    let mut errors = Vec::new();
    if tables.is_empty() {
        errors.push("Add at least one table".into());
        return errors;
    }

    let mut seen: Vec<String> = Vec::new();
    for t in tables {
        let lower = t.name.trim().to_lowercase();
        if lower.is_empty() {
            continue;
        }
        if seen.contains(&lower) {
            errors.push(format!("Duplicate table name: {}", t.name.trim()));
        }
        seen.push(lower);
    }

    for t in tables {
        let label = display_name(t);
        let rows: Vec<ColumnRow> = t.columns.iter().map(|c| c.base.clone()).collect();
        for e in crate::screens::table_editor::validate(&t.name, &rows) {
            errors.push(format!("{}: {}", label, e));
        }
    }

    for (ti, t) in tables.iter().enumerate() {
        for (ci, c) in t.columns.iter().enumerate() {
            if let Err(problem) = check_reference(tables, ti, ci) {
                errors.push(format!(
                    "{}.{}: {}",
                    display_name(t),
                    if c.base.name.trim().is_empty() {
                        "(unnamed)"
                    } else {
                        c.base.name.trim()
                    },
                    problem.message()
                ));
            }
        }
    }

    if let Err(stuck) = creation_order(tables) {
        errors.push(format!(
            "Foreign keys form a cycle between {}; one of them has to be added after the tables exist",
            stuck.join(", ")
        ));
    }

    errors
}

/// Generate the whole design's DDL, referenced tables first.
pub fn generate_sql(
    driver: &DbDriver,
    default_schema: &str,
    tables: &[DesignTable],
) -> Result<Vec<String>, String> {
    let order = creation_order(tables).map_err(|stuck| {
        format!("Foreign keys form a cycle between {}", stuck.join(", "))
    })?;
    let generator = get_ddl_generator(driver).map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for i in order {
        let def = build_definition(default_schema, &tables[i]);
        let result = generator
            .generate_create_table(&def)
            .map_err(|e| format!("{}: {}", display_name(&tables[i]), e))?;
        out.extend(result.sql);
    }
    Ok(out)
}

// ─── loading an existing schema ───────────────────────────────────────────

/// Map a driver's type string back onto a picker choice.
///
/// Returns the kind plus its parameters, since `VARCHAR(50)` has to keep its
/// 50. Anything unrecognised becomes a custom type carrying the original
/// text, so a round trip never silently changes a column's type.
pub fn parse_type(raw: &str) -> (TypeKind, u32, u8, u8, String) {
    let lower = raw.trim().to_lowercase();
    let base = lower.split('(').next().unwrap_or("").trim().to_string();
    let args: Vec<u32> = lower
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(inside, _)| {
            inside
                .split(',')
                .filter_map(|p| p.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default();

    let length = args.first().copied().unwrap_or(255).max(1);
    let precision = args.first().copied().unwrap_or(10).min(38) as u8;
    let scale = args.get(1).copied().unwrap_or(0).min(38) as u8;

    // MySQL spells a boolean TINYINT(1); nothing else about a tinyint says so.
    if base == "tinyint" && args.first() == Some(&1) {
        return (TypeKind::Boolean, length, precision, scale, String::new());
    }

    let kind = match base.as_str() {
        "smallint" | "int2" | "smallserial" => TypeKind::SmallInt,
        "integer" | "int" | "int4" | "serial" | "mediumint" | "tinyint" => TypeKind::Integer,
        "bigint" | "int8" | "bigserial" => TypeKind::BigInt,
        "numeric" | "decimal" | "money" => TypeKind::Decimal,
        "real" | "float4" | "float" => TypeKind::Real,
        "double precision" | "double" | "float8" => TypeKind::DoublePrecision,
        "varchar" | "character varying" | "nvarchar" | "varchar2" => TypeKind::Varchar,
        "char" | "character" | "bpchar" | "nchar" => TypeKind::Char,
        "text" | "longtext" | "mediumtext" | "tinytext" | "clob" | "ntext" => TypeKind::Text,
        "bytea" | "blob" | "longblob" | "varbinary" | "binary" | "image" => TypeKind::Bytea,
        "boolean" | "bool" | "bit" => TypeKind::Boolean,
        "date" => TypeKind::Date,
        "time" | "time without time zone" => TypeKind::Time,
        "timestamptz" | "timestamp with time zone" | "datetimeoffset" => TypeKind::TimestampTz,
        "timestamp" | "timestamp without time zone" | "datetime" | "datetime2" => {
            TypeKind::Timestamp
        }
        "jsonb" => TypeKind::JsonB,
        "json" => TypeKind::Json,
        "uuid" | "uniqueidentifier" => TypeKind::Uuid,
        _ => TypeKind::Custom,
    };

    let custom = if kind == TypeKind::Custom {
        raw.trim().to_string()
    } else {
        String::new()
    };
    (kind, length, precision, scale, custom)
}

/// Build a design from what the driver reported.
///
/// Ids are assigned from 1: loading replaces the whole design, so nothing
/// older can collide with them.
pub fn from_metadata(schemas: &[TableSchema], foreign_keys: &[ForeignKeyInfo]) -> Vec<DesignTable> {
    let mut tables: Vec<DesignTable> = schemas
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let columns = s
                .columns
                .iter()
                .map(|c| {
                    let (kind, length, precision, scale, custom) = parse_type(&c.data_type);
                    // A single-column unique index is how "unique" reaches us;
                    // the primary key's own index is not a separate constraint.
                    let unique = s.indexes.iter().any(|ix| {
                        ix.is_unique
                            && !ix.is_primary
                            && ix.columns.len() == 1
                            && ix.columns[0].eq_ignore_ascii_case(&c.name)
                    });
                    DesignColumn {
                        base: ColumnRow {
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
                        },
                        unique,
                        ref_table: String::new(),
                        ref_column: String::new(),
                        on_delete: ForeignKeyAction::NoAction,
                    }
                })
                .collect();
            DesignTable {
                id: i as u64 + 1,
                name: s.table.name.clone(),
                schema: String::new(),
                columns,
            }
        })
        .collect();

    for fk in foreign_keys {
        let Some(ti) = find_table(&tables, &fk.table) else {
            continue;
        };
        for (col, referenced) in fk.columns.iter().zip(fk.referenced_columns.iter()) {
            let Some(ci) = find_column(&tables[ti], col) else {
                continue;
            };
            tables[ti].columns[ci].ref_table = fk.referenced_table.clone();
            tables[ti].columns[ci].ref_column = referenced.clone();
        }
    }

    tables
}

// ─── screen state ─────────────────────────────────────────────────────────

/// What the current pointer drag is doing.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Pan,
    Table(u64),
    /// Drawing a foreign key out of one column.
    Connect {
        table: u64,
        column: usize,
    },
}

type Loaded = (Vec<TableSchema>, Vec<ForeignKeyInfo>);

pub struct SchemaDesigner {
    pub open: bool,
    pub connection_id: Option<String>,
    /// Schema the design targets, and the one "Load from database" reads.
    pub schema: String,

    pub tables: Vec<DesignTable>,
    pub selected: Option<u64>,
    pub layout: graph_layout::Layout,
    /// Tables the user has placed, in world coordinates. These override the
    /// computed layout, so an arrangement survives a re-layout of the rest.
    pub pinned: HashMap<u64, egui::Pos2>,

    pub pan: egui::Vec2,
    pub zoom: f32,

    pub preview: Vec<String>,
    pub show_preview: bool,
    pub confirming: bool,
    pub confirm_load: bool,
    pub error: Option<String>,

    pub load_job: Job<Result<Loaded, DbError>>,
    /// Statements applied, and the failure that stopped the rest.
    pub apply_job: Job<(usize, Option<String>)>,

    next_id: u64,
    drag: Drag,
    /// Pointer position in world space while drawing a connection.
    drag_pointer: egui::Pos2,
}

impl Default for SchemaDesigner {
    fn default() -> Self {
        Self {
            open: false,
            connection_id: None,
            schema: String::new(),
            tables: Vec::new(),
            selected: None,
            layout: graph_layout::Layout::default(),
            pinned: HashMap::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            preview: Vec::new(),
            show_preview: false,
            confirming: false,
            confirm_load: false,
            error: None,
            load_job: Job::default(),
            apply_job: Job::default(),
            next_id: 1,
            drag: Drag::None,
            drag_pointer: egui::Pos2::ZERO,
        }
    }
}

const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 18.0;
/// How close to a row's edge a press has to be to start a connection rather
/// than drag the table.
const HANDLE_R: f32 = 7.0;

fn table_size(t: &DesignTable) -> (f32, f32) {
    let longest = t
        .columns
        .iter()
        .map(|c| c.base.name.len() + type_label(c).len() + 6)
        .max()
        .unwrap_or(0)
        .max(display_name(t).len());
    let width = (longest as f32 * 7.0 + 32.0).clamp(160.0, 340.0);
    let height = HEADER_H + t.columns.len() as f32 * ROW_H + 8.0;
    (width, height)
}

impl SchemaDesigner {
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        // Reopening the same target keeps the work in progress; switching
        // connection or schema starts a new design.
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            self.open = true;
            return;
        }
        self.connection_id = Some(c);
        self.schema = s;
        self.open = true;
        self.tables.clear();
        self.pinned.clear();
        self.selected = None;
        self.next_id = 1;
        self.pan = egui::Vec2::ZERO;
        self.zoom = 1.0;
        self.preview.clear();
        self.show_preview = false;
        self.confirming = false;
        self.confirm_load = false;
        self.error = None;
    }

    /// Add an empty table at a cascading position, as the web shell did, so
    /// two new tables never land exactly on top of each other.
    pub fn add_table(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let n = self.tables.len() as f32;
        self.tables.push(DesignTable {
            id,
            name: String::new(),
            schema: String::new(),
            columns: vec![DesignColumn {
                base: ColumnRow {
                    name: "id".into(),
                    type_kind: TypeKind::BigInt,
                    nullable: false,
                    primary_key: true,
                    auto_increment: true,
                    ..Default::default()
                },
                ..Default::default()
            }],
        });
        self.pinned
            .insert(id, egui::pos2(40.0 + n * 48.0, 40.0 + n * 34.0));
        self.selected = Some(id);
        id
    }

    pub fn remove_table(&mut self, id: u64) {
        self.tables.retain(|t| t.id != id);
        self.pinned.remove(&id);
        if self.selected == Some(id) {
            self.selected = None;
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.tables.iter().position(|t| t.id == id)
    }

    /// Recompute positions, dropping manual ones so the result is the tidy
    /// layout the user asked for.
    fn relayout(&mut self) {
        let nodes: Vec<Node> = self
            .tables
            .iter()
            .map(|t| {
                let (width, height) = table_size(t);
                Node {
                    id: t.id.to_string(),
                    width,
                    height,
                }
            })
            .collect();
        // Referenced table first, so parents sit left of their children.
        let edges: Vec<Edge> = relationships(&self.tables)
            .into_iter()
            .map(|r| Edge {
                from: self.tables[r.to].id.to_string(),
                to: self.tables[r.from].id.to_string(),
            })
            .collect();
        self.layout = graph_layout::layout(&nodes, &edges, LayoutOptions::default());
    }

    fn position(&self, id: u64) -> egui::Pos2 {
        if let Some(p) = self.pinned.get(&id) {
            return *p;
        }
        self.layout
            .placements
            .get(&id.to_string())
            .map(|p| egui::pos2(p.x, p.y))
            .unwrap_or(egui::Pos2::ZERO)
    }
}

// ─── screen ───────────────────────────────────────────────────────────────

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();

    if app.screens.schema_designer.connection_id.is_none() {
        ui.label(crate::theme::text::muted(
            palette,
            "Open a connection to design against it",
        ));
        return;
    }

    poll_jobs(app);
    toolbar(app, ui, palette);
    ui.separator();

    egui::SidePanel::left("designer_tables")
        .resizable(true)
        .default_width(190.0)
        .show_inside(ui, |ui| table_list(app, ui, palette));

    egui::SidePanel::right("designer_properties")
        .resizable(true)
        .default_width(320.0)
        .show_inside(ui, |ui| properties(app, ui, palette));

    egui::CentralPanel::default().show_inside(ui, |ui| {
        if app.screens.schema_designer.tables.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::heading(palette, "Empty design"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "Add a table, or load the current schema to start from it.",
                ));
            });
            return;
        }
        canvas(app, ui, palette);
    });

    sql_window(app, ui, palette);
}

fn poll_jobs(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.schema_designer.load_job.take_result() {
        match result {
            Ok((schemas, fks)) => {
                let d = &mut app.screens.schema_designer;
                d.tables = from_metadata(&schemas, &fks);
                d.next_id = d.tables.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                d.pinned.clear();
                d.selected = d.tables.first().map(|t| t.id);
                d.error = None;
                d.relayout();
                let n = d.tables.len();
                app.toasts.success(format!("Loaded {} tables", n));
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.schema_designer.error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some((applied, failure)) = app.screens.schema_designer.apply_job.take_result() {
        match failure {
            None => {
                app.screens.schema_designer.show_preview = false;
                app.screens.schema_designer.error = None;
                app.toasts
                    .success(format!("Applied {} statements", applied));
            }
            Some(msg) => {
                // Say how far it got: DDL is not transactional on every
                // engine, so "it failed" alone leaves the user guessing what
                // is now in the database.
                let text = format!("Stopped after {} statements: {}", applied, msg);
                app.screens.schema_designer.error = Some(text.clone());
                app.toasts.error(text);
            }
        }
    }
}

fn toolbar(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut add = false;
    let mut load = false;
    let mut auto = false;
    let mut fit = false;
    let mut preview = false;
    let mut close = false;
    let mut cancel_load = false;

    let running = app.screens.schema_designer.load_job.is_running();
    let confirm_load = app.screens.schema_designer.confirm_load;
    let schema = app.screens.schema_designer.schema.clone();
    let count = app.screens.schema_designer.tables.len();
    let zoom = app.screens.schema_designer.zoom;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(
            palette,
            format!("Schema Designer · {}", schema),
        ));
        ui.separator();
        if ui.button("Add table").clicked() {
            add = true;
        }
        if confirm_load {
            ui.label(crate::theme::text::strong(
                palette,
                "Replace this design with the live schema?",
            ));
            if ui.button("Yes, load it").clicked() {
                load = true;
            }
            if ui.button("Cancel").clicked() {
                cancel_load = true;
            }
        } else if ui
            .add_enabled(!running, egui::Button::new("Load from database"))
            .on_hover_text("Start from the tables that already exist")
            .clicked()
        {
            load = true;
        }
        if running {
            ui.add(egui::Spinner::new());
        }
        ui.separator();
        if ui.button("Auto layout").clicked() {
            auto = true;
        }
        if ui.button("Fit").clicked() {
            fit = true;
        }
        ui.label(crate::theme::text::muted(
            palette,
            format!("{:.0}%", zoom * 100.0),
        ));
        ui.separator();
        if ui
            .add_enabled(count > 0, egui::Button::new("Preview SQL"))
            .clicked()
        {
            preview = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });

    let d = &mut app.screens.schema_designer;
    if add {
        d.add_table();
    }
    if cancel_load {
        d.confirm_load = false;
    }
    if auto {
        d.pinned.clear();
        d.relayout();
    }
    if fit {
        d.pan = egui::Vec2::ZERO;
        d.zoom = 1.0;
    }
    if preview {
        d.show_preview = true;
        d.confirming = false;
    }
    if close {
        d.open = false;
    }
    if load {
        // A non-empty design would be thrown away, so ask first.
        if app.screens.schema_designer.tables.is_empty()
            || app.screens.schema_designer.confirm_load
        {
            app.screens.schema_designer.confirm_load = false;
            start_load(app, ui.ctx());
        } else {
            app.screens.schema_designer.confirm_load = true;
        }
    }
}

fn table_list(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.label(crate::theme::text::strong(
        palette,
        format!("Tables ({})", app.screens.schema_designer.tables.len()),
    ));
    ui.separator();

    let mut select: Option<u64> = None;
    let mut remove: Option<u64> = None;

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() * 0.55)
        .show(ui, |ui| {
            let d = &app.screens.schema_designer;
            if d.tables.is_empty() {
                ui.label(crate::theme::text::muted(palette, "Nothing here yet"));
            }
            for t in &d.tables {
                ui.horizontal(|ui| {
                    let selected = d.selected == Some(t.id);
                    if ui.selectable_label(selected, display_name(t)).clicked() {
                        select = Some(t.id);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Remove table").clicked() {
                            remove = Some(t.id);
                        }
                    });
                });
            }
        });

    if let Some(id) = select {
        app.screens.schema_designer.selected = Some(id);
    }
    if let Some(id) = remove {
        app.screens.schema_designer.remove_table(id);
    }

    ui.separator();
    let issues = validate(&app.screens.schema_designer.tables);
    if issues.is_empty() {
        ui.label(crate::theme::text::muted(palette, "No issues"));
    } else {
        ui.label(crate::theme::text::strong(
            palette,
            format!("Issues ({})", issues.len()),
        ));
        egui::ScrollArea::vertical()
            .id_salt("designer_issues")
            .show(ui, |ui| {
                for e in &issues {
                    ui.colored_label(palette.destructive, e);
                }
            });
    }
    if let Some(err) = app.screens.schema_designer.error.clone() {
        ui.separator();
        ui.colored_label(palette.destructive, err);
    }
}

fn properties(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.label(crate::theme::text::strong(palette, "Table properties"));
    ui.separator();

    let Some(index) = app
        .screens
        .schema_designer
        .selected
        .and_then(|id| app.screens.schema_designer.index_of(id))
    else {
        ui.label(crate::theme::text::muted(palette, "Select a table to edit"));
        return;
    };

    let mut add_column = false;
    let mut remove_column: Option<usize> = None;
    let mut delete_table = false;
    let default_schema = app.screens.schema_designer.schema.clone();

    {
        let t = &mut app.screens.schema_designer.tables[index];
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut t.name)
                    .hint_text("e.g. users")
                    .desired_width(160.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Schema");
            ui.add(
                egui::TextEdit::singleline(&mut t.schema)
                    .hint_text(default_schema.as_str())
                    .desired_width(160.0),
            );
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(crate::theme::text::strong(
                palette,
                format!("Columns ({})", t.columns.len()),
            ));
            if ui.small_button("Add column").clicked() {
                add_column = true;
            }
        });

        egui::ScrollArea::vertical()
            .id_salt("designer_columns")
            .show(ui, |ui| {
                for (i, col) in t.columns.iter_mut().enumerate() {
                    egui::Frame::none()
                        .fill(palette.card)
                        .stroke(egui::Stroke::new(1.0_f32, palette.border))
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::same(6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut col.base.name)
                                        .hint_text("column_name")
                                        .desired_width(120.0),
                                );
                                egui::ComboBox::from_id_salt(("designer_type", index, i))
                                    .selected_text(col.base.type_kind.label())
                                    .width(140.0)
                                    .show_ui(ui, |ui| {
                                        for kind in TypeKind::ALL {
                                            ui.selectable_value(
                                                &mut col.base.type_kind,
                                                *kind,
                                                kind.label(),
                                            );
                                        }
                                    });
                                if ui.small_button("✕").on_hover_text("Remove column").clicked() {
                                    remove_column = Some(i);
                                }
                            });

                            if col.base.type_kind.takes_length() {
                                ui.add(
                                    egui::DragValue::new(&mut col.base.length)
                                        .range(1..=65535)
                                        .prefix("length "),
                                );
                            }
                            if col.base.type_kind.takes_precision() {
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut col.base.precision)
                                            .range(1..=38)
                                            .prefix("p "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut col.base.scale)
                                            .range(0..=38)
                                            .prefix("s "),
                                    );
                                });
                            }
                            if col.base.type_kind.takes_custom_name() {
                                ui.add(
                                    egui::TextEdit::singleline(&mut col.base.custom)
                                        .hint_text("type name")
                                        .desired_width(140.0),
                                );
                            }

                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut col.base.primary_key, "PK").changed()
                                    && col.base.primary_key
                                {
                                    // Keep the form honest rather than
                                    // generating DDL the server rejects.
                                    col.base.nullable = false;
                                }
                                ui.add_enabled(
                                    !col.base.primary_key,
                                    egui::Checkbox::new(&mut col.base.nullable, "Nullable"),
                                );
                                ui.add_enabled(
                                    !col.base.primary_key,
                                    egui::Checkbox::new(&mut col.unique, "Unique"),
                                );
                                ui.add_enabled(
                                    col.base.type_kind.can_auto_increment(),
                                    egui::Checkbox::new(&mut col.base.auto_increment, "Auto"),
                                );
                            });

                            ui.add(
                                egui::TextEdit::singleline(&mut col.base.default)
                                    .hint_text("default expression")
                                    .desired_width(f32::INFINITY),
                            );

                            ui.horizontal(|ui| {
                                ui.label(crate::theme::text::muted(palette, "references"));
                                ui.add(
                                    egui::TextEdit::singleline(&mut col.ref_table)
                                        .hint_text("table")
                                        .desired_width(100.0),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut col.ref_column)
                                        .hint_text("column")
                                        .desired_width(90.0),
                                );
                            });
                            if col.has_reference() {
                                ui.horizontal(|ui| {
                                    ui.label(crate::theme::text::muted(palette, "on delete"));
                                    egui::ComboBox::from_id_salt(("designer_ondelete", index, i))
                                        .selected_text(fk_action_label(&col.on_delete))
                                        .width(130.0)
                                        .show_ui(ui, |ui| {
                                            for action in FK_ACTIONS {
                                                ui.selectable_value(
                                                    &mut col.on_delete,
                                                    action.clone(),
                                                    fk_action_label(action),
                                                );
                                            }
                                        });
                                    if ui.small_button("Clear").clicked() {
                                        col.ref_table.clear();
                                        col.ref_column.clear();
                                    }
                                });
                            }
                        });
                    ui.add_space(4.0);
                }
            });
    }

    ui.separator();
    if ui.button("Delete this table").clicked() {
        delete_table = true;
    }

    let d = &mut app.screens.schema_designer;
    if add_column {
        d.tables[index].columns.push(DesignColumn::default());
    }
    if let Some(i) = remove_column {
        d.tables[index].columns.remove(i);
    }
    if delete_table {
        let id = d.tables[index].id;
        d.remove_table(id);
    }
}

const FK_ACTIONS: &[ForeignKeyAction] = &[
    ForeignKeyAction::NoAction,
    ForeignKeyAction::Restrict,
    ForeignKeyAction::Cascade,
    ForeignKeyAction::SetNull,
    ForeignKeyAction::SetDefault,
];

fn fk_action_label(a: &ForeignKeyAction) -> &'static str {
    match a {
        ForeignKeyAction::NoAction => "NO ACTION",
        ForeignKeyAction::Restrict => "RESTRICT",
        ForeignKeyAction::Cascade => "CASCADE",
        ForeignKeyAction::SetNull => "SET NULL",
        ForeignKeyAction::SetDefault => "SET DEFAULT",
    }
}

// ─── canvas ───────────────────────────────────────────────────────────────

/// The table under a world-space point, topmost first.
fn table_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<usize> {
    for (i, t) in d.tables.iter().enumerate().rev() {
        let (w, h) = table_size(t);
        if egui::Rect::from_min_size(d.position(t.id), egui::vec2(w, h)).contains(world) {
            return Some(i);
        }
    }
    None
}

/// The column row under a world-space point.
fn column_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<(usize, usize)> {
    let i = table_at(d, world)?;
    let t = &d.tables[i];
    let pos = d.position(t.id);
    let row = ((world.y - pos.y - HEADER_H) / ROW_H).floor();
    if row < 0.0 {
        return None;
    }
    let row = row as usize;
    (row < t.columns.len()).then_some((i, row))
}

/// Where a column's connector sits, in world space.
fn handle_pos(d: &SchemaDesigner, table: usize, column: usize, right: bool) -> egui::Pos2 {
    let t = &d.tables[table];
    let (w, _) = table_size(t);
    let pos = d.position(t.id);
    egui::pos2(
        if right { pos.x + w } else { pos.x },
        pos.y + HEADER_H + column as f32 * ROW_H + ROW_H / 2.0,
    )
}

/// A connector near a world-space point, if the press should start a
/// connection rather than move the table.
fn handle_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<(usize, usize)> {
    for (i, t) in d.tables.iter().enumerate().rev() {
        for c in 0..t.columns.len() {
            for right in [true, false] {
                if handle_pos(d, i, c, right).distance(world) <= HANDLE_R {
                    return Some((i, c));
                }
            }
        }
    }
    None
}

fn canvas(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.background);

    // Intents recorded during interaction, applied once the borrow ends.
    let mut connect_made: Option<(usize, usize, usize, usize)> = None;
    let mut connect_failed: Option<String> = None;

    {
        let d = &mut app.screens.schema_designer;

        // Zoom about the pointer, so the thing under the cursor stays put.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old = d.zoom;
                d.zoom = (d.zoom * (1.0 + scroll * 0.002)).clamp(0.25, 3.0);
                if let Some(pointer) = response.hover_pos() {
                    let anchor = (pointer - rect.min - d.pan) / old;
                    d.pan -= anchor * (d.zoom - old);
                }
            }
        }

        let to_world =
            |p: egui::Pos2, d: &SchemaDesigner| (p - rect.min.to_vec2() - d.pan) / d.zoom;

        if response.drag_started() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let w = to_world(pointer, d);
                let world = egui::pos2(w.x, w.y);
                d.drag_pointer = world;
                d.drag = if let Some((ti, ci)) = handle_at(d, world) {
                    d.selected = Some(d.tables[ti].id);
                    Drag::Connect {
                        table: d.tables[ti].id,
                        column: ci,
                    }
                } else if let Some(ti) = table_at(d, world) {
                    d.selected = Some(d.tables[ti].id);
                    Drag::Table(d.tables[ti].id)
                } else {
                    Drag::Pan
                };
            }
        }

        if response.dragged() {
            match d.drag {
                Drag::Pan => d.pan += response.drag_delta(),
                Drag::Table(id) => {
                    let delta = response.drag_delta() / d.zoom;
                    let current = d.position(id);
                    d.pinned.insert(id, current + delta);
                }
                Drag::Connect { .. } => {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let w = to_world(pointer, d);
                        d.drag_pointer = egui::pos2(w.x, w.y);
                    }
                }
                Drag::None => {}
            }
        }

        if response.drag_stopped() {
            if let Drag::Connect { table, column } = d.drag {
                let source = d.index_of(table);
                let target = column_at(d, d.drag_pointer);
                match (source, target) {
                    (Some(si), Some((ti, ci))) if si != ti => {
                        if d.tables[ti].name.trim().is_empty() {
                            connect_failed =
                                Some("Name the target table before referencing it".into());
                        } else if d.tables[ti].columns[ci].base.name.trim().is_empty() {
                            connect_failed =
                                Some("Name the target column before referencing it".into());
                        } else {
                            connect_made = Some((si, column, ti, ci));
                        }
                    }
                    (Some(si), Some((ti, _))) if si == ti => {
                        connect_failed =
                            Some("A column cannot reference its own table this way".into());
                    }
                    _ => {}
                }
            }
            d.drag = Drag::None;
        }

        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let w = to_world(pointer, d);
                let world = egui::pos2(w.x, w.y);
                d.selected = table_at(d, world).map(|i| d.tables[i].id);
            }
        }
    }

    if let Some((si, sc, ti, tc)) = connect_made {
        let d = &mut app.screens.schema_designer;
        let (rt, rc) = (
            d.tables[ti].name.trim().to_string(),
            d.tables[ti].columns[tc].base.name.trim().to_string(),
        );
        d.tables[si].columns[sc].ref_table = rt.clone();
        d.tables[si].columns[sc].ref_column = rc.clone();
        // Say why it did not take rather than silently dropping the gesture.
        if let Err(problem) = check_reference(&d.tables, si, sc) {
            let msg = problem.message();
            d.tables[si].columns[sc].ref_table.clear();
            d.tables[si].columns[sc].ref_column.clear();
            app.toasts.error(format!("Cannot link: {}", msg));
        } else {
            app.toasts
                .success(format!("Linked to {}.{}", rt, rc));
        }
    }
    if let Some(msg) = connect_failed {
        app.toasts.error(msg);
    }

    draw(&app.screens.schema_designer, &painter, rect, palette);
}

fn draw(d: &SchemaDesigner, painter: &egui::Painter, rect: egui::Rect, palette: &Palette) {
    let to_screen = |p: egui::Pos2| rect.min + d.pan + p.to_vec2() * d.zoom;
    let selected_index = d.selected.and_then(|id| d.index_of(id));

    // Relationships first, so the boxes sit on top of the lines.
    for r in relationships(&d.tables) {
        let a = to_screen(handle_pos(d, r.from, r.from_column, true));
        let b = to_screen(handle_pos(d, r.to, r.to_column, false));
        let involved = selected_index == Some(r.from) || selected_index == Some(r.to);
        let colour = if involved {
            palette.primary
        } else {
            palette.muted_foreground
        };
        bezier(painter, a, b, colour, if involved { 2.0 } else { 1.0 });
        painter.circle_filled(b, 3.0 * d.zoom.min(1.5), colour);
    }

    // The connection being drawn right now.
    if let Drag::Connect { table, column } = d.drag {
        if let Some(ti) = d.index_of(table) {
            let a = to_screen(handle_pos(d, ti, column, true));
            let b = to_screen(d.drag_pointer);
            bezier(painter, a, b, palette.primary, 2.0);
        }
    }

    for (i, t) in d.tables.iter().enumerate() {
        let (w, h) = table_size(t);
        let top_left = to_screen(d.position(t.id));
        let size = egui::vec2(w, h) * d.zoom;
        let box_rect = egui::Rect::from_min_size(top_left, size);
        if !rect.intersects(box_rect) {
            continue; // off-screen, skip the text shaping entirely
        }

        let selected = selected_index == Some(i);
        painter.rect_filled(box_rect, 6.0, palette.card);
        painter.rect_stroke(
            box_rect,
            6.0,
            egui::Stroke::new(
                if selected { 2.0_f32 } else { 1.0 },
                if selected {
                    palette.primary
                } else {
                    palette.border
                },
            ),
        );

        let header = egui::Rect::from_min_size(top_left, egui::vec2(size.x, HEADER_H * d.zoom));
        painter.rect_filled(header, 6.0, palette.secondary);
        painter.text(
            header.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            display_name(t),
            egui::FontId::proportional(13.0 * d.zoom),
            palette.foreground,
        );

        for (ci, col) in t.columns.iter().enumerate() {
            let y = top_left.y + (HEADER_H + ci as f32 * ROW_H + ROW_H / 2.0) * d.zoom;
            let name = if col.base.name.trim().is_empty() {
                "…".to_string()
            } else {
                col.base.name.trim().to_string()
            };
            let marker = if col.base.primary_key {
                "PK "
            } else if col.has_reference() {
                "FK "
            } else {
                ""
            };
            painter.text(
                egui::pos2(top_left.x + 8.0 * d.zoom, y),
                egui::Align2::LEFT_CENTER,
                format!("{}{}", marker, name),
                egui::FontId::proportional(11.0 * d.zoom),
                if col.base.primary_key {
                    palette.primary
                } else {
                    palette.foreground
                },
            );

            let mut suffix = type_label(col);
            if col.unique && !col.base.primary_key {
                suffix.push_str(" UNQ");
            }
            if !col.base.nullable && !col.base.primary_key {
                suffix.push_str(" NN");
            }
            painter.text(
                egui::pos2(top_left.x + size.x - 8.0 * d.zoom, y),
                egui::Align2::RIGHT_CENTER,
                suffix,
                egui::FontId::proportional(10.0 * d.zoom),
                palette.muted_foreground,
            );

            // Connectors: what the user drags between to draw a foreign key.
            for right in [true, false] {
                let p = to_screen(handle_pos(d, i, ci, right));
                painter.circle_filled(p, 3.0 * d.zoom.min(1.5), palette.primary);
            }
        }
    }

    minimap(d, painter, rect, palette);
}

fn bezier(
    painter: &egui::Painter,
    a: egui::Pos2,
    b: egui::Pos2,
    colour: egui::Color32,
    width: f32,
) {
    // A horizontal-tangent bezier reads better than a straight line when
    // layers stack, and it makes the direction obvious without arrowheads.
    let dx = ((b.x - a.x).abs() * 0.4).max(24.0);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + egui::vec2(dx, 0.0), b - egui::vec2(dx, 0.0), b],
        false,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(width, colour),
    ));
}

/// A corner overview with the current viewport marked, so a design that has
/// been panned off-screen can still be found.
fn minimap(d: &SchemaDesigner, painter: &egui::Painter, rect: egui::Rect, palette: &Palette) {
    if d.tables.is_empty() {
        return;
    }
    let mut bounds: Option<egui::Rect> = None;
    for t in &d.tables {
        let (w, h) = table_size(t);
        let r = egui::Rect::from_min_size(d.position(t.id), egui::vec2(w, h));
        bounds = Some(match bounds {
            Some(b) => b.union(r),
            None => r,
        });
    }
    let Some(world) = bounds else { return };
    // Include the viewport, so panning away still shows where the design is.
    let view = egui::Rect::from_min_size(
        egui::pos2(-d.pan.x / d.zoom, -d.pan.y / d.zoom),
        rect.size() / d.zoom,
    );
    let world = world.union(view).expand(20.0);

    let panel = egui::Rect::from_min_size(
        rect.right_bottom() - egui::vec2(172.0, 122.0),
        egui::vec2(160.0, 110.0),
    );
    if !rect.contains_rect(panel) {
        return; // canvas too small to spare the space
    }
    painter.rect_filled(panel, 4.0, palette.popover);
    painter.rect_stroke(panel, 4.0, egui::Stroke::new(1.0_f32, palette.border));

    let scale = (panel.width() / world.width()).min(panel.height() / world.height());
    let map = |p: egui::Pos2| panel.min + (p - world.min) * scale;

    for t in &d.tables {
        let (w, h) = table_size(t);
        let r = egui::Rect::from_min_size(map(d.position(t.id)), egui::vec2(w, h) * scale);
        painter.rect_filled(r, 1.0, palette.muted_foreground);
    }
    let vr = egui::Rect::from_min_size(map(view.min), view.size() * scale);
    painter.rect_stroke(vr, 1.0, egui::Stroke::new(1.0_f32, palette.primary));
}

// ─── SQL preview and apply ────────────────────────────────────────────────

fn sql_window(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    if !app.screens.schema_designer.show_preview {
        return;
    }

    let Some(connection_id) = app.screens.schema_designer.connection_id.clone() else {
        return;
    };
    let driver_kind = app
        .state
        .lock()
        .unwrap()
        .get_profile(&connection_id)
        .map(|p| p.driver.clone());

    // Regenerate every frame the window is open, so edits behind it can never
    // leave a stale statement on screen next to an Apply button.
    let issues = validate(&app.screens.schema_designer.tables);
    let mut generation_error: Option<String> = None;
    {
        let d = &mut app.screens.schema_designer;
        if issues.is_empty() {
            match driver_kind.as_ref() {
                Some(driver) => match generate_sql(driver, &d.schema, &d.tables) {
                    Ok(sql) => d.preview = sql,
                    Err(e) => {
                        d.preview.clear();
                        generation_error = Some(e);
                    }
                },
                None => {
                    d.preview.clear();
                    generation_error = Some("This connection has no profile any more".into());
                }
            }
        } else {
            d.preview.clear();
        }
    }

    let statements = app.screens.schema_designer.preview.clone();
    let confirming = app.screens.schema_designer.confirming;
    let running = app.screens.schema_designer.apply_job.is_running();

    let mut open = true;
    let mut set_confirming: Option<bool> = None;
    let mut start = false;

    egui::Window::new("Generated SQL")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(680.0)
        .show(ui.ctx(), |ui| {
            for e in &issues {
                ui.colored_label(palette.destructive, e);
            }
            if let Some(e) = &generation_error {
                ui.colored_label(palette.destructive, e);
            }
            if statements.is_empty() {
                if issues.is_empty() && generation_error.is_none() {
                    ui.label(crate::theme::text::muted(palette, "Nothing to run"));
                }
            } else {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!(
                        "{} statements, referenced tables first",
                        statements.len()
                    ),
                ));
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for stmt in &statements {
                            ui.label(
                                egui::RichText::new(stmt)
                                    .monospace()
                                    .color(palette.foreground),
                            );
                            ui.add_space(4.0);
                        }
                    });
            }

            ui.separator();
            ui.horizontal(|ui| {
                let ready = issues.is_empty() && !statements.is_empty() && !running;
                if confirming {
                    ui.label(crate::theme::text::strong(
                        palette,
                        "Run this DDL? It cannot be undone.",
                    ));
                    if ui.button("Yes, apply it").clicked() {
                        set_confirming = Some(false);
                        start = true;
                    }
                    if ui.button("Cancel").clicked() {
                        set_confirming = Some(false);
                    }
                } else if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(format!("Apply {} statements", statements.len())),
                    )
                    .clicked()
                {
                    set_confirming = Some(true);
                }
                if running {
                    ui.add(egui::Spinner::new());
                }
            });
        });

    if let Some(v) = set_confirming {
        app.screens.schema_designer.confirming = v;
    }
    if !open {
        app.screens.schema_designer.show_preview = false;
        app.screens.schema_designer.confirming = false;
    }
    if start {
        apply(app, ui.ctx());
    }
}

fn start_load(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(connection_id) = app.screens.schema_designer.connection_id.clone() else {
        return;
    };
    let schema = app.screens.schema_designer.schema.clone();

    // Clone the driver out and drop the guard: nothing may hold the lock
    // across an await.
    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.schema_designer.error = Some("Connection is no longer open".into());
        return;
    };

    let handle = app.rt.handle().clone();
    app.screens
        .schema_designer
        .load_job
        .start(&handle, ctx, async move {
            let tables = driver.get_tables(&schema).await?;
            let foreign_keys = driver.get_foreign_keys(&schema).await.unwrap_or_default();

            // A table whose schema cannot be read is skipped rather than
            // failing the whole load: one unreadable view should not stop the
            // user designing against the other forty tables.
            let mut out = Vec::with_capacity(tables.len());
            for t in tables {
                if let Ok(s) = driver.get_table_schema(&schema, &t.name).await {
                    out.push(s);
                }
            }
            Ok((out, foreign_keys))
        });
}

fn apply(app: &mut DbHiveApp, ctx: &egui::Context) {
    let statements = app.screens.schema_designer.preview.clone();
    if statements.is_empty() {
        return;
    }
    let Some(connection_id) = app.screens.schema_designer.connection_id.clone() else {
        return;
    };
    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.schema_designer.error = Some("Connection is no longer open".into());
        return;
    };

    let handle = app.rt.handle().clone();
    app.screens
        .schema_designer
        .apply_job
        .start(&handle, ctx, async move {
            // In order, stopping at the first failure: a child table created
            // after its parent failed would only produce a second, confusing
            // error.
            let mut applied = 0usize;
            for stmt in statements {
                if let Err(e) = driver.execute_query(&stmt).await {
                    return (applied, Some(e.to_string()));
                }
                applied += 1;
            }
            (applied, None)
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{ColumnInfo, IndexInfo, TableInfo};

    fn col(name: &str, kind: TypeKind) -> DesignColumn {
        DesignColumn {
            base: ColumnRow {
                name: name.into(),
                type_kind: kind,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn pk(name: &str, kind: TypeKind) -> DesignColumn {
        let mut c = col(name, kind);
        c.base.primary_key = true;
        c.base.nullable = false;
        c
    }

    fn table(id: u64, name: &str, columns: Vec<DesignColumn>) -> DesignTable {
        DesignTable {
            id,
            name: name.into(),
            schema: String::new(),
            columns,
        }
    }

    /// users(id PK) and orders(id PK, user_id -> users.id).
    fn users_and_orders() -> Vec<DesignTable> {
        let users = table(1, "users", vec![pk("id", TypeKind::BigInt)]);
        let mut fk = col("user_id", TypeKind::BigInt);
        fk.ref_table = "users".into();
        fk.ref_column = "id".into();
        let orders = table(2, "orders", vec![pk("id", TypeKind::BigInt), fk]);
        vec![users, orders]
    }

    // ── relationship validity ────────────────────────────────────────────

    #[test]
    fn a_reference_to_an_existing_primary_key_resolves() {
        let tables = users_and_orders();
        assert_eq!(check_reference(&tables, 1, 1), Ok(Some((0, 0))));
        assert_eq!(
            relationships(&tables),
            vec![Relationship {
                from: 1,
                from_column: 1,
                to: 0,
                to_column: 0
            }]
        );
    }

    #[test]
    fn a_column_with_no_reference_is_not_a_relationship() {
        let tables = vec![table(1, "users", vec![pk("id", TypeKind::BigInt)])];
        assert_eq!(check_reference(&tables, 0, 0), Ok(None));
        assert!(relationships(&tables).is_empty());
    }

    #[test]
    fn a_reference_to_a_missing_table_is_rejected() {
        let mut tables = users_and_orders();
        tables[1].columns[1].ref_table = "customers".into();
        assert_eq!(
            check_reference(&tables, 1, 1),
            Err(RefProblem::UnknownTable("customers".into()))
        );
        assert!(relationships(&tables).is_empty());
    }

    #[test]
    fn a_reference_to_a_missing_column_is_rejected() {
        let mut tables = users_and_orders();
        tables[1].columns[1].ref_column = "uuid".into();
        assert!(matches!(
            check_reference(&tables, 1, 1),
            Err(RefProblem::UnknownColumn { .. })
        ));
    }

    #[test]
    fn half_a_reference_is_rejected() {
        let mut tables = users_and_orders();
        tables[1].columns[1].ref_column.clear();
        assert_eq!(check_reference(&tables, 1, 1), Err(RefProblem::Incomplete));
    }

    #[test]
    fn a_reference_across_type_families_is_rejected() {
        let mut tables = users_and_orders();
        tables[1].columns[1].base.type_kind = TypeKind::Text;
        assert!(matches!(
            check_reference(&tables, 1, 1),
            Err(RefProblem::IncompatibleTypes { .. })
        ));
    }

    #[test]
    fn integer_widths_do_not_have_to_match() {
        // A plain integer referencing a bigserial key is ordinary and legal.
        let mut tables = users_and_orders();
        tables[1].columns[1].base.type_kind = TypeKind::Integer;
        assert!(check_reference(&tables, 1, 1).is_ok());
    }

    #[test]
    fn a_reference_to_a_non_unique_column_is_rejected() {
        let mut tables = users_and_orders();
        tables[0].columns[0].base.primary_key = false;
        assert!(matches!(
            check_reference(&tables, 1, 1),
            Err(RefProblem::TargetNotUnique { .. })
        ));

        // Marking it unique instead of a primary key is enough.
        tables[0].columns[0].unique = true;
        assert!(check_reference(&tables, 1, 1).is_ok());
    }

    #[test]
    fn custom_types_only_match_the_same_named_type() {
        let mut a = col("a", TypeKind::Custom);
        a.base.custom = "citext".into();
        let mut b = pk("b", TypeKind::Custom);
        b.base.custom = "CITEXT".into();
        assert!(types_compatible(&a, &b));

        b.base.custom = "hstore".into();
        assert!(!types_compatible(&a, &b));

        // An unnamed custom type is unknowable, so nothing matches it.
        let mut blank = col("c", TypeKind::Custom);
        blank.base.custom = String::new();
        assert!(!types_compatible(&blank, &blank.clone()));

        // A custom type never silently matches a built-in one.
        assert!(!types_compatible(&a, &pk("d", TypeKind::Text)));
    }

    #[test]
    fn family_groups_widths_together_and_keeps_kinds_apart() {
        assert_eq!(family(TypeKind::SmallInt), family(TypeKind::BigInt));
        assert_eq!(family(TypeKind::Varchar), family(TypeKind::Text));
        assert_eq!(family(TypeKind::Timestamp), family(TypeKind::TimestampTz));
        assert_ne!(family(TypeKind::Integer), family(TypeKind::Decimal));
        assert_ne!(family(TypeKind::Uuid), family(TypeKind::Text));
    }

    // ── ordering and cycles ──────────────────────────────────────────────

    #[test]
    fn creation_order_puts_the_referenced_table_first() {
        let mut tables = users_and_orders();
        tables.swap(0, 1); // orders first, so insertion order cannot be it
        let order = creation_order(&tables).unwrap();
        let names: Vec<String> = order.iter().map(|i| tables[*i].name.clone()).collect();
        assert_eq!(names, vec!["users", "orders"]);
    }

    #[test]
    fn creation_order_covers_every_table_exactly_once() {
        let tables = users_and_orders();
        let mut order = creation_order(&tables).unwrap();
        order.sort_unstable();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn a_reference_cycle_is_detected_and_named() {
        let mut a = table(1, "a", vec![pk("id", TypeKind::BigInt), col("b_id", TypeKind::BigInt)]);
        a.columns[1].ref_table = "b".into();
        a.columns[1].ref_column = "id".into();
        let mut b = table(2, "b", vec![pk("id", TypeKind::BigInt), col("a_id", TypeKind::BigInt)]);
        b.columns[1].ref_table = "a".into();
        b.columns[1].ref_column = "id".into();
        let tables = vec![a, b];

        let stuck = creation_order(&tables).unwrap_err();
        assert_eq!(stuck.len(), 2);
        assert!(stuck.contains(&"a".to_string()) && stuck.contains(&"b".to_string()));

        assert!(validate(&tables).iter().any(|e| e.contains("cycle")));
        assert!(generate_sql(&DbDriver::Postgres, "public", &tables).is_err());
    }

    #[test]
    fn a_self_reference_is_not_a_cycle() {
        // employees.manager_id -> employees.id is ordinary.
        let mut t = table(
            1,
            "employees",
            vec![pk("id", TypeKind::BigInt), col("manager_id", TypeKind::BigInt)],
        );
        t.columns[1].ref_table = "employees".into();
        t.columns[1].ref_column = "id".into();
        let tables = vec![t];
        assert_eq!(creation_order(&tables).unwrap(), vec![0]);
        assert!(!validate(&tables).iter().any(|e| e.contains("cycle")));
    }

    // ── design -> TableDefinition ────────────────────────────────────────

    #[test]
    fn definition_carries_keys_constraints_and_references() {
        let tables = users_and_orders();
        let def = build_definition("public", &tables[1]);

        assert_eq!(def.name, "orders");
        assert_eq!(def.schema, Some("public".into()));
        assert_eq!(def.primary_key, Some(vec!["id".to_string()]));
        assert_eq!(def.foreign_keys.len(), 1);
        assert_eq!(def.foreign_keys[0].columns, vec!["user_id".to_string()]);
        assert_eq!(def.foreign_keys[0].referenced_table, "users");
        assert_eq!(
            def.foreign_keys[0].referenced_columns,
            vec!["id".to_string()]
        );
        assert!(def.foreign_keys[0].name.is_none(), "the dialect names it");
        assert!(!def.if_not_exists, "a silent no-op would hide a mistake");
    }

    #[test]
    fn definition_forces_primary_keys_not_null() {
        let mut t = table(1, "users", vec![pk("id", TypeKind::BigInt)]);
        t.columns[0].base.nullable = true; // the form must not win this
        let def = build_definition("public", &t);
        assert!(!def.columns[0].nullable);
    }

    #[test]
    fn definition_prefers_the_tables_own_schema_and_trims() {
        let mut t = table(1, "  users  ", vec![pk("  id  ", TypeKind::BigInt)]);
        t.schema = " analytics ".into();
        let def = build_definition("public", &t);
        assert_eq!(def.schema, Some("analytics".into()));
        assert_eq!(def.name, "users");
        assert_eq!(def.columns[0].name, "id");

        // With neither, no schema is emitted rather than an empty one.
        let mut plain = table(1, "users", vec![pk("id", TypeKind::BigInt)]);
        plain.schema = String::new();
        assert_eq!(build_definition("", &plain).schema, None);
    }

    #[test]
    fn a_unique_flag_becomes_a_constraint_but_never_duplicates_the_key() {
        let mut t = table(
            1,
            "users",
            vec![pk("id", TypeKind::BigInt), col("email", TypeKind::Text)],
        );
        t.columns[0].unique = true; // already the primary key
        t.columns[1].unique = true;
        let def = build_definition("public", &t);
        assert_eq!(def.unique_constraints.len(), 1);
        assert_eq!(def.unique_constraints[0].columns, vec!["email".to_string()]);
    }

    #[test]
    fn an_empty_default_is_dropped_rather_than_emitted() {
        let mut t = table(1, "users", vec![pk("id", TypeKind::BigInt)]);
        t.columns[0].base.default = "   ".into();
        assert_eq!(build_definition("public", &t).columns[0].default, None);
    }

    // ── validation ───────────────────────────────────────────────────────

    #[test]
    fn an_empty_design_is_rejected() {
        assert!(validate(&[]).iter().any(|e| e.contains("at least one table")));
    }

    #[test]
    fn a_valid_design_has_no_issues() {
        assert!(validate(&users_and_orders()).is_empty());
    }

    #[test]
    fn duplicate_table_names_are_rejected_case_insensitively() {
        let tables = vec![
            table(1, "users", vec![pk("id", TypeKind::BigInt)]),
            table(2, "USERS", vec![pk("id", TypeKind::BigInt)]),
        ];
        assert!(validate(&tables)
            .iter()
            .any(|e| e.contains("Duplicate table name")));
    }

    #[test]
    fn per_column_problems_are_reported_against_their_table() {
        let mut tables = users_and_orders();
        tables[1].columns.push(col("", TypeKind::Text));
        assert!(validate(&tables)
            .iter()
            .any(|e| e.starts_with("orders: ") && e.contains("needs a name")));
    }

    #[test]
    fn a_broken_reference_is_reported_and_not_drawn() {
        let mut tables = users_and_orders();
        tables[1].columns[1].ref_table = "nope".into();
        assert!(validate(&tables)
            .iter()
            .any(|e| e.contains("orders.user_id") && e.contains("unknown table")));
        assert!(relationships(&tables).is_empty());
    }

    // ── DDL generation ───────────────────────────────────────────────────

    #[test]
    fn generated_ddl_creates_the_referenced_table_first() {
        let mut tables = users_and_orders();
        tables.swap(0, 1); // orders listed first
        let sql = generate_sql(&DbDriver::Postgres, "public", &tables).unwrap();
        let joined = sql.join("\n");

        let users_at = joined.find("\"users\"").expect("users missing");
        let orders_at = joined.find("\"orders\"").expect("orders missing");
        assert!(
            users_at < orders_at,
            "orders references users, so users must be created first:\n{}",
            joined
        );
        assert!(joined.contains("REFERENCES"), "{}", joined);
    }

    #[test]
    fn generated_ddl_follows_the_dialect() {
        let tables = users_and_orders();
        let pg = generate_sql(&DbDriver::Postgres, "public", &tables)
            .unwrap()
            .join("\n");
        assert!(pg.to_uppercase().contains("CREATE TABLE"), "{}", pg);

        let my = generate_sql(&DbDriver::MySql, "", &tables)
            .unwrap()
            .join("\n");
        assert!(my.to_uppercase().contains("CREATE TABLE"), "{}", my);
        assert_ne!(pg, my, "two dialects produced identical SQL");
    }

    #[test]
    fn a_driver_without_ddl_reports_rather_than_guessing() {
        let tables = users_and_orders();
        assert!(generate_sql(&DbDriver::Redis, "", &tables).is_err());
    }

    // ── loading an existing schema ───────────────────────────────────────

    #[test]
    fn parse_type_keeps_parameters() {
        assert_eq!(parse_type("VARCHAR(50)").0, TypeKind::Varchar);
        assert_eq!(parse_type("VARCHAR(50)").1, 50);

        let (kind, _, precision, scale, _) = parse_type("numeric(12,4)");
        assert_eq!(kind, TypeKind::Decimal);
        assert_eq!((precision, scale), (12, 4));
    }

    #[test]
    fn parse_type_distinguishes_near_identical_names() {
        assert_eq!(parse_type("timestamp with time zone").0, TypeKind::TimestampTz);
        assert_eq!(parse_type("timestamp").0, TypeKind::Timestamp);
        assert_eq!(parse_type("jsonb").0, TypeKind::JsonB);
        assert_eq!(parse_type("json").0, TypeKind::Json);
        assert_eq!(parse_type("double precision").0, TypeKind::DoublePrecision);
        // MySQL spells a boolean this way, and only this way.
        assert_eq!(parse_type("tinyint(1)").0, TypeKind::Boolean);
        assert_eq!(parse_type("tinyint(4)").0, TypeKind::Integer);
    }

    #[test]
    fn an_unknown_type_survives_as_itself() {
        let (kind, _, _, _, custom) = parse_type("geography(Point,4326)");
        assert_eq!(kind, TypeKind::Custom);
        assert_eq!(custom, "geography(Point,4326)");
    }

    fn schema_of(name: &str, columns: Vec<ColumnInfo>, indexes: Vec<IndexInfo>) -> TableSchema {
        TableSchema {
            table: TableInfo {
                name: name.into(),
                schema: "public".into(),
                row_count: None,
                table_type: "TABLE".into(),
            },
            columns,
            indexes,
        }
    }

    fn info(name: &str, ty: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable: !pk,
            default_value: None,
            is_primary_key: pk,
            is_auto_increment: false,
        }
    }

    #[test]
    fn from_metadata_rebuilds_keys_uniques_and_foreign_keys() {
        let schemas = vec![
            schema_of("users", vec![info("id", "bigint", true)], vec![]),
            schema_of(
                "orders",
                vec![
                    info("id", "bigint", true),
                    info("user_id", "bigint", false),
                    info("code", "varchar(20)", false),
                ],
                vec![IndexInfo {
                    name: "orders_code_key".into(),
                    columns: vec!["code".into()],
                    is_unique: true,
                    is_primary: false,
                }],
            ),
        ];
        let fks = vec![ForeignKeyInfo {
            name: "orders_user_id_fkey".into(),
            table: "orders".into(),
            schema: "public".into(),
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }];

        let tables = from_metadata(&schemas, &fks);
        assert_eq!(tables.len(), 2);
        assert!(tables[0].id != tables[1].id, "ids must be distinct");

        assert!(tables[1].columns[1].has_reference());
        assert_eq!(tables[1].columns[1].ref_table, "users");
        assert_eq!(tables[1].columns[1].ref_column, "id");
        assert!(tables[1].columns[2].unique, "unique index was not carried over");
        assert_eq!(tables[1].columns[2].base.length, 20);

        // The primary key must not come back nullable.
        assert!(tables[0].columns[0].base.primary_key);
        assert!(!tables[0].columns[0].base.nullable);

        // A round trip through the loader produces a design that validates
        // and generates.
        assert!(validate(&tables).is_empty(), "{:?}", validate(&tables));
        let sql = generate_sql(&DbDriver::Postgres, "public", &tables).unwrap();
        let joined = sql.join("\n");
        assert!(joined.find("\"users\"").unwrap() < joined.find("\"orders\"").unwrap());
    }

    #[test]
    fn from_metadata_ignores_foreign_keys_it_cannot_place() {
        let schemas = vec![schema_of("orders", vec![info("id", "bigint", true)], vec![])];
        let fks = vec![ForeignKeyInfo {
            name: "dangling".into(),
            table: "missing_table".into(),
            schema: "public".into(),
            columns: vec!["x".into()],
            referenced_table: "users".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }];
        let tables = from_metadata(&schemas, &fks);
        assert!(tables[0].columns.iter().all(|c| !c.has_reference()));
    }

    // ── screen state ─────────────────────────────────────────────────────

    #[test]
    fn new_tables_get_distinct_ids_and_cascading_positions() {
        let mut d = SchemaDesigner::default();
        let a = d.add_table();
        let b = d.add_table();
        assert_ne!(a, b);
        assert_ne!(d.position(a), d.position(b), "new tables landed on top of each other");
        assert_eq!(d.selected, Some(b));
    }

    #[test]
    fn removing_a_table_clears_its_position_and_selection() {
        let mut d = SchemaDesigner::default();
        let a = d.add_table();
        d.remove_table(a);
        assert!(d.tables.is_empty());
        assert!(d.pinned.is_empty());
        assert_eq!(d.selected, None);
    }

    #[test]
    fn a_manual_position_overrides_the_computed_layout() {
        let mut d = SchemaDesigner::default();
        d.tables = users_and_orders();
        d.next_id = 3;
        d.relayout();
        let auto = d.position(1);
        d.pinned.insert(1, egui::pos2(500.0, 500.0));
        assert_ne!(auto, d.position(1));
        assert_eq!(d.position(1), egui::pos2(500.0, 500.0));
    }

    #[test]
    fn relayout_places_the_referenced_table_in_an_earlier_layer() {
        let mut d = SchemaDesigner::default();
        d.tables = users_and_orders();
        d.relayout();
        assert_eq!(d.layout.placements.len(), 2);
        assert!(
            d.layout.placements["1"].layer < d.layout.placements["2"].layer,
            "users is referenced by orders, so it should be laid out first"
        );
    }

    #[test]
    fn reopening_the_same_target_keeps_the_work_in_progress() {
        let mut d = SchemaDesigner::default();
        d.open_for("conn", "public");
        d.add_table();
        d.open_for("conn", "public");
        assert_eq!(d.tables.len(), 1, "a no-op reopen discarded the design");

        // A different schema is a different design, so it does reset.
        d.open_for("conn", "analytics");
        assert!(d.tables.is_empty());
    }
}
