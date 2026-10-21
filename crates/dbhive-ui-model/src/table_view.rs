//! The table inspector as a view of one workspace table tab.
//!
//! `table_inspector` holds the SQL and paging arithmetic lifted from the egui
//! screen. This module is the state machine around it for a shell that keeps
//! one tab per table: which view is showing (Data or Structure), where the
//! pager is, the filters being edited versus the filters the page was
//! fetched with, the sort, the schema load, and which cell is selected.
//!
//! Pure state, no toolkit and no async. A shell starts a load, gets a
//! sequence number, and hands the outcome back with that number; a stale
//! outcome is refused, the same discipline `workspace_tabs` uses for runs.
//!
//! Also here, because they are decisions and not drawing: what copy-cell,
//! copy-row, copy-column and the JSON side panel put on the clipboard, and
//! the Structure view's rows (columns with PK/FK badges, indexes, relations).

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{DbDriver, ForeignKeyInfo, TableSchema};

use crate::results_grid::cell_text;
use crate::table_inspector::{
    count_sql, default_label, index_kind_label, max_safe_page, offset_for_page,
    paged_select_sql_sorted, primary_key_columns, thousands, total_pages, DataQuery, DialectRules,
    Filter, FilterOp, SortSpec, TableMeta, MAX_SAFE_OFFSET, PAGE_SIZES,
};

/// Default rows per page, same as the egui inspector.
pub const DEFAULT_PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Data,
    Structure,
}

/// The schema and foreign keys of the table, as a load that can be in any of
/// four states. The Structure view needs all four drawn.
pub enum Meta {
    Idle,
    Loading,
    Ready(TableMeta),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRef {
    pub row: usize,
    pub col: usize,
}

pub struct TableView {
    pub view: View,
    pub page: usize,
    pub page_size: usize,
    /// Filters being edited. They reach SQL only through `apply_filters`.
    pub draft: Vec<Filter>,
    applied: Vec<Filter>,
    pub sort: Option<SortSpec>,
    /// Row count for the applied filters, once known.
    pub total: Option<u64>,
    pub counting: bool,
    pub meta: Meta,
    /// The requested page was past `MAX_SAFE_OFFSET` and was pulled back.
    pub deep_offset_blocked: bool,
    pub selection: Option<CellRef>,
    /// Rows ticked in the grid's checkbox column, as indexes into the shown
    /// page (new unsaved rows come after the page's own). A new page, sort or
    /// filter drops them, like the selection, because the indexes would then
    /// point at different rows.
    pub checked: std::collections::BTreeSet<usize>,
    pub panel_open: bool,
    pub filters_open: bool,
    /// Row editing: pending changes, the cell being typed in, the review page.
    pub edit: crate::row_edits::EditUi,
    meta_seq: u64,
    count_seq: u64,
}

impl Default for TableView {
    fn default() -> Self {
        Self {
            view: View::Data,
            page: 1,
            page_size: DEFAULT_PAGE_SIZE,
            draft: Vec::new(),
            applied: Vec::new(),
            sort: None,
            total: None,
            counting: false,
            meta: Meta::Idle,
            deep_offset_blocked: false,
            selection: None,
            checked: std::collections::BTreeSet::new(),
            panel_open: false,
            filters_open: false,
            edit: crate::row_edits::EditUi::default(),
            meta_seq: 0,
            count_seq: 0,
        }
    }
}

impl TableView {
    // -- schema load -------------------------------------------------------

    pub fn begin_meta(&mut self) -> u64 {
        self.meta_seq += 1;
        self.meta = Meta::Loading;
        self.meta_seq
    }

    /// Land the schema load. `false` when it is stale and nothing changed.
    pub fn finish_meta(&mut self, seq: u64, outcome: Result<TableMeta, String>) -> bool {
        if seq != self.meta_seq {
            return false;
        }
        self.meta = match outcome {
            Ok(m) => Meta::Ready(m),
            Err(e) => Meta::Failed(e),
        };
        true
    }

    pub fn meta_loading(&self) -> bool {
        matches!(self.meta, Meta::Loading)
    }

    pub fn meta_error(&self) -> Option<&str> {
        match &self.meta {
            Meta::Failed(e) => Some(e),
            _ => None,
        }
    }

    pub fn schema(&self) -> Option<&TableMeta> {
        match &self.meta {
            Meta::Ready(m) => Some(m),
            _ => None,
        }
    }

    /// The stable paging key. Empty until the schema lands, or if it failed;
    /// the rows then load unordered rather than not at all.
    pub fn order_by(&self) -> Vec<String> {
        self.schema()
            .map(|m| primary_key_columns(&m.schema))
            .unwrap_or_default()
    }

    // -- row count ---------------------------------------------------------

    pub fn begin_count(&mut self) -> u64 {
        self.count_seq += 1;
        self.counting = true;
        self.count_seq
    }

    /// Land a row count. A failed count leaves the total unknown, which the
    /// pager already copes with (it infers "has next" from a full page).
    pub fn finish_count(&mut self, seq: u64, outcome: Result<Option<u64>, String>) -> bool {
        if seq != self.count_seq {
            return false;
        }
        self.counting = false;
        self.total = outcome.ok().flatten();
        true
    }

    // -- paging ------------------------------------------------------------

    pub fn pager(&self, shown: usize) -> PagerState {
        pager_state(self.page, self.page_size, shown, self.total)
    }

