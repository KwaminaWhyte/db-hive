//! The schema browser's tree: databases, schemas and tables as rows.
//!
//! Pure state, no toolkit and no async. The shell feeds it what the server
//! reported (an overview, then one schema's tables at a time, lazily) and gets
//! back flat rows to draw plus an `Action` for anything that needs the
//! database (load a schema, open a table, switch database). Keyboard
//! navigation, the live filter, the context menu entries and the drop-table
//! confirmation all live here so they are tested without a window.
//!
//! Shape of the tree:
//!
//! ```text
//! database            (the active one is expanded; others switch on open)
//!   schema            (Postgres family only)
//!     table / view
//! ```
//!
//! A driver with no schema level (MySQL, SQLite) hangs its tables straight off
//! the database row; the one hidden schema node still exists in the state so
//! every table has an owner.

use dbhive_core::models::{DbDriver, TableInfo};
use dbhive_core::service_schema::{Overview, TableRef};

use crate::format::thousands;
use crate::table_editor::drop_confirmed;

/// Most unloaded schemas a live search will load on the user's behalf.
pub const MAX_SEARCH_LOADS: usize = 25;

/// Whether a table-like object is a view.
pub fn is_view(table_type: &str) -> bool {
    table_type.to_uppercase().contains("VIEW")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    Database(String),
    Schema(String),
    Table {
        schema: String,
        name: String,
    },
    /// A line under a schema that says something instead of being an object.
    Note(String, NoteKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Loading,
    Error,
    Empty,
}

/// Where a lazy load stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct SchemaNode {
    /// What `get_tables` takes: a schema name, or the database for a driver
    /// with no schema level.
    pub name: String,
    pub tables: Vec<TableInfo>,
    pub load: Load,
    pub expanded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Database,
    Schema,
    Table,
    View,
    Loading,
    Error,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub id: NodeId,
    pub depth: u8,
    pub kind: RowKind,
    pub label: String,
    /// Row count for a table, "N tables" for a schema once loaded, a hint for
    /// a note.
    pub detail: String,
    pub expandable: bool,
    pub expanded: bool,
    /// The connection's database, or the schema other tools act on.
    pub active: bool,
    /// A server's own bookkeeping (information_schema, mysql, ...).
    pub dim: bool,
    /// Children are being fetched.
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelStatus {
    /// Nothing is known yet.
    Loading,
    /// The overview could not be read.
    Error,
    /// The (only) schema has no tables.
    Empty,
    /// A search matched nothing.
    NoMatches,
    Populated,
}

#[derive(Debug, Clone)]
pub struct TreeView {
    pub status: PanelStatus,
    pub rows: Vec<TreeRow>,
    /// Index into `rows` of the keyboard cursor, if it is visible.
    pub cursor: Option<usize>,
    pub error: String,
    /// Tables across every loaded schema, for the header count.
    pub table_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Home,
    End,
    Left,
    Right,
    Enter,
}

/// What the shell must do after an interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing changed.
    None,
    /// State changed; redraw the rows.
    Redraw,
    OpenTable {
        schema: String,
        table: String,
    },
    SwitchDatabase(String),
    LoadSchema(String),
    CreateTable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ViewData,
    GenerateSelect,
    GenerateInsert,
    CopyName,
    EditTable,
    Refresh,
    DropTable,
    CreateTable,
    SwitchDatabase,
    CreateDatabase,
    /// Open the import wizard aimed at this table, or at this schema.
    ImportData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub label: &'static str,
    /// Name of a kit icon; the view maps it to the image.
    pub icon: &'static str,
    pub enabled: bool,
    pub danger: bool,
    pub divider_above: bool,
}

/// What the connection allows, so a menu does not offer what would fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps {
    /// Tables can be created, edited and dropped (a DDL generator exists).
    pub ddl: bool,
    pub switch_database: bool,
    pub create_database: bool,
}

