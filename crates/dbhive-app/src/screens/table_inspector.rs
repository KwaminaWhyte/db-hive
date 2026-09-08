//! Table inspector: browse a table's rows, columns, indexes and relations.
//!
//! Parity target: the React `TableInspector.tsx` tab rendered inside the
//! query workspace (see 2.4 in docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! Two rules shape this file:
//!
//! * Every identifier that reaches SQL goes through the driver's
//!   `quote_identifier`, and every value through `escape_string_literal`.
//!   A table called `order details` or a column called `select` has to work,
//!   and a filter value containing a quote must not end the literal.
//! * Nothing blocks the frame. Schema, foreign keys, row count and page data
//!   are four independent `Job`s; the UI draws whatever has landed so far.
//!
//! Tabular output is rendered exclusively through `widgets::results_grid`,
//! including the Columns/Indexes/Relations tabs, which are projected into a
//! `QueryResult` rather than getting a second hand-rolled table.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::{DbDriver, DbError, ForeignKeyInfo, TableSchema};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::results_grid;

/// OFFSET paging makes the database scan and discard every preceding row, so
/// paging far into a large table degrades badly. Past this offset the pager
/// stops and offers a jump back to the deepest page that is still cheap.
/// Same value and same reasoning as the React shell.
pub const MAX_SAFE_OFFSET: usize = 100_000;

const PAGE_SIZES: [usize; 4] = [25, 50, 100, 250];

// ---------------------------------------------------------------------------
// Pure logic: dialect rules, filters, SQL generation, formatting
// ---------------------------------------------------------------------------

/// The parts of a dialect this screen has to know about when it writes SQL.
///
/// Snapshotted from the connection profile rather than asked of the driver on
/// every keystroke, so it stays cheap and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialectRules {
    /// Whether `ILIKE` exists (Postgres family). Everyone else gets `LIKE`.
    pub ilike: bool,
    /// The type name `CAST(col AS ...)` takes for text comparison.
    pub cast_type: &'static str,
}

impl Default for DialectRules {
    fn default() -> Self {
        Self {
            ilike: false,
            cast_type: "TEXT",
        }
    }
}

pub fn rules_for(driver: &DbDriver) -> DialectRules {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => DialectRules {
            ilike: true,
            cast_type: "TEXT",
        },
        // MySQL has no TEXT cast target; CHAR is the documented spelling.
        DbDriver::MySql => DialectRules {
            ilike: false,
            cast_type: "CHAR",
        },
        // T-SQL has neither ILIKE nor a TEXT cast target.
        DbDriver::SqlServer => DialectRules {
            ilike: false,
            cast_type: "NVARCHAR(MAX)",
        },
        DbDriver::Sqlite | DbDriver::Turso | DbDriver::MongoDb | DbDriver::Redis => {
            DialectRules::default()
        }
    }
}

/// Comparison used by one row of the structured filter builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    pub const ALL: [FilterOp; 12] = [
        FilterOp::Contains,
        FilterOp::NotContains,
        FilterOp::Equals,
        FilterOp::NotEquals,
        FilterOp::StartsWith,
        FilterOp::EndsWith,
        FilterOp::Gt,
        FilterOp::Gte,
        FilterOp::Lt,
        FilterOp::Lte,
        FilterOp::IsNull,
        FilterOp::IsNotNull,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Contains => "contains",
            FilterOp::NotContains => "does not contain",
            FilterOp::Equals => "equals",
            FilterOp::NotEquals => "not equals",
            FilterOp::StartsWith => "starts with",
            FilterOp::EndsWith => "ends with",
            FilterOp::Gt => ">",
            FilterOp::Gte => ">=",
            FilterOp::Lt => "<",
            FilterOp::Lte => "<=",
            FilterOp::IsNull => "is null",
            FilterOp::IsNotNull => "is not null",
        }
    }

    /// Null checks take no operand, so an empty value box is still valid.
    pub fn takes_value(self) -> bool {
        !matches!(self, FilterOp::IsNull | FilterOp::IsNotNull)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}

impl Filter {
    pub fn new(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            op: FilterOp::Contains,
            value: String::new(),
        }
    }

    /// A filter only reaches SQL once it names a column and, for operators
    /// that need one, carries a value. Half-typed rows are ignored rather
    /// than producing `col LIKE '%%'`.
    pub fn is_active(&self) -> bool {
        !self.column.is_empty() && (!self.op.takes_value() || !self.value.trim().is_empty())
    }
}

/// `schema.table`, both quoted. A driver with no schema concept (SQLite via
/// an empty schema, say) yields just the table.
pub fn qualified_name(quote: &impl Fn(&str) -> String, schema: &str, table: &str) -> String {
    if schema.is_empty() {
        quote(table)
    } else {
        format!("{}.{}", quote(schema), quote(table))
    }
}

/// `WHERE ...` for the active filters, or an empty string.
///
/// Text comparisons cast the column so that a filter works on numeric and
/// date columns too; ordering comparisons do not, or `10 > '9'` would answer
/// the wrong question.
pub fn where_clause(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    rules: DialectRules,
    filters: &[Filter],
) -> String {
    let like = |negated: bool| match (rules.ilike, negated) {
        (true, false) => "ILIKE",
        (true, true) => "NOT ILIKE",
        (false, false) => "LIKE",
        (false, true) => "NOT LIKE",
    };

    let clauses: Vec<String> = filters
        .iter()
        .filter(|f| f.is_active())
        .map(|f| {
            let ident = quote(&f.column);
            let text = format!("CAST({} AS {})", ident, rules.cast_type);
            let v = escape(f.value.trim());
            match f.op {
                FilterOp::Contains => format!("{} {} '%{}%'", text, like(false), v),
                FilterOp::NotContains => format!("{} {} '%{}%'", text, like(true), v),
                FilterOp::Equals => format!("{} = '{}'", text, v),
                FilterOp::NotEquals => format!("{} <> '{}'", text, v),
                FilterOp::StartsWith => format!("{} {} '{}%'", text, like(false), v),
                FilterOp::EndsWith => format!("{} {} '%{}'", text, like(false), v),
                FilterOp::Gt => format!("{} > '{}'", ident, v),
                FilterOp::Gte => format!("{} >= '{}'", ident, v),
                FilterOp::Lt => format!("{} < '{}'", ident, v),
                FilterOp::Lte => format!("{} <= '{}'", ident, v),
                FilterOp::IsNull => format!("{} IS NULL", ident),
                FilterOp::IsNotNull => format!("{} IS NOT NULL", ident),
            }
        })
        .collect();

    if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    }
}