    /// Settle the page about to be fetched. A page past the deep-offset guard
    /// is pulled back to the deepest cheap page and flagged.
    pub fn prepare_page(&mut self) -> usize {
        if offset_for_page(self.page, self.page_size) > MAX_SAFE_OFFSET {
            self.deep_offset_blocked = true;
            self.page = max_safe_page(self.page_size);
        } else {
            self.deep_offset_blocked = false;
        }
        self.page
    }

    fn goto(&mut self, page: usize) -> bool {
        let page = page.max(1);
        let changed = page != self.page;
        self.page = page;
        self.selection = None;
        self.checked.clear();
        changed
    }

    pub fn first(&mut self) -> bool {
        self.goto(1)
    }

    pub fn prev(&mut self) -> bool {
        self.goto(self.page.saturating_sub(1))
    }

    /// Step forward if the pager allows it. `shown` is the rows on the
    /// current page, for the case where the total is unknown.
    pub fn next(&mut self, shown: usize) -> bool {
        if !self.pager(shown).can_next {
            return false;
        }
        self.goto(self.page + 1)
    }

    pub fn jump_to_safe_page(&mut self) -> bool {
        self.goto(max_safe_page(self.page_size))
    }

    /// The old page number points at a different slice under a new size;
    /// page 1 is the only reading that cannot skip rows.
    pub fn set_page_size(&mut self, size: usize) -> bool {
        if size == self.page_size || !PAGE_SIZES.contains(&size) {
            return false;
        }
        self.page_size = size;
        self.page = 1;
        self.selection = None;
        self.checked.clear();
        true
    }

    // -- filters -----------------------------------------------------------

    pub fn add_filter(&mut self, default_column: &str) {
        self.draft.push(Filter::new(default_column));
        self.filters_open = true;
    }

    pub fn remove_filter(&mut self, index: usize) {
        if index < self.draft.len() {
            self.draft.remove(index);
        }
    }

    pub fn set_filter_column(&mut self, index: usize, column: &str) {
        if let Some(f) = self.draft.get_mut(index) {
            f.column = column.to_string();
        }
    }

    pub fn set_filter_op(&mut self, index: usize, op_index: usize) {
        if let (Some(f), Some(op)) = (self.draft.get_mut(index), FilterOp::ALL.get(op_index)) {
            f.op = *op;
        }
    }

    pub fn set_filter_value(&mut self, index: usize, value: &str) {
        if let Some(f) = self.draft.get_mut(index) {
            f.value = value.to_string();
        }
    }

    fn active(filters: &[Filter]) -> Vec<Filter> {
        filters.iter().filter(|f| f.is_active()).cloned().collect()
    }

    /// Whether the draft would fetch something different from what is shown.
    pub fn filters_dirty(&self) -> bool {
        Self::active(&self.draft) != self.applied
    }

    /// How many filters the shown rows were fetched with.
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    pub fn applied(&self) -> &[Filter] {
        &self.applied
    }

    /// Make the draft the filters in force. Returns whether anything changed
    /// (and so whether the page and the count must be fetched again). The
    /// page returns to 1: the old page number means nothing for a new set.
    pub fn apply_filters(&mut self) -> bool {
        let next = Self::active(&self.draft);
        if next == self.applied {
            return false;
        }
        self.applied = next;
        self.page = 1;
        self.total = None;
        self.selection = None;
        self.checked.clear();
        true
    }

    pub fn clear_filters(&mut self) -> bool {
        self.draft.clear();
        self.apply_filters()
    }

    pub fn filter_rows(&self) -> Vec<FilterRowView> {
        self.draft
            .iter()
            .map(|f| FilterRowView {
                column: f.column.clone(),
                op: f.op.label(),
                op_index: FilterOp::ALL.iter().position(|o| *o == f.op).unwrap_or(0),
                value: f.value.clone(),
                takes_value: f.op.takes_value(),
            })
            .collect()
    }

    // -- sort --------------------------------------------------------------

    /// Click on a header: ascending, then descending, then unsorted. Sorting
    /// happens in the database, so it applies to the whole table and not just
    /// the rows on this page. Returns the new sort for the caller's benefit.
    pub fn toggle_sort(&mut self, column: &str) {
        self.sort = match &self.sort {
            Some(s) if s.column == column && !s.descending => Some(SortSpec {
                column: column.to_string(),
                descending: true,
            }),
            Some(s) if s.column == column => None,
            _ => Some(SortSpec {
                column: column.to_string(),
                descending: false,
            }),
        };
        self.page = 1;
        self.selection = None;
        self.checked.clear();
    }

    /// Index of the sorted column among `columns`, for the header arrow.
    pub fn sort_column_index(&self, columns: &[String]) -> Option<usize> {
        let s = self.sort.as_ref()?;
        columns.iter().position(|c| *c == s.column)
    }

    // -- SQL ---------------------------------------------------------------

    /// The query for the page about to be fetched. Identifiers go through
    /// `quote` and filter values through `escape`, both from the driver.
    pub fn data_sql(
        &mut self,
        quote: &impl Fn(&str) -> String,
        escape: &impl Fn(&str) -> String,
        rules: DialectRules,
        qualifier: &str,
        table: &str,
    ) -> String {
        let page = self.prepare_page();
        let order_by = self.order_by();
        paged_select_sql_sorted(
            quote,
            escape,
            rules,
            &DataQuery {
                schema: qualifier,
                table,
                filters: &self.applied,
                order_by: &order_by,
                page,
                page_size: self.page_size,
            },
            self.sort.as_ref(),
        )
    }