impl Caps {
    pub fn for_driver(driver: &DbDriver) -> Self {
        Self {
            ddl: dbhive_core::ddl::get_ddl_generator(driver).is_ok(),
            switch_database: driver.is_postgres_compatible() || matches!(driver, DbDriver::MySql),
            create_database: driver.is_postgres_compatible()
                || matches!(driver, DbDriver::MySql | DbDriver::SqlServer),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SchemaTree {
    /// Postgres-style: schemas sit between the database and its tables.
    schema_level: bool,
    databases: Vec<String>,
    active_database: String,
    db_expanded: bool,
    schemas: Vec<SchemaNode>,
    active_schema: String,
    list: Load,
    filter: String,
    cursor: Option<NodeId>,
}

fn contains_ci(haystack: &str, needle_lower: &str) -> bool {
    needle_lower.is_empty() || haystack.to_lowercase().contains(needle_lower)
}

/// A database the server keeps for itself.
fn is_system_database(name: &str) -> bool {
    matches!(
        name,
        "information_schema" | "performance_schema" | "mysql" | "sys" | "template0" | "template1"
    )
}

/// The last path component, so a SQLite file path and the file name the
/// driver lists for it compare equal.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

impl SchemaTree {
    pub fn new() -> Self {
        Self::default()
    }

    // -- Loading -----------------------------------------------------------------

    pub fn begin_listing(&mut self) {
        self.list = Load::Loading;
    }

    pub fn fail_listing(&mut self, message: impl Into<String>) {
        self.list = Load::Failed(message.into());
    }

    /// A refresh was abandoned (a database switch failed): the rows already
    /// here are still good.
    pub fn finish_listing(&mut self) {
        if self.list == Load::Loading {
            self.list = Load::Ready;
        }
    }

    pub fn listing(&self) -> &Load {
        &self.list
    }

    /// Take what the server reported. Schemas that were already known keep
    /// their tables and expansion; the active schema is expanded.
    ///
    /// `active_schema` is what the workspace's other tools act on, and for a
    /// driver with no schema level it is what names the single hidden node.
    pub fn apply_overview(&mut self, ov: &Overview, active_schema: &str) {
        self.schema_level = !ov.schemas.is_empty();

        let mut databases = ov.databases.clone();
        // The connection's own database may be listed under another spelling
        // (a SQLite path versus its file name).
        let active = databases
            .iter()
            .find(|d| **d == ov.database)
            .or_else(|| databases.iter().find(|d| **d == file_name(&ov.database)))
            .cloned();
        self.active_database = match active {
            Some(name) => name,
            None => {
                if !ov.database.is_empty() {
                    databases.insert(0, ov.database.clone());
                }
                ov.database.clone()
            }
        };
        self.databases = databases;
        if self.list != Load::Ready {
            self.db_expanded = true;
        }

        let names: Vec<String> = if self.schema_level {
            ov.schemas.clone()
        } else {
            vec![active_schema.to_string()]
        };
        let old = std::mem::take(&mut self.schemas);
        self.schemas = names
            .into_iter()
            .map(|name| {
                let kept = old.iter().find(|s| s.name == name).cloned();
                kept.unwrap_or_else(|| SchemaNode {
                    // The schema the workspace is on starts open; a refresh
                    // leaves a schema the user closed closed.
                    expanded: name == active_schema,
                    name,
                    ..SchemaNode::default()
                })
            })
            .collect();
        self.active_schema = active_schema.to_string();
        self.list = Load::Ready;
    }

    /// Mark a schema as being fetched.
    pub fn begin_schema(&mut self, schema: &str) {
        if let Some(s) = self.schemas.iter_mut().find(|s| s.name == schema) {
            s.load = Load::Loading;
        }
    }

    /// Land one schema's tables (or its failure).
    pub fn set_tables(&mut self, schema: &str, result: Result<Vec<TableInfo>, String>) {
        let Some(s) = self.schemas.iter_mut().find(|s| s.name == schema) else {
            return;
        };
        match result {
            Ok(mut tables) => {
                tables.sort_by_key(|t| t.name.to_lowercase());
                s.tables = tables;
                s.load = Load::Ready;
            }
            Err(e) => {
                s.tables.clear();
                s.load = Load::Failed(e);
            }
        }
    }

    /// Forget a schema's tables so the next expand fetches them again.
    pub fn invalidate_schema(&mut self, schema: &str) {
        if let Some(s) = self.schemas.iter_mut().find(|s| s.name == schema) {
            s.load = Load::Idle;
        }
    }

    /// Forget every schema's tables (a manual refresh). Expansion is kept, so
    /// `expanded_unloaded` then says which ones to fetch again.
    pub fn invalidate_all(&mut self) {
        for s in &mut self.schemas {
            s.load = Load::Idle;
        }
    }

    /// Schemas the user has open whose tables are not here yet.
    pub fn expanded_unloaded(&self) -> Vec<String> {
        if !self.db_expanded {
            return Vec::new();
        }
        self.schemas
            .iter()
            .filter(|s| s.expanded && s.load == Load::Idle)
            .map(|s| s.name.clone())
            .collect()
    }

    pub fn set_active_schema(&mut self, schema: &str) {
        self.active_schema = schema.to_string();
    }

    pub fn active_schema(&self) -> &str {
        &self.active_schema
    }

    pub fn active_database(&self) -> &str {
        &self.active_database
    }

    pub fn databases(&self) -> &[String] {
        &self.databases
    }

    pub fn schema_level(&self) -> bool {
        self.schema_level
    }

    pub fn schema(&self, name: &str) -> Option<&SchemaNode> {
        self.schemas.iter().find(|s| s.name == name)
    }

    pub fn schema_names(&self) -> Vec<String> {
        self.schemas.iter().map(|s| s.name.clone()).collect()
    }

    pub fn table(&self, schema: &str, name: &str) -> Option<&TableInfo> {
        self.schema(schema)?.tables.iter().find(|t| t.name == name)
    }

    /// Whether the server has nothing in the one schema this tree shows.
    pub fn is_blank(&self) -> bool {
        self.list == Load::Ready
            && self.schemas.len() <= 1
            && self
                .schemas
                .first()
                .is_none_or(|s| s.load == Load::Ready && s.tables.is_empty())
    }

    // -- Filter -------------------------------------------------------------------

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.to_string();
    }

    fn needle(&self) -> String {
        self.filter.trim().to_lowercase()
    }

    /// Schemas a live search should load, so it can see inside them. Capped.
    pub fn schemas_to_load_for_search(&self) -> Vec<String> {
        if self.needle().is_empty() || !self.db_expanded {
            return Vec::new();
        }
        self.schemas
            .iter()
            .filter(|s| s.load == Load::Idle)
            .take(MAX_SEARCH_LOADS)
            .map(|s| s.name.clone())
            .collect()
    }

    // -- Rows -----------------------------------------------------------------------

    fn database_matches(&self, name: &str, needle: &str) -> bool {
        contains_ci(name, needle)
            || (name == self.active_database
                && self
                    .schemas
                    .iter()
                    .any(|s| self.schema_has_match(s, needle)))
    }

    fn schema_has_match(&self, s: &SchemaNode, needle: &str) -> bool {
        contains_ci(&s.name, needle) && self.schema_level
            || s.tables.iter().any(|t| contains_ci(&t.name, needle))
    }

    fn table_row(&self, s: &SchemaNode, t: &TableInfo, depth: u8) -> TreeRow {
        let view = is_view(&t.table_type);
        TreeRow {
            id: NodeId::Table {
                schema: s.name.clone(),
                name: t.name.clone(),
            },
            depth,
            kind: if view { RowKind::View } else { RowKind::Table },
            label: t.name.clone(),
            detail: match (view, t.row_count) {
                (false, Some(n)) => format!("~{}", thousands(n as usize)),
                _ => String::new(),
            },
            expandable: false,
            expanded: false,
            active: false,
            dim: false,
            busy: false,
        }
    }

    fn note_row(
        &self,
        schema: &str,
        kind: NoteKind,
        label: String,
        detail: &str,
        depth: u8,
    ) -> TreeRow {
        TreeRow {
            id: NodeId::Note(schema.to_string(), kind),
            depth,
            kind: match kind {
                NoteKind::Loading => RowKind::Loading,
                NoteKind::Error => RowKind::Error,
                NoteKind::Empty => RowKind::Empty,
            },
            label,
            detail: detail.to_string(),
            expandable: false,
            expanded: false,
            active: false,
            dim: true,
            busy: false,
        }
    }

    /// Rows for one schema's tables at `depth`.
    fn table_rows(&self, s: &SchemaNode, depth: u8, needle: &str, out: &mut Vec<TreeRow>) {
        // A schema whose name matched shows everything in it.
        let show_all = self.schema_level && contains_ci(&s.name, needle);
        match &s.load {
            Load::Idle => {}
            Load::Loading => {
                if needle.is_empty() {
                    out.push(self.note_row(
                        &s.name,
                        NoteKind::Loading,
                        "Loading tables…".into(),
                        "",
                        depth,
                    ));
                }
            }
            Load::Failed(_) => out.push(self.note_row(
                &s.name,
                NoteKind::Error,
                // The reason is too long for a row; the shell says it in a toast.
                "Could not load tables".to_string(),
                "Retry",
                depth,
            )),
            Load::Ready => {
                if s.tables.is_empty() {
                    // A sole schema says so with a panel of its own.
                    if needle.is_empty() && self.schema_level && self.schemas.len() > 1 {
                        out.push(self.note_row(
                            &s.name,
                            NoteKind::Empty,
                            "No tables".into(),
                            "Create",
                            depth,
                        ));
                    }
                    return;
                }
                for t in &s.tables {
                    if show_all || contains_ci(&t.name, needle) {
                        out.push(self.table_row(s, t, depth));
                    }
                }
            }
        }
    }

    pub fn rows(&self) -> Vec<TreeRow> {
        let needle = self.needle();
        let filtering = !needle.is_empty();
        let mut out = Vec::new();
        let db_level = !self.databases.is_empty();
        let base: u8 = u8::from(db_level);

        if !db_level {
            // No database level to hang schemas on: tables only.
            for s in &self.schemas {
                if self.schema_level {
                    self.push_schema(s, 0, &needle, &mut out);
                } else {
                    self.table_rows(s, 0, &needle, &mut out);
                }
            }
            return out;
        }

        for db in &self.databases {
            let active = *db == self.active_database;
            if filtering && !self.database_matches(db, &needle) {
                continue;
            }
            let expanded = active && (self.db_expanded || filtering);
            let dim = is_system_database(db);
            out.push(TreeRow {
                id: NodeId::Database(db.clone()),
                depth: 0,
                kind: RowKind::Database,
                label: db.clone(),
                detail: String::new(),
                // An inactive database opens by switching to it, so it still
                // has a chevron: it will have children once it is the active one.
                expandable: true,
                expanded,
                active,
                dim,
                busy: active && self.list == Load::Loading,
            });
            if !active || !expanded {
                continue;
            }
            for s in &self.schemas {
                if self.schema_level {
                    self.push_schema(s, base, &needle, &mut out);
                } else {
                    self.table_rows(s, base, &needle, &mut out);
                }
            }
        }
        out
    }

    fn push_schema(&self, s: &SchemaNode, depth: u8, needle: &str, out: &mut Vec<TreeRow>) {
        let filtering = !needle.is_empty();
        if filtering && !self.schema_has_match(s, needle) {
            return;
        }
        let expanded = s.expanded || (filtering && s.load == Load::Ready);
        out.push(TreeRow {
            id: NodeId::Schema(s.name.clone()),
            depth,
            kind: RowKind::Schema,
            label: s.name.clone(),
            detail: if s.load == Load::Ready {
                format!("{}", s.tables.len())
            } else {
                String::new()
            },
            expandable: true,
            expanded,
            active: s.name == self.active_schema,
            dim: false,
            busy: s.load == Load::Loading,
        });
        if expanded {
            self.table_rows(s, depth + 1, needle, out);
        }
    }

    pub fn view(&self) -> TreeView {
        let rows = self.rows();
        let table_count = self.schemas.iter().map(|s| s.tables.len()).sum();
        let cursor = self
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().position(|r| &r.id == c));
        let (status, error) = match &self.list {
            Load::Failed(e) if self.databases.is_empty() && self.schemas.is_empty() => {
                (PanelStatus::Error, e.clone())
            }
            Load::Idle | Load::Loading if self.databases.is_empty() && self.schemas.is_empty() => {
                (PanelStatus::Loading, String::new())
            }
            // A search waiting on schemas it asked for is not "no matches" yet.
            _ if !self.needle().is_empty()
                && rows.is_empty()
                && self.schemas.iter().any(|s| s.load == Load::Loading) =>
            {
                (PanelStatus::Loading, String::new())
            }
            _ if !self.needle().is_empty()
                && !rows.iter().any(|r| {
                    matches!(
                        r.kind,
                        RowKind::Table | RowKind::View | RowKind::Schema | RowKind::Database
                    )
                }) =>
            {
                (PanelStatus::NoMatches, String::new())
            }
            _ if self.needle().is_empty() && self.is_blank() => (PanelStatus::Empty, String::new()),
            _ => (PanelStatus::Populated, String::new()),
        };
        TreeView {
            status,
            rows,
            cursor,
            error,
            table_count,
        }
    }

    // -- Interaction ------------------------------------------------------------------

    pub fn cursor(&self) -> Option<&NodeId> {
        self.cursor.as_ref()
    }

    pub fn set_cursor(&mut self, id: Option<NodeId>) {
        self.cursor = id;
    }

    pub fn id_at(&self, index: usize) -> Option<NodeId> {
        self.rows().get(index).map(|r| r.id.clone())
    }

    fn expand_schema(&mut self, name: &str) -> Action {
        let Some(s) = self.schemas.iter_mut().find(|s| s.name == name) else {
            return Action::None;
        };
        s.expanded = true;
        match s.load {
            Load::Idle | Load::Failed(_) => Action::LoadSchema(name.to_string()),
            _ => Action::Redraw,
        }
    }

    fn collapse_schema(&mut self, name: &str) -> Action {
        match self.schemas.iter_mut().find(|s| s.name == name) {
            Some(s) if s.expanded => {
                s.expanded = false;
                Action::Redraw
            }
            _ => Action::None,
        }
    }

    /// The chevron: expand or collapse a container.
    pub fn toggle(&mut self, id: &NodeId) -> Action {
        match id {
            NodeId::Database(name) if *name != self.active_database => {
                Action::SwitchDatabase(name.clone())
            }
            NodeId::Database(_) => {
                self.db_expanded = !self.db_expanded;
                Action::Redraw
            }
            NodeId::Schema(name) => {
                let open = self.schema(name).is_some_and(|s| s.expanded);
                if open {
                    self.collapse_schema(name)
                } else {
                    self.expand_schema(name)
                }
            }
            _ => Action::None,
        }
    }

    /// A single click: highlight. A note row is a button, so it acts.
    pub fn click(&mut self, id: &NodeId) -> Action {
        self.cursor = Some(id.clone());
        match id {
            NodeId::Note(schema, NoteKind::Error) => Action::LoadSchema(schema.clone()),
            NodeId::Note(schema, NoteKind::Empty) => Action::CreateTable(schema.clone()),
            _ => Action::Redraw,
        }
    }

    /// A double click: open a table, switch to a database, open or close a
    /// schema.
    pub fn open(&mut self, id: &NodeId) -> Action {
        self.cursor = Some(id.clone());
        match id {
            NodeId::Table { schema, name } => Action::OpenTable {
                schema: schema.clone(),
                table: name.clone(),
            },
            NodeId::Database(_) | NodeId::Schema(_) => self.toggle(id),
            NodeId::Note(..) => self.click(id),
        }
    }

    /// The row before or after `from` in `rows`, clamped to the ends.
    fn step(rows: &[TreeRow], from: Option<usize>, delta: isize) -> Option<usize> {
        if rows.is_empty() {
            return None;
        }
        let last = rows.len() as isize - 1;
        match from {
            None => Some(if delta < 0 { last } else { 0 } as usize),
            Some(i) => Some((i as isize + delta).clamp(0, last) as usize),
        }
    }

    /// The nearest row above `index` that is shallower: the parent.
    fn parent_index(rows: &[TreeRow], index: usize) -> Option<usize> {
        let depth = rows[index].depth;
        if depth == 0 {
            return None;
        }
        (0..index).rev().find(|&i| rows[i].depth < depth)
    }

    /// Arrow keys, Home/End and Enter. Left collapses an open container, or
    /// else goes to the parent; Right opens a closed one, or else goes to its
    /// first child; Enter opens a table or toggles a container.
    pub fn key(&mut self, key: NavKey) -> Action {
        let rows = self.rows();
        if rows.is_empty() {
            return Action::None;
        }
        let at = self
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().position(|r| &r.id == c));

        let move_to = |this: &mut Self, i: Option<usize>| match i {
            Some(i) if Some(i) != at => {
                this.cursor = Some(rows[i].id.clone());
                Action::Redraw
            }
            Some(_) | None => Action::None,
        };

        match key {
            NavKey::Down => {
                let to = Self::step(&rows, at, 1);
                move_to(self, to)
            }
            NavKey::Up => {
                let to = Self::step(&rows, at, -1);
                move_to(self, to)
            }
            NavKey::Home => move_to(self, Some(0)),
            NavKey::End => move_to(self, Some(rows.len() - 1)),
            NavKey::Left => {
                let Some(i) = at else {
                    return move_to(self, Some(0));
                };
                let row = &rows[i];
                if row.expandable && row.expanded {
                    let id = row.id.clone();
                    return self.toggle_collapse(&id);
                }
                let parent = Self::parent_index(&rows, i);
                move_to(self, parent)
            }
            NavKey::Right => {
                let Some(i) = at else {
                    return move_to(self, Some(0));
                };
                let row = &rows[i];
                if row.expandable && !row.expanded {
                    let id = row.id.clone();
                    return self.toggle(&id);
                }
                // Already open: step into the first child, if it has one.
                let child = rows
                    .get(i + 1)
                    .filter(|next| next.depth > row.depth)
                    .map(|_| i + 1);
                move_to(self, child)
            }
            NavKey::Enter => {
                let Some(i) = at else {
                    return move_to(self, Some(0));
                };
                let id = rows[i].id.clone();
                self.open(&id)
            }
        }
    }