/// Everything the SELECT builder needs, so callers do not juggle six
/// positional arguments.
#[derive(Debug, Clone)]
pub struct DataQuery<'a> {
    pub schema: &'a str,
    pub table: &'a str,
    pub filters: &'a [Filter],
    /// Columns to order by (the primary key when known). Without a stable
    /// order, OFFSET paging can repeat or skip rows between pages.
    pub order_by: &'a [String],
    /// 1-based. Page 0 is treated as page 1.
    pub page: usize,
    pub page_size: usize,
}

/// The offset page `page` starts at. 1-based pages; page 1 starts at 0.
pub fn offset_for_page(page: usize, page_size: usize) -> usize {
    page.saturating_sub(1).saturating_mul(page_size)
}

/// The deepest page still within `MAX_SAFE_OFFSET`.
pub fn max_safe_page(page_size: usize) -> usize {
    if page_size == 0 {
        return 1;
    }
    MAX_SAFE_OFFSET / page_size + 1
}

/// Total pages for a known row count. Always at least one, so an empty table
/// reads "Page 1 of 1" rather than "Page 1 of 0".
pub fn total_pages(total: u64, page_size: usize) -> usize {
    if page_size == 0 {
        return 1;
    }
    let per = page_size as u64;
    (((total + per - 1) / per).max(1)) as usize
}

/// `SELECT * FROM schema.table [WHERE ...] [ORDER BY ...] LIMIT n OFFSET n`.
pub fn paged_select_sql(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    rules: DialectRules,
    q: &DataQuery<'_>,
) -> String {
    let mut sql = format!(
        "SELECT * FROM {}",
        qualified_name(quote, q.schema, q.table)
    );

    let where_sql = where_clause(quote, escape, rules, q.filters);
    if !where_sql.is_empty() {
        sql.push(' ');
        sql.push_str(&where_sql);
    }

    if !q.order_by.is_empty() {
        let cols: Vec<String> = q.order_by.iter().map(|c| quote(c)).collect();
        sql.push_str(" ORDER BY ");
        sql.push_str(&cols.join(", "));
    }

    sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        q.page_size,
        offset_for_page(q.page, q.page_size)
    ));
    sql
}

/// `SELECT COUNT(*) ... [WHERE ...]`, using the same filters as the page.
pub fn count_sql(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    rules: DialectRules,
    schema: &str,
    table: &str,
    filters: &[Filter],
) -> String {
    let mut sql = format!(
        "SELECT COUNT(*) FROM {}",
        qualified_name(quote, schema, table)
    );
    let where_sql = where_clause(quote, escape, rules, filters);
    if !where_sql.is_empty() {
        sql.push(' ');
        sql.push_str(&where_sql);
    }
    sql
}

/// Read a COUNT(*) result out of whatever shape the driver returned it in.
/// Drivers differ: some hand back a JSON number, some a string.
pub fn parse_count(result: &QueryResult) -> Option<u64> {
    let cell = result.rows.first()?.first()?;
    match cell {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

// -- formatting -------------------------------------------------------------

/// Group digits so a row count is readable at a glance.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn nullable_label(nullable: bool) -> &'static str {
    if nullable {
        "YES"
    } else {
        "NO"
    }
}

/// A column's default, or an explicit placeholder. An empty cell would be
/// ambiguous with a default of the empty string.
pub fn default_label(default: Option<&String>) -> String {
    match default {
        Some(d) if !d.is_empty() => d.clone(),
        Some(_) => "''".to_string(),
        None => "(none)".to_string(),
    }
}

/// Key role of a column: primary key, and whether the database supplies it.
pub fn key_label(is_primary_key: bool, is_auto_increment: bool) -> String {
    match (is_primary_key, is_auto_increment) {
        (true, true) => "PK, auto".to_string(),
        (true, false) => "PK".to_string(),
        (false, true) => "auto".to_string(),
        (false, false) => String::new(),
    }
}

pub fn index_kind_label(is_primary: bool, is_unique: bool) -> &'static str {
    if is_primary {
        "PRIMARY"
    } else if is_unique {
        "UNIQUE"
    } else {
        "INDEX"
    }
}

/// "1-50 of 1,204" for the pager. `shown` is how many rows actually came back,
/// which is what makes the last page read correctly.
pub fn page_range_label(page: usize, page_size: usize, shown: usize, total: Option<u64>) -> String {
    if shown == 0 {
        return match total {
            Some(t) => format!("0 of {} rows", thousands(t)),
            None => "No rows".to_string(),
        };
    }
    let start = offset_for_page(page, page_size) + 1;
    let end = start + shown - 1;
    match total {
        Some(t) => format!(
            "{}-{} of {} rows",
            thousands(start as u64),
            thousands(end as u64),
            thousands(t)
        ),
        None => format!("{}-{}", thousands(start as u64), thousands(end as u64)),
    }
}

/// Primary key columns, in declaration order. Used as the paging sort key.
pub fn primary_key_columns(schema: &TableSchema) -> Vec<String> {
    schema
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.clone())
        .collect()
}

// -- metadata projected into grid results ------------------------------------

pub fn columns_result(schema: &TableSchema) -> QueryResult {
    let rows = schema
        .columns
        .iter()
        .map(|c| {
            vec![
                serde_json::Value::String(c.name.clone()),
                serde_json::Value::String(c.data_type.clone()),
                serde_json::Value::String(nullable_label(c.nullable).to_string()),
                serde_json::Value::String(default_label(c.default_value.as_ref())),
                serde_json::Value::String(key_label(c.is_primary_key, c.is_auto_increment)),
            ]
        })
        .collect();
    QueryResult::with_data(
        vec![
            "Column".into(),
            "Type".into(),
            "Nullable".into(),
            "Default".into(),
            "Key".into(),
        ],
        rows,
    )
}