    pub fn count_sql(
        &self,
        quote: &impl Fn(&str) -> String,
        escape: &impl Fn(&str) -> String,
        rules: DialectRules,
        qualifier: &str,
        table: &str,
    ) -> String {
        count_sql(quote, escape, rules, qualifier, table, &self.applied)
    }

    // -- selection ---------------------------------------------------------

    pub fn select(&mut self, row: usize, col: usize) {
        self.selection = Some(CellRef { row, col });
        self.panel_open = true;
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.checked.clear();
    }

    /// Tick or untick one row of the checkbox column.
    pub fn toggle_checked(&mut self, row: usize) {
        if !self.checked.remove(&row) {
            self.checked.insert(row);
        }
    }

    /// The header checkbox: tick every one of `rows` rows, or, when they are
    /// all ticked already, none.
    pub fn toggle_all_checked(&mut self, rows: usize) {
        if rows > 0 && (0..rows).all(|r| self.checked.contains(&r)) {
            self.checked.clear();
        } else {
            self.checked = (0..rows).collect();
        }
    }

    /// Whether every one of `rows` rows is ticked (false for no rows).
    pub fn all_checked(&self, rows: usize) -> bool {
        rows > 0 && (0..rows).all(|r| self.checked.contains(&r))
    }

    /// Column names for the filter dropdown: the schema's when it has
    /// loaded, else whatever the page returned.
    pub fn filter_column_names(&self, page: Option<&QueryResult>) -> Vec<String> {
        if let Some(m) = self.schema() {
            return m.schema.columns.iter().map(|c| c.name.clone()).collect();
        }
        page.map(|r| r.columns.clone()).unwrap_or_default()
    }
}

/// One editable filter row, as the shell draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRowView {
    pub column: String,
    pub op: &'static str,
    pub op_index: usize,
    pub value: String,
    pub takes_value: bool,
}

/// Labels for the operator dropdown, in `FilterOp::ALL` order.
pub fn filter_op_labels() -> Vec<&'static str> {
    FilterOp::ALL.iter().map(|o| o.label()).collect()
}

// ---------------------------------------------------------------------------
// Pager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagerState {
    /// "Page 2 of 5", or "Page 2" when the total is unknown.
    pub page_label: String,
    pub can_first: bool,
    pub can_prev: bool,
    pub can_next: bool,
    /// There is a next page but it is past the deep-offset guard.
    pub show_jump: bool,
}

pub fn pager_state(page: usize, page_size: usize, shown: usize, total: Option<u64>) -> PagerState {
    let pages = total.map(|t| total_pages(t, page_size));
    // Without a total, "is there a next page" is inferred from a full page.
    let has_next = match pages {
        Some(p) => page < p,
        None => shown == page_size,
    };
    let next_ok = offset_for_page(page + 1, page_size) <= MAX_SAFE_OFFSET;
    PagerState {
        page_label: match pages {
            Some(p) => format!("Page {} of {}", thousands(page as u64), thousands(p as u64)),
            None => format!("Page {}", thousands(page as u64)),
        },
        can_first: page > 1,
        can_prev: page > 1,
        can_next: has_next && next_ok,
        show_jump: has_next && !next_ok,
    }
}

pub fn page_size_labels() -> Vec<String> {
    PAGE_SIZES.iter().map(|s| format!("{s} / page")).collect()
}

pub fn page_size_label(size: usize) -> String {
    format!("{size} / page")
}

pub fn page_size_at(index: usize) -> Option<usize> {
    PAGE_SIZES.get(index).copied()
}