    /// Collapse without ever switching database (Left on the active database).
    fn toggle_collapse(&mut self, id: &NodeId) -> Action {
        match id {
            NodeId::Database(name) if *name == self.active_database => {
                self.db_expanded = false;
                Action::Redraw
            }
            NodeId::Schema(name) => self.collapse_schema(name),
            _ => Action::None,
        }
    }

    // -- Context menu --------------------------------------------------------------------

    pub fn menu_for(&self, id: &NodeId, caps: Caps) -> Vec<MenuEntry> {
        let entry = |action, label, icon, enabled| MenuEntry {
            action,
            label,
            icon,
            enabled,
            danger: false,
            divider_above: false,
        };
        match id {
            NodeId::Database(name) => {
                let mut items = Vec::new();
                if *name != self.active_database {
                    items.push(entry(
                        MenuAction::SwitchDatabase,
                        "Switch to this database",
                        "database",
                        caps.switch_database,
                    ));
                }
                items.push(entry(
                    MenuAction::Refresh,
                    "Refresh",
                    "refresh-cw",
                    *name == self.active_database,
                ));
                if caps.create_database {
                    let mut e = entry(MenuAction::CreateDatabase, "Create Database…", "plus", true);
                    e.divider_above = true;
                    items.push(e);
                }
                items
            }
            NodeId::Schema(_) | NodeId::Note(..) => vec![
                entry(MenuAction::CreateTable, "Create Table…", "plus", caps.ddl),
                entry(MenuAction::ImportData, "Import Data…", "upload", caps.ddl),
                entry(MenuAction::Refresh, "Refresh", "refresh-cw", true),
            ],
            NodeId::Table { schema, name } => {
                let view = self
                    .table(schema, name)
                    .is_some_and(|t| is_view(&t.table_type));
                let mut items = vec![
                    entry(MenuAction::ViewData, "View Data", "table", true),
                    entry(MenuAction::GenerateSelect, "Generate SELECT", "code", true),
                ];
                if !view {
                    items.push(entry(
                        MenuAction::GenerateInsert,
                        "Generate INSERT",
                        "code",
                        true,
                    ));
                }
                items.push(entry(MenuAction::CopyName, "Copy Name", "copy", true));
                if !view {
                    let mut edit = entry(MenuAction::EditTable, "Edit Table…", "pencil", caps.ddl);
                    edit.divider_above = true;
                    items.push(edit);
                    items.push(entry(
                        MenuAction::ImportData,
                        "Import Data…",
                        "upload",
                        caps.ddl,
                    ));
                }
                items.push(entry(MenuAction::Refresh, "Refresh", "refresh-cw", true));
                if !view {
                    let mut drop = entry(MenuAction::DropTable, "Drop Table…", "trash-2", caps.ddl);
                    drop.danger = true;
                    drop.divider_above = true;
                    items.push(drop);
                }
                items
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dropping a table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dependents {
    Loading,
    Ready(Vec<TableRef>),
    /// The foreign keys could not be read. The drop may still go ahead (the
    /// user typed the name), but the dialog says it could not check.
    Failed(String),
}

/// The typed-name confirmation behind "Drop Table".
#[derive(Debug, Clone)]
pub struct DropDialog {
    /// The schema node the table belongs to (what metadata calls take).
    pub schema: String,
    /// What goes in front of the table in SQL, empty when the dialect
    /// addresses tables bare.
    pub qualifier: String,
    pub table: String,
    pub typed: String,
    pub dependents: Dependents,
    pub cascade: bool,
    pub postgres: bool,
    pub running: bool,
    pub error: String,
}

impl DropDialog {
    pub fn new(schema: &str, qualifier: &str, table: &str, postgres: bool) -> Self {
        Self {
            schema: schema.to_string(),
            qualifier: qualifier.to_string(),
            table: table.to_string(),
            typed: String::new(),
            dependents: Dependents::Loading,
            cascade: false,
            postgres,
            running: false,
            error: String::new(),
        }
    }

    pub fn dependent_tables(&self) -> &[TableRef] {
        match &self.dependents {
            Dependents::Ready(d) => d,
            _ => &[],
        }
    }

    /// CASCADE is offered only on Postgres, and only once the dialog has
    /// shown which tables depend on this one.
    pub fn cascade_offered(&self) -> bool {
        self.postgres && !self.dependent_tables().is_empty()
    }

    /// The name must be typed exactly, and the dependency check must have
    /// finished (or failed, which the dialog reports).
    pub fn can_drop(&self) -> bool {
        !self.running
            && self.dependents != Dependents::Loading
            && drop_confirmed(&self.table, &self.typed)
    }

    pub fn warning(&self) -> String {
        match &self.dependents {
            Dependents::Loading => "Checking which tables depend on this one…".into(),
            Dependents::Failed(e) => format!(
                "Could not check which tables depend on {}: {e}. Dropping it may fail or break them.",
                self.table
            ),
            Dependents::Ready(d) if d.is_empty() => String::new(),
            Dependents::Ready(d) => {
                let names: Vec<String> = d.iter().take(8).map(TableRef::display).collect();
                let more = d.len().saturating_sub(8);
                let mut list = names.join(", ");
                if more > 0 {
                    list.push_str(&format!(" and {more} more"));
                }
                let noun = if d.len() == 1 { "table has" } else { "tables have" };
                format!("{} {noun} foreign keys to {}: {list}.", d.len(), self.table)
            }
        }
    }

    pub fn typed_hint(&self) -> String {
        format!("Type {} to confirm", self.table)
    }
}

// ---------------------------------------------------------------------------
// Creating a database
// ---------------------------------------------------------------------------

/// The "Create Database" dialog: a name, the statement it would run, and the
/// server's answer if it refused.
#[derive(Debug, Clone)]
pub struct CreateDatabaseDialog {
    pub driver: DbDriver,
    pub name: String,
    pub running: bool,
    /// What the server said when it refused.
    pub server_error: String,
}

impl CreateDatabaseDialog {
    pub fn new(driver: &DbDriver) -> Self {
        Self {
            driver: driver.clone(),
            name: String::new(),
            running: false,
            server_error: String::new(),
        }
    }

    /// Why the name is not acceptable, in the core's own words. Empty while
    /// the box is still empty: that is a prompt, not a complaint.
    pub fn name_error(&self) -> String {
        crate::table_editor::database_name_errors(&self.name)
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    /// The validation message, or else the server's refusal.
    pub fn error(&self) -> String {
        let local = self.name_error();
        if local.is_empty() {
            self.server_error.clone()
        } else {
            local
        }
    }

    pub fn can_create(&self) -> bool {
        !self.running && !self.name.trim().is_empty() && self.name_error().is_empty()
    }

    /// The statement the dialog would run, or nothing while the name is empty
    /// or invalid.
    pub fn sql(&self) -> String {
        if self.name.trim().is_empty() || !self.name_error().is_empty() {
            return String::new();
        }
        crate::table_editor::preview_create_database_sql(&self.driver, &self.name)
            .ok()
            .and_then(|v| v.into_iter().next())
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Dragging a table into the editor
// ---------------------------------------------------------------------------

/// Where a drag from the tree may land, in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DropZone {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DropZone {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.x
            && y >= self.y
            && x < self.x + self.width
            && y < self.y + self.height
    }
}

/// Whether a name dropped after `c` needs a space between them. Not after
/// whitespace, an opening parenthesis or a qualifying dot.
fn space_after(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '(' | '.')
}

/// Whether a name dropped before `c` needs a space between them. Not before
/// whitespace or the punctuation that follows a name.
fn space_before(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ')' | ',' | ';' | '.')
}

/// Put a dropped (already quoted) name into `text` at byte offset `caret`,
/// with a space on either side where it would otherwise run into the text
/// around it.
/// A caret that is unknown (negative) or past the end means the end; one
/// inside a character moves back to its start. Returns the new text and the
/// caret after the name.
pub fn insert_dropped_name(text: &str, caret: i64, name: &str) -> (String, usize) {
    let mut at = usize::try_from(caret).unwrap_or(text.len()).min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let (before, after) = text.split_at(at);
    let lead = before.chars().next_back().is_some_and(space_after);
    let trail = after.chars().next().is_some_and(space_before);
    let mut out = String::with_capacity(text.len() + name.len() + 2);
    out.push_str(before);
    if lead {
        out.push(' ');
    }
    out.push_str(name);
    let caret = out.len();
    if trail {
        out.push(' ');
    }
    out.push_str(after);
    (out, caret)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, kind: &str, rows: Option<u64>) -> TableInfo {
        let mut t = TableInfo::new(name.to_string(), "s".to_string(), kind.to_string());
        t.row_count = rows;
        t
    }

    fn pg_tree() -> SchemaTree {
        let mut t = SchemaTree::new();
        t.apply_overview(
            &Overview {
                databases: vec!["analytics".into(), "app".into()],
                database: "app".into(),
                schemas: vec!["public".into(), "billing".into()],
            },
            "public",
        );
        t.set_tables(
            "public",
            Ok(vec![
                table("orders", "BASE TABLE", Some(12_345)),
                table("Customers", "BASE TABLE", Some(5)),
                table("order_totals", "VIEW", None),
            ]),
        );
        t
    }

    fn labels(t: &SchemaTree) -> Vec<String> {
        t.rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth as usize), r.label))
            .collect()
    }

    fn mysql_tree() -> SchemaTree {
        let mut t = SchemaTree::new();
        t.apply_overview(
            &Overview {
                databases: vec!["app".into(), "mysql".into()],
                database: "app".into(),
                schemas: vec![],
            },
            "app",
        );
        t.set_tables("app", Ok(vec![table("users", "BASE TABLE", Some(3))]));
        t
    }

    #[test]
    fn databases_hold_schemas_which_hold_tables() {
        let t = pg_tree();
        assert_eq!(
            labels(&t),
            vec![
                "analytics",
                "app",
                "  public",
                "    Customers",
                "    order_totals",
                "    orders",
                "  billing",
            ]
        );
    }

    #[test]
    fn a_driver_without_schemas_hangs_tables_off_the_database() {
        let t = mysql_tree();
        assert_eq!(labels(&t), vec!["app", "  users", "mysql"]);
        assert!(!t.schema_level());
    }

    #[test]
    fn views_are_told_apart_and_only_tables_show_a_count() {
        let t = pg_tree();
        let rows = t.rows();
        let view = rows.iter().find(|r| r.label == "order_totals").unwrap();
        assert_eq!(view.kind, RowKind::View);
        assert_eq!(view.detail, "");
        let orders = rows.iter().find(|r| r.label == "orders").unwrap();
        assert_eq!(orders.kind, RowKind::Table);
        assert_eq!(orders.detail, "~12,345");
    }

    #[test]
    fn the_active_database_and_schema_are_marked() {
        let t = pg_tree();
        let rows = t.rows();
        let active: Vec<&str> = rows
            .iter()
            .filter(|r| r.active)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(active, vec!["app", "public"]);
    }

    #[test]
    fn system_databases_are_dimmed() {
        let t = mysql_tree();
        let rows = t.rows();
        assert!(rows.iter().find(|r| r.label == "mysql").unwrap().dim);
        assert!(!rows.iter().find(|r| r.label == "app").unwrap().dim);
    }

    #[test]
    fn expanding_an_unloaded_schema_asks_for_its_tables() {
        let mut t = pg_tree();
        let billing = NodeId::Schema("billing".into());
        assert_eq!(t.toggle(&billing), Action::LoadSchema("billing".into()));
        t.begin_schema("billing");
        assert!(t.rows().iter().any(|r| r.kind == RowKind::Loading));
        t.set_tables(
            "billing",
            Ok(vec![table("invoices", "BASE TABLE", Some(9))]),
        );
        assert!(labels(&t).contains(&"    invoices".to_string()));
        // Collapsing and expanding again does not refetch.
        assert_eq!(t.toggle(&billing), Action::Redraw);
        assert_eq!(t.toggle(&billing), Action::Redraw);
    }

    #[test]
    fn a_failed_schema_shows_the_error_and_retries_on_click() {
        let mut t = pg_tree();
        t.toggle(&NodeId::Schema("billing".into()));
        t.set_tables("billing", Err("permission denied".into()));
        let rows = t.rows();
        let err = rows.iter().find(|r| r.kind == RowKind::Error).unwrap();
        assert_eq!(err.label, "Could not load tables");
        assert_eq!(err.detail, "Retry");
        assert_eq!(
            t.click(&err.id.clone()),
            Action::LoadSchema("billing".into())
        );
    }

    #[test]
    fn an_empty_schema_among_several_offers_create_table() {
        let mut t = pg_tree();
        t.toggle(&NodeId::Schema("billing".into()));
        t.set_tables("billing", Ok(vec![]));
        let rows = t.rows();
        let empty = rows.iter().find(|r| r.kind == RowKind::Empty).unwrap();
        assert_eq!(
            t.click(&empty.id.clone()),
            Action::CreateTable("billing".into())
        );
    }

    #[test]
    fn the_sole_empty_schema_is_the_empty_panel_not_a_row() {
        let mut t = SchemaTree::new();
        t.apply_overview(
            &Overview {
                databases: vec!["app".into()],
                database: "app".into(),
                schemas: vec![],
            },
            "app",
        );
        t.set_tables("app", Ok(vec![]));
        let v = t.view();
        assert_eq!(v.status, PanelStatus::Empty);
        // The database row stays, so the user can still switch away.
        assert_eq!(v.rows.len(), 1);
    }

    #[test]
    fn the_four_panel_states() {
        let mut t = SchemaTree::new();
        assert_eq!(t.view().status, PanelStatus::Loading);
        t.begin_listing();
        assert_eq!(t.view().status, PanelStatus::Loading);
        t.fail_listing("timeout");
        let v = t.view();
        assert_eq!(v.status, PanelStatus::Error);
        assert_eq!(v.error, "timeout");
        assert_eq!(pg_tree().view().status, PanelStatus::Populated);
    }

    #[test]
    fn filtering_keeps_matches_and_their_parents() {
        let mut t = pg_tree();
        t.set_filter("ORDER");
        assert_eq!(
            labels(&t),
            vec!["app", "  public", "    order_totals", "    orders"]
        );
    }

    #[test]
    fn a_schema_name_match_shows_its_whole_contents() {
        let mut t = pg_tree();
        t.set_filter("publ");
        assert_eq!(
            labels(&t),
            vec![
                "app",
                "  public",
                "    Customers",
                "    order_totals",
                "    orders"
            ]
        );
    }

    #[test]
    fn filtering_matches_a_database_name_too() {
        let mut t = pg_tree();
        t.set_filter("analy");
        assert_eq!(labels(&t), vec!["analytics"]);
    }

    #[test]
    fn a_filter_that_matches_nothing_is_the_no_matches_state() {
        let mut t = pg_tree();
        t.set_filter("zzz");
        let v = t.view();
        assert_eq!(v.status, PanelStatus::NoMatches);
        assert!(v.rows.is_empty());
        t.set_filter("");
        assert_eq!(t.view().status, PanelStatus::Populated);
    }

    #[test]
    fn a_search_asks_for_schemas_it_has_not_seen_yet() {
        let mut t = pg_tree();
        assert!(t.schemas_to_load_for_search().is_empty());
        t.set_filter("inv");
        assert_eq!(t.schemas_to_load_for_search(), vec!["billing".to_string()]);
        t.begin_schema("billing");
        assert!(t.schemas_to_load_for_search().is_empty());
    }

    // -- Keyboard --------------------------------------------------------------------

    fn cursor_label(t: &SchemaTree) -> String {
        let v = t.view();
        v.cursor
            .map(|i| v.rows[i].label.clone())
            .unwrap_or_default()
    }

    #[test]
    fn arrows_walk_the_visible_rows_and_stop_at_the_ends() {
        let mut t = pg_tree();
        assert_eq!(cursor_label(&t), "");
        t.key(NavKey::Down);
        assert_eq!(cursor_label(&t), "analytics");
        assert_eq!(t.key(NavKey::Up), Action::None);
        t.key(NavKey::End);
        assert_eq!(cursor_label(&t), "billing");
        assert_eq!(t.key(NavKey::Down), Action::None);
        t.key(NavKey::Home);
        assert_eq!(cursor_label(&t), "analytics");
    }

    #[test]
    fn left_collapses_then_goes_to_the_parent() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Schema("public".into())));
        t.key(NavKey::Left);
        assert!(!labels(&t).iter().any(|l| l.contains("orders")));
        assert_eq!(cursor_label(&t), "public");
        t.key(NavKey::Left);
        assert_eq!(cursor_label(&t), "app");
    }

    #[test]
    fn right_expands_then_steps_into_the_first_child() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Schema("public".into())));
        t.key(NavKey::Left);
        t.key(NavKey::Right);
        assert!(labels(&t).iter().any(|l| l.contains("orders")));
        assert_eq!(cursor_label(&t), "public");
        t.key(NavKey::Right);
        assert_eq!(cursor_label(&t), "Customers");
        // A leaf has nowhere to go.
        assert_eq!(t.key(NavKey::Right), Action::None);
    }

    #[test]
    fn left_on_a_table_goes_to_its_schema() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        }));
        t.key(NavKey::Left);
        assert_eq!(cursor_label(&t), "public");
    }

    #[test]
    fn right_on_an_unloaded_schema_loads_it() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Schema("billing".into())));
        assert_eq!(t.key(NavKey::Right), Action::LoadSchema("billing".into()));
    }

    #[test]
    fn enter_opens_a_table_and_toggles_a_schema() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        }));
        assert_eq!(
            t.key(NavKey::Enter),
            Action::OpenTable {
                schema: "public".into(),
                table: "orders".into()
            }
        );
        t.set_cursor(Some(NodeId::Schema("public".into())));
        assert_eq!(t.key(NavKey::Enter), Action::Redraw);
        assert!(!labels(&t).iter().any(|l| l.contains("orders")));
    }

    #[test]
    fn enter_on_another_database_switches_to_it() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Database("analytics".into())));
        assert_eq!(
            t.key(NavKey::Enter),
            Action::SwitchDatabase("analytics".into())
        );
    }

    #[test]
    fn left_on_the_active_database_collapses_it_and_never_switches() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Database("app".into())));
        assert_eq!(t.key(NavKey::Left), Action::Redraw);
        assert_eq!(labels(&t), vec!["analytics", "app"]);
        assert_eq!(t.key(NavKey::Right), Action::Redraw);
        assert!(labels(&t).len() > 2);
    }

    #[test]
    fn a_cursor_on_a_hidden_row_starts_over_from_the_top() {
        let mut t = pg_tree();
        t.set_cursor(Some(NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        }));
        t.set_filter("analy");
        t.key(NavKey::Down);
        assert_eq!(cursor_label(&t), "analytics");
    }

    #[test]
    fn navigation_on_an_empty_tree_does_nothing() {
        let mut t = SchemaTree::new();
        for k in [
            NavKey::Up,
            NavKey::Down,
            NavKey::Home,
            NavKey::End,
            NavKey::Left,
            NavKey::Right,
            NavKey::Enter,
        ] {
            assert_eq!(t.key(k), Action::None);
        }
    }

    // -- Mouse -----------------------------------------------------------------------------

    #[test]
    fn a_click_highlights_and_a_double_click_opens() {
        let mut t = pg_tree();
        let orders = NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        };
        assert_eq!(t.click(&orders), Action::Redraw);
        assert_eq!(t.cursor(), Some(&orders));
        assert_eq!(
            t.open(&orders),
            Action::OpenTable {
                schema: "public".into(),
                table: "orders".into()
            }
        );
    }

    #[test]
    fn a_click_on_another_database_only_highlights_it() {
        let mut t = pg_tree();
        let other = NodeId::Database("analytics".into());
        assert_eq!(t.click(&other), Action::Redraw);
        assert_eq!(t.open(&other), Action::SwitchDatabase("analytics".into()));
    }

    // -- Refresh keeps the user's place --------------------------------------------------------

    #[test]
    fn a_new_overview_keeps_loaded_schemas_and_expansion() {
        let mut t = pg_tree();
        t.toggle(&NodeId::Schema("public".into()));
        t.apply_overview(
            &Overview {
                databases: vec!["analytics".into(), "app".into()],
                database: "app".into(),
                schemas: vec!["public".into(), "billing".into()],
            },
            "public",
        );
        let public = t.schema("public").unwrap();
        assert_eq!(public.tables.len(), 3);
        assert!(!public.expanded, "a collapsed schema stays collapsed");
    }

    #[test]
    fn a_manual_refresh_refetches_only_what_is_open() {
        let mut t = pg_tree();
        t.toggle(&NodeId::Schema("billing".into()));
        t.set_tables("billing", Ok(vec![]));
        t.invalidate_all();
        assert_eq!(
            t.expanded_unloaded(),
            vec!["public".to_string(), "billing".to_string()]
        );
        t.toggle(&NodeId::Schema("billing".into()));
        assert_eq!(t.expanded_unloaded(), vec!["public".to_string()]);
    }

    #[test]
    fn a_sqlite_path_matches_the_file_name_the_driver_lists() {
        let mut t = SchemaTree::new();
        t.apply_overview(
            &Overview {
                databases: vec!["shop.sqlite".into()],
                database: "/tmp/x/shop.sqlite".into(),
                schemas: vec![],
            },
            "/tmp/x/shop.sqlite",
        );
        assert_eq!(t.active_database(), "shop.sqlite");
        assert_eq!(t.databases().len(), 1);
        assert!(t.rows()[0].active);
    }

    // -- Menus --------------------------------------------------------------------------------

    fn caps() -> Caps {
        Caps {
            ddl: true,
            switch_database: true,
            create_database: false,
        }
    }

    fn actions(items: &[MenuEntry]) -> Vec<MenuAction> {
        items.iter().map(|m| m.action).collect()
    }

    #[test]
    fn a_table_menu_has_the_documented_entries_in_order() {
        let t = pg_tree();
        let id = NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        };
        assert_eq!(
            actions(&t.menu_for(&id, caps())),
            vec![
                MenuAction::ViewData,
                MenuAction::GenerateSelect,
                MenuAction::GenerateInsert,
                MenuAction::CopyName,
                MenuAction::EditTable,
                MenuAction::ImportData,
                MenuAction::Refresh,
                MenuAction::DropTable,
            ]
        );
        let items = t.menu_for(&id, caps());
        assert!(items.last().unwrap().danger);
    }

    #[test]
    fn a_view_cannot_be_edited_dropped_or_inserted_into() {
        let t = pg_tree();
        let id = NodeId::Table {
            schema: "public".into(),
            name: "order_totals".into(),
        };
        let a = actions(&t.menu_for(&id, caps()));
        assert!(!a.contains(&MenuAction::EditTable));
        assert!(!a.contains(&MenuAction::DropTable));
        assert!(!a.contains(&MenuAction::GenerateInsert));
        assert!(!a.contains(&MenuAction::ImportData), "a view takes no rows");
        assert!(a.contains(&MenuAction::GenerateSelect));
    }

    #[test]
    fn a_schema_menu_is_create_table_import_and_refresh() {
        let t = pg_tree();
        let items = t.menu_for(&NodeId::Schema("public".into()), caps());
        assert_eq!(
            actions(&items),
            vec![
                MenuAction::CreateTable,
                MenuAction::ImportData,
                MenuAction::Refresh
            ]
        );
        assert_eq!(items[1].label, "Import Data…");
    }

    #[test]
    fn ddl_entries_are_disabled_when_the_driver_cannot_do_ddl() {
        let t = pg_tree();
        let no_ddl = Caps {
            ddl: false,
            ..caps()
        };
        let id = NodeId::Table {
            schema: "public".into(),
            name: "orders".into(),
        };
        let items = t.menu_for(&id, no_ddl);
        for m in items.iter().filter(|m| {
            matches!(
                m.action,
                MenuAction::EditTable | MenuAction::DropTable | MenuAction::ImportData
            )
        }) {
            assert!(!m.enabled, "{:?} should be disabled", m.action);
        }
        assert!(
            items
                .iter()
                .find(|m| m.action == MenuAction::ViewData)
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn a_database_menu_offers_switching_only_for_the_others() {
        let t = pg_tree();
        let active = actions(&t.menu_for(&NodeId::Database("app".into()), caps()));
        assert!(!active.contains(&MenuAction::SwitchDatabase));
        let other = actions(&t.menu_for(&NodeId::Database("analytics".into()), caps()));
        assert!(other.contains(&MenuAction::SwitchDatabase));
    }

    #[test]
    fn caps_follow_the_driver() {
        let pg = Caps::for_driver(&DbDriver::Postgres);
        assert!(pg.ddl && pg.switch_database && pg.create_database);
        let lite = Caps::for_driver(&DbDriver::Sqlite);
        assert!(lite.ddl && !lite.switch_database && !lite.create_database);
        let redis = Caps::for_driver(&DbDriver::Redis);
        assert!(!redis.ddl);
    }

    // -- Create database ---------------------------------------------------------------------

    #[test]
    fn a_new_database_needs_a_valid_name() {
        let mut d = CreateDatabaseDialog::new(&DbDriver::Postgres);
        assert!(!d.can_create(), "empty is not creatable");
        assert_eq!(d.error(), "", "empty is a prompt, not an error");
        d.name = "1bad".into();
        assert!(!d.can_create());
        assert!(d.error().contains("start with a"), "{}", d.error());
        d.name = "my db".into();
        assert!(!d.can_create());
        d.name = "analytics_2026".into();
        assert!(d.can_create());
        assert_eq!(d.error(), "");
    }

    #[test]
    fn the_statement_is_quoted_by_the_dialect() {
        let mut d = CreateDatabaseDialog::new(&DbDriver::Postgres);
        d.name = "my-db".into();
        assert_eq!(d.sql(), "CREATE DATABASE \"my-db\"");
        let mut m = CreateDatabaseDialog::new(&DbDriver::MySql);
        m.name = "my-db".into();
        assert_eq!(m.sql(), "CREATE DATABASE `my-db`");
    }

    #[test]
    fn sql_server_can_create_a_database_in_brackets() {
        assert!(Caps::for_driver(&DbDriver::SqlServer).create_database);
        let mut d = CreateDatabaseDialog::new(&DbDriver::SqlServer);
        d.name = "my-db".into();
        assert!(d.can_create());
        assert_eq!(d.sql(), "CREATE DATABASE [my-db]");
        // Nothing that could close the bracket gets as far as a statement.
        d.name = "x]; DROP DATABASE app; --".into();
        assert!(!d.can_create());
        assert_eq!(d.sql(), "");
    }

    #[test]
    fn no_statement_is_shown_for_a_name_that_cannot_be_created() {
        let mut d = CreateDatabaseDialog::new(&DbDriver::Postgres);
        d.name = "x; DROP DATABASE app".into();
        assert_eq!(d.sql(), "");
        assert!(!d.can_create());
    }

    #[test]
    fn a_server_refusal_shows_until_the_name_becomes_invalid() {
        let mut d = CreateDatabaseDialog::new(&DbDriver::Postgres);
        d.name = "app".into();
        d.server_error = "database \"app\" already exists".into();
        assert!(d.error().contains("already exists"));
        d.name = "9".into();
        assert!(
            d.error().contains("start with a"),
            "a local problem outranks a stale refusal"
        );
    }

    #[test]
    fn nothing_is_created_twice() {
        let mut d = CreateDatabaseDialog::new(&DbDriver::Postgres);
        d.name = "fresh".into();
        d.running = true;
        assert!(!d.can_create());
    }

    // -- Drop dialog ----------------------------------------------------------------------------

    fn dep(schema: &str, name: &str) -> TableRef {
        TableRef {
            schema: schema.into(),
            name: name.into(),
        }
    }

    #[test]
    fn the_name_must_be_typed_exactly_and_the_check_finished() {
        let mut d = DropDialog::new("public", "public", "orders", true);
        d.typed = "orders".into();
        assert!(!d.can_drop(), "still checking dependents");
        d.dependents = Dependents::Ready(vec![]);
        assert!(d.can_drop());
        d.typed = "Orders".into();
        assert!(!d.can_drop(), "case matters");
        d.typed = "order".into();
        assert!(!d.can_drop());
        d.typed = " orders ".into();
        assert!(d.can_drop(), "surrounding space is forgiven");
        d.running = true;
        assert!(!d.can_drop());
    }

    #[test]
    fn a_failed_dependency_check_still_allows_a_typed_drop_but_warns() {
        let mut d = DropDialog::new("public", "public", "orders", true);
        d.dependents = Dependents::Failed("denied".into());
        d.typed = "orders".into();
        assert!(d.can_drop());
        assert!(d.warning().contains("Could not check"));
    }

    #[test]
    fn dependents_produce_a_warning_that_lists_them() {
        let mut d = DropDialog::new("public", "public", "customers", true);
        d.dependents = Dependents::Ready(vec![dep("public", "orders"), dep("billing", "invoices")]);
        let w = d.warning();
        assert!(
            w.starts_with("2 tables have foreign keys to customers"),
            "{w}"
        );
        assert!(w.contains("public.orders") && w.contains("billing.invoices"));
        d.dependents = Dependents::Ready(vec![dep("public", "orders")]);
        assert!(d.warning().starts_with("1 table has foreign keys"));
    }

    #[test]
    fn a_long_dependent_list_is_cut_off() {
        let mut d = DropDialog::new("public", "public", "t", true);
        d.dependents = Dependents::Ready(
            (0..12)
                .map(|i| dep("public", &format!("child_{i:02}")))
                .collect(),
        );
        let w = d.warning();
        assert!(w.contains("and 4 more"), "{w}");
        assert!(!w.contains("child_11"));
    }

    #[test]
    fn cascade_is_offered_on_postgres_and_only_with_dependents() {
        let mut d = DropDialog::new("public", "public", "customers", true);
        assert!(
            !d.cascade_offered(),
            "not before the warning has anything to show"
        );
        d.dependents = Dependents::Ready(vec![]);
        assert!(!d.cascade_offered());
        d.dependents = Dependents::Ready(vec![dep("public", "orders")]);
        assert!(d.cascade_offered());
        d.postgres = false;
        assert!(!d.cascade_offered(), "other databases have no CASCADE");
    }

    #[test]
    fn nothing_is_dropped_while_cascade_is_merely_available() {
        let mut d = DropDialog::new("public", "public", "customers", true);
        d.dependents = Dependents::Ready(vec![dep("public", "orders")]);
        assert!(!d.cascade, "off until the user turns it on");
    }

    // -- Dragging into the editor -------------------------------------------------------------

    #[test]
    fn a_drop_zone_contains_its_inside_only() {
        let z = DropZone {
            x: 300.0,
            y: 100.0,
            width: 500.0,
            height: 200.0,
        };
        assert!(z.contains(300.0, 100.0));
        assert!(z.contains(799.0, 299.0));
        assert!(!z.contains(800.0, 150.0));
        assert!(!z.contains(299.0, 150.0));
        assert!(
            !DropZone::default().contains(0.0, 0.0),
            "an unpublished zone takes nothing"
        );
    }

    #[test]
    fn a_dropped_name_goes_in_at_the_caret_with_spaces_where_needed() {
        let (t, c) = insert_dropped_name("SELECT * FROM ", 14, "\"orders\"");
        assert_eq!(t, "SELECT * FROM \"orders\"");
        assert_eq!(c, t.len());

        // Between two words both sides get a space.
        let (t, c) = insert_dropped_name("SELECT aFROM", 8, "\"t\"");
        assert_eq!(t, "SELECT a \"t\" FROM");
        assert_eq!(&t[..c], "SELECT a \"t\"");

        // After a statement's semicolon, and before a comma, as typed.
        let (t, _) = insert_dropped_name("SELECT 1;", 9, "\"t\"");
        assert_eq!(t, "SELECT 1; \"t\"");
        let (t, _) = insert_dropped_name("SELECT a, FROM", 8, "b");
        assert_eq!(t, "SELECT a b, FROM");
        let (t, _) = insert_dropped_name("count()", 6, "x");
        assert_eq!(t, "count(x)");

        // An unknown caret appends.
        let (t, _) = insert_dropped_name("SELECT 1", -1, "`t`");
        assert_eq!(t, "SELECT 1 `t`");
        let (t, _) = insert_dropped_name("", -1, "[t]");
        assert_eq!(t, "[t]");
    }

    #[test]
    fn a_caret_past_the_end_or_inside_a_character_is_made_safe() {
        let (t, _) = insert_dropped_name("ab", 99, "x");
        assert_eq!(t, "ab x");
        // "é" is two bytes; offset 1 is inside it.
        let (t, c) = insert_dropped_name("é", 1, "x");
        assert_eq!(t, "x é");
        assert_eq!(c, 1);
    }
}
