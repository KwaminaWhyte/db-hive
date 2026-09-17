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
//!
//! The pure logic (dialect rules, filters, SQL generation, paging, labels and
//! the metadata projections) lives in `dbhive_ui_model::table_inspector` and
//! is re-exported here, so existing `screens::table_inspector::*` paths keep
//! working.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::DbError;

pub use dbhive_ui_model::table_inspector::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::results_grid;

// ---------------------------------------------------------------------------
// Screen state (holds jobs and grid state, so it stays with the shell)
// ---------------------------------------------------------------------------

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
    use dbhive_core::models::DbDriver;

    /// Standard SQL quoting, as Postgres and SQLite use.
    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
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