/// The qualifier generated SQL puts before the table name.
///
/// The Postgres family qualifies with the schema. MySQL's "schema" is the
/// database, which qualifies too. SQLite's "schema" in the workspace is the
/// file path and SQL Server's is a database name, neither of which is valid
/// in front of a table, so they are left bare and resolve against the
/// connection's own default.
pub fn sql_qualifier(driver: &DbDriver, schema_arg: &str) -> String {
    if driver.is_postgres_compatible() || matches!(driver, DbDriver::MySql) {
        schema_arg.to_string()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Copying
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKind {
    Cell,
    /// The row as tab-separated values, for pasting into a spreadsheet.
    Row,
    /// Every value in the column on this page, one per line.
    Column,
    /// The row as a JSON object.
    RowJson,
}

/// What a copy puts on the clipboard, or `None` if the cell is out of range.
///
/// A database NULL copies as the text `NULL` in the plain forms, matching what
/// the grid shows, and as `null` in JSON.
pub fn copy_text(result: &QueryResult, kind: CopyKind, row: usize, col: usize) -> Option<String> {
    match kind {
        CopyKind::Cell => Some(cell_text(result.rows.get(row)?.get(col)?).0),
        CopyKind::Row => {
            let r = result.rows.get(row)?;
            Some(
                r.iter()
                    .map(|v| tsv_field(&cell_text(v).0))
                    .collect::<Vec<_>>()
                    .join("\t"),
            )
        }
        CopyKind::Column => {
            if col >= result.columns.len() {
                return None;
            }
            Some(
                result
                    .rows
                    .iter()
                    .map(|r| {
                        r.get(col)
                            .map(|v| tsv_field(&cell_text(v).0))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        CopyKind::RowJson => row_json(result, row),
    }
}

/// A TSV field. Tabs, newlines and quotes would break the row apart on paste,
/// so such a value is quoted with doubled quotes, as spreadsheets expect.
fn tsv_field(s: &str) -> String {
    if s.contains(['\t', '\n', '\r', '"']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One row as a pretty JSON object, keys in column order. (`serde_json::Map`
/// would sort them, so the object is written by hand.)
pub fn row_json(result: &QueryResult, row: usize) -> Option<String> {
    let r = result.rows.get(row)?;
    if result.columns.is_empty() {
        return Some("{}".to_string());
    }
    let mut out = String::from("{\n");
    for (i, name) in result.columns.iter().enumerate() {
        let key = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
        let value = r.get(i).unwrap_or(&serde_json::Value::Null);
        let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
        // Nested values span lines; indent every line after the first.
        let pretty = pretty.replace('\n', "\n  ");
        out.push_str("  ");
        out.push_str(&key);
        out.push_str(": ");
        out.push_str(&pretty);
        if i + 1 < result.columns.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push('}');
    Some(out)
}

/// The ticked rows as one pretty JSON array of objects keyed by column name,
/// in grid order. Indexes past the page (new unsaved rows) are skipped: they
/// have no stored values to copy.
pub fn rows_json(result: &QueryResult, rows: &std::collections::BTreeSet<usize>) -> String {
    // Built from `row_json` so keys keep the column order (serde_json's map
    // would sort them).
    let objects: Vec<String> = rows
        .iter()
        .filter_map(|&i| row_json(result, i))
        .map(|o| format!("  {}", o.replace('\n', "\n  ")))
        .collect();
    if objects.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{}\n]", objects.join(",\n"))
    }
}

/// "id · 42" style caption for the selected cell, shown above the JSON.
pub fn selection_label(result: &QueryResult, cell: CellRef) -> String {
    let name = result
        .columns
        .get(cell.col)
        .map(String::as_str)
        .unwrap_or("");
    format!("Row {} · {}", thousands(cell.row as u64 + 1), name)
}

// ---------------------------------------------------------------------------
// Structure view
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRow {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: String,
    pub primary: bool,
    pub auto: bool,
    /// "customers(id)" when the column is (part of) a foreign key.
    pub foreign: String,
    /// The table that foreign key points at, so a click can open it.
    pub foreign_table: String,
}

pub fn column_rows(
    schema: &TableSchema,
    keys: &[ForeignKeyInfo],
    schema_name: &str,
) -> Vec<ColumnRow> {
    let table = schema.table.name.as_str();
    let mine: Vec<&ForeignKeyInfo> = keys
        .iter()
        .filter(|fk| fk.table == table && (schema_name.is_empty() || fk.schema == schema_name))
        .collect();

    schema
        .columns
        .iter()
        .map(|c| {
            let fk = mine.iter().find_map(|fk| {
                let pos = fk.columns.iter().position(|n| *n == c.name)?;
                Some((
                    format!(
                        "{}({})",
                        fk.referenced_table,
                        fk.referenced_columns.get(pos).cloned().unwrap_or_default()
                    ),
                    fk.referenced_table.clone(),
                ))
            });
            ColumnRow {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                nullable: c.nullable,
                default: default_label(c.default_value.as_ref()),
                primary: c.is_primary_key,
                auto: c.is_auto_increment,
                foreign: fk.as_ref().map(|f| f.0.clone()).unwrap_or_default(),
                foreign_table: fk.map(|f| f.1).unwrap_or_default(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub name: String,
    pub columns: String,
    pub kind: &'static str,
}

pub fn index_rows(schema: &TableSchema) -> Vec<IndexRow> {
    schema
        .indexes
        .iter()
        .map(|i| IndexRow {
            name: i.name.clone(),
            columns: i.columns.join(", "),
            kind: index_kind_label(i.is_primary, i.is_unique),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationRow {
    pub constraint: String,
    /// True when this table holds the key; false when another table points here.
    pub outgoing: bool,
    pub from: String,
    pub to: String,
    pub on_delete: String,
    pub on_update: String,
    /// The table on the other end, which a click opens.
    pub other_table: String,
}

pub fn relation_rows(keys: &[ForeignKeyInfo], schema_name: &str, table: &str) -> Vec<RelationRow> {
    let same = |s: &str, t: &str| t == table && (schema_name.is_empty() || s == schema_name);
    keys.iter()
        .filter(|fk| {
            same(&fk.schema, &fk.table) || same(&fk.referenced_schema, &fk.referenced_table)
        })
        .map(|fk| {
            let outgoing = same(&fk.schema, &fk.table);
            RelationRow {
                constraint: fk.name.clone(),
                outgoing,
                from: format!("{}({})", fk.table, fk.columns.join(", ")),
                to: format!(
                    "{}({})",
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                ),
                on_delete: fk.on_delete.clone().unwrap_or_else(|| "NO ACTION".into()),
                on_update: fk.on_update.clone().unwrap_or_else(|| "NO ACTION".into()),
                other_table: if outgoing {
                    fk.referenced_table.clone()
                } else {
                    fk.table.clone()
                },
            }
        })
        .collect()
}

/// "1,204 rows" for the toolbar, with a hint when a filter is narrowing it.
pub fn total_label(total: Option<u64>, counting: bool, filtered: bool) -> String {
    match (total, counting) {
        (Some(t), _) => format!(
            "{} {}{}",
            thousands(t),
            if t == 1 { "row" } else { "rows" },
            if filtered { " match" } else { "" }
        ),
        (None, true) => "Counting rows…".to_string(),
        (None, false) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{ColumnInfo, IndexInfo, TableInfo};
    use serde_json::json;

    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
    fn esc(v: &str) -> String {
        v.replace('\'', "''")
    }

    fn orders_schema() -> TableSchema {
        TableSchema {
            table: TableInfo::new("orders".into(), "main".into(), "TABLE".into()),
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    data_type: "INTEGER".into(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: true,
                    is_auto_increment: true,
                },
                ColumnInfo {
                    name: "customer_id".into(),
                    data_type: "INTEGER".into(),
                    nullable: false,
                    default_value: None,
                    is_primary_key: false,
                    is_auto_increment: false,
                },
                ColumnInfo {
                    name: "note".into(),
                    data_type: "TEXT".into(),
                    nullable: true,
                    default_value: Some(String::new()),
                    is_primary_key: false,
                    is_auto_increment: false,
                },
            ],
            indexes: vec![
                IndexInfo::new("pk_orders".into(), vec!["id".into()], true, true),
                IndexInfo::new(
                    "ix_customer".into(),
                    vec!["customer_id".into()],
                    false,
                    false,
                ),
            ],
        }
    }

    fn keys() -> Vec<ForeignKeyInfo> {
        let mut out = ForeignKeyInfo::new(
            "fk_orders_customer".into(),
            "orders".into(),
            "main".into(),
            vec!["customer_id".into()],
            "customers".into(),
            "main".into(),
            vec!["id".into()],
        );
        out.on_delete = Some("CASCADE".into());
        let inc = ForeignKeyInfo::new(
            "fk_items_order".into(),
            "order_items".into(),
            "main".into(),
            vec!["order_id".into()],
            "orders".into(),
            "main".into(),
            vec!["id".into()],
        );
        let unrelated = ForeignKeyInfo::new(
            "fk_x".into(),
            "a".into(),
            "main".into(),
            vec!["b_id".into()],
            "b".into(),
            "main".into(),
            vec!["id".into()],
        );
        vec![out, inc, unrelated]
    }

    fn ready(view: &mut TableView) {
        let seq = view.begin_meta();
        assert!(view.finish_meta(
            seq,
            Ok(TableMeta {
                schema: orders_schema(),
                foreign_keys: keys(),
            })
        ));
    }

    fn pg() -> DialectRules {
        crate::table_inspector::rules_for(&DbDriver::Postgres)
    }

    // -- paging ------------------------------------------------------------

    #[test]
    fn walking_forward_stops_on_the_last_page() {
        let mut v = TableView::default();
        v.total = Some(120); // 3 pages of 50
        assert!(v.next(50));
        assert!(v.next(50));
        assert_eq!(v.page, 3);
        assert!(!v.next(20), "page 3 of 3 has no next");
        assert_eq!(v.page, 3);
        assert!(v.prev());
        assert!(v.first());
        assert!(!v.first(), "already on the first page");
        assert!(!v.prev());
    }

    #[test]
    fn without_a_total_a_full_page_means_there_may_be_more() {
        let mut v = TableView::default();
        assert!(v.next(50));
        assert!(!v.next(49), "a short page is the last one");
    }

    #[test]
    fn pager_labels_read_page_n_of_m() {
        let p = pager_state(2, 25, 25, Some(60));
        assert_eq!(p.page_label, "Page 2 of 3");
        assert!(p.can_prev && p.can_next && !p.show_jump);
        assert_eq!(pager_state(1, 25, 3, None).page_label, "Page 1");
    }

    #[test]
    fn the_deep_offset_guard_stops_next_and_offers_a_jump() {
        // 25 per page: page 4001 starts at offset 100,000 (allowed), page 4002 is past it.
        let at_edge = pager_state(4001, 25, 25, Some(1_000_000));
        assert!(!at_edge.can_next, "next would cross the guard");
        assert!(at_edge.show_jump);
        let before = pager_state(4000, 25, 25, Some(1_000_000));
        assert!(before.can_next && !before.show_jump);
        // On the last page there is nothing to jump to.
        assert!(!pager_state(4001, 25, 25, Some(100_025)).show_jump);
    }

    #[test]
    fn a_requested_page_past_the_guard_is_pulled_back_and_flagged() {
        let mut v = TableView::default();
        v.page_size = 25;
        v.page = 9_999;
        assert_eq!(v.prepare_page(), max_safe_page(25));
        assert!(v.deep_offset_blocked);
        assert!(crate::table_inspector::offset_for_page(v.page, 25) <= MAX_SAFE_OFFSET);
        v.page = 3;
        v.prepare_page();
        assert!(
            !v.deep_offset_blocked,
            "the flag clears once the page is fine"
        );
    }

    #[test]
    fn jumping_lands_on_the_deepest_safe_page() {
        let mut v = TableView::default();
        v.page_size = 100;
        assert!(v.jump_to_safe_page());
        assert_eq!(v.page, max_safe_page(100));
    }

    #[test]
    fn changing_page_size_returns_to_page_one_and_rejects_odd_sizes() {
        let mut v = TableView::default();
        v.page = 7;
        assert!(v.set_page_size(100));
        assert_eq!((v.page, v.page_size), (1, 100));
        assert!(!v.set_page_size(100), "same size is not a change");
        assert!(!v.set_page_size(33), "only the offered sizes");
        assert_eq!(v.page_size, 100);
    }

    #[test]
    fn page_size_labels_match_the_offered_sizes() {
        assert_eq!(
            page_size_labels(),
            vec!["25 / page", "50 / page", "100 / page", "250 / page"]
        );
        assert_eq!(page_size_at(2), Some(100));
        assert_eq!(page_size_at(9), None);
    }

    // -- stale loads -------------------------------------------------------

    #[test]
    fn a_stale_schema_or_count_is_refused() {
        let mut v = TableView::default();
        let first = v.begin_meta();
        let second = v.begin_meta();
        assert!(!v.finish_meta(first, Err("old".into())));
        assert!(v.meta_loading(), "the stale result must not end the load");
        assert!(v.finish_meta(second, Err("boom".into())));
        assert_eq!(v.meta_error(), Some("boom"));

        let a = v.begin_count();
        let b = v.begin_count();
        assert!(!v.finish_count(a, Ok(Some(5))));
        assert!(v.counting);
        assert!(v.finish_count(b, Ok(Some(7))));
        assert_eq!((v.total, v.counting), (Some(7), false));
        // A failed count leaves the total unknown.
        let c = v.begin_count();
        v.finish_count(c, Err("x".into()));
        assert_eq!(v.total, None);
    }

    #[test]
    fn the_paging_key_is_the_primary_key_once_the_schema_is_in() {
        let mut v = TableView::default();
        assert!(v.order_by().is_empty());
        ready(&mut v);
        assert_eq!(v.order_by(), vec!["id".to_string()]);
    }

    // -- filters -----------------------------------------------------------

    #[test]
    fn editing_a_filter_changes_nothing_until_it_is_applied() {
        let mut v = TableView::default();
        v.add_filter("note");
        v.set_filter_value(0, "gift");
        assert!(v.filters_dirty());
        assert_eq!(v.applied_count(), 0);
        assert!(v.apply_filters());
        assert_eq!(v.applied_count(), 1);
        assert!(!v.filters_dirty());
        assert!(
            !v.apply_filters(),
            "applying the same filters is not a change"
        );
    }

    #[test]
    fn applying_filters_returns_to_page_one_and_forgets_the_count() {
        let mut v = TableView::default();
        v.page = 4;
        v.total = Some(400);
        v.add_filter("note");
        v.set_filter_value(0, "x");
        assert!(v.apply_filters());
        assert_eq!(v.page, 1);
        assert_eq!(v.total, None);
    }

    #[test]
    fn half_typed_filters_are_not_applied() {
        let mut v = TableView::default();
        v.add_filter("note"); // no value yet
        assert!(!v.filters_dirty(), "an empty value is not a pending change");
        assert!(!v.apply_filters());
        v.set_filter_op(
            0,
            FilterOp::ALL
                .iter()
                .position(|o| *o == FilterOp::IsNull)
                .unwrap(),
        );
        assert!(v.filters_dirty(), "is null needs no value");
        assert!(v.apply_filters());
    }

    #[test]
    fn clearing_filters_is_a_change_only_if_some_were_applied() {
        let mut v = TableView::default();
        v.add_filter("note");
        assert!(!v.clear_filters());
        v.add_filter("note");
        v.set_filter_value(0, "a");
        v.apply_filters();
        assert!(v.clear_filters());
        assert!(v.draft.is_empty() && v.applied().is_empty());
    }

    #[test]
    fn filter_rows_report_the_operator_and_whether_it_takes_a_value() {
        let mut v = TableView::default();
        v.add_filter("note");
        v.set_filter_op(0, 11);
        let rows = v.filter_rows();
        assert_eq!(rows[0].op, "is not null");
        assert!(!rows[0].takes_value);
        assert_eq!(filter_op_labels().len(), FilterOp::ALL.len());
        // Out-of-range indexes are ignored, not panics.
        v.set_filter_op(0, 99);
        v.set_filter_op(5, 0);
        v.set_filter_value(5, "x");
        v.remove_filter(5);
    }

    // -- sort and SQL ------------------------------------------------------

    #[test]
    fn a_header_click_cycles_ascending_descending_off() {
        let mut v = TableView::default();
        v.page = 3;
        v.toggle_sort("note");
        assert_eq!(
            v.sort,
            Some(SortSpec {
                column: "note".into(),
                descending: false
            })
        );
        assert_eq!(v.page, 1);
        v.toggle_sort("note");
        assert!(v.sort.as_ref().unwrap().descending);
        v.toggle_sort("note");
        assert_eq!(v.sort, None);
        v.toggle_sort("note");
        v.toggle_sort("id");
        assert_eq!(
            v.sort,
            Some(SortSpec {
                column: "id".into(),
                descending: false
            }),
            "a new column starts ascending"
        );
    }

    #[test]
    fn sort_column_index_finds_the_header() {
        let mut v = TableView::default();
        let cols = vec!["id".to_string(), "note".to_string()];
        assert_eq!(v.sort_column_index(&cols), None);
        v.toggle_sort("note");
        assert_eq!(v.sort_column_index(&cols), Some(1));
        v.toggle_sort("gone");
        assert_eq!(v.sort_column_index(&cols), None);
    }

    #[test]
    fn data_sql_puts_the_user_sort_before_the_primary_key() {
        let mut v = TableView::default();
        ready(&mut v);
        v.toggle_sort("note");
        v.toggle_sort("note"); // descending
        v.page = 2;
        v.page_size = 25;
        let sql = v.data_sql(&q, &esc, pg(), "public", "orders");
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"orders\" ORDER BY \"note\" DESC, \"id\" LIMIT 25 OFFSET 25"
        );
    }

    #[test]
    fn sorting_by_the_key_does_not_repeat_it() {
        let mut v = TableView::default();
        ready(&mut v);
        v.toggle_sort("id");
        let sql = v.data_sql(&q, &esc, pg(), "", "orders");
        assert_eq!(
            sql,
            "SELECT * FROM \"orders\" ORDER BY \"id\" ASC LIMIT 50 OFFSET 0"
        );
    }

    #[test]
    fn data_and_count_sql_share_the_applied_filters_and_never_the_draft() {
        let mut v = TableView::default();
        v.add_filter("note");
        v.set_filter_value(0, "it's");
        // Not applied yet: the SQL must not see the draft.
        let before = v.data_sql(&q, &esc, pg(), "", "orders");
        assert!(!before.contains("WHERE"));
        v.apply_filters();
        let data = v.data_sql(&q, &esc, pg(), "", "orders");
        let count = v.count_sql(&q, &esc, pg(), "", "orders");
        assert!(
            data.contains("WHERE CAST(\"note\" AS TEXT) ILIKE '%it''s%'"),
            "{data}"
        );
        assert!(
            count.contains("WHERE CAST(\"note\" AS TEXT) ILIKE '%it''s%'"),
            "{count}"
        );
        assert!(count.starts_with("SELECT COUNT(*) FROM \"orders\""));
    }

    #[test]
    fn a_hostile_sort_column_stays_inside_the_quoting() {
        let mut v = TableView::default();
        v.toggle_sort("x\"; DROP TABLE orders; --");
        let sql = v.data_sql(&q, &esc, pg(), "", "orders");
        assert!(
            sql.contains("ORDER BY \"x\"\"; DROP TABLE orders; --\" ASC"),
            "{sql}"
        );
    }

    #[test]
    fn qualifier_depends_on_the_dialect() {
        assert_eq!(sql_qualifier(&DbDriver::Postgres, "public"), "public");
        assert_eq!(sql_qualifier(&DbDriver::MySql, "shop"), "shop");
        assert_eq!(sql_qualifier(&DbDriver::Sqlite, "/data/app.db"), "");
        assert_eq!(sql_qualifier(&DbDriver::SqlServer, "master"), "");
    }

    #[test]
    fn totals_read_naturally() {
        assert_eq!(total_label(Some(1204), false, false), "1,204 rows");
        assert_eq!(total_label(Some(1), false, false), "1 row");
        assert_eq!(total_label(Some(3), false, true), "3 rows match");
        assert_eq!(total_label(None, true, false), "Counting rows…");
        assert_eq!(total_label(None, false, false), "");
    }

    // -- copy --------------------------------------------------------------

    fn sample() -> QueryResult {
        QueryResult::with_data(
            vec!["id".into(), "note".into(), "meta".into()],
            vec![
                vec![json!(1), json!("plain"), json!({"a": [1, 2]})],
                vec![json!(2), json!("tab\there \"q\""), json!(null)],
                vec![json!(3), json!(null), json!(true)],
            ],
        )
    }

    #[test]
    fn a_cell_copies_as_shown_with_null_as_null_text() {
        let r = sample();
        assert_eq!(copy_text(&r, CopyKind::Cell, 0, 1).unwrap(), "plain");
        assert_eq!(copy_text(&r, CopyKind::Cell, 2, 1).unwrap(), "NULL");
        assert_eq!(copy_text(&r, CopyKind::Cell, 0, 0).unwrap(), "1");
        assert_eq!(copy_text(&r, CopyKind::Cell, 9, 0), None);
        assert_eq!(copy_text(&r, CopyKind::Cell, 0, 9), None);
    }

    #[test]
    fn a_row_copies_as_tsv_and_quotes_what_would_break_it() {
        let r = sample();
        assert_eq!(
            copy_text(&r, CopyKind::Row, 0, 0).unwrap(),
            "1\tplain\t\"{\"\"a\"\":[1,2]}\""
        );
        assert_eq!(
            copy_text(&r, CopyKind::Row, 1, 0).unwrap(),
            "2\t\"tab\there \"\"q\"\"\"\tNULL"
        );
        assert_eq!(copy_text(&r, CopyKind::Row, 7, 0), None);
    }

    #[test]
    fn a_column_copies_one_value_per_line() {
        let r = sample();
        assert_eq!(copy_text(&r, CopyKind::Column, 0, 0).unwrap(), "1\n2\n3");
        assert_eq!(
            copy_text(&r, CopyKind::Column, 0, 1).unwrap(),
            "plain\n\"tab\there \"\"q\"\"\"\nNULL"
        );
        assert_eq!(copy_text(&r, CopyKind::Column, 0, 5), None);
    }

    #[test]
    fn row_json_keeps_column_order_nests_and_writes_null() {
        let r = QueryResult::with_data(
            vec!["zeta".into(), "alpha".into(), "n".into()],
            vec![vec![json!(1), json!({"k": "v"}), json!(null)]],
        );
        let j = row_json(&r, 0).unwrap();
        assert_eq!(
            j,
            "{\n  \"zeta\": 1,\n  \"alpha\": {\n    \"k\": \"v\"\n  },\n  \"n\": null\n}"
        );
        // It is valid JSON, and keys with quotes survive.
        let parsed: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(parsed["alpha"]["k"], "v");
        let odd = QueryResult::with_data(vec!["a\"b".into()], vec![vec![json!("x")]]);
        let parsed: serde_json::Value = serde_json::from_str(&row_json(&odd, 0).unwrap()).unwrap();
        assert_eq!(parsed["a\"b"], "x");
        assert_eq!(row_json(&r, 3), None);
    }

    #[test]
    fn rows_json_is_an_array_in_grid_order_skipping_rows_that_are_not_stored() {
        let r = QueryResult::with_data(
            vec!["zeta".into(), "alpha".into()],
            vec![
                vec![json!(1), json!("a")],
                vec![json!(2), json!(null)],
                vec![json!(3), json!("c")],
            ],
        );
        let pick: std::collections::BTreeSet<usize> = [2, 0, 9].into_iter().collect();
        let j = rows_json(&r, &pick);
        let parsed: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2, "row 9 is not on the page");
        assert_eq!(parsed[0]["zeta"], 1);
        assert_eq!(parsed[1]["alpha"], "c");
        assert!(j.find("zeta").unwrap() < j.find("alpha").unwrap(), "column order kept");
        assert_eq!(rows_json(&r, &Default::default()), "[]");
    }

    #[test]
    fn checking_rows_toggles_one_or_all_and_a_new_page_drops_them() {
        let mut v = TableView::default();
        v.toggle_checked(1);
        v.toggle_checked(3);
        assert_eq!(v.checked.iter().copied().collect::<Vec<_>>(), vec![1, 3]);
        v.toggle_checked(1);
        assert_eq!(v.checked.iter().copied().collect::<Vec<_>>(), vec![3]);
        // The header box ticks everything, then clears when everything is ticked.
        v.toggle_all_checked(4);
        assert!(v.all_checked(4));
        v.toggle_all_checked(4);
        assert!(v.checked.is_empty());
        assert!(!v.all_checked(0), "no rows is not 'all ticked'");
        v.toggle_all_checked(0);
        assert!(v.checked.is_empty());
        v.toggle_checked(2);
        v.total = Some(1000);
        assert!(v.next(50));
        assert!(v.checked.is_empty(), "row 2 of page 1 is not row 2 of page 2");
    }

    #[test]
    fn selection_label_names_the_row_and_column() {
        let r = sample();
        assert_eq!(
            selection_label(&r, CellRef { row: 1, col: 1 }),
            "Row 2 · note"
        );
    }

    #[test]
    fn selecting_opens_the_panel_and_a_new_page_drops_the_selection() {
        let mut v = TableView::default();
        v.total = Some(500);
        v.select(2, 1);
        assert_eq!(v.selection, Some(CellRef { row: 2, col: 1 }));
        assert!(v.panel_open);
        v.next(50);
        assert_eq!(v.selection, None, "row 2 of page 1 is not row 2 of page 2");
    }

    // -- structure ---------------------------------------------------------

    #[test]
    fn columns_carry_key_badges_defaults_and_their_foreign_target() {
        let rows = column_rows(&orders_schema(), &keys(), "main");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].primary && rows[0].auto && rows[0].foreign.is_empty());
        assert_eq!(rows[1].foreign, "customers(id)");
        assert_eq!(rows[1].foreign_table, "customers");
        assert!(!rows[1].primary);
        assert_eq!(rows[2].default, "''", "an empty default is not no default");
        assert!(rows[2].nullable);
        assert_eq!(rows[0].default, "(none)");
    }

    #[test]
    fn indexes_are_labelled_primary_unique_or_plain() {
        let rows = index_rows(&orders_schema());
        assert_eq!(rows[0].kind, "PRIMARY");
        assert_eq!(rows[1].kind, "INDEX");
        assert_eq!(rows[1].columns, "customer_id");
    }

    #[test]
    fn relations_cover_both_directions_and_name_the_other_table() {
        let rows = relation_rows(&keys(), "main", "orders");
        assert_eq!(rows.len(), 2, "the unrelated key is excluded");
        let out = rows.iter().find(|r| r.outgoing).unwrap();
        assert_eq!(out.other_table, "customers");
        assert_eq!(out.from, "orders(customer_id)");
        assert_eq!(out.to, "customers(id)");
        assert_eq!(out.on_delete, "CASCADE");
        assert_eq!(out.on_update, "NO ACTION");
        let inc = rows.iter().find(|r| !r.outgoing).unwrap();
        assert_eq!(inc.other_table, "order_items");
    }

    #[test]
    fn filter_columns_come_from_the_schema_else_the_page() {
        let mut v = TableView::default();
        let page = QueryResult::with_data(vec!["a".into(), "b".into()], vec![]);
        assert_eq!(v.filter_column_names(Some(&page)), vec!["a", "b"]);
        assert!(v.filter_column_names(None).is_empty());
        ready(&mut v);
        assert_eq!(
            v.filter_column_names(Some(&page)),
            vec!["id", "customer_id", "note"]
        );
    }
}
