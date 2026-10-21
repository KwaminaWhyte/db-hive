//! The visual schema designer's editing state for the Slint shell.
//!
//! `schema_designer` (lifted from egui) holds the design model, validation,
//! DDL generation and relationship resolution. This adds what an editing
//! session needs on top: ids, selection, positions with snap, every edit the
//! properties panel can make (with references kept consistent when a table or
//! column is renamed), the per-table creation plan and how a failure in one
//! table affects the ones that depend on it.
//!
//! No toolkit types, and no SQL built by hand: statements come from the DDL
//! generator for the connected driver.

use std::collections::{HashMap, HashSet};

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::{DbDriver, ForeignKeyAction};

use crate::er_diagram::Point;
use crate::erd_scene::{Badge, Rect, RowView, FOOT_H, GRID, HEADER_H, ROW_H};
use crate::graph_layout::{self, Edge, LayoutOptions, Node};
use crate::schema_designer::{
    self as sd, build_definition, cascade_position, check_reference, column_display_label,
    creation_order, display_name, new_table, relationships, validate, DesignColumn, DesignTable,
    FK_ACTIONS,
};
use crate::table_editor::TypeKind;

const MIN_WIDTH: f32 = 210.0;
const MAX_WIDTH: f32 = 380.0;

/// A relationship between two designed tables, parent to child.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignLink {
    pub parent: usize,
    pub child: usize,
    pub label: String,
}

/// One table to create, in creation order.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTable {
    pub id: u64,
    pub name: String,
    pub statements: Vec<String>,
    /// Names of the other designed tables this one references.
    pub depends_on: Vec<String>,
}

