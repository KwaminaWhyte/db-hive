//! Lifted from the egui `schema_designer` screen. See lib.rs.
//!
//! The pure half of the visual schema designer: the design model,
//! relationship validity, type families, cycle detection and creation order,
//! the mapping onto `TableDefinition`s and DDL generation, loading a design
//! back from driver metadata, and canvas geometry (table sizing, pinned
//! positions, hit testing) in plain world coordinates.
//!
//! The design is turned into `TableDefinition`s and handed to
//! `dbhive_core::ddl`, which knows every dialect; no SQL is ever built here by
//! concatenation. Drawing, the drag state machine and the jobs stay in the
//! shell.

use std::collections::HashMap;

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::{
    ColumnDefinition, DbDriver, ForeignKeyAction, ForeignKeyConstraint, ForeignKeyInfo,
    TableDefinition, TableSchema, UniqueConstraint,
};

use crate::er_diagram::Point;
use crate::graph_layout::{self, Edge, LayoutOptions, Node};
use crate::table_editor::{column_type, ColumnRow, TypeKind};

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
    pub fn has_reference(&self) -> bool {
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
    UnknownColumn {
        table: String,
        column: String,
    },
    IncompatibleTypes {
        from: String,
        to: String,
    },
    /// SQL requires the referenced column to be a primary key or unique.
    TargetNotUnique {
        table: String,
        column: String,
    },
}