pub fn indexes_result(schema: &TableSchema) -> QueryResult {
    let rows = schema
        .indexes
        .iter()
        .map(|i| {
            vec![
                serde_json::Value::String(i.name.clone()),
                serde_json::Value::String(i.columns.join(", ")),
                serde_json::Value::String(
                    index_kind_label(i.is_primary, i.is_unique).to_string(),
                ),
            ]
        })
        .collect();
    QueryResult::with_data(
        vec!["Index".into(), "Columns".into(), "Kind".into()],
        rows,
    )
}

/// Relations touching `table`: keys it declares, and keys pointing at it.
/// `get_foreign_keys` is schema-wide, so both directions are already present
/// in the input and only need labelling.
pub fn relations_result(keys: &[ForeignKeyInfo], schema: &str, table: &str) -> QueryResult {
    let same = |s: &str, t: &str| t == table && (schema.is_empty() || s == schema);

    let rows = keys
        .iter()
        .filter(|fk| {
            same(&fk.schema, &fk.table) || same(&fk.referenced_schema, &fk.referenced_table)
        })
        .map(|fk| {
            let direction = if same(&fk.schema, &fk.table) {
                "references"
            } else {
                "referenced by"
            };
            vec![
                serde_json::Value::String(fk.name.clone()),
                serde_json::Value::String(direction.to_string()),
                serde_json::Value::String(format!(
                    "{}({})",
                    qualified_plain(&fk.schema, &fk.table),
                    fk.columns.join(", ")
                )),
                serde_json::Value::String(format!(
                    "{}({})",
                    qualified_plain(&fk.referenced_schema, &fk.referenced_table),
                    fk.referenced_columns.join(", ")
                )),
                serde_json::Value::String(
                    fk.on_delete.clone().unwrap_or_else(|| "NO ACTION".into()),
                ),
                serde_json::Value::String(
                    fk.on_update.clone().unwrap_or_else(|| "NO ACTION".into()),
                ),
            ]
        })
        .collect();

    QueryResult::with_data(
        vec![
            "Constraint".into(),
            "Direction".into(),
            "From".into(),
            "To".into(),
            "On delete".into(),
            "On update".into(),
        ],
        rows,
    )
}

/// Display-only qualified name. Never used in SQL, so it is not quoted.
fn qualified_plain(schema: &str, table: &str) -> String {
    if schema.is_empty() {
        table.to_string()
    } else {
        format!("{}.{}", schema, table)
    }
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Data,
    Columns,
    Indexes,
    Relations,
}

/// Which table the inspector is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub connection_id: String,
    pub schema: String,
    pub table: String,
}

/// Schema and foreign keys arrive together so the two-request load is a
/// single job with a single failure path.
pub struct TableMeta {
    pub schema: TableSchema,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

pub struct TableInspector {
    /// Whether the inspector has a table to show. Kept so a host screen can
    /// treat it as a panel that is open or closed.
    pub open: bool,
    pub target: Option<Target>,
    pub tab: Tab,

    pub meta: Option<TableMeta>,
    pub data: Option<QueryResult>,
    pub row_count: Option<u64>,
    pub last_error: Option<String>,
    pub elapsed_ms: Option<u128>,

    pub page: usize,
    pub page_size: usize,
    pub filters: Vec<Filter>,
    pub show_filters: bool,
    /// Set when the requested page would exceed MAX_SAFE_OFFSET.
    pub deep_offset_blocked: bool,

    pub data_grid: results_grid::GridState,
    pub columns_grid: results_grid::GridState,
    pub indexes_grid: results_grid::GridState,
    pub relations_grid: results_grid::GridState,

    pub meta_job: Job<Result<TableMeta, DbError>>,
    pub data_job: Job<(Result<QueryResult, DbError>, u128)>,
    pub count_job: Job<Result<Option<u64>, DbError>>,

    needs_meta: bool,
    needs_data: bool,
    needs_count: bool,
    /// Set when the schema load failed, so the row fetch stops waiting for a
    /// sort key that is never going to arrive.
    meta_failed: bool,
}

impl Default for TableInspector {
    fn default() -> Self {
        Self {
            open: false,
            target: None,
            tab: Tab::Data,
            meta: None,
            data: None,
            row_count: None,
            last_error: None,
            elapsed_ms: None,
            page: 1,
            page_size: 50,
            filters: Vec::new(),
            show_filters: false,
            deep_offset_blocked: false,
            data_grid: results_grid::GridState::default(),
            columns_grid: results_grid::GridState::default(),
            indexes_grid: results_grid::GridState::default(),
            relations_grid: results_grid::GridState::default(),
            meta_job: Job::default(),
            data_job: Job::default(),
            count_job: Job::default(),
            needs_meta: false,
            needs_data: false,
            needs_count: false,
            meta_failed: false,
        }
    }
}

impl TableInspector {
    /// Point the inspector at a table. Re-opening the table already shown is
    /// a no-op, so clicking the same table twice does not throw away the
    /// user's page position and filters.
    pub fn open_table(
        &mut self,
        connection_id: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) {
        let target = Target {
            connection_id: connection_id.into(),
            schema: schema.into(),
            table: table.into(),
        };
        self.open = true;
        if self.target.as_ref() == Some(&target) {
            return;
        }
        let page_size = self.page_size;
        *self = Self {
            open: true,
            target: Some(target),
            page_size,
            needs_meta: true,
            ..Self::default()
        };
    }

    pub fn close(&mut self) {
        let page_size = self.page_size;
        *self = Self {
            page_size,
            ..Self::default()
        };
    }

    /// Reload schema, foreign keys, page data and row count.
    pub fn refresh(&mut self) {
        self.needs_meta = true;
        self.needs_data = true;
        self.needs_count = true;
        self.meta_failed = false;
        self.last_error = None;
    }

    fn go_to_page(&mut self, page: usize) {
        self.page = page.max(1);
        self.needs_data = true;
    }