/// What happened to one table when the design was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Failed(String),
    /// Not attempted because a table it references was not created.
    Skipped(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableResult {
    pub name: String,
    pub outcome: Outcome,
}

/// Why `planned` must not be attempted, given the tables that did not get
/// created. A child created after its parent failed would only add a second,
/// confusing error.
pub fn skip_reason(planned: &PlannedTable, not_created: &HashSet<String>) -> Option<String> {
    planned
        .depends_on
        .iter()
        .find(|d| not_created.contains(*d))
        .map(|d| format!("{} was not created", d))
}

/// The statements of a plan as one script for the preview.
pub fn script(plan: &[PlannedTable]) -> String {
    let mut parts = Vec::new();
    for t in plan {
        for s in &t.statements {
            let s = s.trim();
            if s.is_empty() {
                continue;
            }
            parts.push(if s.ends_with(';') {
                s.to_string()
            } else {
                format!("{};", s)
            });
        }
    }
    parts.join("\n\n")
}

/// "3 created, 1 failed, 1 skipped".
pub fn results_summary(results: &[TableResult]) -> String {
    let n = |f: fn(&Outcome) -> bool| results.iter().filter(|r| f(&r.outcome)).count();
    let created = n(|o| matches!(o, Outcome::Created));
    let failed = n(|o| matches!(o, Outcome::Failed(_)));
    let skipped = n(|o| matches!(o, Outcome::Skipped(_)));
    let mut parts = vec![format!("{} created", created)];
    if failed > 0 {
        parts.push(format!("{} failed", failed));
    }
    if skipped > 0 {
        parts.push(format!("{} skipped", skipped));
    }
    parts.join(", ")
}

pub struct Designer {
    pub tables: Vec<DesignTable>,
    selected: Option<u64>,
    selected_column: Option<usize>,
    pinned: HashMap<u64, Point>,
    next_id: u64,
    pub snap: bool,
    drag_raw: Option<(u64, Point)>,
    /// How the last apply went; cleared by any edit.
    pub results: Vec<TableResult>,
    created: HashSet<u64>,
}

impl Default for Designer {
    fn default() -> Self {
        Self::new()
    }
}

impl Designer {
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            selected: None,
            selected_column: None,
            pinned: HashMap::new(),
            next_id: 1,
            snap: true,
            drag_raw: None,
            results: Vec::new(),
            created: HashSet::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.tables.iter().position(|t| t.id == id)
    }

    fn edited(&mut self) {
        self.results.clear();
    }

    // -- Selection ----------------------------------------------------------

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected.and_then(|id| self.index_of(id))
    }

    pub fn selected_column(&self) -> Option<usize> {
        let t = &self.tables[self.selected_index()?];
        self.selected_column.filter(|c| *c < t.columns.len())
    }

    pub fn select_index(&mut self, i: Option<usize>) {
        let id = i.and_then(|i| self.tables.get(i)).map(|t| t.id);
        if id != self.selected {
            self.selected_column = None;
        }
        self.selected = id;
    }

    pub fn select_column(&mut self, c: Option<usize>) {
        self.selected_column = c;
    }

    // -- Tables ----------------------------------------------------------------

    /// Add an empty table (an `id` primary key and nothing else), select it
    /// and return its id.
    pub fn add_table(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let pos = cascade_position(self.tables.len());
        self.pinned.insert(id, pos);
        self.tables.push(new_table(id));
        self.selected = Some(id);
        self.selected_column = None;
        self.edited();
        id
    }

    /// Remove a table, and clear the references other tables had to it.
    pub fn remove_table(&mut self, id: u64) {
        let Some(i) = self.index_of(id) else { return };
        let name = self.tables[i].name.trim().to_lowercase();
        self.tables.remove(i);
        self.pinned.remove(&id);
        self.created.remove(&id);
        if !name.is_empty() {
            for t in &mut self.tables {
                for c in &mut t.columns {
                    if c.ref_table.trim().to_lowercase() == name {
                        c.ref_table.clear();
                        c.ref_column.clear();
                    }
                }
            }
        }
        if self.selected == Some(id) {
            self.selected = None;
            self.selected_column = None;
        }
        self.edited();
    }

    /// Rename a table; references to the old name follow it.
    pub fn rename_table(&mut self, id: u64, name: &str) {
        let Some(i) = self.index_of(id) else { return };
        let old = self.tables[i].name.trim().to_lowercase();
        self.tables[i].name = name.to_string();
        if !old.is_empty() {
            for t in &mut self.tables {
                for c in &mut t.columns {
                    if c.ref_table.trim().to_lowercase() == old {
                        c.ref_table = name.trim().to_string();
                    }
                }
            }
        }
        self.edited();
    }

    pub fn set_table_schema(&mut self, id: u64, schema: &str) {
        if let Some(i) = self.index_of(id) {
            self.tables[i].schema = schema.to_string();
            self.edited();
        }
    }

    // -- Columns ---------------------------------------------------------------

    fn col_mut(&mut self, id: u64, c: usize) -> Option<&mut DesignColumn> {
        let i = self.index_of(id)?;
        self.tables[i].columns.get_mut(c)
    }

    pub fn add_column(&mut self, id: u64) -> Option<usize> {
        let i = self.index_of(id)?;
        let n = self.tables[i].columns.len();
        self.tables[i].columns.push(DesignColumn::default());
        self.selected = Some(id);
        self.selected_column = Some(n);
        self.edited();
        Some(n)
    }

    pub fn remove_column(&mut self, id: u64, c: usize) {
        let Some(i) = self.index_of(id) else { return };
        if c >= self.tables[i].columns.len() {
            return;
        }
        let table = self.tables[i].name.trim().to_lowercase();
        let col = self.tables[i].columns[c].base.name.trim().to_lowercase();
        self.tables[i].columns.remove(c);
        if !col.is_empty() {
            for t in &mut self.tables {
                for x in &mut t.columns {
                    if x.ref_table.trim().to_lowercase() == table
                        && x.ref_column.trim().to_lowercase() == col
                    {
                        x.ref_table.clear();
                        x.ref_column.clear();
                    }
                }
            }
        }
        self.selected_column = None;
        self.edited();
    }

    /// Rename a column; references to it follow.
    pub fn rename_column(&mut self, id: u64, c: usize, name: &str) {
        let Some(i) = self.index_of(id) else { return };
        let Some(col) = self.tables[i].columns.get(c) else {
            return;
        };
        let old = col.base.name.trim().to_lowercase();
        let table = self.tables[i].name.trim().to_lowercase();
        self.tables[i].columns[c].base.name = name.to_string();
        if !old.is_empty() && !table.is_empty() {
            for t in &mut self.tables {
                for x in &mut t.columns {
                    if x.ref_table.trim().to_lowercase() == table
                        && x.ref_column.trim().to_lowercase() == old
                    {
                        x.ref_column = name.trim().to_string();
                    }
                }
            }
        }
        self.edited();
    }

    /// Pick a type by its index in `TypeKind::ALL`. A type that cannot
    /// auto-increment drops that flag.
    pub fn set_column_type(&mut self, id: u64, c: usize, type_index: usize) {
        let Some(kind) = TypeKind::ALL.get(type_index).copied() else {
            return;
        };
        if let Some(col) = self.col_mut(id, c) {
            col.base.type_kind = kind;
            if !kind.can_auto_increment() {
                col.base.auto_increment = false;
            }
            self.edited();
        }
    }

    pub fn set_column_length(&mut self, id: u64, c: usize, text: &str) {
        if let (Some(col), Ok(n)) = (self.col_mut(id, c), text.trim().parse::<u32>()) {
            col.base.length = n;
            self.edited();
        }
    }

    pub fn set_column_precision(&mut self, id: u64, c: usize, text: &str) {
        if let (Some(col), Ok(n)) = (self.col_mut(id, c), text.trim().parse::<u8>()) {
            col.base.precision = n;
            self.edited();
        }
    }

    pub fn set_column_scale(&mut self, id: u64, c: usize, text: &str) {
        if let (Some(col), Ok(n)) = (self.col_mut(id, c), text.trim().parse::<u8>()) {
            col.base.scale = n;
            self.edited();
        }
    }

    pub fn set_column_custom(&mut self, id: u64, c: usize, text: &str) {
        if let Some(col) = self.col_mut(id, c) {
            col.base.custom = text.to_string();
            self.edited();
        }
    }

    pub fn set_column_default(&mut self, id: u64, c: usize, text: &str) {
        if let Some(col) = self.col_mut(id, c) {
            col.base.default = text.to_string();
            self.edited();
        }
    }

    /// A primary key is never nullable, so turning it on turns nullable off.
    pub fn toggle_primary_key(&mut self, id: u64, c: usize) {
        if let Some(col) = self.col_mut(id, c) {
            col.base.primary_key = !col.base.primary_key;
            if col.base.primary_key {
                col.base.nullable = false;
            }
            self.edited();
        }
    }

    /// Ignored on a primary key: it cannot be nullable.
    pub fn toggle_nullable(&mut self, id: u64, c: usize) {
        if let Some(col) = self.col_mut(id, c) {
            if !col.base.primary_key {
                col.base.nullable = !col.base.nullable;
                self.edited();
            }
        }
    }

    pub fn toggle_unique(&mut self, id: u64, c: usize) {
        if let Some(col) = self.col_mut(id, c) {
            col.unique = !col.unique;
            self.edited();
        }
    }

    /// Ignored for a type that cannot auto-increment.
    pub fn toggle_auto_increment(&mut self, id: u64, c: usize) {
        if let Some(col) = self.col_mut(id, c) {
            if col.base.type_kind.can_auto_increment() {
                col.base.auto_increment = !col.base.auto_increment;
                self.edited();
            }
        }
    }

    // -- Foreign keys ----------------------------------------------------------

    /// Names a column may reference: every named table.
    pub fn reference_table_options(&self) -> Vec<String> {
        self.tables
            .iter()
            .filter(|t| !t.name.trim().is_empty())
            .map(|t| t.name.trim().to_string())
            .collect()
    }

    /// Columns of `table` that SQL allows a foreign key to point at: primary
    /// key or unique, and named.
    pub fn reference_column_options(&self, table: &str) -> Vec<String> {
        let needle = table.trim().to_lowercase();
        self.tables
            .iter()
            .find(|t| !t.name.trim().is_empty() && t.name.trim().to_lowercase() == needle)
            .map(|t| {
                t.columns
                    .iter()
                    .filter(|c| (c.base.primary_key || c.unique) && !c.base.name.trim().is_empty())
                    .map(|c| c.base.name.trim().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Point a column at `table`. The column is chosen if there is exactly one
    /// candidate, otherwise it is left for the user.
    pub fn set_reference_table(&mut self, id: u64, c: usize, table: &str) {
        let options = self.reference_column_options(table);
        if let Some(col) = self.col_mut(id, c) {
            col.ref_table = table.trim().to_string();
            col.ref_column = if options.len() == 1 {
                options[0].clone()
            } else {
                String::new()
            };
            self.edited();
        }
    }

    pub fn set_reference_column(&mut self, id: u64, c: usize, column: &str) {
        if let Some(col) = self.col_mut(id, c) {
            col.ref_column = column.trim().to_string();
            self.edited();
        }
    }

    pub fn clear_reference(&mut self, id: u64, c: usize) {
        if let Some(col) = self.col_mut(id, c) {
            col.ref_table.clear();
            col.ref_column.clear();
            col.on_delete = ForeignKeyAction::NoAction;
            self.edited();
        }
    }

    /// Pick the ON DELETE action by its index in `FK_ACTIONS`.
    pub fn set_on_delete(&mut self, id: u64, c: usize, action_index: usize) {
        if let (Some(col), Some(a)) = (self.col_mut(id, c), FK_ACTIONS.get(action_index)) {
            col.on_delete = a.clone();
            self.edited();
        }
    }

    /// Why the reference on this column cannot be used, if it cannot.
    pub fn reference_problem(&self, table: usize, column: usize) -> Option<String> {
        check_reference(&self.tables, table, column)
            .err()
            .map(|p| p.message())
    }

    // -- Geometry --------------------------------------------------------------

    pub fn position(&self, i: usize) -> Point {
        self.pinned
            .get(&self.tables[i].id)
            .copied()
            .unwrap_or(Point::ZERO)
    }

    pub fn size(&self, i: usize) -> (f32, f32) {
        let t = &self.tables[i];
        let body = t
            .columns
            .iter()
            .map(|c| {
                column_display_label(c).chars().count() as f32 * 7.1
                    + column_detail_text(c).chars().count() as f32 * 6.2
                    + 64.0
            })
            .fold(0.0, f32::max);
        let title = display_name(t).chars().count() as f32 * 8.0 + 32.0;
        let width = body.max(title).clamp(MIN_WIDTH, MAX_WIDTH);
        (width, HEADER_H + t.columns.len() as f32 * ROW_H + FOOT_H)
    }

    pub fn rect(&self, i: usize) -> Rect {
        let p = self.position(i);
        let (w, h) = self.size(i);
        Rect::new(p.x, p.y, w, h)
    }

    pub fn bounds(&self) -> Option<Rect> {
        (0..self.len()).map(|i| self.rect(i)).reduce(Rect::union)
    }

    pub fn hit(&self, world: Point) -> Option<usize> {
        (0..self.len())
            .rev()
            .find(|i| self.rect(*i).contains(world))
    }

    pub fn title(&self, i: usize) -> String {
        display_name(&self.tables[i])
    }

    pub fn rows(&self, i: usize) -> Vec<RowView> {
        self.tables[i]
            .columns
            .iter()
            .map(|c| RowView {
                name: if c.base.name.trim().is_empty() {
                    "(unnamed)".to_string()
                } else {
                    c.base.name.trim().to_string()
                },
                data_type: column_detail_text(c),
                badge: if c.base.primary_key {
                    Badge::Pk
                } else if c.has_reference() {
                    Badge::Fk
                } else {
                    Badge::None
                },
            })
            .collect()
    }

    pub fn type_width(&self, i: usize) -> f32 {
        let longest = self.tables[i]
            .columns
            .iter()
            .map(|c| column_detail_text(c).chars().count())
            .max()
            .unwrap_or(0) as f32;
        let (w, _) = self.size(i);
        (longest * 6.2 + 8.0).min(w * 0.55).max(40.0)
    }

    /// Relationships that resolve; broken ones are listed by `errors`, not drawn.
    pub fn links(&self) -> Vec<DesignLink> {
        relationships(&self.tables)
            .into_iter()
            .map(|r| DesignLink {
                parent: r.to,
                child: r.from,
                label: format!(
                    "{} \u{2192} {}",
                    self.tables[r.from].columns[r.from_column].base.name.trim(),
                    self.tables[r.to].columns[r.to_column].base.name.trim()
                ),
            })
            .collect()
    }

    pub fn begin_drag(&mut self, i: usize) {
        if let Some(t) = self.tables.get(i) {
            let id = t.id;
            if self.drag_raw.map(|(d, _)| d) != Some(id) {
                self.drag_raw = Some((id, self.position(i)));
            }
        }
    }

    /// Move a table by a world-space delta, snapping to the grid if on.
    pub fn drag(&mut self, i: usize, dx: f32, dy: f32) -> Option<Point> {
        let id = self.tables.get(i)?.id;
        self.begin_drag(i);
        let (_, raw) = self.drag_raw.as_mut()?;
        raw.x += dx;
        raw.y += dy;
        let eff = if self.snap {
            let s = |v: f32| (v / GRID).round() * GRID;
            Point::new(s(raw.x), s(raw.y))
        } else {
            *raw
        };
        self.pinned.insert(id, eff);
        Some(eff)
    }

    pub fn end_drag(&mut self) {
        self.drag_raw = None;
    }

    /// Lay every table out by its relationships (parents left of children)
    /// and pin the result, so tables then stay where they were put.
    pub fn auto_layout(&mut self) {
        let nodes: Vec<Node> = (0..self.len())
            .map(|i| {
                let (w, h) = self.size(i);
                Node {
                    id: self.tables[i].id.to_string(),
                    width: w,
                    height: h,
                }
            })
            .collect();
        let edges: Vec<Edge> = self
            .links()
            .iter()
            .map(|l| Edge {
                from: self.tables[l.parent].id.to_string(),
                to: self.tables[l.child].id.to_string(),
            })
            .collect();
        let layout = graph_layout::layout(
            &nodes,
            &edges,
            LayoutOptions {
                layer_gap: 130.0,
                node_gap: 36.0,
                horizontal: true,
                sweeps: 4,
            },
        );
        for t in &self.tables {
            if let Some(p) = layout.placements.get(&t.id.to_string()) {
                self.pinned.insert(t.id, Point::new(p.x + 40.0, p.y + 40.0));
            }
        }
        self.drag_raw = None;
    }

    // -- Validation and planning ----------------------------------------------------

    /// Everything wrong with the design.
    pub fn errors(&self) -> Vec<String> {
        validate(&self.tables)
    }

    /// The tables to create, referenced ones first, each with the statements
    /// the connected driver's generator produces for it. `Err` carries the
    /// problems that stop anything from being created.
    pub fn plan(
        &self,
        driver: &DbDriver,
        default_schema: &str,
    ) -> Result<Vec<PlannedTable>, Vec<String>> {
        let errors = validate(&self.tables);
        if !errors.is_empty() {
            return Err(errors);
        }
        let order = creation_order(&self.tables).map_err(|stuck| {
            vec![format!(
                "Foreign keys form a cycle between {}",
                stuck.join(", ")
            )]
        })?;
        let generator = get_ddl_generator(driver).map_err(|e| vec![e.to_string()])?;
        let rels = relationships(&self.tables);
        let mut out = Vec::with_capacity(order.len());
        for i in order {
            // Already created by an earlier apply: still part of the design
            // (others may reference it) but not created again.
            if self.created.contains(&self.tables[i].id) {
                continue;
            }
            let def = build_definition(default_schema, &self.tables[i]);
            let result = generator
                .generate_create_table(&def)
                .map_err(|e| vec![format!("{}: {}", display_name(&self.tables[i]), e)])?;
            let mut depends_on: Vec<String> = Vec::new();
            for r in rels.iter().filter(|r| r.from == i && r.to != i) {
                let n = self.tables[r.to].name.trim().to_string();
                if !depends_on.contains(&n) {
                    depends_on.push(n);
                }
            }
            out.push(PlannedTable {
                id: self.tables[i].id,
                name: self.tables[i].name.trim().to_string(),
                statements: result.sql,
                depends_on,
            });
        }
        Ok(out)
    }

    /// Record how an apply went.
    pub fn set_results(&mut self, results: Vec<TableResult>) {
        self.results = results;
    }

    /// Remember which tables the last apply created. They stay in the design
    /// (a table still to be created may reference them) but are not planned
    /// again, so a second apply cannot try to create them twice.
    pub fn mark_created(&mut self) {
        let created: HashSet<String> = self
            .results
            .iter()
            .filter(|r| r.outcome == Outcome::Created)
            .map(|r| r.name.to_lowercase())
            .collect();
        for t in &self.tables {
            if created.contains(&t.name.trim().to_lowercase()) {
                self.created.insert(t.id);
            }
        }
    }

    pub fn is_created(&self, i: usize) -> bool {
        self.tables
            .get(i)
            .is_some_and(|t| self.created.contains(&t.id))
    }

    /// How many tables an apply would create now.
    pub fn pending_count(&self) -> usize {
        self.tables
            .iter()
            .filter(|t| !self.created.contains(&t.id))
            .count()
    }
}

/// The type as it will be created, parameters included: `VARCHAR(255)`,
/// `DECIMAL(10,2)`, or the custom name as typed.
pub fn display_type(c: &DesignColumn) -> String {
    let b = &c.base;
    match b.type_kind {
        TypeKind::Varchar => format!("VARCHAR({})", b.length),
        TypeKind::Char => format!("CHAR({})", b.length),
        TypeKind::Decimal => format!("DECIMAL({},{})", b.precision, b.scale),
        TypeKind::Custom if b.custom.trim().is_empty() => "custom".to_string(),
        TypeKind::Custom => b.custom.trim().to_string(),
        k => k.label().to_string(),
    }
}

/// The right-hand text in a table box: the type plus UNQ and NN flags that a
/// primary key already implies and so does not repeat.
pub fn column_detail_text(c: &DesignColumn) -> String {
    let mut s = display_type(c);
    if c.unique && !c.base.primary_key {
        s.push_str(" UNQ");
    }
    if !c.base.nullable && !c.base.primary_key {
        s.push_str(" NN");
    }
    s
}

/// Everything the properties panel shows about one column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDetail {
    pub name: String,
    pub type_index: usize,
    pub type_label: String,
    pub takes_length: bool,
    pub length: String,
    pub takes_precision: bool,
    pub precision: String,
    pub scale: String,
    pub takes_custom: bool,
    pub custom: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub can_auto_increment: bool,
    pub default: String,
    pub has_reference: bool,
    pub ref_table: String,
    pub ref_column: String,
    /// Tables the column may point at.
    pub ref_tables: Vec<String>,
    /// Columns of the chosen table it may point at.
    pub ref_columns: Vec<String>,
    pub on_delete_index: usize,
    /// Why the reference does not resolve, or empty.
    pub problem: String,
}

impl Designer {
    pub fn column_detail(&self, table: usize, column: usize) -> Option<ColumnDetail> {
        let c = self.tables.get(table)?.columns.get(column)?;
        let b = &c.base;
        Some(ColumnDetail {
            name: b.name.clone(),
            type_index: type_index(b.type_kind),
            type_label: b.type_kind.label().to_string(),
            takes_length: b.type_kind.takes_length(),
            length: b.length.to_string(),
            takes_precision: b.type_kind.takes_precision(),
            precision: b.precision.to_string(),
            scale: b.scale.to_string(),
            takes_custom: b.type_kind.takes_custom_name(),
            custom: b.custom.clone(),
            nullable: b.nullable,
            primary_key: b.primary_key,
            unique: c.unique,
            auto_increment: b.auto_increment,
            can_auto_increment: b.type_kind.can_auto_increment(),
            default: b.default.clone(),
            has_reference: c.has_reference(),
            ref_table: c.ref_table.clone(),
            ref_column: c.ref_column.clone(),
            ref_tables: self.reference_table_options(),
            ref_columns: if c.ref_table.trim().is_empty() {
                Vec::new()
            } else {
                self.reference_column_options(&c.ref_table)
            },
            on_delete_index: action_index(&c.on_delete),
            problem: self.reference_problem(table, column).unwrap_or_default(),
        })
    }
}

/// The type picker's labels, in `TypeKind::ALL` order.
pub fn type_labels() -> Vec<String> {
    TypeKind::ALL
        .iter()
        .map(|k| k.label().to_string())
        .collect()
}

pub fn type_index(kind: TypeKind) -> usize {
    TypeKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

/// The ON DELETE picker's labels, in `FK_ACTIONS` order.
pub fn action_labels() -> Vec<String> {
    FK_ACTIONS
        .iter()
        .map(|a| sd::fk_action_label(a).to_string())
        .collect()
}

pub fn action_index(a: &ForeignKeyAction) -> usize {
    FK_ACTIONS.iter().position(|x| x == a).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// users(id) and orders(id, user_id -> users.id), named and valid.
    fn users_and_orders() -> Designer {
        let mut d = Designer::new();
        let u = d.add_table();
        d.rename_table(u, "users");
        let o = d.add_table();
        d.rename_table(o, "orders");
        let c = d.add_column(o).unwrap();
        d.rename_column(o, c, "user_id");
        d.set_column_type(o, c, type_index(TypeKind::BigInt));
        d.set_reference_table(o, c, "users");
        d
    }

    #[test]
    fn new_tables_get_distinct_ids_and_positions_and_are_selected() {
        let mut d = Designer::new();
        let a = d.add_table();
        let b = d.add_table();
        assert_ne!(a, b);
        assert_ne!(
            d.position(0),
            d.position(1),
            "new tables landed on top of each other"
        );
        assert_eq!(d.selected(), Some(b));
        assert_eq!(d.selected_index(), Some(1));
    }

    #[test]
    fn a_new_table_has_an_id_primary_key_and_is_not_valid_until_named() {
        let mut d = Designer::new();
        let a = d.add_table();
        assert!(d.tables[0].columns[0].base.primary_key);
        assert!(d.errors().iter().any(|e| e.contains("Table name")));
        d.rename_table(a, "widgets");
        assert!(d.errors().is_empty(), "{:?}", d.errors());
    }

    #[test]
    fn removing_a_table_clears_its_position_selection_and_inbound_references() {
        let mut d = users_and_orders();
        let users = d.tables[0].id;
        assert!(d.tables[1].columns[1].has_reference());
        d.remove_table(users);
        assert_eq!(d.len(), 1);
        assert!(
            !d.tables[0].columns[1].has_reference(),
            "a dangling reference survived"
        );
        assert!(d.errors().is_empty(), "{:?}", d.errors());
        d.remove_table(9999);
    }

    #[test]
    fn renaming_a_table_or_column_keeps_references_pointing_at_it() {
        let mut d = users_and_orders();
        let (users, orders) = (d.tables[0].id, d.tables[1].id);
        d.rename_table(users, "people");
        assert_eq!(d.tables[1].columns[1].ref_table, "people");
        d.rename_column(users, 0, "person_id");
        assert_eq!(d.tables[1].columns[1].ref_column, "person_id");
        assert!(d.errors().is_empty(), "{:?}", d.errors());
        d.rename_column(orders, 1, "buyer");
        assert_eq!(d.tables[1].columns[1].ref_table, "people");
    }

    #[test]
    fn removing_a_referenced_column_clears_the_reference() {
        let mut d = users_and_orders();
        let users = d.tables[0].id;
        d.remove_column(users, 0);
        assert!(!d.tables[1].columns[1].has_reference());
    }

    #[test]
    fn making_a_column_a_primary_key_makes_it_not_null() {
        let mut d = users_and_orders();
        let o = d.tables[1].id;
        assert!(d.tables[1].columns[1].base.nullable);
        d.toggle_primary_key(o, 1);
        assert!(d.tables[1].columns[1].base.primary_key);
        assert!(!d.tables[1].columns[1].base.nullable);
        // And nullable cannot be turned back on while it is a key.
        d.toggle_nullable(o, 1);
        assert!(!d.tables[1].columns[1].base.nullable);
    }

    #[test]
    fn auto_increment_is_refused_for_a_type_that_cannot_and_dropped_on_change() {
        let mut d = Designer::new();
        let t = d.add_table();
        assert!(d.tables[0].columns[0].base.auto_increment);
        d.set_column_type(t, 0, type_index(TypeKind::Text));
        assert!(
            !d.tables[0].columns[0].base.auto_increment,
            "TEXT cannot auto-increment"
        );
        d.toggle_auto_increment(t, 0);
        assert!(!d.tables[0].columns[0].base.auto_increment);
        d.set_column_type(t, 0, type_index(TypeKind::Integer));
        d.toggle_auto_increment(t, 0);
        assert!(d.tables[0].columns[0].base.auto_increment);
    }

    #[test]
    fn choosing_a_referenced_table_picks_its_only_candidate_column() {
        let d = users_and_orders();
        assert_eq!(d.tables[1].columns[1].ref_column, "id");
        assert_eq!(d.reference_table_options(), vec!["users", "orders"]);
        assert_eq!(d.reference_column_options("USERS"), vec!["id"]);
        assert!(d.reference_column_options("nope").is_empty());
    }

    #[test]
    fn only_key_or_unique_columns_are_offered_as_targets() {
        let mut d = users_and_orders();
        let u = d.tables[0].id;
        let c = d.add_column(u).unwrap();
        d.rename_column(u, c, "email");
        assert_eq!(d.reference_column_options("users"), vec!["id"]);
        d.toggle_unique(u, c);
        assert_eq!(d.reference_column_options("users"), vec!["id", "email"]);
    }

    #[test]
    fn an_unresolvable_reference_is_reported_and_not_drawn() {
        let mut d = users_and_orders();
        let o = d.tables[1].id;
        d.set_reference_column(o, 1, "missing");
        assert!(d.reference_problem(1, 1).unwrap().contains("missing"));
        assert!(
            d.links().is_empty(),
            "a broken reference should not be drawn"
        );
        assert!(d.errors().iter().any(|e| e.contains("orders.user_id")));
    }

    #[test]
    fn links_run_from_the_referenced_table_to_the_referencing_one() {
        let d = users_and_orders();
        let links = d.links();
        assert_eq!(links.len(), 1);
        assert_eq!(d.title(links[0].parent), "users");
        assert_eq!(d.title(links[0].child), "orders");
        assert_eq!(links[0].label, "user_id \u{2192} id");
    }

    #[test]
    fn rows_show_keys_and_flags() {
        let d = users_and_orders();
        let rows = d.rows(1);
        assert_eq!(rows[0].badge, Badge::Pk);
        assert_eq!(rows[1].badge, Badge::Fk);
        assert!(
            rows[1].data_type.contains("BIGINT"),
            "{}",
            rows[1].data_type
        );
        let mut d = d;
        let o = d.tables[1].id;
        let c = d.add_column(o).unwrap();
        assert_eq!(d.rows(1)[c].name, "(unnamed)");
    }

    #[test]
    fn dragging_accumulates_snaps_and_ignores_unknown_tables() {
        let mut d = Designer::new();
        d.add_table();
        d.snap = false;
        let p0 = d.position(0);
        d.drag(0, 10.0, 5.0);
        let p = d.drag(0, 10.0, 5.0).unwrap();
        assert_eq!(p, Point::new(p0.x + 20.0, p0.y + 10.0));
        d.snap = true;
        d.end_drag();
        let p = d.drag(0, 3.0, 3.0).unwrap();
        assert_eq!(p.x % GRID, 0.0);
        assert_eq!(d.drag(7, 1.0, 1.0), None);
    }

    #[test]
    fn auto_layout_puts_parents_left_of_children_and_pins_the_result() {
        let mut d = users_and_orders();
        d.auto_layout();
        assert!(
            d.position(0).x < d.position(1).x,
            "users should be left of orders"
        );
        let before = d.position(0);
        d.drag(1, 100.0, 0.0);
        assert_eq!(d.position(0), before);
    }

    #[test]
    fn hit_finds_the_table_under_a_world_point() {
        let d = users_and_orders();
        let r = d.rect(1);
        assert_eq!(d.hit(Point::new(r.x + 2.0, r.y + 2.0)), Some(1));
        assert_eq!(d.hit(Point::new(-5000.0, -5000.0)), None);
    }

    // -- planning --------------------------------------------------------------

    #[test]
    fn the_plan_creates_referenced_tables_first_with_real_ddl() {
        let mut d = users_and_orders();
        // Declare orders first so the order of creation is not just the order added.
        d.tables.swap(0, 1);
        let plan = d.plan(&DbDriver::Sqlite, "").unwrap();
        let names: Vec<_> = plan.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["users", "orders"]);
        assert_eq!(plan[1].depends_on, vec!["users"]);
        assert!(plan[0].depends_on.is_empty());
        let sql = script(&plan);
        assert!(sql.contains("CREATE TABLE \"users\""), "{}", sql);
        assert!(sql.contains("REFERENCES"), "{}", sql);
        assert!(
            sql.matches(';').count() >= 2,
            "each statement ends with a semicolon: {}",
            sql
        );
    }

    #[test]
    fn a_design_with_problems_has_no_plan() {
        let mut d = Designer::new();
        d.add_table(); // unnamed
        let err = d.plan(&DbDriver::Sqlite, "").unwrap_err();
        assert!(err.iter().any(|e| e.contains("Table name")));
        assert!(Designer::new().plan(&DbDriver::Sqlite, "").unwrap_err()[0]
            .contains("at least one table"));
    }

    #[test]
    fn a_reference_cycle_is_reported_not_sorted_around() {
        let mut d = users_and_orders();
        let u = d.tables[0].id;
        let c = d.add_column(u).unwrap();
        d.rename_column(u, c, "last_order");
        d.set_column_type(u, c, type_index(TypeKind::BigInt));
        d.set_reference_table(u, c, "orders");
        // orders needs a key target
        let err = d.plan(&DbDriver::Sqlite, "").unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("cycle") || e.contains("last_order")),
            "{:?}",
            err
        );
    }

    #[test]
    fn a_table_is_skipped_when_something_it_references_was_not_created() {
        let d = users_and_orders();
        let plan = d.plan(&DbDriver::Sqlite, "").unwrap();
        let mut failed = HashSet::new();
        assert_eq!(skip_reason(&plan[1], &failed), None);
        failed.insert("users".to_string());
        assert_eq!(
            skip_reason(&plan[1], &failed).unwrap(),
            "users was not created"
        );
        assert_eq!(
            skip_reason(&plan[0], &failed),
            None,
            "users itself is not skipped by its own failure"
        );
    }

    #[test]
    fn results_summarise_and_created_tables_are_not_planned_again_but_still_referenced() {
        let mut d = users_and_orders();
        d.set_results(vec![
            TableResult {
                name: "users".into(),
                outcome: Outcome::Created,
            },
            TableResult {
                name: "orders".into(),
                outcome: Outcome::Failed("boom".into()),
            },
            TableResult {
                name: "x".into(),
                outcome: Outcome::Skipped("y was not created".into()),
            },
        ]);
        assert_eq!(
            results_summary(&d.results),
            "1 created, 1 failed, 1 skipped"
        );
        assert_eq!(d.pending_count(), 2);
        d.mark_created();
        assert!(d.is_created(0) && !d.is_created(1));
        assert_eq!(d.pending_count(), 1);
        assert_eq!(d.results.len(), 3, "the report is kept");

        // Retrying creates only orders, and users still resolves as its target.
        let plan = d.plan(&DbDriver::Sqlite, "").unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].name, "orders");
        assert!(d.errors().is_empty(), "{:?}", d.errors());
        assert_eq!(
            d.links().len(),
            1,
            "the relationship to the created table is still drawn"
        );
    }

    #[test]
    fn removing_a_created_table_forgets_that_it_was_created() {
        let mut d = users_and_orders();
        d.set_results(vec![TableResult {
            name: "users".into(),
            outcome: Outcome::Created,
        }]);
        d.mark_created();
        let u = d.tables[0].id;
        d.remove_table(u);
        assert_eq!(d.pending_count(), 1);
        assert!(!d.is_created(0));
    }

    #[test]
    fn any_edit_clears_the_last_report() {
        let mut d = users_and_orders();
        d.set_results(vec![TableResult {
            name: "users".into(),
            outcome: Outcome::Created,
        }]);
        let u = d.tables[0].id;
        d.rename_table(u, "people");
        assert!(d.results.is_empty());
    }

    #[test]
    fn types_show_their_parameters_and_flags() {
        let mut d = Designer::new();
        let t = d.add_table();
        let c = d.add_column(t).unwrap();
        d.set_column_type(t, c, type_index(TypeKind::Varchar));
        assert_eq!(display_type(&d.tables[0].columns[c]), "VARCHAR(255)");
        d.set_column_length(t, c, "40");
        d.toggle_nullable(t, c);
        d.toggle_unique(t, c);
        assert_eq!(
            column_detail_text(&d.tables[0].columns[c]),
            "VARCHAR(40) UNQ NN"
        );
        d.set_column_type(t, c, type_index(TypeKind::Decimal));
        d.set_column_precision(t, c, "12");
        d.set_column_scale(t, c, "4");
        assert_eq!(display_type(&d.tables[0].columns[c]), "DECIMAL(12,4)");
        d.set_column_type(t, c, type_index(TypeKind::Custom));
        assert_eq!(display_type(&d.tables[0].columns[c]), "custom");
        d.set_column_custom(t, c, " citext ");
        assert_eq!(display_type(&d.tables[0].columns[c]), "citext");
        // The key column implies NN and UNQ, so it does not repeat them.
        assert_eq!(column_detail_text(&d.tables[0].columns[0]), "BIGINT");
        // Junk in a numeric field is ignored, not stored.
        d.set_column_length(t, c, "lots");
        assert_eq!(d.tables[0].columns[c].base.length, 40);
    }

    #[test]
    fn column_detail_carries_the_choices_the_panel_needs() {
        let d = users_and_orders();
        let det = d.column_detail(1, 1).unwrap();
        assert_eq!(det.name, "user_id");
        assert_eq!(det.type_label, "BIGINT");
        assert!(det.has_reference && det.problem.is_empty());
        assert_eq!(det.ref_table, "users");
        assert_eq!(det.ref_tables, vec!["users", "orders"]);
        assert_eq!(det.ref_columns, vec!["id"]);
        assert!(det.can_auto_increment && !det.auto_increment);
        assert!(d.column_detail(1, 9).is_none());
        assert!(d.column_detail(9, 0).is_none());
    }

    #[test]
    fn pickers_line_up_with_their_enums() {
        assert_eq!(type_labels().len(), TypeKind::ALL.len());
        assert_eq!(type_index(TypeKind::BigInt), 2);
        assert_eq!(action_labels().len(), FK_ACTIONS.len());
        assert_eq!(action_index(&ForeignKeyAction::Cascade), 2);
    }

    #[test]
    fn on_delete_is_stored_and_reaches_the_ddl() {
        let mut d = users_and_orders();
        let o = d.tables[1].id;
        d.set_on_delete(o, 1, action_index(&ForeignKeyAction::Cascade));
        let sql = script(&d.plan(&DbDriver::Sqlite, "").unwrap());
        assert!(sql.to_uppercase().contains("ON DELETE CASCADE"), "{}", sql);
    }
}
