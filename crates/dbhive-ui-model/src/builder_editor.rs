//! The visual query builder's editing layer.
//!
//! `query_builder` holds the statement model and the SQL generator. This file
//! is what a screen drives: every edit the builder UI can make is an
//! `apply(action, i, j, text)` call, and everything the screen draws comes
//! back from `view`. A shell with no toolkit knowledge can therefore run the
//! whole builder in a test, which is what keeps `.slint` free of logic.
//!
//! Rules that carry over from the generator:
//!
//! * Partial state produces no SQL, never broken SQL. A half-filled join or
//!   condition is dropped from the statement and explained as a warning.
//! * Identifiers and literals are quoted and escaped by the caller-supplied
//!   driver functions; nothing here writes user text into SQL.
//!
//! Row addressing: lists the screen shows are addressed by position, and the
//! WHERE tree is addressed by its depth-first position in [`flatten_filters`].
//! The screen never sees a path, so it cannot hold a stale one.

use dbhive_core::models::ForeignKeyInfo;

use crate::query_builder::{
    active_aliases, count_conditions, generate_sql, join_suggestions, pagination_summary, validate,
    Aggregate, BuilderState, CompareOp, Condition, ConditionGroup, DialectRules, GroupByClause,
    HavingCondition, Join, JoinType, Logical, OrderByClause, QueryTable, SelectedColumn, SortDir,
    ValueKind, MAX_GROUP_DEPTH, STAR,
};

/// How many catalog tables the add-table list shows at once.
pub const CATALOG_LIMIT: usize = 200;

/// The seven collapsible sections, in screen order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Tables,
    Joins,
    Columns,
    Filters,
    GroupBy,
    OrderBy,
    Limit,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Tables,
        Section::Joins,
        Section::Columns,
        Section::Filters,
        Section::GroupBy,
        Section::OrderBy,
        Section::Limit,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Tables => "Tables",
            Section::Joins => "Joins",
            Section::Columns => "Columns",
            Section::Filters => "Where",
            Section::GroupBy => "Group by and having",
            Section::OrderBy => "Order by",
            Section::Limit => "Limit and offset",
        }
    }
}

/// What an edit changed, so the shell redraws no more than it must.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing happened (a refused or unknown action).
    None,
    /// Only text typed into a row changed. Row models must NOT be rebuilt:
    /// rebuilding recreates the focused field and drops the caret. The SQL,
    /// warnings and counts still need refreshing.
    Text,
    /// The shape of a list changed; rebuild the rows.
    Structure,
}

/// The builder: statement state plus the few screen-only settings.
#[derive(Debug, Clone)]
pub struct BuilderEditor {
    pub state: BuilderState,
    pub open: [bool; 7],
    pub table_search: String,
    pub limit_text: String,
    pub offset_text: String,
}

impl Default for BuilderEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Value kinds and operators
// ---------------------------------------------------------------------------

/// The operators that make sense for a column of this kind. A boolean has no
/// ordering and no pattern; a number has no pattern; text keeps all of them
/// because dates and UUIDs arrive as text and are compared with `<` and `>`.
pub fn ops_for(kind: ValueKind) -> Vec<CompareOp> {
    match kind {
        ValueKind::Bool => vec![
            CompareOp::Eq,
            CompareOp::Ne,
            CompareOp::IsNull,
            CompareOp::IsNotNull,
        ],
        ValueKind::Number => vec![
            CompareOp::Eq,
            CompareOp::Ne,
            CompareOp::Gt,
            CompareOp::Lt,
            CompareOp::Gte,
            CompareOp::Lte,
            CompareOp::Between,
            CompareOp::In,
            CompareOp::NotIn,
            CompareOp::IsNull,
            CompareOp::IsNotNull,
        ],
        ValueKind::Text => vec![
            CompareOp::Eq,
            CompareOp::Ne,
            CompareOp::Like,
            CompareOp::NotLike,
            CompareOp::In,
            CompareOp::NotIn,
            CompareOp::Gt,
            CompareOp::Lt,
            CompareOp::Gte,
            CompareOp::Lte,
            CompareOp::Between,
            CompareOp::IsNull,
            CompareOp::IsNotNull,
        ],
    }
}

fn is_number(raw: &str) -> bool {
    let t = raw.trim();
    !t.is_empty() && t.parse::<f64>().is_ok()
}

/// A gentle note under a value that will not be written the way the user
/// probably means. Never blocks: the generator quotes it as text instead.
pub fn value_hint(c: &Condition) -> String {
    if c.kind != ValueKind::Number {
        return String::new();
    }
    let bad = |raw: &str| !raw.trim().is_empty() && !is_number(raw);
    let any_bad = match c.op.arity() {
        crate::query_builder::Arity::None => false,
        crate::query_builder::Arity::One => bad(&c.value),
        crate::query_builder::Arity::Range => bad(&c.value) || bad(&c.value2),
        crate::query_builder::Arity::List => crate::query_builder::parse_list(&c.value)
            .iter()
            .any(|v| bad(v)),
    };
    if any_bad {
        "Not a number, so it is written as text".to_string()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// LIMIT / OFFSET
// ---------------------------------------------------------------------------

/// Parse a count typed into the limit or offset box. Empty means "not set".
pub fn parse_count(text: &str, minimum: u64, what: &str) -> Result<Option<u64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if !t.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{} must be a whole number", what));
    }
    let n: u64 = t.parse().map_err(|_| format!("{} is too large", what))?;
    if n > i64::MAX as u64 {
        return Err(format!("{} is too large", what));
    }
    if n < minimum {
        return Err(format!("{} must be at least {}", what, minimum));
    }
    Ok(Some(n))
}

// ---------------------------------------------------------------------------
// The WHERE tree, flattened
// ---------------------------------------------------------------------------

/// One drawn row of the WHERE tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatFilter {
    /// 0 for the root group.
    pub depth: usize,
    /// Group indices from the root to the group this row is, or belongs to.
    pub path: Vec<usize>,
    /// `None` for a group header, the condition's index in its group otherwise.
    pub condition: Option<usize>,
}

/// The tree in drawing order: each group header, then its conditions, then
/// its nested groups.
pub fn flatten_filters(root: &ConditionGroup) -> Vec<FlatFilter> {
    fn walk(g: &ConditionGroup, path: &mut Vec<usize>, depth: usize, out: &mut Vec<FlatFilter>) {
        out.push(FlatFilter {
            depth,
            path: path.clone(),
            condition: None,
        });
        for ci in 0..g.conditions.len() {
            out.push(FlatFilter {
                depth,
                path: path.clone(),
                condition: Some(ci),
            });
        }
        for (gi, child) in g.groups.iter().enumerate() {
            path.push(gi);
            walk(child, path, depth + 1, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), 0, &mut out);
    out
}

fn group_at<'a>(root: &'a mut ConditionGroup, path: &[usize]) -> Option<&'a mut ConditionGroup> {
    let mut g = root;
    for &i in path {
        g = g.groups.get_mut(i)?;
    }
    Some(g)
}