    /// Filters changed: the current page number no longer means anything, and
    /// the row count has to be recomputed.
    fn apply_filters(&mut self) {
        self.page = 1;
        self.needs_data = true;
        self.needs_count = true;
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Clone the driver out of state so the lock is dropped before any await.
fn driver(app: &DbHiveApp, connection_id: &str) -> Option<Arc<dyn DatabaseDriver>> {
    app.state
        .lock()
        .unwrap()
        .get_connection(connection_id)
        .cloned()
}

fn dialect_rules(app: &DbHiveApp, connection_id: &str) -> DialectRules {
    let guard = app.state.lock().unwrap();
    guard
        .get_profile(connection_id)
        .map(|p| rules_for(&p.driver))
        .unwrap_or_default()
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();

    let Some(target) = app.screens.table_inspector.target.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Select a table to inspect",
            ));
        });
        return;
    };

    // Drain finished work before drawing, so this frame shows the newest data.
    {
        let ti = &mut app.screens.table_inspector;
        if let Some(result) = ti.meta_job.take_result() {
            match result {
                Ok(meta) => {
                    // First schema load is what supplies the ORDER BY key, so
                    // the page fetch waits for it rather than paging unsorted.
                    ti.row_count = ti.row_count.or(meta.schema.table.row_count);
                    ti.meta = Some(meta);
                    ti.needs_data = true;
                    ti.needs_count = true;
                }
                Err(e) => {
                    // Without a schema there is no sort key, but the rows are
                    // still worth showing; they just page unordered.
                    ti.meta_failed = true;
                    ti.needs_data = true;
                    ti.needs_count = true;
                    ti.last_error = Some(e.to_string());
                }
            }
        }
        if let Some((result, elapsed)) = ti.data_job.take_result() {
            ti.elapsed_ms = Some(elapsed);
            match result {
                Ok(r) => {
                    ti.data_grid = results_grid::GridState::default();
                    ti.data = Some(r);
                    ti.last_error = None;
                }
                Err(e) => {
                    ti.data = None;
                    ti.last_error = Some(e.to_string());
                }
            }
        }
        if let Some(result) = ti.count_job.take_result() {
            match result {
                Ok(n) => ti.row_count = n.or(ti.row_count),
                // A failed COUNT(*) is not worth an error banner over the
                // rows the user asked for; the pager just loses its total.
                Err(e) => eprintln!("table inspector: row count failed: {}", e),
            }
        }
    }

    header(app, ui, &target);
    ui.add_space(6.0);
    tab_bar(app, ui);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    if let Some(err) = app.screens.table_inspector.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                app.screens.table_inspector.last_error = None;
            }
        });
        ui.add_space(4.0);
    }

    match app.screens.table_inspector.tab {
        Tab::Data => data_tab(app, ui),
        Tab::Columns => metadata_tab(app, ui, Tab::Columns),
        Tab::Indexes => metadata_tab(app, ui, Tab::Indexes),
        Tab::Relations => metadata_tab(app, ui, Tab::Relations),
    }

    // Kick off whatever the frame's interactions asked for, after all borrows
    // of the screen state have ended.
    start_pending(app, ui.ctx(), &target);
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, target: &Target) {
    let palette = app.mode.palette();
    let ti = &mut app.screens.table_inspector;

    let title = qualified_plain(&target.schema, &target.table);
    let kind = ti
        .meta
        .as_ref()
        .map(|m| m.schema.table.table_type.clone())
        .unwrap_or_default();
    let busy = ti.meta_job.is_running() || ti.data_job.is_running();

    let mut refresh = false;
    let mut close = false;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, title));
        if !kind.is_empty() {
            ui.label(crate::theme::text::muted(palette, kind));
        }
        if let Some(n) = ti.row_count {
            ui.label(crate::theme::text::muted(
                palette,
                format!("{} rows", thousands(n)),
            ));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Refresh"))
                .on_hover_text("Reload schema, relations and the current page")
                .clicked()
            {
                refresh = true;
            }
        });
    });

    if refresh {
        ti.refresh();
    }
    if close {
        ti.close();
    }
}

fn tab_bar(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let ti = &mut app.screens.table_inspector;
    let (n_cols, n_idx) = ti
        .meta
        .as_ref()
        .map(|m| (m.schema.columns.len(), m.schema.indexes.len()))
        .unwrap_or((0, 0));
    let n_rel = ti
        .meta
        .as_ref()
        .and_then(|m| ti.target.as_ref().map(|t| (m, t)))
        .map(|(m, t)| relations_result(&m.foreign_keys, &t.schema, &t.table).rows.len())
        .unwrap_or(0);

    ui.horizontal(|ui| {
        ui.selectable_value(&mut ti.tab, Tab::Data, "Data");
        ui.selectable_value(&mut ti.tab, Tab::Columns, format!("Columns ({})", n_cols));
        ui.selectable_value(&mut ti.tab, Tab::Indexes, format!("Indexes ({})", n_idx));
        ui.selectable_value(
            &mut ti.tab,
            Tab::Relations,
            format!("Relations ({})", n_rel),
        );
    });
}

fn data_tab(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();

    filter_builder(app, ui);
    pager(app, ui);
    ui.add_space(4.0);

    let ti = &mut app.screens.table_inspector;

    if ti.deep_offset_blocked {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.warning, "Too deep to page:");
            ui.label(crate::theme::text::muted(
                palette,
                format!(
                    "skipping more than {} rows gets slow. Filter to a range instead.",
                    thousands(MAX_SAFE_OFFSET as u64)
                ),
            ));
        });
        ui.add_space(4.0);
    }

    if ti.data.is_none() && ti.data_job.is_running() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading rows"));
        });
        return;
    }

    // The grid state is moved out first so the immutable borrow of the result
    // and the mutable borrow of the state never overlap.
    let mut grid = std::mem::take(&mut ti.data_grid);
    match &ti.data {
        Some(result) if !result.columns.is_empty() => {
            results_grid::show(ui, palette, result, &mut grid);
        }
        _ => {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::muted(palette, "No rows on this page"));
            });
        }
    }
    ti.data_grid = grid;
}