impl RefProblem {
    pub fn message(&self) -> String {
        match self {
            RefProblem::Incomplete => {
                "a foreign key needs both a referenced table and column".into()
            }
            RefProblem::UnknownTable(t) => format!("references unknown table \"{}\"", t),
            RefProblem::UnknownColumn { table, column } => {
                format!(
                    "references \"{}\" which table \"{}\" does not have",
                    column, table
                )
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
            && from
                .base
                .custom
                .trim()
                .eq_ignore_ascii_case(to.base.custom.trim())
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
pub fn type_label(c: &DesignColumn) -> String {
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
        for e in crate::table_editor::validate(&t.name, &rows) {
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
    let order = creation_order(tables)
        .map_err(|stuck| format!("Foreign keys form a cycle between {}", stuck.join(", ")))?;
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

// ─── foreign key actions ──────────────────────────────────────────────────

pub const FK_ACTIONS: &[ForeignKeyAction] = &[
    ForeignKeyAction::NoAction,
    ForeignKeyAction::Restrict,
    ForeignKeyAction::Cascade,
    ForeignKeyAction::SetNull,
    ForeignKeyAction::SetDefault,
];

pub fn fk_action_label(a: &ForeignKeyAction) -> &'static str {
    match a {
        ForeignKeyAction::NoAction => "NO ACTION",
        ForeignKeyAction::Restrict => "RESTRICT",
        ForeignKeyAction::Cascade => "CASCADE",
        ForeignKeyAction::SetNull => "SET NULL",
        ForeignKeyAction::SetDefault => "SET DEFAULT",
    }
}

// ─── editing ──────────────────────────────────────────────────────────────

/// A fresh table: unnamed, with a `bigint` auto-increment primary key `id`.
pub fn new_table(id: u64) -> DesignTable {
    DesignTable {
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
    }
}

/// Where the `n`th new table lands (0-based count of tables before it), at a
/// cascading position as the web shell did, so two new tables never land
/// exactly on top of each other.
pub fn cascade_position(n: usize) -> Point {
    let n = n as f32;
    Point::new(40.0 + n * 48.0, 40.0 + n * 34.0)
}

/// Decide what a finished connector drag does.
///
/// `source` is the index of the table the drag started from (if it still
/// exists) and `source_column` its column; `target` is the column row under
/// the pointer on release. `None` means the gesture does nothing,
/// `Some(Ok((si, sc, ti, tc)))` a link to make, `Some(Err(msg))` a gesture
/// to refuse with a message.
#[allow(clippy::type_complexity)]
pub fn resolve_connection(
    tables: &[DesignTable],
    source: Option<usize>,
    source_column: usize,
    target: Option<(usize, usize)>,
) -> Option<Result<(usize, usize, usize, usize), String>> {
    match (source, target) {
        (Some(si), Some((ti, ci))) if si != ti => {
            if tables[ti].name.trim().is_empty() {
                Some(Err("Name the target table before referencing it".into()))
            } else if tables[ti].columns[ci].base.name.trim().is_empty() {
                Some(Err("Name the target column before referencing it".into()))
            } else {
                Some(Ok((si, source_column, ti, ci)))
            }
        }
        (Some(si), Some((ti, _))) if si == ti => Some(Err(
            "A column cannot reference its own table this way".into(),
        )),
        _ => None,
    }
}

/// Point column `sc` of table `si` at column `tc` of table `ti`.
///
/// If the reference does not resolve, it is cleared again and the problem
/// returned, so the caller can say why it did not take rather than silently
/// dropping the gesture. On success returns the referenced table and column
/// names as written.
pub fn link(
    tables: &mut [DesignTable],
    si: usize,
    sc: usize,
    ti: usize,
    tc: usize,
) -> Result<(String, String), String> {
    let (rt, rc) = (
        tables[ti].name.trim().to_string(),
        tables[ti].columns[tc].base.name.trim().to_string(),
    );
    tables[si].columns[sc].ref_table = rt.clone();
    tables[si].columns[sc].ref_column = rc.clone();
    if let Err(problem) = check_reference(tables, si, sc) {
        let msg = problem.message();
        tables[si].columns[sc].ref_table.clear();
        tables[si].columns[sc].ref_column.clear();
        Err(msg)
    } else {
        Ok((rt, rc))
    }
}

/// The name as drawn in a table box: a key marker and the trimmed name, or
/// an ellipsis while the column is still unnamed.
pub fn column_display_label(col: &DesignColumn) -> String {
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
    format!("{}{}", marker, name)
}

/// The right-hand text in a table box: the type plus UNQ / NN flags that the
/// primary key already implies and so does not repeat.
pub fn column_display_suffix(col: &DesignColumn) -> String {
    let mut suffix = type_label(col);
    if col.unique && !col.base.primary_key {
        suffix.push_str(" UNQ");
    }
    if !col.base.nullable && !col.base.primary_key {
        suffix.push_str(" NN");
    }
    suffix
}

// ─── canvas geometry ──────────────────────────────────────────────────────

pub const HEADER_H: f32 = 26.0;
pub const ROW_H: f32 = 18.0;
/// How close to a row's edge a press has to be to start a connection rather
/// than drag the table.
pub const HANDLE_R: f32 = 7.0;

pub fn table_size(t: &DesignTable) -> (f32, f32) {
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

/// Compute positions for every table. Nodes are keyed by table id.
pub fn relayout(tables: &[DesignTable]) -> graph_layout::Layout {
    let nodes: Vec<Node> = tables
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
    let edges: Vec<Edge> = relationships(tables)
        .into_iter()
        .map(|r| Edge {
            from: tables[r.to].id.to_string(),
            to: tables[r.from].id.to_string(),
        })
        .collect();
    graph_layout::layout(&nodes, &edges, LayoutOptions::default())
}

/// Where a table sits in world space: a manual position if the user placed
/// it, otherwise the computed layout.
pub fn position(layout: &graph_layout::Layout, pinned: &HashMap<u64, Point>, id: u64) -> Point {
    if let Some(p) = pinned.get(&id) {
        return *p;
    }
    layout
        .placements
        .get(&id.to_string())
        .map(|p| Point::new(p.x, p.y))
        .unwrap_or(Point::ZERO)
}

/// The table under a world-space point, topmost first.
pub fn table_at(
    tables: &[DesignTable],
    layout: &graph_layout::Layout,
    pinned: &HashMap<u64, Point>,
    world: Point,
) -> Option<usize> {
    for (i, t) in tables.iter().enumerate().rev() {
        if world.in_box(position(layout, pinned, t.id), table_size(t)) {
            return Some(i);
        }
    }
    None
}

/// The column row under a world-space point.
pub fn column_at(
    tables: &[DesignTable],
    layout: &graph_layout::Layout,
    pinned: &HashMap<u64, Point>,
    world: Point,
) -> Option<(usize, usize)> {
    let i = table_at(tables, layout, pinned, world)?;
    let t = &tables[i];
    let pos = position(layout, pinned, t.id);
    let row = ((world.y - pos.y - HEADER_H) / ROW_H).floor();
    if row < 0.0 {
        return None;
    }
    let row = row as usize;
    (row < t.columns.len()).then_some((i, row))
}

/// Where a column's connector sits, in world space.
pub fn handle_pos(
    tables: &[DesignTable],
    layout: &graph_layout::Layout,
    pinned: &HashMap<u64, Point>,
    table: usize,
    column: usize,
    right: bool,
) -> Point {
    let t = &tables[table];
    let (w, _) = table_size(t);
    let pos = position(layout, pinned, t.id);
    Point::new(
        if right { pos.x + w } else { pos.x },
        pos.y + HEADER_H + column as f32 * ROW_H + ROW_H / 2.0,
    )
}

/// A connector near a world-space point, if the press should start a
/// connection rather than move the table.
pub fn handle_at(
    tables: &[DesignTable],
    layout: &graph_layout::Layout,
    pinned: &HashMap<u64, Point>,
    world: Point,
) -> Option<(usize, usize)> {
    for (i, t) in tables.iter().enumerate().rev() {
        for c in 0..t.columns.len() {
            for right in [true, false] {
                if handle_pos(tables, layout, pinned, i, c, right).distance(world) <= HANDLE_R {
                    return Some((i, c));
                }
            }
        }
    }
    None
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
        let mut a = table(
            1,
            "a",
            vec![pk("id", TypeKind::BigInt), col("b_id", TypeKind::BigInt)],
        );
        a.columns[1].ref_table = "b".into();
        a.columns[1].ref_column = "id".into();
        let mut b = table(
            2,
            "b",
            vec![pk("id", TypeKind::BigInt), col("a_id", TypeKind::BigInt)],
        );
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
            vec![
                pk("id", TypeKind::BigInt),
                col("manager_id", TypeKind::BigInt),
            ],
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
        assert!(validate(&[])
            .iter()
            .any(|e| e.contains("at least one table")));
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
        assert_eq!(
            parse_type("timestamp with time zone").0,
            TypeKind::TimestampTz
        );
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
        assert!(
            tables[1].columns[2].unique,
            "unique index was not carried over"
        );
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
        let schemas = vec![schema_of(
            "orders",
            vec![info("id", "bigint", true)],
            vec![],
        )];
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

    // ── editing ──────────────────────────────────────────────────────────

    #[test]
    fn a_new_table_starts_with_an_auto_increment_key() {
        let t = new_table(7);
        assert_eq!(t.id, 7);
        assert!(t.name.is_empty());
        assert_eq!(t.columns.len(), 1);
        let id = &t.columns[0].base;
        assert_eq!(id.name, "id");
        assert_eq!(id.type_kind, TypeKind::BigInt);
        assert!(id.primary_key && id.auto_increment && !id.nullable);
    }

    #[test]
    fn cascading_positions_never_coincide() {
        assert_eq!(cascade_position(0), Point::new(40.0, 40.0));
        assert_ne!(cascade_position(0), cascade_position(1));
        assert_eq!(cascade_position(2), Point::new(136.0, 108.0));
    }

    #[test]
    fn a_connection_to_another_named_column_is_made() {
        let tables = users_and_orders();
        assert_eq!(
            resolve_connection(&tables, Some(1), 1, Some((0, 0))),
            Some(Ok((1, 1, 0, 0)))
        );
        // Released on empty space, or the source vanished: nothing happens.
        assert_eq!(resolve_connection(&tables, Some(1), 1, None), None);
        assert_eq!(resolve_connection(&tables, None, 1, Some((0, 0))), None);
    }

    #[test]
    fn a_connection_is_refused_with_a_reason() {
        let mut tables = users_and_orders();
        assert_eq!(
            resolve_connection(&tables, Some(1), 1, Some((1, 0))),
            Some(Err(
                "A column cannot reference its own table this way".into()
            ))
        );

        tables[0].columns[0].base.name = "  ".into();
        assert_eq!(
            resolve_connection(&tables, Some(1), 1, Some((0, 0))),
            Some(Err("Name the target column before referencing it".into()))
        );

        tables[0].name = String::new();
        assert_eq!(
            resolve_connection(&tables, Some(1), 1, Some((0, 0))),
            Some(Err("Name the target table before referencing it".into()))
        );
    }

    #[test]
    fn linking_sets_a_valid_reference_and_undoes_an_invalid_one() {
        let mut tables = users_and_orders();
        tables[1].columns[1].ref_table.clear();
        tables[1].columns[1].ref_column.clear();
        assert_eq!(
            link(&mut tables, 1, 1, 0, 0),
            Ok(("users".to_string(), "id".to_string()))
        );
        assert_eq!(tables[1].columns[1].ref_table, "users");
        assert_eq!(tables[1].columns[1].ref_column, "id");

        // A text column cannot reference a bigint key; the link is cleared.
        let mut tables = users_and_orders();
        tables[1].columns.push(col("note", TypeKind::Text));
        let err = link(&mut tables, 1, 2, 0, 0).unwrap_err();
        assert!(err.contains("cannot reference"), "{}", err);
        assert!(!tables[1].columns[2].has_reference());
    }

    #[test]
    fn fk_actions_have_sql_labels() {
        assert_eq!(FK_ACTIONS.len(), 5);
        assert_eq!(fk_action_label(&ForeignKeyAction::SetNull), "SET NULL");
        assert_eq!(fk_action_label(&ForeignKeyAction::NoAction), "NO ACTION");
    }

    #[test]
    fn column_labels_mark_keys_and_flags() {
        let tables = users_and_orders();
        assert_eq!(column_display_label(&tables[1].columns[0]), "PK id");
        assert_eq!(column_display_label(&tables[1].columns[1]), "FK user_id");
        assert_eq!(column_display_label(&col("  ", TypeKind::Text)), "…");

        let key = &tables[1].columns[0];
        assert!(
            !column_display_suffix(key).contains("NN"),
            "a key implies NOT NULL"
        );
        let mut c = col("email", TypeKind::Text);
        c.unique = true;
        c.base.nullable = false;
        let suffix = column_display_suffix(&c);
        assert!(suffix.ends_with(" UNQ NN"), "{}", suffix);
    }

    // ── canvas geometry ──────────────────────────────────────────────────

    #[test]
    fn a_manual_position_overrides_the_computed_layout() {
        let tables = users_and_orders();
        let layout = relayout(&tables);
        let mut pinned = HashMap::new();
        let auto = position(&layout, &pinned, 1);
        pinned.insert(1, Point::new(500.0, 500.0));
        assert_ne!(auto, position(&layout, &pinned, 1));
        assert_eq!(position(&layout, &pinned, 1), Point::new(500.0, 500.0));
    }

    #[test]
    fn relayout_places_the_referenced_table_in_an_earlier_layer() {
        let layout = relayout(&users_and_orders());
        assert_eq!(layout.placements.len(), 2);
        assert!(
            layout.placements["1"].layer < layout.placements["2"].layer,
            "users is referenced by orders, so it should be laid out first"
        );
    }

    #[test]
    fn table_width_is_clamped() {
        let mut t = table(1, "x", vec![col("a", TypeKind::Text)]);
        t.columns[0].base.name = "a".repeat(500);
        assert!(table_size(&t).0 <= 340.0);
        assert!(table_size(&table(1, "a", vec![])).0 >= 160.0);
        assert_eq!(table_size(&table(1, "a", vec![])).1, HEADER_H + 8.0);
    }

    #[test]
    fn hit_testing_finds_tables_rows_and_connectors() {
        let tables = users_and_orders();
        let layout = graph_layout::Layout::default();
        let mut pinned = HashMap::new();
        pinned.insert(1, Point::new(0.0, 0.0));
        pinned.insert(2, Point::new(1000.0, 0.0));

        assert_eq!(
            table_at(&tables, &layout, &pinned, Point::new(5.0, 5.0)),
            Some(0)
        );
        assert_eq!(
            table_at(&tables, &layout, &pinned, Point::new(1005.0, 5.0)),
            Some(1)
        );
        assert_eq!(
            table_at(&tables, &layout, &pinned, Point::new(500.0, 5.0)),
            None
        );

        // The header is not a row; rows start below it.
        assert_eq!(
            column_at(&tables, &layout, &pinned, Point::new(1005.0, 5.0)),
            None
        );
        let second_row = Point::new(1005.0, HEADER_H + ROW_H + 1.0);
        assert_eq!(
            column_at(&tables, &layout, &pinned, second_row),
            Some((1, 1))
        );

        let right = handle_pos(&tables, &layout, &pinned, 1, 1, true);
        let (w, _) = table_size(&tables[1]);
        assert_eq!(
            right,
            Point::new(1000.0 + w, HEADER_H + ROW_H + ROW_H / 2.0)
        );
        let near = Point::new(right.x + HANDLE_R - 1.0, right.y);
        assert_eq!(handle_at(&tables, &layout, &pinned, near), Some((1, 1)));
        let far = Point::new(right.x + HANDLE_R + 1.0, right.y);
        assert_eq!(handle_at(&tables, &layout, &pinned, far), None);
    }
}