// ---------------------------------------------------------------------------
// Projection (plain data; a shell maps these onto its own row types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionView {
    pub title: &'static str,
    pub open: bool,
    /// The badge on the header: items in the section, 0 hides it.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceView {
    pub alias: String,
    pub columns: Vec<String>,
    /// `columns` with "All columns (*)" first, for the add-column picker.
    pub star_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedTableView {
    pub alias: String,
    pub name: String,
    /// "users" when alias equals the name, "users (as users_2)" otherwise.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinView {
    pub type_label: String,
    pub type_index: i32,
    pub left_alias: String,
    pub left_alias_index: i32,
    pub left_column: String,
    pub left_column_index: i32,
    pub right_alias: String,
    pub right_alias_index: i32,
    pub right_column: String,
    pub right_column_index: i32,
    pub takes_on: bool,
    /// Set on a CROSS JOIN: the Cartesian-product warning for this row.
    pub warning: String,
    /// The join is missing something and is left out of the statement.
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnView {
    pub label: String,
    pub aggregate: String,
    pub aggregate_index: i32,
    pub alias: String,
    pub distinct: bool,
    /// Whether the DISTINCT toggle means anything on this row.
    pub distinct_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterView {
    /// 0 group header, 1 condition.
    pub kind: i32,
    pub depth: i32,
    pub group_operator: String,
    pub can_nest: bool,
    pub is_root: bool,
    pub alias: String,
    pub alias_index: i32,
    pub column: String,
    pub column_index: i32,
    pub op_label: String,
    pub op_index: i32,
    pub ops: Vec<String>,
    /// 0 no value, 1 one value, 2 range, 3 list.
    pub arity: i32,
    pub is_bool: bool,
    pub value: String,
    pub value2: String,
    pub placeholder: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HavingView {
    pub aggregate: String,
    pub aggregate_index: i32,
    pub alias: String,
    pub alias_index: i32,
    pub column: String,
    pub column_index: i32,
    pub op_label: String,
    pub op_index: i32,
    pub value: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderView {
    pub label: String,
    pub descending: bool,
}

/// Everything the builder screen draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderView {
    pub sections: Vec<SectionView>,
    pub sources: Vec<SourceView>,
    pub catalog: Vec<String>,
    pub catalog_truncated: bool,
    pub added: Vec<AddedTableView>,
    pub joins: Vec<JoinView>,
    pub join_type_labels: Vec<String>,
    pub join_suggestions: Vec<String>,
    pub query_distinct: bool,
    pub columns: Vec<ColumnView>,
    pub aggregate_labels: Vec<String>,
    pub filters: Vec<FilterView>,
    pub group_by: Vec<String>,
    pub having: Vec<HavingView>,
    pub having_aggregate_labels: Vec<String>,
    pub having_op_labels: Vec<String>,
    /// Live GROUP BY / HAVING problems, shown inside that section.
    pub group_issues: Vec<String>,
    pub order_by: Vec<OrderView>,
    pub limit_text: String,
    pub offset_text: String,
    pub limit_error: String,
    pub offset_error: String,
    pub pagination: String,
    /// The statement, or empty when there is not enough to build one.
    pub sql: String,
    /// Every warning, in the order to fix them.
    pub warnings: Vec<String>,
    pub table_search: String,
}

// ---------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------

impl BuilderEditor {
    pub fn new() -> Self {
        let mut open = [true; 7];
        // Same defaults as the UI builder: the first four open.
        for (i, s) in Section::ALL.iter().enumerate() {
            open[i] = matches!(
                s,
                Section::Tables | Section::Joins | Section::Columns | Section::Filters
            );
        }
        Self {
            state: BuilderState::default(),
            open,
            table_search: String::new(),
            limit_text: String::new(),
            offset_text: String::new(),
        }
    }

    /// Back to an empty builder.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn is_empty(&self) -> bool {
        self.state == BuilderState::default()
    }

    // -- Tables ---------------------------------------------------------------

    /// Bring a table into the query under a fresh alias and return the alias.
    /// The same table twice gets `name_2`, which is what makes a self-join
    /// possible.
    pub fn add_table(
        &mut self,
        schema: &str,
        name: &str,
        columns: Vec<(String, String)>,
    ) -> String {
        let alias = self.state.free_alias(name);
        self.state
            .tables
            .push(QueryTable::new(alias.clone(), schema, name).with_columns(columns));
        alias
    }

    pub fn remove_table(&mut self, index: usize) {
        if let Some(t) = self.state.tables.get(index) {
            let alias = t.alias.clone();
            self.state.remove_table(&alias);
        }
    }

    fn table(&self, index: i32) -> Option<&QueryTable> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.state.tables.get(i))
    }

    fn alias_at(&self, index: i32) -> Option<String> {
        self.table(index).map(|t| t.alias.clone())
    }

    fn column_at(
        &self,
        alias_index: i32,
        column_index: i32,
    ) -> Option<(String, String, ValueKind)> {
        let t = self.table(alias_index)?;
        let (name, ty) = t.columns.get(usize::try_from(column_index).ok()?)?;
        Some((t.alias.clone(), name.clone(), ValueKind::from_data_type(ty)))
    }

    fn alias_index(&self, alias: &str) -> i32 {
        self.state
            .tables
            .iter()
            .position(|t| t.alias == alias)
            .map_or(-1, |i| i as i32)
    }

    fn column_index(&self, alias: &str, column: &str) -> i32 {
        self.state
            .columns_of(alias)
            .iter()
            .position(|(n, _)| n == column)
            .map_or(-1, |i| i as i32)
    }

    fn kind_of(&self, alias: &str, column: &str) -> ValueKind {
        self.state
            .columns_of(alias)
            .iter()
            .find(|(n, _)| n == column)
            .map(|(_, ty)| ValueKind::from_data_type(ty))
            .unwrap_or_default()
    }

    /// The first column of the first table, or nothing when there is none.
    fn default_column(&self) -> Option<(String, String, ValueKind)> {
        let t = self.state.tables.first()?;
        let (name, ty) = t.columns.first()?;
        Some((t.alias.clone(), name.clone(), ValueKind::from_data_type(ty)))
    }

    // -- Joins ---------------------------------------------------------------

    /// A blank join: from the first table to the first table not yet joined
    /// in, falling back to the last one. It stays out of the statement until
    /// both its columns are chosen.
    pub fn add_join(&mut self) -> bool {
        if self.state.tables.len() < 2 {
            return false;
        }
        let left = self.state.tables[0].alias.clone();
        let right = self
            .state
            .tables
            .iter()
            .skip(1)
            .find(|t| !self.state.joins.iter().any(|j| j.right_table == t.alias))
            .or_else(|| self.state.tables.last())
            .map(|t| t.alias.clone())
            .unwrap_or_default();
        self.state.joins.push(Join::new(left, right));
        true
    }

    /// Joins the schema's foreign keys imply, written so they resolve.
    ///
    /// A foreign key reads child to parent, but a statement can only join a
    /// new table onto one already in it. When the parent is the table already
    /// placed (FROM customers, then add orders), the sides are swapped, so
    /// the suggestion is `customers.id = orders.customer_id` rather than a
    /// join the generator would leave out as unreachable. A pair already
    /// joined, in either orientation, is not offered again.
    pub fn suggestions(&self, keys: &[ForeignKeyInfo]) -> Vec<Join> {
        let active = active_aliases(&self.state);
        let mut out: Vec<Join> = Vec::new();
        for mut j in join_suggestions(&self.state, keys) {
            if !active.contains(&j.left_table) && active.contains(&j.right_table) {
                std::mem::swap(&mut j.left_table, &mut j.right_table);
                std::mem::swap(&mut j.left_column, &mut j.right_column);
            }
            let same = |a: &Join, b: &Join| {
                a.left_table == b.left_table
                    && a.left_column == b.left_column
                    && a.right_table == b.right_table
                    && a.right_column == b.right_column
            };
            let flipped = Join {
                left_table: j.right_table.clone(),
                left_column: j.right_column.clone(),
                right_table: j.left_table.clone(),
                right_column: j.left_column.clone(),
                ..j.clone()
            };
            let taken = self
                .state
                .joins
                .iter()
                .any(|e| same(e, &j) || same(e, &flipped));
            if !taken && !out.iter().any(|o| same(o, &j)) {
                out.push(j);
            }
        }
        out
    }

    fn accept_suggestion(&mut self, k: usize, keys: &[ForeignKeyInfo]) -> bool {
        match self.suggestions(keys).into_iter().nth(k) {
            Some(j) => {
                self.state.joins.push(j);
                true
            }
            None => false,
        }
    }

    // -- Output columns -----------------------------------------------------

    /// `column_index` counts the "All columns" entry as 0, matching the
    /// `star_columns` list the picker shows.
    fn add_output_column(&mut self, alias_index: i32, column_index: i32) -> bool {
        let Some(alias) = self.alias_at(alias_index) else {
            return false;
        };
        let column = if column_index == 0 {
            STAR.to_string()
        } else {
            match self.column_at(alias_index, column_index - 1) {
                Some((_, name, _)) => name,
                None => return false,
            }
        };
        self.state.columns.push(SelectedColumn::new(alias, column));
        true
    }

    // -- Filters -------------------------------------------------------------

    fn new_condition(&self) -> Option<Condition> {
        let (alias, column, kind) = self.default_column()?;
        Some(Condition::new(alias, column, kind))
    }

    fn condition_mut(&mut self, flat_index: i32) -> Option<&mut Condition> {
        let flat = flatten_filters(&self.state.filters);
        let row = flat.get(usize::try_from(flat_index).ok()?)?.clone();
        let ci = row.condition?;
        group_at(&mut self.state.filters, &row.path)?
            .conditions
            .get_mut(ci)
    }

    /// Re-point a condition at a column, keeping its operator when the new
    /// column's kind still allows it.
    fn retarget_condition(&mut self, flat_index: i32, alias: String, column: String) {
        let kind = self.kind_of(&alias, &column);
        if let Some(c) = self.condition_mut(flat_index) {
            c.table_alias = alias;
            c.column = column;
            c.kind = kind;
            if !ops_for(kind).contains(&c.op) {
                c.op = CompareOp::Eq;
                c.value.clear();
                c.value2.clear();
            } else if kind == ValueKind::Bool && !matches!(c.value.as_str(), "true" | "false") {
                c.value.clear();
            }
        }
    }

    // -- Having --------------------------------------------------------------

    fn having_kind(&self, h: &HavingCondition) -> ValueKind {
        match h.aggregate {
            Aggregate::Min | Aggregate::Max => self.kind_of(&h.table_alias, &h.column),
            _ => ValueKind::Number,
        }
    }

    fn refresh_having_kind(&mut self, i: usize) {
        if let Some(h) = self.state.having.get(i).cloned() {
            let kind = self.having_kind(&h);
            self.state.having[i].kind = kind;
        }
    }

    // -- Limit / offset ------------------------------------------------------

    fn limit_error(&self) -> String {
        parse_count(&self.limit_text, 1, "Limit")
            .err()
            .unwrap_or_default()
    }

    fn offset_error(&self) -> String {
        parse_count(&self.offset_text, 0, "Offset")
            .err()
            .unwrap_or_default()
    }

    fn sync_limits(&mut self) {
        self.state.limit = parse_count(&self.limit_text, 1, "Limit").ok().flatten();
        self.state.offset = parse_count(&self.offset_text, 0, "Offset").ok().flatten();
    }

    // -- Actions -------------------------------------------------------------

    /// Apply one edit from the screen. `i` and `j` are row and choice
    /// positions, `text` is typed text. `keys` are the schema's foreign keys,
    /// needed only to accept a join suggestion.
    pub fn apply(
        &mut self,
        action: &str,
        i: i32,
        j: i32,
        text: &str,
        keys: &[ForeignKeyInfo],
    ) -> Change {
        use Change::{None as Nothing, Structure, Text};
        let idx = usize::try_from(i).ok();
        let done = |ok: bool| if ok { Structure } else { Nothing };

        match action {
            "section" => match idx.filter(|k| *k < self.open.len()) {
                Some(k) => {
                    self.open[k] = !self.open[k];
                    Structure
                }
                None => Nothing,
            },
            "reset" => {
                self.reset();
                Structure
            }

            // Tables.
            "table-search" => {
                self.table_search = text.to_string();
                Structure
            }
            "table-remove" => match idx.filter(|k| *k < self.state.tables.len()) {
                Some(k) => {
                    self.remove_table(k);
                    Structure
                }
                None => Nothing,
            },

            // Joins.
            "join-add" => done(self.add_join()),
            "join-suggest" => done(idx.is_some_and(|k| self.accept_suggestion(k, keys))),
            "join-remove" => match idx.filter(|k| *k < self.state.joins.len()) {
                Some(k) => {
                    self.state.joins.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "join-type" => {
                let Some(t) = usize::try_from(j)
                    .ok()
                    .and_then(|k| JoinType::ALL.get(k))
                    .copied()
                else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.joins.get_mut(k)) {
                    Some(join) => {
                        join.join_type = t;
                        Structure
                    }
                    None => Nothing,
                }
            }
            "join-lt" | "join-rt" => {
                let Some(alias) = self.alias_at(j) else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.joins.get_mut(k)) {
                    Some(join) => {
                        // A new table invalidates the column picked on it.
                        if action == "join-lt" {
                            join.left_table = alias;
                            join.left_column.clear();
                        } else {
                            join.right_table = alias;
                            join.right_column.clear();
                        }
                        Structure
                    }
                    None => Nothing,
                }
            }
            "join-lc" | "join-rc" => {
                let Some(join) = idx.and_then(|k| self.state.joins.get(k)).cloned() else {
                    return Nothing;
                };
                let alias = if action == "join-lc" {
                    &join.left_table
                } else {
                    &join.right_table
                };
                let Some(name) = self
                    .state
                    .columns_of(alias)
                    .get(usize::try_from(j).unwrap_or(usize::MAX))
                    .map(|(n, _)| n.clone())
                else {
                    return Nothing;
                };
                let join = &mut self.state.joins[idx.unwrap_or(0)];
                if action == "join-lc" {
                    join.left_column = name;
                } else {
                    join.right_column = name;
                }
                Structure
            }

            // Output columns.
            "col-add" => done(self.add_output_column(i, j)),
            "col-remove" => match idx.filter(|k| *k < self.state.columns.len()) {
                Some(k) => {
                    self.state.columns.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "col-agg" => {
                let Some(a) = usize::try_from(j)
                    .ok()
                    .and_then(|k| Aggregate::ALL.get(k))
                    .copied()
                else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.columns.get_mut(k)) {
                    Some(c) => {
                        c.aggregate = a;
                        Structure
                    }
                    None => Nothing,
                }
            }
            "col-alias" => match idx.and_then(|k| self.state.columns.get_mut(k)) {
                Some(c) => {
                    c.alias = text.trim().to_string();
                    Text
                }
                None => Nothing,
            },
            "col-distinct" => match idx.and_then(|k| self.state.columns.get_mut(k)) {
                Some(c) => {
                    c.distinct = !c.distinct;
                    Structure
                }
                None => Nothing,
            },
            "distinct" => {
                self.state.distinct = !self.state.distinct;
                Structure
            }

            // Filters.
            "f-cond-add" | "f-group-add" => {
                let flat = flatten_filters(&self.state.filters);
                let Some(row) = idx.and_then(|k| flat.get(k)).cloned() else {
                    return Nothing;
                };
                if row.condition.is_some() {
                    return Nothing;
                }
                if action == "f-cond-add" {
                    let Some(c) = self.new_condition() else {
                        return Nothing;
                    };
                    match group_at(&mut self.state.filters, &row.path) {
                        Some(g) => {
                            g.conditions.push(c);
                            Structure
                        }
                        None => Nothing,
                    }
                } else {
                    if row.depth + 1 >= MAX_GROUP_DEPTH {
                        return Nothing;
                    }
                    match group_at(&mut self.state.filters, &row.path) {
                        Some(g) => {
                            g.groups.push(ConditionGroup::default());
                            Structure
                        }
                        None => Nothing,
                    }
                }
            }
            "f-remove" => {
                let flat = flatten_filters(&self.state.filters);
                let Some(row) = idx.and_then(|k| flat.get(k)).cloned() else {
                    return Nothing;
                };
                match row.condition {
                    Some(ci) => match group_at(&mut self.state.filters, &row.path) {
                        Some(g) if ci < g.conditions.len() => {
                            g.conditions.remove(ci);
                            Structure
                        }
                        _ => Nothing,
                    },
                    None => {
                        // The root cannot be removed; clear it instead.
                        let Some((&last, parent_path)) = row.path.split_last() else {
                            self.state.filters = ConditionGroup::default();
                            return Structure;
                        };
                        match group_at(&mut self.state.filters, parent_path) {
                            Some(p) if last < p.groups.len() => {
                                p.groups.remove(last);
                                Structure
                            }
                            _ => Nothing,
                        }
                    }
                }
            }
            "f-op" => {
                let flat = flatten_filters(&self.state.filters);
                let Some(row) = idx.and_then(|k| flat.get(k)).cloned() else {
                    return Nothing;
                };
                match group_at(&mut self.state.filters, &row.path) {
                    Some(g) if row.condition.is_none() => {
                        g.operator = match g.operator {
                            Logical::And => Logical::Or,
                            Logical::Or => Logical::And,
                        };
                        Structure
                    }
                    _ => Nothing,
                }
            }
            "f-alias" => {
                let Some(t) = self.table(j).cloned() else {
                    return Nothing;
                };
                let Some((first, _)) = t.columns.first().cloned() else {
                    return Nothing;
                };
                if self.condition_mut(i).is_none() {
                    return Nothing;
                }
                self.retarget_condition(i, t.alias, first);
                Structure
            }
            "f-col" => {
                let alias = match self.condition_mut(i) {
                    Some(c) => c.table_alias.clone(),
                    None => return Nothing,
                };
                let ai = self.alias_index(&alias);
                match self.column_at(ai, j) {
                    Some((alias, name, _)) => {
                        self.retarget_condition(i, alias, name);
                        Structure
                    }
                    None => Nothing,
                }
            }
            "f-cmp" => {
                let kind = match self.condition_mut(i) {
                    Some(c) => c.kind,
                    None => return Nothing,
                };
                let Some(op) = usize::try_from(j)
                    .ok()
                    .and_then(|k| ops_for(kind).get(k).copied())
                else {
                    return Nothing;
                };
                if let Some(c) = self.condition_mut(i) {
                    let was = c.op.arity();
                    c.op = op;
                    // A different shape of operand makes the old text wrong.
                    if was != op.arity() {
                        c.value.clear();
                        c.value2.clear();
                    }
                }
                Structure
            }
            "f-val" => match self.condition_mut(i) {
                Some(c) => {
                    c.value = text.to_string();
                    Text
                }
                None => Nothing,
            },
            "f-val2" => match self.condition_mut(i) {
                Some(c) => {
                    c.value2 = text.to_string();
                    Text
                }
                None => Nothing,
            },
            "f-bool" => match self.condition_mut(i) {
                Some(c) => {
                    c.value = if j == 0 { "true" } else { "false" }.to_string();
                    Structure
                }
                None => Nothing,
            },

            // Group by.
            "gb-add" => {
                let Some((alias, name, _)) = self.column_at(i, j) else {
                    return Nothing;
                };
                let dup = self
                    .state
                    .group_by
                    .iter()
                    .any(|g| g.table_alias == alias && g.column == name);
                if dup {
                    return Nothing;
                }
                self.state.group_by.push(GroupByClause {
                    table_alias: alias,
                    column: name,
                });
                Structure
            }
            "gb-remove" => match idx.filter(|k| *k < self.state.group_by.len()) {
                Some(k) => {
                    self.state.group_by.remove(k);
                    Structure
                }
                None => Nothing,
            },

            // Having.
            "hv-add" => {
                let Some((alias, name, _)) = self.default_column() else {
                    return Nothing;
                };
                let mut h = HavingCondition::new(alias, name);
                h.kind = ValueKind::Number;
                self.state.having.push(h);
                Structure
            }
            "hv-remove" => match idx.filter(|k| *k < self.state.having.len()) {
                Some(k) => {
                    self.state.having.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "hv-agg" => {
                let Some(a) = usize::try_from(j)
                    .ok()
                    .and_then(|k| Aggregate::FOR_HAVING.get(k))
                    .copied()
                else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.having.get_mut(k)) {
                    Some(h) => {
                        h.aggregate = a;
                        self.refresh_having_kind(idx.unwrap_or(0));
                        Structure
                    }
                    None => Nothing,
                }
            }
            "hv-alias" => {
                let Some(t) = self.table(j).cloned() else {
                    return Nothing;
                };
                let Some((first, _)) = t.columns.first().cloned() else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.having.get_mut(k)) {
                    Some(h) => {
                        h.table_alias = t.alias;
                        h.column = first;
                        self.refresh_having_kind(idx.unwrap_or(0));
                        Structure
                    }
                    None => Nothing,
                }
            }
            "hv-col" => {
                let Some(h) = idx.and_then(|k| self.state.having.get(k)).cloned() else {
                    return Nothing;
                };
                let ai = self.alias_index(&h.table_alias);
                match self.column_at(ai, j) {
                    Some((_, name, _)) => {
                        self.state.having[idx.unwrap_or(0)].column = name;
                        self.refresh_having_kind(idx.unwrap_or(0));
                        Structure
                    }
                    None => Nothing,
                }
            }
            "hv-op" => {
                let Some(op) = usize::try_from(j)
                    .ok()
                    .and_then(|k| CompareOp::FOR_HAVING.get(k))
                    .copied()
                else {
                    return Nothing;
                };
                match idx.and_then(|k| self.state.having.get_mut(k)) {
                    Some(h) => {
                        h.op = op;
                        Structure
                    }
                    None => Nothing,
                }
            }
            "hv-val" => match idx.and_then(|k| self.state.having.get_mut(k)) {
                Some(h) => {
                    h.value = text.to_string();
                    Text
                }
                None => Nothing,
            },

            // Order by.
            "ob-add" => {
                let Some((alias, name, _)) = self.column_at(i, j) else {
                    return Nothing;
                };
                self.state.order_by.push(OrderByClause {
                    table_alias: alias,
                    column: name,
                    direction: SortDir::Asc,
                });
                Structure
            }
            "ob-remove" => match idx.filter(|k| *k < self.state.order_by.len()) {
                Some(k) => {
                    self.state.order_by.remove(k);
                    Structure
                }
                None => Nothing,
            },
            "ob-dir" => match idx.and_then(|k| self.state.order_by.get_mut(k)) {
                Some(o) => {
                    o.direction = o.direction.toggled();
                    Structure
                }
                None => Nothing,
            },

            // Limit / offset. Typed into boxes outside any list row, so a
            // rebuild is harmless, but nothing needs one.
            "limit" => {
                self.limit_text = text.to_string();
                self.sync_limits();
                Text
            }
            "offset" => {
                self.offset_text = text.to_string();
                self.sync_limits();
                Text
            }

            _ => Nothing,
        }
    }

    // -- Output --------------------------------------------------------------

    /// The statement, or `None` while there is not enough to build one.
    pub fn sql(
        &self,
        quote: &impl Fn(&str) -> String,
        escape: &impl Fn(&str) -> String,
        rules: DialectRules,
    ) -> Option<String> {
        generate_sql(quote, escape, rules, &self.state)
    }

    /// Everything wrong or surprising, with the limit and offset problems
    /// first because they are the ones the user just typed.
    pub fn warnings(&self, rules: DialectRules) -> Vec<String> {
        let mut out = Vec::new();
        let (le, oe) = (self.limit_error(), self.offset_error());
        if !le.is_empty() {
            out.push(format!("{}; it is left out.", le));
        }
        if !oe.is_empty() {
            out.push(format!("{}; it is left out.", oe));
        }
        out.extend(validate(&self.state, rules));
        out
    }

    /// Whether the statement can be sent to a tab: it exists and the limit
    /// and offset boxes hold no error.
    pub fn can_use(&self, sql: &Option<String>) -> bool {
        sql.is_some() && self.limit_error().is_empty() && self.offset_error().is_empty()
    }

    pub fn view(
        &self,
        catalog: &[String],
        keys: &[ForeignKeyInfo],
        quote: &impl Fn(&str) -> String,
        escape: &impl Fn(&str) -> String,
        rules: DialectRules,
    ) -> BuilderView {
        let s = &self.state;

        // Section badges.
        let counts = [
            s.tables.len(),
            s.joins.len(),
            s.columns.len(),
            count_conditions(&s.filters),
            s.group_by.len() + s.having.len(),
            s.order_by.len(),
            usize::from(s.limit.is_some()) + usize::from(s.offset.is_some()),
        ];
        let sections = Section::ALL
            .iter()
            .enumerate()
            .map(|(k, sec)| SectionView {
                title: sec.title(),
                open: self.open[k],
                count: counts[k],
            })
            .collect();

        let sources = s
            .tables
            .iter()
            .map(|t| {
                let columns: Vec<String> = t.columns.iter().map(|(n, _)| n.clone()).collect();
                let mut star_columns = vec!["All columns (*)".to_string()];
                star_columns.extend(columns.iter().cloned());
                SourceView {
                    alias: t.alias.clone(),
                    columns,
                    star_columns,
                }
            })
            .collect();

        // The catalog, minus nothing: the same table may be added again.
        let needle = self.table_search.trim().to_lowercase();
        let matching: Vec<String> = catalog
            .iter()
            .filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        let catalog_truncated = matching.len() > CATALOG_LIMIT;
        let catalog_rows = matching.into_iter().take(CATALOG_LIMIT).collect();

        let added = s
            .tables
            .iter()
            .map(|t| AddedTableView {
                alias: t.alias.clone(),
                name: t.name.clone(),
                detail: if t.alias == t.name {
                    format!("{} columns", t.columns.len())
                } else {
                    format!("{} as {}", t.name, t.alias)
                },
            })
            .collect();

        let joins = s
            .joins
            .iter()
            .map(|j| {
                let col_label = |alias: &str, col: &str| {
                    if col.is_empty() {
                        "Column".to_string()
                    } else {
                        let _ = alias;
                        col.to_string()
                    }
                };
                JoinView {
                    type_label: j.join_type.sql().to_string(),
                    type_index: JoinType::ALL
                        .iter()
                        .position(|t| *t == j.join_type)
                        .unwrap_or(0) as i32,
                    left_alias: j.left_table.clone(),
                    left_alias_index: self.alias_index(&j.left_table),
                    left_column: col_label(&j.left_table, &j.left_column),
                    left_column_index: self.column_index(&j.left_table, &j.left_column),
                    right_alias: j.right_table.clone(),
                    right_alias_index: self.alias_index(&j.right_table),
                    right_column: col_label(&j.right_table, &j.right_column),
                    right_column_index: self.column_index(&j.right_table, &j.right_column),
                    takes_on: j.join_type.takes_on(),
                    warning: if j.join_type == JoinType::Cross {
                        "Pairs every row with every row. The result can be enormous.".to_string()
                    } else {
                        String::new()
                    },
                    incomplete: !j.is_complete(),
                }
            })
            .collect();

        let suggestion_labels = self
            .suggestions(keys)
            .iter()
            .map(|j| {
                format!(
                    "{}.{} = {}.{}",
                    j.left_table, j.left_column, j.right_table, j.right_column
                )
            })
            .collect();

        let columns = s
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| ColumnView {
                label: if c.column == STAR {
                    format!("{}.*", c.table_alias)
                } else {
                    format!("{}.{}", c.table_alias, c.column)
                },
                aggregate: c.aggregate.label().to_string(),
                aggregate_index: Aggregate::ALL
                    .iter()
                    .position(|a| *a == c.aggregate)
                    .unwrap_or(0) as i32,
                alias: c.alias.clone(),
                distinct: c.distinct,
                // Per-column DISTINCT only works on the first plain column.
                distinct_enabled: i == 0 && !c.aggregate.is_some() && !s.distinct,
            })
            .collect();

        let filters = flatten_filters(&s.filters)
            .iter()
            .map(|row| self.filter_view(row))
            .collect();

        let group_by = s
            .group_by
            .iter()
            .map(|g| format!("{}.{}", g.table_alias, g.column))
            .collect();

        let having = s
            .having
            .iter()
            .map(|h| HavingView {
                aggregate: h.aggregate.label().to_string(),
                aggregate_index: Aggregate::FOR_HAVING
                    .iter()
                    .position(|a| *a == h.aggregate)
                    .unwrap_or(0) as i32,
                alias: h.table_alias.clone(),
                alias_index: self.alias_index(&h.table_alias),
                column: h.column.clone(),
                column_index: self.column_index(&h.table_alias, &h.column),
                op_label: h.op.sql().to_string(),
                op_index: CompareOp::FOR_HAVING
                    .iter()
                    .position(|o| *o == h.op)
                    .unwrap_or(0) as i32,
                value: h.value.clone(),
                hint: if h.kind == ValueKind::Number
                    && !h.value.trim().is_empty()
                    && !is_number(&h.value)
                {
                    "Not a number, so it is written as text".to_string()
                } else {
                    String::new()
                },
            })
            .collect();

        let order_by = s
            .order_by
            .iter()
            .map(|o| OrderView {
                label: format!("{}.{}", o.table_alias, o.column),
                descending: o.direction == SortDir::Desc,
            })
            .collect();

        // Every warning is listed beside the preview. The GROUP BY and HAVING
        // ones are repeated inside their own section, where they are fixed.
        let warnings = self.warnings(rules);
        let group_issues: Vec<String> = warnings
            .iter()
            .filter(|w| w.contains("GROUP BY") || w.contains("HAVING"))
            .cloned()
            .collect();

        let sql = self.sql(quote, escape, rules).unwrap_or_default();

        BuilderView {
            sections,
            sources,
            catalog: catalog_rows,
            catalog_truncated,
            added,
            joins,
            join_type_labels: JoinType::ALL.iter().map(|t| t.sql().to_string()).collect(),
            join_suggestions: suggestion_labels,
            query_distinct: s.distinct,
            columns,
            aggregate_labels: Aggregate::ALL
                .iter()
                .map(|a| a.label().to_string())
                .collect(),
            filters,
            group_by,
            having,
            having_aggregate_labels: Aggregate::FOR_HAVING
                .iter()
                .map(|a| a.label().to_string())
                .collect(),
            having_op_labels: CompareOp::FOR_HAVING
                .iter()
                .map(|o| o.sql().to_string())
                .collect(),
            group_issues,
            order_by,
            limit_text: self.limit_text.clone(),
            offset_text: self.offset_text.clone(),
            limit_error: self.limit_error(),
            offset_error: self.offset_error(),
            pagination: pagination_summary(s.limit, s.offset),
            sql,
            warnings,
            table_search: self.table_search.clone(),
        }
    }

    fn filter_view(&self, row: &FlatFilter) -> FilterView {
        let mut v = FilterView {
            kind: 0,
            depth: row.depth as i32,
            group_operator: String::new(),
            can_nest: row.depth + 1 < MAX_GROUP_DEPTH,
            is_root: row.path.is_empty(),
            alias: String::new(),
            alias_index: -1,
            column: String::new(),
            column_index: -1,
            op_label: String::new(),
            op_index: 0,
            ops: Vec::new(),
            arity: 0,
            is_bool: false,
            value: String::new(),
            value2: String::new(),
            placeholder: String::new(),
            hint: String::new(),
        };

        // Walk to the row's group read-only.
        let mut g = &self.state.filters;
        for &i in &row.path {
            match g.groups.get(i) {
                Some(next) => g = next,
                None => return v,
            }
        }
        v.group_operator = g.operator.sql().to_string();

        let Some(ci) = row.condition else {
            return v;
        };
        let Some(c) = g.conditions.get(ci) else {
            return v;
        };
        let ops = ops_for(c.kind);
        v.kind = 1;
        v.alias = c.table_alias.clone();
        v.alias_index = self.alias_index(&c.table_alias);
        v.column = c.column.clone();
        v.column_index = self.column_index(&c.table_alias, &c.column);
        v.op_label = c.op.sql().to_string();
        v.op_index = ops.iter().position(|o| *o == c.op).unwrap_or(0) as i32;
        v.ops = ops.iter().map(|o| o.sql().to_string()).collect();
        v.arity = match c.op.arity() {
            crate::query_builder::Arity::None => 0,
            crate::query_builder::Arity::One => 1,
            crate::query_builder::Arity::Range => 2,
            crate::query_builder::Arity::List => 3,
        };
        v.is_bool = c.kind == ValueKind::Bool && v.arity == 1;
        v.value = c.value.clone();
        v.value2 = c.value2.clone();
        v.placeholder = match (c.op.arity(), c.kind) {
            (crate::query_builder::Arity::List, _) => "a, b, c".to_string(),
            (_, ValueKind::Number) => "Number".to_string(),
            (_, ValueKind::Text) if matches!(c.op, CompareOp::Like | CompareOp::NotLike) => {
                "Pattern, e.g. %term%".to_string()
            }
            _ => "Value".to_string(),
        };
        v.hint = value_hint(c);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_builder::rules_for;
    use dbhive_core::models::DbDriver;

    fn q(i: &str) -> String {
        format!("\"{}\"", i.replace('"', "\"\""))
    }
    fn esc(v: &str) -> String {
        v.replace('\'', "''")
    }
    fn pg() -> DialectRules {
        rules_for(&DbDriver::Postgres)
    }

    fn cols(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn users() -> Vec<(String, String)> {
        cols(&[("id", "integer"), ("name", "text"), ("active", "boolean")])
    }
    fn orders() -> Vec<(String, String)> {
        cols(&[
            ("id", "integer"),
            ("user_id", "integer"),
            ("total", "numeric"),
        ])
    }

    fn editor() -> BuilderEditor {
        let mut e = BuilderEditor::new();
        e.add_table("public", "users", users());
        e
    }

    fn two_tables() -> BuilderEditor {
        let mut e = editor();
        e.add_table("public", "orders", orders());
        e
    }

    fn fk() -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: "orders_user_fk".into(),
            table: "orders".into(),
            schema: "public".into(),
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }
    }

    fn sql(e: &BuilderEditor) -> String {
        e.sql(&q, &esc, pg()).unwrap_or_default()
    }

    // -- tables ---------------------------------------------------------------

    #[test]
    fn the_same_table_twice_gets_a_fresh_alias() {
        let mut e = editor();
        assert_eq!(e.add_table("public", "users", users()), "users_2");
        assert_eq!(e.state.tables[1].name, "users");
        assert_eq!(e.add_table("public", "users", users()), "users_3");
    }

    #[test]
    fn removing_a_table_cascades_to_everything_that_used_it() {
        let mut e = two_tables();
        e.apply("col-add", 1, 3, "", &[]);
        e.apply("join-add", 0, 0, "", &[]);
        e.apply("gb-add", 1, 0, "", &[]);
        e.apply("ob-add", 1, 0, "", &[]);
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-alias", 1, 1, "", &[]);
        assert_eq!(e.state.filters.conditions[0].table_alias, "orders");

        e.apply("table-remove", 1, 0, "", &[]);

        assert_eq!(e.state.tables.len(), 1);
        assert!(e.state.columns.is_empty());
        assert!(e.state.joins.is_empty());
        assert!(e.state.group_by.is_empty());
        assert!(e.state.order_by.is_empty());
        assert!(e.state.filters.conditions.is_empty());
    }

    #[test]
    fn the_catalog_is_filtered_case_insensitively_and_capped() {
        let mut e = BuilderEditor::new();
        let names: Vec<String> = (0..300).map(|i| format!("Table_{i}")).collect();
        let v = e.view(&names, &[], &q, &esc, pg());
        assert_eq!(v.catalog.len(), CATALOG_LIMIT);
        assert!(v.catalog_truncated);

        e.apply("table-search", 0, 0, "TABLE_29", &[]);
        let v = e.view(&names, &[], &q, &esc, pg());
        assert_eq!(v.catalog.len(), 11); // 29, 290..299
        assert!(!v.catalog_truncated);
    }

    // -- joins ----------------------------------------------------------------

    #[test]
    fn a_join_needs_two_tables_and_stays_out_of_sql_until_complete() {
        let mut e = editor();
        assert_eq!(e.apply("join-add", 0, 0, "", &[]), Change::None);

        e.add_table("public", "orders", orders());
        assert_eq!(e.apply("join-add", 0, 0, "", &[]), Change::Structure);
        assert!(!sql(&e).contains("JOIN"));
        assert!(e
            .warnings(pg())
            .iter()
            .any(|w| w.contains("missing a column")));

        e.apply("join-lc", 0, 0, "", &[]); // users.id
        e.apply("join-rc", 0, 1, "", &[]); // orders.user_id
        let s = sql(&e);
        assert!(
            s.contains("INNER JOIN \"public\".\"orders\" AS \"orders\""),
            "{s}"
        );
        assert!(
            s.contains("ON \"users\".\"id\" = \"orders\".\"user_id\""),
            "{s}"
        );
    }

    #[test]
    fn changing_a_joins_table_clears_the_column_picked_on_it() {
        let mut e = two_tables();
        e.apply("join-add", 0, 0, "", &[]);
        e.apply("join-lc", 0, 1, "", &[]);
        assert_eq!(e.state.joins[0].left_column, "name");
        e.apply("join-lt", 0, 1, "", &[]);
        assert_eq!(e.state.joins[0].left_table, "orders");
        assert_eq!(e.state.joins[0].left_column, "");
    }

    #[test]
    fn a_foreign_key_suggestion_becomes_a_complete_join() {
        let mut e = two_tables();
        let keys = vec![fk()];
        let v = e.view(&[], &keys, &q, &esc, pg());
        // users is the FROM table, so the join is written from users to orders.
        assert_eq!(
            v.join_suggestions,
            vec!["users.id = orders.user_id".to_string()]
        );

        assert_eq!(e.apply("join-suggest", 0, 0, "", &keys), Change::Structure);
        assert_eq!(e.state.joins.len(), 1);
        assert!(e.state.joins[0].is_complete());
        // It must actually reach the statement, not be left out as unreachable.
        let s = sql(&e);
        assert!(s.contains("INNER JOIN \"public\".\"orders\" AS \"orders\" ON \"users\".\"id\" = \"orders\".\"user_id\""), "{s}");
        assert!(e.warnings(pg()).iter().all(|w| !w.contains("not joined")));
        // Accepted, so no longer offered.
        assert!(e
            .view(&[], &keys, &q, &esc, pg())
            .join_suggestions
            .is_empty());
        assert_eq!(e.apply("join-suggest", 0, 0, "", &keys), Change::None);
    }

    #[test]
    fn a_cross_join_warns_about_the_cartesian_product() {
        let mut e = two_tables();
        e.apply("join-add", 0, 0, "", &[]);
        e.apply("join-type", 0, 4, "", &[]); // CROSS
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.joins[0].warning.contains("every row"));
        assert!(!v.joins[0].takes_on);
        assert!(sql(&e).contains("CROSS JOIN"));
        assert!(v.warnings.iter().any(|w| w.contains("CROSS JOIN")));
    }

    // -- columns --------------------------------------------------------------

    #[test]
    fn output_columns_take_aggregates_aliases_and_distinct() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "", &[]); // users.*
        assert_eq!(e.state.columns[0].column, STAR);
        e.apply("col-add", 0, 2, "", &[]); // users.name
        e.apply("col-agg", 0, 1, "", &[]); // COUNT on *
        e.apply("col-alias", 0, 0, "  n  ", &[]);
        assert_eq!(e.state.columns[0].alias, "n");
        assert_eq!(
            sql(&e),
            "SELECT COUNT(*) AS \"n\", \"users\".\"name\"\nFROM \"public\".\"users\" AS \"users\";"
        );

        e.apply("distinct", 0, 0, "", &[]);
        assert!(sql(&e).starts_with("SELECT DISTINCT "));
    }

    #[test]
    fn distinct_is_only_offered_on_the_first_plain_column() {
        let mut e = editor();
        e.apply("col-add", 0, 1, "", &[]);
        e.apply("col-add", 0, 2, "", &[]);
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.columns[0].distinct_enabled);
        assert!(!v.columns[1].distinct_enabled);
        e.apply("col-agg", 0, 2, "", &[]);
        assert!(!e.view(&[], &[], &q, &esc, pg()).columns[0].distinct_enabled);
    }

    #[test]
    fn a_star_under_sum_is_refused_with_a_reason() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "", &[]);
        e.apply("col-agg", 0, 2, "", &[]); // SUM
        assert!(e.warnings(pg()).iter().any(|w| w.contains("only COUNT")));
        assert!(!sql(&e).contains("SUM"));
    }

    // -- filters --------------------------------------------------------------

    #[test]
    fn conditions_nest_to_three_levels_and_no_further() {
        let mut e = editor();
        assert_eq!(e.apply("f-group-add", 0, 0, "", &[]), Change::Structure); // level 2
                                                                              // The flat order is root, then the nested group.
        assert_eq!(e.apply("f-group-add", 1, 0, "", &[]), Change::Structure); // level 3
        assert_eq!(e.apply("f-group-add", 2, 0, "", &[]), Change::None); // level 4 refused

        let v = e.view(&[], &[], &q, &esc, pg());
        let depths: Vec<i32> = v.filters.iter().map(|f| f.depth).collect();
        assert_eq!(depths, vec![0, 1, 2]);
        assert!(v.filters[0].can_nest && v.filters[1].can_nest);
        assert!(!v.filters[2].can_nest);
    }

    #[test]
    fn a_condition_in_a_nested_group_lands_in_that_group() {
        let mut e = editor();
        e.apply("f-group-add", 0, 0, "", &[]);
        e.apply("f-cond-add", 1, 0, "", &[]);
        assert!(e.state.filters.conditions.is_empty());
        assert_eq!(e.state.filters.groups[0].conditions.len(), 1);

        e.apply("f-cond-add", 0, 0, "", &[]);
        assert_eq!(e.state.filters.conditions.len(), 1);
        // Conditions come before nested groups in the drawn order.
        let kinds: Vec<i32> = e
            .view(&[], &[], &q, &esc, pg())
            .filters
            .iter()
            .map(|f| f.kind)
            .collect();
        assert_eq!(kinds, vec![0, 1, 0, 1]);
    }

    #[test]
    fn nested_groups_are_parenthesised_and_the_operator_toggles() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]); // flat 1
        e.apply("f-val", 1, 0, "7", &[]);
        e.apply("f-group-add", 0, 0, "", &[]);
        // flat: 0 root, 1 cond, 2 group, 3 cond in group
        e.apply("f-cond-add", 2, 0, "", &[]);
        e.apply("f-col", 3, 1, "", &[]); // name
        e.apply("f-val", 3, 0, "Ann", &[]);
        e.apply("f-op", 2, 0, "", &[]); // inner group to OR
        let s = sql(&e);
        assert!(
            s.contains("WHERE \"users\".\"id\" = 7 AND (\"users\".\"name\" = 'Ann')"),
            "{s}"
        );

        e.apply("f-cond-add", 2, 0, "", &[]);
        e.apply("f-col", 4, 1, "", &[]);
        e.apply("f-val", 4, 0, "Bob", &[]);
        let s = sql(&e);
        assert!(
            s.contains("(\"users\".\"name\" = 'Ann' OR \"users\".\"name\" = 'Bob')"),
            "{s}"
        );
    }

    #[test]
    fn removing_a_group_removes_what_is_inside_it() {
        let mut e = editor();
        e.apply("f-group-add", 0, 0, "", &[]);
        e.apply("f-cond-add", 1, 0, "", &[]);
        e.apply("f-cond-add", 1, 0, "", &[]);
        assert_eq!(count_conditions(&e.state.filters), 2);
        assert_eq!(e.apply("f-remove", 1, 0, "", &[]), Change::Structure);
        assert_eq!(count_conditions(&e.state.filters), 0);
        assert!(e.state.filters.groups.is_empty());
    }

    #[test]
    fn removing_the_root_clears_it_instead() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-group-add", 0, 0, "", &[]);
        assert_eq!(e.apply("f-remove", 0, 0, "", &[]), Change::Structure);
        assert_eq!(e.state.filters, ConditionGroup::default());
    }

    #[test]
    fn the_operator_list_follows_the_column_type() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]); // id, integer
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.filters[1].ops.contains(&">".to_string()));
        assert!(!v.filters[1].ops.contains(&"LIKE".to_string()));

        e.apply("f-col", 1, 1, "", &[]); // name, text
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.filters[1].ops.contains(&"LIKE".to_string()));

        e.apply("f-col", 1, 2, "", &[]); // active, boolean
        let v = e.view(&[], &[], &q, &esc, pg());
        assert_eq!(v.filters[1].ops, vec!["=", "!=", "IS NULL", "IS NOT NULL"]);
        assert!(v.filters[1].is_bool);
    }

    #[test]
    fn switching_to_a_column_that_forbids_the_operator_resets_it() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-col", 1, 1, "", &[]); // text
        let like = ops_for(ValueKind::Text)
            .iter()
            .position(|o| *o == CompareOp::Like)
            .unwrap();
        e.apply("f-cmp", 1, like as i32, "", &[]);
        e.apply("f-val", 1, 0, "%a%", &[]);
        assert_eq!(e.state.filters.conditions[0].op, CompareOp::Like);

        e.apply("f-col", 1, 0, "", &[]); // integer: LIKE not offered
        let c = &e.state.filters.conditions[0];
        assert_eq!(c.op, CompareOp::Eq);
        assert!(c.value.is_empty(), "a stale pattern survived");
    }

    #[test]
    fn the_value_editor_follows_the_operator() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        let idx = |op: CompareOp| {
            ops_for(ValueKind::Number)
                .iter()
                .position(|o| *o == op)
                .unwrap() as i32
        };
        let arity = |e: &BuilderEditor| e.view(&[], &[], &q, &esc, pg()).filters[1].arity;

        assert_eq!(arity(&e), 1);
        e.apply("f-cmp", 1, idx(CompareOp::Between), "", &[]);
        assert_eq!(arity(&e), 2);
        e.apply("f-cmp", 1, idx(CompareOp::In), "", &[]);
        assert_eq!(arity(&e), 3);
        e.apply("f-cmp", 1, idx(CompareOp::IsNull), "", &[]);
        assert_eq!(arity(&e), 0);
        assert!(sql(&e).contains("IS NULL"));
    }

    #[test]
    fn changing_operand_shape_drops_the_old_text() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-val", 1, 0, "5", &[]);
        let between = ops_for(ValueKind::Number)
            .iter()
            .position(|o| *o == CompareOp::Between)
            .unwrap();
        e.apply("f-cmp", 1, between as i32, "", &[]);
        assert!(e.state.filters.conditions[0].value.is_empty());
    }

    #[test]
    fn a_boolean_condition_is_set_from_a_choice() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-col", 1, 2, "", &[]);
        e.apply("f-bool", 1, 0, "", &[]);
        assert!(sql(&e).contains("\"users\".\"active\" = TRUE"));
        e.apply("f-bool", 1, 1, "", &[]);
        assert!(sql(&e).contains("\"users\".\"active\" = FALSE"));
    }

    #[test]
    fn typed_values_are_escaped_and_never_become_sql() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-col", 1, 1, "", &[]);
        e.apply("f-val", 1, 0, "x'; DROP TABLE users; --", &[]);
        let s = sql(&e);
        assert!(s.contains("= 'x''; DROP TABLE users; --'"), "{s}");
    }

    #[test]
    fn a_non_numeric_value_for_a_number_column_gets_a_hint_not_a_block() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        e.apply("f-val", 1, 0, "abc", &[]);
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.filters[1].hint.contains("Not a number"));
        assert!(sql(&e).contains("= 'abc'"));
        e.apply("f-val", 1, 0, "42", &[]);
        assert!(e.view(&[], &[], &q, &esc, pg()).filters[1].hint.is_empty());
    }

    #[test]
    fn typing_into_a_row_reports_text_so_rows_are_not_rebuilt() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        assert_eq!(e.apply("f-val", 1, 0, "a", &[]), Change::Text);
        assert_eq!(e.apply("col-alias", 99, 0, "a", &[]), Change::None);
        e.apply("col-add", 0, 1, "", &[]);
        assert_eq!(e.apply("col-alias", 0, 0, "a", &[]), Change::Text);
    }

    #[test]
    fn a_half_typed_condition_is_left_out_not_emitted() {
        let mut e = editor();
        e.apply("f-cond-add", 0, 0, "", &[]);
        assert!(!sql(&e).contains("WHERE"));
    }

    // -- group by / having ---------------------------------------------------

    #[test]
    fn group_by_validates_live_against_the_selected_columns() {
        let mut e = editor();
        e.apply("col-add", 0, 1, "", &[]); // id
        e.apply("col-add", 0, 2, "", &[]); // name
        e.apply("gb-add", 0, 0, "", &[]); // group by id
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v
            .group_issues
            .iter()
            .any(|w| w.contains("users.name must be in GROUP BY")));
        // The same problem also reaches the warnings beside the preview, so it
        // is seen even with the section folded away.
        assert!(v
            .warnings
            .iter()
            .any(|w| w.contains("users.name must be in GROUP BY")));

        e.apply("gb-add", 0, 1, "", &[]);
        assert!(e.view(&[], &[], &q, &esc, pg()).group_issues.is_empty());
        // The same column is not added twice.
        assert_eq!(e.apply("gb-add", 0, 1, "", &[]), Change::None);
    }

    #[test]
    fn having_without_group_by_is_flagged_and_numbers_stay_numeric() {
        let mut e = editor();
        e.apply("col-add", 0, 0, "", &[]);
        e.apply("hv-add", 0, 0, "", &[]);
        e.apply("hv-val", 0, 0, "5", &[]);
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v
            .group_issues
            .iter()
            .any(|w| w.contains("HAVING without GROUP BY")));
        assert!(
            sql(&e).contains("HAVING COUNT(\"users\".\"id\") > 5"),
            "{}",
            sql(&e)
        );
    }

    #[test]
    fn having_min_and_max_take_the_columns_kind() {
        let mut e = editor();
        e.apply("hv-add", 0, 0, "", &[]);
        e.apply("hv-col", 0, 1, "", &[]); // name: text
                                          // COUNT stays numeric whatever the column is.
        assert_eq!(e.state.having[0].kind, ValueKind::Number);
        let min = Aggregate::FOR_HAVING
            .iter()
            .position(|a| *a == Aggregate::Min)
            .unwrap();
        e.apply("hv-agg", 0, min as i32, "", &[]);
        assert_eq!(e.state.having[0].kind, ValueKind::Text);
    }

    // -- order by -------------------------------------------------------------

    #[test]
    fn order_by_toggles_direction() {
        let mut e = editor();
        e.apply("ob-add", 0, 1, "", &[]);
        assert!(sql(&e).contains("ORDER BY \"users\".\"name\" ASC"));
        e.apply("ob-dir", 0, 0, "", &[]);
        assert!(sql(&e).contains("ORDER BY \"users\".\"name\" DESC"));
        assert!(e.view(&[], &[], &q, &esc, pg()).order_by[0].descending);
        e.apply("ob-remove", 0, 0, "", &[]);
        assert!(!sql(&e).contains("ORDER BY"));
    }

    // -- limit / offset -------------------------------------------------------

    #[test]
    fn parse_count_accepts_whole_numbers_and_explains_the_rest() {
        assert_eq!(parse_count("", 1, "Limit"), Ok(None));
        assert_eq!(parse_count("  25 ", 1, "Limit"), Ok(Some(25)));
        assert_eq!(parse_count("0", 0, "Offset"), Ok(Some(0)));
        assert!(parse_count("0", 1, "Limit")
            .unwrap_err()
            .contains("at least 1"));
        assert!(parse_count("-3", 0, "Offset")
            .unwrap_err()
            .contains("whole number"));
        assert!(parse_count("1.5", 1, "Limit")
            .unwrap_err()
            .contains("whole number"));
        assert!(parse_count("ten", 1, "Limit")
            .unwrap_err()
            .contains("whole number"));
        assert!(parse_count("99999999999999999999999", 1, "Limit")
            .unwrap_err()
            .contains("too large"));
        assert!(parse_count("9223372036854775808", 1, "Limit")
            .unwrap_err()
            .contains("too large"));
        assert_eq!(
            parse_count("9223372036854775807", 1, "Limit"),
            Ok(Some(i64::MAX as u64))
        );
    }

    #[test]
    fn limit_and_offset_reach_the_statement_only_when_valid() {
        let mut e = editor();
        e.apply("limit", 0, 0, "10", &[]);
        e.apply("offset", 0, 0, "20", &[]);
        assert!(sql(&e).ends_with("LIMIT 10 OFFSET 20;"), "{}", sql(&e));
        let v = e.view(&[], &[], &q, &esc, pg());
        assert_eq!(v.pagination, "Rows 21 to 30");

        e.apply("limit", 0, 0, "abc", &[]);
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.limit_error.contains("whole number"));
        assert!(
            !sql(&e).contains("LIMIT 10"),
            "an invalid limit must not keep the old one"
        );
        assert!(v.warnings[0].contains("Limit must be a whole number"));
        assert!(!e.can_use(&e.sql(&q, &esc, pg())));

        e.apply("limit", 0, 0, "", &[]);
        assert!(e.can_use(&e.sql(&q, &esc, pg())));
    }

    #[test]
    fn sql_server_limit_without_order_by_is_warned() {
        let mut e = editor();
        e.apply("limit", 0, 0, "5", &[]);
        let w = e.warnings(rules_for(&DbDriver::SqlServer));
        assert!(w.iter().any(|w| w.contains("ORDER BY")));
    }

    // -- sections, reset, misc ------------------------------------------------

    #[test]
    fn sections_toggle_and_badge_their_counts() {
        let mut e = two_tables();
        e.apply("col-add", 0, 1, "", &[]);
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.sections[0].open && !v.sections[4].open);
        assert_eq!(v.sections[0].count, 2);
        assert_eq!(v.sections[2].count, 1);

        e.apply("section", 4, 0, "", &[]);
        assert!(e.view(&[], &[], &q, &esc, pg()).sections[4].open);
        assert_eq!(e.apply("section", 99, 0, "", &[]), Change::None);
    }

    #[test]
    fn reset_returns_to_an_empty_builder() {
        let mut e = two_tables();
        e.apply("col-add", 0, 1, "", &[]);
        e.apply("limit", 0, 0, "5", &[]);
        e.apply("section", 6, 0, "", &[]);
        assert!(!e.is_empty());
        e.apply("reset", 0, 0, "", &[]);
        assert!(e.is_empty());
        assert!(e.limit_text.is_empty());
        assert!(!e.open[6]);
        assert_eq!(sql(&e), "");
    }

    #[test]
    fn unknown_and_out_of_range_actions_change_nothing() {
        let mut e = editor();
        let before = e.state.clone();
        for (a, i, j) in [
            ("nope", 0, 0),
            ("table-remove", 9, 0),
            ("table-remove", -1, 0),
            ("join-remove", 0, 0),
            ("col-remove", 0, 0),
            ("col-add", 5, 0),
            ("col-add", 0, 99),
            ("f-remove", 9, 0),
            ("f-cond-add", 9, 0),
            ("gb-remove", 0, 0),
            ("ob-dir", 0, 0),
            ("hv-val", 0, 0),
        ] {
            assert_eq!(e.apply(a, i, j, "x", &[]), Change::None, "{a} {i} {j}");
        }
        assert_eq!(e.state, before);
    }

    #[test]
    fn an_empty_builder_has_no_sql_and_says_to_add_a_table() {
        let e = BuilderEditor::new();
        let v = e.view(&[], &[], &q, &esc, pg());
        assert!(v.sql.is_empty());
        assert!(v.warnings[0].contains("Add a table"));
    }

    #[test]
    fn flatten_addresses_groups_and_conditions_in_drawing_order() {
        let mut root = ConditionGroup::default();
        root.conditions
            .push(Condition::new("t", "a", ValueKind::Text));
        let mut inner = ConditionGroup::default();
        inner
            .conditions
            .push(Condition::new("t", "b", ValueKind::Text));
        root.groups.push(inner);
        let flat = flatten_filters(&root);
        let shape: Vec<(usize, Option<usize>, Vec<usize>)> = flat
            .iter()
            .map(|f| (f.depth, f.condition, f.path.clone()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, None, vec![]),
                (0, Some(0), vec![]),
                (1, None, vec![0]),
                (1, Some(0), vec![0]),
            ]
        );
    }
}