fn filter_builder(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let columns: Vec<String> = app
        .screens
        .table_inspector
        .meta
        .as_ref()
        .map(|m| m.schema.columns.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();

    let ti = &mut app.screens.table_inspector;
    let active = ti.filters.iter().filter(|f| f.is_active()).count();

    let mut apply = false;
    let mut clear = false;

    ui.horizontal(|ui| {
        let label = if active == 0 {
            "Filters".to_string()
        } else {
            format!("Filters ({})", active)
        };
        if ui.selectable_label(ti.show_filters, label).clicked() {
            ti.show_filters = !ti.show_filters;
        }
        if active > 0 {
            ui.label(crate::theme::text::muted(palette, "applied"));
        }
    });

    if !ti.show_filters {
        return;
    }

    if columns.is_empty() {
        ui.label(crate::theme::text::muted(
            palette,
            "Filters need the table's columns, which are still loading.",
        ));
        return;
    }

    let mut remove: Option<usize> = None;
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            for (i, filter) in ti.filters.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("ti_filter_col", i))
                        .selected_text(filter.column.clone())
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for c in &columns {
                                ui.selectable_value(&mut filter.column, c.clone(), c);
                            }
                        });
                    egui::ComboBox::from_id_salt(("ti_filter_op", i))
                        .selected_text(filter.op.label())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for op in FilterOp::ALL {
                                ui.selectable_value(&mut filter.op, op, op.label());
                            }
                        });
                    ui.add_enabled(
                        filter.op.takes_value(),
                        egui::TextEdit::singleline(&mut filter.value)
                            .hint_text("value")
                            .desired_width(160.0),
                    );
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Add filter").clicked() {
                    ti.filters.push(Filter::new(columns[0].clone()));
                }
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if !ti.filters.is_empty() && ui.button("Clear").clicked() {
                    clear = true;
                }
            });
        });

    if let Some(i) = remove {
        ti.filters.remove(i);
        apply = true;
    }
    if clear {
        ti.filters.clear();
        apply = true;
    }
    if apply {
        ti.apply_filters();
    }
    ui.add_space(4.0);
}

fn pager(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let ti = &mut app.screens.table_inspector;

    let shown = ti.data.as_ref().map(|d| d.rows.len()).unwrap_or(0);
    let total = ti.row_count;
    let pages = total.map(|t| total_pages(t, ti.page_size));
    let page = ti.page;
    let page_size = ti.page_size;
    let busy = ti.data_job.is_running();

    // Without a total, "is there a next page" is inferred from a full page.
    let has_next = match pages {
        Some(p) => page < p,
        None => shown == page_size,
    };
    let next_offset_ok = offset_for_page(page + 1, page_size) <= MAX_SAFE_OFFSET;

    let mut go: Option<usize> = None;
    let mut new_size: Option<usize> = None;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(page > 1 && !busy, egui::Button::new("First"))
            .clicked()
        {
            go = Some(1);
        }
        if ui
            .add_enabled(page > 1 && !busy, egui::Button::new("Prev"))
            .clicked()
        {
            go = Some(page - 1);
        }
        ui.label(crate::theme::text::strong(
            palette,
            match pages {
                Some(p) => format!("Page {} of {}", thousands(page as u64), thousands(p as u64)),
                None => format!("Page {}", thousands(page as u64)),
            },
        ));
        if ui
            .add_enabled(has_next && next_offset_ok && !busy, egui::Button::new("Next"))
            .clicked()
        {
            go = Some(page + 1);
        }
        if has_next && !next_offset_ok {
            if ui
                .button("Jump to last safe page")
                .on_hover_text("Deeper offsets make the database scan and discard too many rows")
                .clicked()
            {
                go = Some(max_safe_page(page_size));
            }
        }

        ui.separator();
        ui.label(crate::theme::text::muted(
            palette,
            page_range_label(page, page_size, shown, total),
        ));
        if let Some(ms) = ti.elapsed_ms {
            ui.label(crate::theme::text::muted(palette, format!("{} ms", ms)));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("ti_page_size")
                .selected_text(format!("{} / page", page_size))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for size in PAGE_SIZES {
                        if ui
                            .selectable_label(size == page_size, format!("{} / page", size))
                            .clicked()
                            && size != page_size
                        {
                            new_size = Some(size);
                        }
                    }
                });
        });
    });

    if let Some(size) = new_size {
        ti.page_size = size;
        // The old page number points at a different slice under a new size;
        // going back to page 1 is the only interpretation that cannot skip rows.
        ti.go_to_page(1);
    } else if let Some(p) = go {
        ti.go_to_page(p);
    }
}

fn metadata_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, tab: Tab) {
    let palette = app.mode.palette();
    let ti = &mut app.screens.table_inspector;

    // Built into an owned QueryResult so the borrow of the metadata ends
    // before the grid state is borrowed mutably.
    let result: Option<QueryResult> = ti.meta.as_ref().map(|meta| match tab {
        Tab::Columns => columns_result(&meta.schema),
        Tab::Indexes => indexes_result(&meta.schema),
        _ => {
            let (schema, table) = ti
                .target
                .as_ref()
                .map(|t| (t.schema.as_str(), t.table.as_str()))
                .unwrap_or(("", ""));
            relations_result(&meta.foreign_keys, schema, table)
        }
    });

    let Some(result) = result else {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            if ti.meta_job.is_running() {
                ui.add(egui::Spinner::new());
                ui.label(crate::theme::text::muted(palette, "Loading schema"));
            } else {
                ui.label(crate::theme::text::muted(palette, "No schema loaded"));
            }
        });
        return;
    };

    let grid = match tab {
        Tab::Columns => &mut ti.columns_grid,
        Tab::Indexes => &mut ti.indexes_grid,
        _ => &mut ti.relations_grid,
    };

    if result.rows.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            let msg = match tab {
                Tab::Columns => "This table has no columns",
                Tab::Indexes => "No indexes on this table",
                _ => "No foreign key relations touch this table",
            };
            ui.label(crate::theme::text::muted(palette, msg));
        });
        return;
    }

    results_grid::show(ui, palette, &result, grid);
}

// ---------------------------------------------------------------------------
// Async work
// ---------------------------------------------------------------------------

fn start_pending(app: &mut DbHiveApp, ctx: &egui::Context, target: &Target) {
    let (needs_meta, needs_data, needs_count) = {
        let ti = &app.screens.table_inspector;
        (ti.needs_meta, ti.needs_data, ti.needs_count)
    };
    if !(needs_meta || needs_data || needs_count) {
        return;
    }

    let Some(d) = driver(app, &target.connection_id) else {
        let ti = &mut app.screens.table_inspector;
        ti.needs_meta = false;
        ti.needs_data = false;
        ti.needs_count = false;
        ti.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let rules = dialect_rules(app, &target.connection_id);
    let handle = app.rt.handle().clone();

    if needs_meta {
        let ti = &mut app.screens.table_inspector;
        ti.needs_meta = false;
        let d = d.clone();
        let schema = target.schema.clone();
        let table = target.table.clone();
        ti.meta_job.start(&handle, ctx, async move {
            let table_schema = d.get_table_schema(&schema, &table).await?;
            // Relations are a nice-to-have; a driver that cannot report them
            // must not cost the user the columns and indexes.
            let foreign_keys = d.get_foreign_keys(&schema).await.unwrap_or_default();
            Ok(TableMeta {
                schema: table_schema,
                foreign_keys,
            })
        });
    }

    // Paging needs a stable sort key, which comes from the schema. Until it
    // lands, the data fetch waits rather than paging in arbitrary order.
    let order_by: Vec<String> = match app.screens.table_inspector.meta.as_ref() {
        Some(m) => primary_key_columns(&m.schema),
        None if app.screens.table_inspector.meta_failed => Vec::new(),
        None => return,
    };

    let quote = {
        let d = d.clone();
        move |s: &str| d.quote_identifier(s)
    };
    let escape = {
        let d = d.clone();
        move |s: &str| d.escape_string_literal(s)
    };

    if needs_data {
        let ti = &mut app.screens.table_inspector;
        ti.needs_data = false;
        let offset = offset_for_page(ti.page, ti.page_size);
        if offset > MAX_SAFE_OFFSET {
            ti.deep_offset_blocked = true;
            ti.page = max_safe_page(ti.page_size);
        } else {
            ti.deep_offset_blocked = false;
        }
        let sql = paged_select_sql(
            &quote,
            &escape,
            rules,
            &DataQuery {
                schema: &target.schema,
                table: &target.table,
                filters: &ti.filters,
                order_by: &order_by,
                page: ti.page,
                page_size: ti.page_size,
            },
        );
        let d = d.clone();
        ti.data_job.start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = d.execute_query(&sql).await;
            (out, started.elapsed().as_millis())
        });
    }

    if needs_count {
        let ti = &mut app.screens.table_inspector;
        ti.needs_count = false;
        let sql = count_sql(
            &quote,
            &escape,
            rules,
            &target.schema,
            &target.table,
            &ti.filters,
        );
        let d = d.clone();
        ti.count_job.start(&handle, ctx, async move {
            let r = d.execute_query(&sql).await?;
            Ok(parse_count(&r))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{ColumnInfo, IndexInfo, TableInfo};

    /// Standard SQL quoting, as Postgres and SQLite use.
    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
    /// MySQL backtick quoting, to prove nothing assumes double quotes.
    fn q_mysql(ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
    fn esc(v: &str) -> String {
        v.replace('\'', "''")
    }

    fn pg() -> DialectRules {
        rules_for(&DbDriver::Postgres)
    }

    fn query<'a>(page: usize, page_size: usize) -> DataQuery<'a> {
        DataQuery {
            schema: "public",
            table: "users",
            filters: &[],
            order_by: &[],
            page,
            page_size,
        }
    }

    // -- SQL generation ----------------------------------------------------

    #[test]
    fn paged_select_quotes_schema_and_table() {
        let sql = paged_select_sql(&q, &esc, pg(), &query(1, 50));
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"users\" LIMIT 50 OFFSET 0"
        );
    }

    #[test]
    fn identifiers_with_spaces_and_reserved_words_survive() {
        let sql = paged_select_sql(
            &q,
            &esc,
            pg(),
            &DataQuery {
                schema: "my schema",
                table: "order details",
                filters: &[],
                order_by: &["select".to_string()],
                page: 1,
                page_size: 10,
            },
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"my schema\".\"order details\" ORDER BY \"select\" LIMIT 10 OFFSET 0"
        );
    }

    #[test]
    fn an_identifier_containing_the_quote_char_is_escaped_not_terminated() {
        // A table literally named `we"ird` must not be able to close the
        // quoted identifier and inject trailing SQL.
        let sql = paged_select_sql(
            &q,
            &esc,
            pg(),
            &DataQuery {
                schema: "",
                table: "we\"ird",
                filters: &[],
                order_by: &[],
                page: 1,
                page_size: 5,
            },
        );
        assert_eq!(sql, "SELECT * FROM \"we\"\"ird\" LIMIT 5 OFFSET 0");
    }

    #[test]
    fn empty_schema_yields_a_bare_table_name() {
        assert_eq!(qualified_name(&q, "", "users"), "\"users\"");
        assert_eq!(qualified_name(&q, "main", "users"), "\"main\".\"users\"");
    }

    #[test]
    fn quoting_follows_the_driver_not_a_hardcoded_char() {
        let sql = paged_select_sql(&q_mysql, &esc, rules_for(&DbDriver::MySql), &query(1, 25));
        assert_eq!(sql, "SELECT * FROM `public`.`users` LIMIT 25 OFFSET 0");
    }

    #[test]
    fn offset_arithmetic_walks_pages_without_gaps_or_overlap() {
        assert_eq!(offset_for_page(1, 50), 0);
        assert_eq!(offset_for_page(2, 50), 50);
        assert_eq!(offset_for_page(3, 50), 100);
        // Page 0 is not a page; it must not produce a negative-style wrap.
        assert_eq!(offset_for_page(0, 50), 0);
    }

    #[test]
    fn page_two_offsets_by_exactly_one_page() {
        let sql = paged_select_sql(&q, &esc, pg(), &query(2, 35));
        assert!(sql.ends_with("LIMIT 35 OFFSET 35"), "{}", sql);
        let sql = paged_select_sql(&q, &esc, pg(), &query(4, 35));
        assert!(sql.ends_with("LIMIT 35 OFFSET 105"), "{}", sql);
    }

    #[test]
    fn order_by_lists_every_primary_key_column_in_order() {
        let sql = paged_select_sql(
            &q,
            &esc,
            pg(),
            &DataQuery {
                schema: "public",
                table: "line_items",
                filters: &[],
                order_by: &["order_id".to_string(), "line_no".to_string()],
                page: 1,
                page_size: 10,
            },
        );
        assert!(
            sql.contains("ORDER BY \"order_id\", \"line_no\" LIMIT"),
            "{}",
            sql
        );
    }

    #[test]
    fn clauses_appear_in_where_order_by_limit_order() {
        let filters = vec![Filter {
            column: "name".into(),
            op: FilterOp::Equals,
            value: "ada".into(),
        }];
        let sql = paged_select_sql(
            &q,
            &esc,
            pg(),
            &DataQuery {
                schema: "public",
                table: "users",
                filters: &filters,
                order_by: &["id".to_string()],
                page: 2,
                page_size: 20,
            },
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"users\" WHERE CAST(\"name\" AS TEXT) = 'ada' \
             ORDER BY \"id\" LIMIT 20 OFFSET 20"
        );
    }

    // -- page boundaries ---------------------------------------------------

    #[test]
    fn total_pages_rounds_up_and_never_reports_zero() {
        assert_eq!(total_pages(0, 50), 1);
        assert_eq!(total_pages(1, 50), 1);
        assert_eq!(total_pages(50, 50), 1);
        assert_eq!(total_pages(51, 50), 2);
        assert_eq!(total_pages(100, 50), 2);
        assert_eq!(total_pages(101, 50), 3);
    }

    #[test]
    fn max_safe_page_stays_within_the_offset_guard() {
        let size = 50;
        let p = max_safe_page(size);
        assert_eq!(p, 2001);
        assert!(offset_for_page(p, size) <= MAX_SAFE_OFFSET);
        assert!(offset_for_page(p + 1, size) > MAX_SAFE_OFFSET);
    }

    #[test]
    fn page_size_of_zero_cannot_divide_by_zero() {
        assert_eq!(max_safe_page(0), 1);
        assert_eq!(total_pages(10, 0), 1);
    }

    #[test]
    fn page_range_label_describes_the_current_slice() {
        assert_eq!(page_range_label(1, 50, 50, Some(1204)), "1-50 of 1,204 rows");
        assert_eq!(page_range_label(2, 50, 50, Some(1204)), "51-100 of 1,204 rows");
        // Last page: fewer rows than the page size.
        assert_eq!(page_range_label(25, 50, 4, Some(1204)), "1,201-1,204 of 1,204 rows");
        assert_eq!(page_range_label(1, 50, 0, Some(0)), "0 of 0 rows");
        assert_eq!(page_range_label(1, 50, 10, None), "1-10");
    }

    // -- filters -----------------------------------------------------------

    #[test]
    fn incomplete_filters_are_ignored() {
        let filters = vec![
            Filter::new("name"),                       // contains, no value
            Filter {
                column: String::new(),
                op: FilterOp::Equals,
                value: "x".into(),
            },
        ];
        assert_eq!(where_clause(&q, &esc, pg(), &filters), "");
    }

    #[test]
    fn null_checks_need_no_value() {
        let filters = vec![Filter {
            column: "deleted_at".into(),
            op: FilterOp::IsNull,
            value: String::new(),
        }];
        assert_eq!(
            where_clause(&q, &esc, pg(), &filters),
            "WHERE \"deleted_at\" IS NULL"
        );
    }

    #[test]
    fn filter_values_cannot_break_out_of_the_literal() {
        let filters = vec![Filter {
            column: "name".into(),
            op: FilterOp::Equals,
            value: "o'brien".into(),
        }];
        assert_eq!(
            where_clause(&q, &esc, pg(), &filters),
            "WHERE CAST(\"name\" AS TEXT) = 'o''brien'"
        );
    }

    #[test]
    fn postgres_gets_ilike_and_mysql_gets_like_with_a_char_cast() {
        let filters = vec![Filter {
            column: "name".into(),
            op: FilterOp::Contains,
            value: "ada".into(),
        }];
        assert_eq!(
            where_clause(&q, &esc, pg(), &filters),
            "WHERE CAST(\"name\" AS TEXT) ILIKE '%ada%'"
        );
        assert_eq!(
            where_clause(&q_mysql, &esc, rules_for(&DbDriver::MySql), &filters),
            "WHERE CAST(`name` AS CHAR) LIKE '%ada%'"
        );
    }

    #[test]
    fn ordering_comparisons_do_not_cast_the_column() {
        // Casting here would make 10 > 9 compare as text and answer wrongly.
        let filters = vec![Filter {
            column: "age".into(),
            op: FilterOp::Gt,
            value: "18".into(),
        }];
        assert_eq!(
            where_clause(&q, &esc, pg(), &filters),
            "WHERE \"age\" > '18'"
        );
    }

    #[test]
    fn multiple_filters_are_anded() {
        let filters = vec![
            Filter {
                column: "a".into(),
                op: FilterOp::Equals,
                value: "1".into(),
            },
            Filter {
                column: "b".into(),
                op: FilterOp::IsNotNull,
                value: String::new(),
            },
        ];
        assert_eq!(
            where_clause(&q, &esc, pg(), &filters),
            "WHERE CAST(\"a\" AS TEXT) = '1' AND \"b\" IS NOT NULL"
        );
    }

    #[test]
    fn count_sql_reuses_the_page_filters() {
        let filters = vec![Filter {
            column: "active".into(),
            op: FilterOp::Equals,
            value: "true".into(),
        }];
        assert_eq!(
            count_sql(&q, &esc, pg(), "public", "users", &filters),
            "SELECT COUNT(*) FROM \"public\".\"users\" WHERE CAST(\"active\" AS TEXT) = 'true'"
        );
        assert_eq!(
            count_sql(&q, &esc, pg(), "public", "users", &[]),
            "SELECT COUNT(*) FROM \"public\".\"users\""
        );
    }

    #[test]
    fn count_is_read_from_numbers_and_strings_alike() {
        let numeric = QueryResult::with_data(vec!["count".into()], vec![vec![serde_json::json!(42)]]);
        assert_eq!(parse_count(&numeric), Some(42));
        // MySQL and SQL Server drivers hand back BIGINT counts as strings.
        let text =
            QueryResult::with_data(vec!["count".into()], vec![vec![serde_json::json!("42")]]);
        assert_eq!(parse_count(&text), Some(42));
        assert_eq!(parse_count(&QueryResult::empty()), None);
    }

    // -- formatting --------------------------------------------------------

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn nullability_reads_the_same_way_as_the_information_schema() {
        assert_eq!(nullable_label(true), "YES");
        assert_eq!(nullable_label(false), "NO");
    }

    #[test]
    fn a_default_of_empty_string_is_distinguishable_from_no_default() {
        assert_eq!(default_label(None), "(none)");
        assert_eq!(default_label(Some(&String::new())), "''");
        assert_eq!(default_label(Some(&"now()".to_string())), "now()");
    }

    #[test]
    fn key_label_covers_pk_and_auto_increment() {
        assert_eq!(key_label(true, true), "PK, auto");
        assert_eq!(key_label(true, false), "PK");
        assert_eq!(key_label(false, true), "auto");
        assert_eq!(key_label(false, false), "");
    }

    #[test]
    fn index_kind_prefers_primary_over_unique() {
        assert_eq!(index_kind_label(true, true), "PRIMARY");
        assert_eq!(index_kind_label(false, true), "UNIQUE");
        assert_eq!(index_kind_label(false, false), "INDEX");
    }

    // -- metadata projections ---------------------------------------------

    fn sample_schema() -> TableSchema {
        TableSchema::new(
            TableInfo::new("users".into(), "public".into(), "TABLE".into()),
            vec![
                ColumnInfo {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    default_value: Some("nextval('users_id_seq')".into()),
                    is_primary_key: true,
                    is_auto_increment: true,
                },
                ColumnInfo::new("email".into(), "text".into(), true),
            ],
            vec![IndexInfo::new(
                "users_pkey".into(),
                vec!["id".into()],
                true,
                true,
            )],
        )
    }

    #[test]
    fn primary_key_columns_keep_declaration_order() {
        let mut s = sample_schema();
        s.columns[1].is_primary_key = true;
        assert_eq!(primary_key_columns(&s), vec!["id", "email"]);
    }

    #[test]
    fn columns_project_into_a_grid_result() {
        let r = columns_result(&sample_schema());
        assert_eq!(r.columns, ["Column", "Type", "Nullable", "Default", "Key"]);
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.rows[0][0], serde_json::json!("id"));
        assert_eq!(r.rows[0][2], serde_json::json!("NO"));
        assert_eq!(r.rows[0][4], serde_json::json!("PK, auto"));
        assert_eq!(r.rows[1][2], serde_json::json!("YES"));
        assert_eq!(r.rows[1][3], serde_json::json!("(none)"));
    }

    #[test]
    fn indexes_project_into_a_grid_result() {
        let r = indexes_result(&sample_schema());
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][1], serde_json::json!("id"));
        assert_eq!(r.rows[0][2], serde_json::json!("PRIMARY"));
    }

    #[test]
    fn relations_include_both_directions_and_exclude_unrelated_keys() {
        let keys = vec![
            // users is referenced by orders.
            ForeignKeyInfo::new(
                "orders_user_fk".into(),
                "orders".into(),
                "public".into(),
                vec!["user_id".into()],
                "users".into(),
                "public".into(),
                vec!["id".into()],
            ),
            // users references countries.
            ForeignKeyInfo::new(
                "users_country_fk".into(),
                "users".into(),
                "public".into(),
                vec!["country_id".into()],
                "countries".into(),
                "public".into(),
                vec!["id".into()],
            ),
            // Nothing to do with users.
            ForeignKeyInfo::new(
                "items_order_fk".into(),
                "items".into(),
                "public".into(),
                vec!["order_id".into()],
                "orders".into(),
                "public".into(),
                vec!["id".into()],
            ),
        ];
        let r = relations_result(&keys, "public", "users");
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.rows[0][1], serde_json::json!("referenced by"));
        assert_eq!(r.rows[0][2], serde_json::json!("public.orders(user_id)"));
        assert_eq!(r.rows[0][3], serde_json::json!("public.users(id)"));
        assert_eq!(r.rows[1][1], serde_json::json!("references"));
        // No declared action shows the SQL default rather than an empty cell.
        assert_eq!(r.rows[1][4], serde_json::json!("NO ACTION"));
    }

    // -- state -------------------------------------------------------------

    #[test]
    fn opening_the_same_table_twice_keeps_the_users_place() {
        let mut ti = TableInspector::default();
        ti.open_table("c1", "public", "users");
        ti.needs_meta = false;
        ti.page = 4;
        ti.filters.push(Filter::new("id"));

        ti.open_table("c1", "public", "users");
        assert_eq!(ti.page, 4);
        assert_eq!(ti.filters.len(), 1);
        assert!(!ti.needs_meta);
    }

    #[test]
    fn opening_a_different_table_resets_paging_and_reloads() {
        let mut ti = TableInspector::default();
        ti.open_table("c1", "public", "users");
        ti.page = 4;
        ti.page_size = 100;
        ti.filters.push(Filter::new("id"));
        ti.data = Some(QueryResult::empty());

        ti.open_table("c1", "public", "orders");
        assert_eq!(ti.page, 1);
        assert!(ti.filters.is_empty());
        assert!(ti.data.is_none());
        assert!(ti.needs_meta);
        // Page size is a user preference, not per-table state.
        assert_eq!(ti.page_size, 100);
    }

    #[test]
    fn changing_filters_returns_to_the_first_page_and_recounts() {
        let mut ti = TableInspector::default();
        ti.open_table("c1", "public", "users");
        ti.needs_data = false;
        ti.needs_count = false;
        ti.page = 7;

        ti.apply_filters();
        assert_eq!(ti.page, 1);
        assert!(ti.needs_data);
        assert!(ti.needs_count);
    }

    #[test]
    fn a_failed_schema_load_still_lets_rows_load_unordered() {
        let mut ti = TableInspector::default();
        ti.open_table("c1", "public", "users");
        ti.meta_failed = true;
        // No schema means no primary key, so no ORDER BY is generated.
        assert!(ti.meta.is_none());
        let sql = paged_select_sql(&q, &esc, pg(), &query(1, 50));
        assert!(!sql.contains("ORDER BY"), "{}", sql);
    }

    #[test]
    fn closing_clears_the_open_table() {
        let mut ti = TableInspector::default();
        ti.open_table("c1", "public", "users");
        assert!(ti.open);
        ti.close();
        assert!(!ti.open);
        assert!(ti.target.is_none());
    }
}
