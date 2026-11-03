//! Visual query builder: compose a SELECT without writing SQL.
//!
//! Parity target: the React `QueryBuilder.tsx` route plus the seven panels
//! under `src/components/queryBuilder/` and the pure generator in
//! `src/lib/sqlGenerator.ts` (see 2.8 in
//! docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! Three rules shape this file:
//!
//! * **Everything that reaches SQL is quoted or escaped by the driver.**
//!   Identifiers go through `quote_identifier`, values through
//!   `escape_string_literal`. A builder that concatenates the user's text
//!   into a statement is an injection hole, and a table called
//!   `order details` or a column called `select` has to work. The only text
//!   that ever lands unquoted is a number this file has itself proved to be
//!   numeric.
//! * **Partial state produces no SQL, never broken SQL.** A half-typed
//!   condition, a join missing its columns, a table nothing joins to: each is
//!   dropped from generation and reported as a warning, so the preview is
//!   always something the database would accept.
//! * **Nothing blocks the frame.** The table list, per-table schema and the
//!   query run are separate `Job`s; the driver `Arc` is cloned out of state
//!   and the lock dropped before any await.
//!
//! The generator is split from the drawing on purpose: `generate_sql` and its
//! helpers are pure functions over `BuilderState`. They live, with their
//! tests, in `dbhive_ui_model::query_builder`; this file is drawing and glue.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::{DbDriver, DbError, ForeignKeyInfo, TableInfo, TableSchema};

// Re-exported so `screens::query_builder::BuilderAction` and friends keep
// their paths.
pub use dbhive_ui_model::query_builder::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

/// Table and column names for the preview editor's completion.
fn schema_index(state: &BuilderState) -> sql_editor::SchemaIndex {
    let mut index = sql_editor::SchemaIndex::default();
    for t in &state.tables {
        index.tables.insert(t.name.clone());
        index.tables.insert(t.alias.clone());
        for (name, _) in &t.columns {
            index.columns.insert(name.clone());
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Screen state
/// The table list and the schema's foreign keys, loaded together so the join
/// suggestions arrive with the tables they describe.
type Catalog = (Vec<TableInfo>, Vec<ForeignKeyInfo>);

pub struct QueryBuilder {
    pub open: bool,
    pub connection_id: Option<String>,
    pub schema: String,

    pub state: BuilderState,
    pub sections: Sections,

    /// Tables offered in the picker, and the schema's foreign keys.
    pub catalog: Vec<TableInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub table_search: String,

    /// The generated statement, kept so hand edits survive until the builder
    /// state next changes.
    pub preview: String,
    generated: Option<String>,
    pub editor: sql_editor::EditorState,

    pub result: Option<QueryResult>,
    pub grid: results_grid::GridState,
    pub elapsed_ms: Option<u128>,
    pub last_error: Option<String>,

    pub catalog_job: Job<Result<Catalog, DbError>>,
    pub add_job: Job<Result<TableSchema, DbError>>,
    pub run_job: Job<(Result<QueryResult, DbError>, u128)>,
    /// Alias reserved for the table whose schema is loading.
    pending_add: Option<(String, String)>,
    needs_catalog: bool,
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self {
            open: false,
            connection_id: None,
            schema: String::new(),
            state: BuilderState::default(),
            sections: Sections::default(),
            catalog: Vec::new(),
            foreign_keys: Vec::new(),
            table_search: String::new(),
            preview: String::new(),
            generated: None,
            editor: sql_editor::EditorState::default(),
            result: None,
            grid: results_grid::GridState::default(),
            elapsed_ms: None,
            last_error: None,
            catalog_job: Job::default(),
            add_job: Job::default(),
            run_job: Job::default(),
            pending_add: None,
            needs_catalog: false,
        }
    }
}

impl QueryBuilder {
    /// Point the builder at a connection and schema.
    ///
    /// Reopening the same target keeps the query the user was building;
    /// switching target starts clean, since aliases and columns from another
    /// schema would not resolve.
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            self.open = true;
            return;
        }
        *self = Self {
            open: true,
            connection_id: Some(c),
            schema: s,
            needs_catalog: true,
            ..Self::default()
        };
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Throw the query away, keeping the loaded catalog.
    pub fn reset(&mut self) {
        self.state = BuilderState::default();
        self.result = None;
        self.grid = results_grid::GridState::default();
        self.elapsed_ms = None;
        self.last_error = None;
        self.preview.clear();
        self.generated = None;
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

fn profile_driver(app: &DbHiveApp, connection_id: &str) -> Option<DbDriver> {
    let guard = app.state.lock().unwrap();
    guard.get_profile(connection_id).map(|p| p.driver.clone())
}

/// Intent recorded while drawing, applied once the borrows have ended.
enum Intent {
    AddTable(String),
    RemoveTable(String),
    AddJoin(Join),
    Run,
    UseSql,
    Reset,
    Close,
    Reload,
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) -> BuilderAction {
    let palette = app.mode.palette();
    let mut action = BuilderAction::None;

    let Some(connection_id) = app.screens.query_builder.connection_id.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Connect to a database to build a query",
            ));
        });
        return action;
    };

    poll(app);

    let db_driver = profile_driver(app, &connection_id);
    if let Some(d) = &db_driver {
        if !supports_builder(d) {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::strong(
                    palette,
                    "This connection does not speak SQL",
                ));
                ui.label(crate::theme::text::muted(
                    palette,
                    "The visual builder composes SELECT statements, which MongoDB and Redis do not accept.",
                ));
                if ui.button("Close").clicked() {
                    app.screens.query_builder.close();
                }
            });
            return action;
        }
    }
    let rules = db_driver.as_ref().map(rules_for).unwrap_or_default();

    let Some(d) = driver(app, &connection_id) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(palette.destructive, "Connection is no longer open");
            if ui.button("Close").clicked() {
                app.screens.query_builder.close();
            }
        });
        return action;
    };
    let quote = {
        let d = d.clone();
        move |s: &str| d.quote_identifier(s)
    };
    let escape = {
        let d = d.clone();
        move |s: &str| d.escape_string_literal(s)
    };

    // Regenerate before drawing so the preview reflects this frame's state.
    // Hand edits are kept until the builder state itself changes, so tweaking
    // the statement and running it is possible without losing the tweak.
    {
        let qb = &mut app.screens.query_builder;
        let generated = generate_sql(&quote, &escape, rules, &qb.state);
        if generated != qb.generated {
            qb.preview = generated.clone().unwrap_or_default();
            qb.generated = generated;
        }
    }

    let warnings = validate(&app.screens.query_builder.state, rules);
    let mut intents: Vec<Intent> = Vec::new();

    header(app, ui, &mut intents, &warnings);
    ui.add_space(4.0);
    ui.separator();

    egui::SidePanel::left("query_builder_sections")
        .resizable(true)
        .default_width(430.0)
        .min_width(320.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("qb_sections_scroll")
                .show(ui, |ui| {
                    tables_section(app, ui, &mut intents);
                    joins_section(app, ui, &mut intents);
                    columns_section(app, ui);
                    filters_section(app, ui);
                    group_by_section(app, ui);
                    order_by_section(app, ui);
                    limit_section(app, ui, rules);
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        preview_and_results(app, ui, &mut intents, &warnings);
    });

    for intent in intents {
        match intent {
            Intent::AddTable(name) => start_add_table(app, ui.ctx(), &connection_id, name),
            Intent::RemoveTable(alias) => app.screens.query_builder.state.remove_table(&alias),
            Intent::AddJoin(join) => app.screens.query_builder.state.joins.push(join),
            Intent::Run => run(app, ui.ctx(), &connection_id),
            Intent::UseSql => {
                let sql = app.screens.query_builder.preview.clone();
                if sql.trim().is_empty() {
                    app.toasts.info("Nothing to send yet");
                } else {
                    action = BuilderAction::UseSql(sql);
                }
            }
            Intent::Reset => app.screens.query_builder.reset(),
            Intent::Close => app.screens.query_builder.close(),
            Intent::Reload => {
                app.screens.query_builder.needs_catalog = true;
            }
        }
    }

    start_pending(app, ui.ctx(), &connection_id);
    action
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>, warnings: &[String]) {
    let palette = app.mode.palette();
    let qb = &app.screens.query_builder;
    let schema = qb.schema.clone();
    let busy = qb.catalog_job.is_running() || qb.add_job.is_running();
    let running = qb.run_job.is_running();
    let has_sql = !qb.preview.trim().is_empty();

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Query Builder"));
        if !schema.is_empty() {
            ui.label(crate::theme::text::muted(palette, schema));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        if !warnings.is_empty() {
            ui.label(
                egui::RichText::new(format!("{} note(s)", warnings.len()))
                    .small()
                    .color(palette.warning),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                intents.push(Intent::Close);
            }
            if ui
                .add_enabled(has_sql, egui::Button::new("Open in SQL editor"))
                .on_hover_text("Send this statement to a query tab")
                .clicked()
            {
                intents.push(Intent::UseSql);
            }
            if ui
                .add_enabled(has_sql && !running, egui::Button::new("Run"))
                .on_hover_text("Execute the generated statement here")
                .clicked()
            {
                intents.push(Intent::Run);
            }
            if running {
                ui.add(egui::Spinner::new().size(14.0));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Reset"))
                .on_hover_text("Clear every clause")
                .clicked()
            {
                intents.push(Intent::Reset);
            }
        });
    });
}

/// A collapsible section header with a count badge, matching the web layout.
fn section<R>(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    id: &str,
    title: &str,
    count: usize,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.horizontal(|ui| {
        let arrow = if *open { "▾" } else { "▸" };
        let label = if count == 0 {
            format!("{}  {}", arrow, title)
        } else {
            format!("{}  {}  ({})", arrow, title, count)
        };
        if ui
            .add(
                egui::Label::new(crate::theme::text::strong(palette, label))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            *open = !*open;
        }
    });
    if *open {
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.push_id(id, |ui| {
                    ui.set_width(ui.available_width());
                    body(ui);
                });
            });
    }
    ui.add_space(8.0);
}

fn tables_section(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    let mut open = qb.sections.tables;
    let count = qb.state.tables.len();
    let loading = qb.catalog_job.is_running();
    let catalog: Vec<String> = qb.catalog.iter().map(|t| t.name.clone()).collect();
    let chosen: Vec<(String, String, usize)> = qb
        .state
        .tables
        .iter()
        .map(|t| (t.alias.clone(), t.name.clone(), t.columns.len()))
        .collect();
    let adding = qb.add_job.is_running();

    section(ui, palette, "qb_tables", "Tables", count, &mut open, |ui| {
        for (i, (alias, name, cols)) in chosen.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(crate::theme::text::strong(palette, name.clone()));
                if alias != name {
                    ui.label(crate::theme::text::muted(palette, format!("AS {}", alias)));
                }
                if i == 0 {
                    ui.label(crate::theme::text::muted(palette, "FROM"));
                }
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} column(s)", cols),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Also removes its columns, joins, grouping and filters")
                        .clicked()
                    {
                        intents.push(Intent::RemoveTable(alias.clone()));
                    }
                });
            });
        }
        if !chosen.is_empty() {
            ui.add_space(6.0);
            ui.separator();
        }

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut qb.table_search)
                    .hint_text("Search tables...")
                    .desired_width(180.0),
            );
            if ui.small_button("Reload").clicked() {
                intents.push(Intent::Reload);
            }
            if loading {
                ui.add(egui::Spinner::new().size(12.0));
            }
        });

        if catalog.is_empty() {
            ui.label(crate::theme::text::muted(
                palette,
                if loading {
                    "Loading tables"
                } else {
                    "No tables in this schema"
                },
            ));
            return;
        }

        let needle = qb.table_search.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("qb_table_list")
            .max_height(180.0)
            .show(ui, |ui| {
                for name in catalog
                    .iter()
                    .filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle))
                {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!adding, egui::Button::new("+").small())
                            .on_hover_text("Add to the query")
                            .clicked()
                        {
                            intents.push(Intent::AddTable(name.clone()));
                        }
                        ui.label(name);
                    });
                }
            });
        ui.label(crate::theme::text::muted(
            palette,
            "Adding the same table twice gives it a second alias, for self-joins.",
        ));
    });

    app.screens.query_builder.sections.tables = open;
}

fn joins_section(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.len() < 2 {
        return;
    }
    let mut open = qb.sections.joins;
    let count = qb.state.joins.len();

    // Snapshots so the pickers can be drawn while the joins are borrowed.
    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();
    let suggestions = join_suggestions(&qb.state, &qb.foreign_keys);

    let mut remove: Option<usize> = None;
    section(ui, palette, "qb_joins", "Joins", count, &mut open, |ui| {
        for (i, join) in qb.state.joins.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("qb_join_type", i))
                    .selected_text(join.join_type.sql())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for t in JoinType::ALL {
                            ui.selectable_value(&mut join.join_type, t, t.sql());
                        }
                    });
                egui::ComboBox::from_id_salt(("qb_join_right", i))
                    .selected_text(join.right_table.clone())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for a in &aliases {
                            ui.selectable_value(&mut join.right_table, a.clone(), a);
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });

            if join.join_type.takes_on() {
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::muted(palette, "ON"));
                    alias_column_picker(
                        ui,
                        ("qb_join_lt", i),
                        &aliases,
                        &columns,
                        &mut join.left_table,
                        &mut join.left_column,
                    );
                    ui.label("=");
                    let right_alias = join.right_table.clone();
                    column_picker(
                        ui,
                        ("qb_join_rc", i),
                        columns_for(&columns, &right_alias),
                        &mut join.right_column,
                    );
                });
            } else {
                ui.label(
                    egui::RichText::new(
                        "CROSS JOIN pairs every row with every row; no ON condition.",
                    )
                    .small()
                    .color(palette.warning),
                );
            }
            ui.add_space(4.0);
            ui.separator();
        }

        ui.horizontal(|ui| {
            if ui.button("Add join").clicked() {
                let left = aliases.first().cloned().unwrap_or_default();
                let right = aliases
                    .iter()
                    .find(|a| !qb.state.joins.iter().any(|j| &&j.right_table == a) && **a != left)
                    .cloned()
                    .unwrap_or_default();
                intents.push(Intent::AddJoin(Join::new(left, right)));
            }
        });

        if !suggestions.is_empty() {
            ui.add_space(4.0);
            ui.label(crate::theme::text::muted(
                palette,
                "Suggested from foreign keys",
            ));
            for join in &suggestions {
                ui.horizontal(|ui| {
                    if ui.small_button("Use").clicked() {
                        intents.push(Intent::AddJoin(join.clone()));
                    }
                    ui.label(crate::theme::text::muted(
                        palette,
                        format!(
                            "{}.{} = {}.{}",
                            join.left_table, join.left_column, join.right_table, join.right_column
                        ),
                    ));
                });
            }
        }
    });

    if let Some(i) = remove {
        app.screens.query_builder.state.joins.remove(i);
    }
    app.screens.query_builder.sections.joins = open;
}

fn column_picker(ui: &mut egui::Ui, id: (&str, usize), columns: &[String], target: &mut String) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(if target.is_empty() {
            "column".to_string()
        } else {
            target.clone()
        })
        .width(140.0)
        .show_ui(ui, |ui| {
            for c in columns {
                ui.selectable_value(target, c.clone(), c);
            }
        });
}

/// Alias picker followed by a column picker scoped to that alias.
fn alias_column_picker(
    ui: &mut egui::Ui,
    id: (&str, usize),
    aliases: &[String],
    columns: &[(String, Vec<String>)],
    alias: &mut String,
    column: &mut String,
) {
    let before = alias.clone();
    egui::ComboBox::from_id_salt((id.0, id.1, "a"))
        .selected_text(if alias.is_empty() {
            "table".to_string()
        } else {
            alias.clone()
        })
        .width(110.0)
        .show_ui(ui, |ui| {
            for a in aliases {
                ui.selectable_value(alias, a.clone(), a);
            }
        });
    // A column from the old table would not exist on the new one.
    if *alias != before {
        column.clear();
    }
    column_picker(ui, (id.0, id.1), columns_for(columns, alias), column);
}

fn columns_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.columns;
    let count = qb.state.columns.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let available: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            let mut cols = vec![STAR.to_string()];
            cols.extend(t.columns.iter().map(|(n, _)| n.clone()));
            (t.alias.clone(), cols)
        })
        .collect();

    let mut remove: Option<usize> = None;
    let mut add: Option<SelectedColumn> = None;
    let global_distinct = qb.state.distinct;

    section(
        ui,
        palette,
        "qb_columns",
        "Columns",
        count,
        &mut open,
        |ui| {
            if qb.state.columns.is_empty() {
                ui.label(crate::theme::text::muted(
                    palette,
                    "No columns picked, so the query selects every column.",
                ));
            }

            for (i, col) in qb.state.columns.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    alias_column_picker(
                        ui,
                        ("qb_col", i),
                        &aliases,
                        &available,
                        &mut col.table_alias,
                        &mut col.column,
                    );
                    egui::ComboBox::from_id_salt(("qb_col_agg", i))
                        .selected_text(col.aggregate.label())
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for a in Aggregate::ALL {
                                ui.selectable_value(&mut col.aggregate, a, a.label());
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut col.alias)
                            .hint_text("output name (optional)")
                            .desired_width(180.0),
                    );
                    ui.add_enabled(
                        !col.aggregate.is_some() && !global_distinct,
                        egui::Checkbox::new(&mut col.distinct, "DISTINCT"),
                    )
                    .on_hover_text("Applies to the first output column only");
                });
                ui.add_space(4.0);
                ui.separator();
            }

            ui.label(crate::theme::text::muted(palette, "Add a column"));
            egui::ScrollArea::vertical()
                .id_salt("qb_col_available")
                .max_height(150.0)
                .show(ui, |ui| {
                    for (alias, cols) in &available {
                        for c in cols {
                            let already = qb
                                .state
                                .columns
                                .iter()
                                .any(|s| &s.table_alias == alias && &s.column == c);
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(!already, egui::Button::new("+").small())
                                    .clicked()
                                {
                                    add = Some(SelectedColumn::new(alias.clone(), c.clone()));
                                }
                                let label = if c == STAR {
                                    format!("{}.* (all columns)", alias)
                                } else {
                                    format!("{}.{}", alias, c)
                                };
                                ui.label(crate::theme::text::muted(palette, label));
                            });
                        }
                    }
                });
        },
    );

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove {
        qb.state.columns.remove(i);
    }
    if let Some(c) = add {
        qb.state.columns.push(c);
    }
    qb.sections.columns = open;
}

fn filters_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.filters;
    let count = count_conditions(&qb.state.filters);

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let typed: Vec<(String, Vec<(String, String)>)> = qb
        .state
        .tables
        .iter()
        .map(|t| (t.alias.clone(), t.columns.clone()))
        .collect();

    section(
        ui,
        palette,
        "qb_filters",
        "Filters",
        count,
        &mut open,
        |ui| {
            let group = std::mem::take(&mut qb.state.filters);
            let mut group = group;
            draw_group(ui, palette, &mut group, &aliases, &typed, 0, "g");
            qb.state.filters = group;
        },
    );

    app.screens.query_builder.sections.filters = open;
}

/// One AND/OR group, recursively. `path` keeps widget ids unique per nesting
/// position, since egui identifies widgets by id, not by position in a tree.
fn draw_group(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    group: &mut ConditionGroup,
    aliases: &[String],
    typed: &[(String, Vec<(String, String)>)],
    depth: usize,
    path: &str,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(palette, "Match"));
        egui::ComboBox::from_id_salt((path, "op"))
            .selected_text(match group.operator {
                Logical::And => "all (AND)",
                Logical::Or => "any (OR)",
            })
            .width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut group.operator, Logical::And, "all (AND)");
                ui.selectable_value(&mut group.operator, Logical::Or, "any (OR)");
            });
    });

    let mut remove: Option<usize> = None;
    for (i, cond) in group.conditions.iter_mut().enumerate() {
        let id = format!("{}c{}", path, i);
        ui.horizontal_wrapped(|ui| {
            let before = cond.table_alias.clone();
            egui::ComboBox::from_id_salt((id.clone(), "a"))
                .selected_text(if cond.table_alias.is_empty() {
                    "table".to_string()
                } else {
                    cond.table_alias.clone()
                })
                .width(100.0)
                .show_ui(ui, |ui| {
                    for a in aliases {
                        ui.selectable_value(&mut cond.table_alias, a.clone(), a);
                    }
                });
            if cond.table_alias != before {
                cond.column.clear();
            }

            let cols = typed
                .iter()
                .find(|(a, _)| a == &cond.table_alias)
                .map(|(_, c)| c.as_slice())
                .unwrap_or(&[]);
            let before_col = cond.column.clone();
            egui::ComboBox::from_id_salt((id.clone(), "c"))
                .selected_text(if cond.column.is_empty() {
                    "column".to_string()
                } else {
                    cond.column.clone()
                })
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (name, _) in cols {
                        ui.selectable_value(&mut cond.column, name.clone(), name);
                    }
                });
            if cond.column != before_col {
                // The value editor and the literal form both follow the
                // column's declared type, so it is re-read on every change.
                cond.kind = cols
                    .iter()
                    .find(|(n, _)| n == &cond.column)
                    .map(|(_, t)| ValueKind::from_data_type(t))
                    .unwrap_or_default();
            }

            egui::ComboBox::from_id_salt((id.clone(), "o"))
                .selected_text(cond.op.sql())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for op in CompareOp::ALL {
                        ui.selectable_value(&mut cond.op, op, op.sql());
                    }
                });

            value_editor(ui, palette, cond, &id);

            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        group.conditions.remove(i);
    }

    let mut remove_group: Option<usize> = None;
    for (i, nested) in group.groups.iter_mut().enumerate() {
        let id = format!("{}g{}", path, i);
        egui::Frame::none()
            .fill(palette.secondary)
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::muted(palette, "Nested group"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove group").clicked() {
                            remove_group = Some(i);
                        }
                    });
                });
                draw_group(ui, palette, nested, aliases, typed, depth + 1, &id);
            });
        ui.add_space(4.0);
    }
    if let Some(i) = remove_group {
        group.groups.remove(i);
    }

    ui.horizontal(|ui| {
        if ui.button("Add condition").clicked() {
            let alias = aliases.first().cloned().unwrap_or_default();
            let (column, kind) = typed
                .iter()
                .find(|(a, _)| a == &alias)
                .and_then(|(_, c)| c.first())
                .map(|(n, t)| (n.clone(), ValueKind::from_data_type(t)))
                .unwrap_or_default();
            group.conditions.push(Condition::new(alias, column, kind));
        }
        if depth + 1 < MAX_GROUP_DEPTH {
            if ui
                .button("Add group")
                .on_hover_text("A nested set of conditions with its own AND/OR")
                .clicked()
            {
                group.groups.push(ConditionGroup::default());
            }
        } else {
            ui.label(crate::theme::text::muted(
                palette,
                format!("Nesting stops at {} levels", MAX_GROUP_DEPTH),
            ));
        }
    });
}

/// The value editor the operator and the column's type ask for.
fn value_editor(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    cond: &mut Condition,
    id: &str,
) {
    match cond.op.arity() {
        Arity::None => {
            ui.label(crate::theme::text::muted(palette, "no value"));
        }
        Arity::List => {
            ui.add(
                egui::TextEdit::singleline(&mut cond.value)
                    .id_salt(format!("{}v", id))
                    .hint_text("a, b, c")
                    .desired_width(160.0),
            );
        }
        Arity::Range => {
            ui.add(
                egui::TextEdit::singleline(&mut cond.value)
                    .id_salt(format!("{}v", id))
                    .hint_text("from")
                    .desired_width(80.0),
            );
            ui.label(crate::theme::text::muted(palette, "and"));
            ui.add(
                egui::TextEdit::singleline(&mut cond.value2)
                    .id_salt(format!("{}v2", id))
                    .hint_text("to")
                    .desired_width(80.0),
            );
        }
        Arity::One => {
            if cond.kind == ValueKind::Bool
                && !matches!(cond.op, CompareOp::Like | CompareOp::NotLike)
            {
                let mut on = matches!(cond.value.to_ascii_lowercase().as_str(), "true" | "t" | "1");
                if ui.checkbox(&mut on, "true").changed() {
                    cond.value = if on { "true" } else { "false" }.to_string();
                }
                if cond.value.is_empty() {
                    cond.value = "false".to_string();
                }
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut cond.value)
                        .id_salt(format!("{}v", id))
                        .hint_text("value")
                        .desired_width(140.0),
                );
            }
        }
    }
}

fn group_by_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.group_by;
    let count = qb.state.group_by.len() + qb.state.having.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();

    let mut remove_group: Option<usize> = None;
    let mut remove_having: Option<usize> = None;
    let mut add_group = false;
    let mut add_having = false;

    section(
        ui,
        palette,
        "qb_group_by",
        "Group by / having",
        count,
        &mut open,
        |ui| {
            for (i, g) in qb.state.group_by.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    alias_column_picker(
                        ui,
                        ("qb_gb", i),
                        &aliases,
                        &columns,
                        &mut g.table_alias,
                        &mut g.column,
                    );
                    if ui.small_button("Remove").clicked() {
                        remove_group = Some(i);
                    }
                });
            }
            if ui.button("Add group by").clicked() {
                add_group = true;
            }

            ui.add_space(6.0);
            ui.separator();
            ui.label(crate::theme::text::muted(palette, "HAVING"));

            for (i, h) in qb.state.having.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt(("qb_hav_agg", i))
                        .selected_text(h.aggregate.label())
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for a in Aggregate::FOR_HAVING {
                                ui.selectable_value(&mut h.aggregate, a, a.label());
                            }
                        });
                    alias_column_picker(
                        ui,
                        ("qb_hav", i),
                        &aliases,
                        &columns,
                        &mut h.table_alias,
                        &mut h.column,
                    );
                    egui::ComboBox::from_id_salt(("qb_hav_op", i))
                        .selected_text(h.op.sql())
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for op in CompareOp::FOR_HAVING {
                                ui.selectable_value(&mut h.op, op, op.sql());
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut h.value)
                            .hint_text("value")
                            .desired_width(90.0),
                    );
                    if ui.small_button("Remove").clicked() {
                        remove_having = Some(i);
                    }
                });
            }
            if ui.button("Add having").clicked() {
                add_having = true;
            }
        },
    );

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove_group {
        qb.state.group_by.remove(i);
    }
    if let Some(i) = remove_having {
        qb.state.having.remove(i);
    }
    if add_group {
        let (alias, column) = first_column(&qb.state);
        qb.state.group_by.push(GroupByClause {
            table_alias: alias,
            column,
        });
    }
    if add_having {
        let (alias, column) = first_column(&qb.state);
        qb.state.having.push(HavingCondition::new(alias, column));
    }
    qb.sections.group_by = open;
}

fn order_by_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.order_by;
    let count = qb.state.order_by.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();

    let mut remove: Option<usize> = None;
    let mut add = false;
    let mut move_up: Option<usize> = None;

    section(
        ui,
        palette,
        "qb_order_by",
        "Order by",
        count,
        &mut open,
        |ui| {
            for (i, o) in qb.state.order_by.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    alias_column_picker(
                        ui,
                        ("qb_ob", i),
                        &aliases,
                        &columns,
                        &mut o.table_alias,
                        &mut o.column,
                    );
                    if ui
                        .small_button(o.direction.sql())
                        .on_hover_text("Toggle direction")
                        .clicked()
                    {
                        o.direction = o.direction.toggled();
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("⬆").small())
                        .on_hover_text("Sort by this earlier")
                        .clicked()
                    {
                        move_up = Some(i);
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if ui.button("Add order by").clicked() {
                add = true;
            }
        },
    );

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove {
        qb.state.order_by.remove(i);
    }
    if let Some(i) = move_up {
        qb.state.order_by.swap(i - 1, i);
    }
    if add {
        let (table_alias, column) = first_column(&qb.state);
        qb.state.order_by.push(OrderByClause {
            table_alias,
            column,
            direction: SortDir::Asc,
        });
    }
    qb.sections.order_by = open;
}

fn limit_section(app: &mut DbHiveApp, ui: &mut egui::Ui, rules: DialectRules) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.limit;
    let count = usize::from(qb.state.limit.is_some()) + usize::from(qb.state.offset.is_some());

    section(
        ui,
        palette,
        "qb_limit",
        "Limit / offset",
        count,
        &mut open,
        |ui| {
            ui.checkbox(&mut qb.state.distinct, "DISTINCT (whole result)");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(crate::theme::text::muted(palette, "LIMIT"));
                optional_number(ui, "qb_limit_v", &mut qb.state.limit);
                ui.label(crate::theme::text::muted(palette, "OFFSET"));
                optional_number(ui, "qb_offset_v", &mut qb.state.offset);
            });
            ui.label(crate::theme::text::muted(
                palette,
                pagination_summary(qb.state.limit, qb.state.offset),
            ));
            if rules.limit_style == LimitStyle::OffsetFetch {
                ui.label(crate::theme::text::muted(
                    palette,
                    "SQL Server needs an ORDER BY for OFFSET/FETCH.",
                ));
            }
        },
    );

    app.screens.query_builder.sections.limit = open;
}

/// A number box where empty means "no clause". Non-numeric text is simply not
/// accepted, so the state can never hold something that is not a number.
fn optional_number(ui: &mut egui::Ui, id: &str, target: &mut Option<u64>) {
    let mut text = target.map(|v| v.to_string()).unwrap_or_default();
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id_salt(id)
            .hint_text("none")
            .desired_width(70.0),
    );
    if response.changed() {
        let trimmed = text.trim();
        *target = if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<u64>().ok().or(*target)
        };
    }
}

fn preview_and_results(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    intents: &mut Vec<Intent>,
    warnings: &[String],
) {
    let palette = app.mode.palette();

    if !warnings.is_empty() {
        egui::Frame::none()
            .fill(palette.secondary)
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for w in warnings {
                    ui.label(egui::RichText::new(w).small().color(palette.warning));
                }
            });
        ui.add_space(6.0);
    }

    let index = schema_index(&app.screens.query_builder.state);
    let qb = &mut app.screens.query_builder;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "SQL"));
        let edited = qb.generated.as_deref() != Some(qb.preview.as_str());
        if edited && !qb.preview.is_empty() {
            ui.label(crate::theme::text::muted(
                palette,
                "edited by hand; changing a clause regenerates it",
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!qb.preview.trim().is_empty(), egui::Button::new("Copy"))
                .clicked()
            {
                let sql = qb.preview.clone();
                ui.output_mut(|o| o.copied_text = sql);
            }
        });
    });

    if qb.preview.trim().is_empty() {
        ui.add_space(12.0);
        ui.label(crate::theme::text::muted(
            palette,
            "Pick a table on the left and the statement appears here.",
        ));
    } else {
        let mut buffer = std::mem::take(&mut qb.preview);
        let action = sql_editor::show(ui, palette, &mut qb.editor, &index, &mut buffer);
        qb.preview = buffer;
        if action == sql_editor::EditorAction::Execute {
            intents.push(Intent::Run);
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    if let Some(err) = qb.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Query failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                qb.last_error = None;
            }
        });
        return;
    }

    if qb.run_job.is_running() && qb.result.is_none() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Running"));
        });
        return;
    }

    match qb.result.take() {
        Some(result) => {
            if let Some(ms) = qb.elapsed_ms {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} row(s) in {} ms", result.rows.len(), ms),
                ));
            }
            results_grid::show(ui, palette, &result, &mut qb.grid);
            qb.result = Some(result);
        }
        None => {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    "Run the query to see results",
                ));
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Async work
// ---------------------------------------------------------------------------

/// Drain finished jobs so this frame draws the newest data.
fn poll(app: &mut DbHiveApp) {
    let qb = &mut app.screens.query_builder;

    if let Some(result) = qb.catalog_job.take_result() {
        match result {
            Ok((tables, keys)) => {
                qb.catalog = tables;
                qb.foreign_keys = keys;
            }
            Err(e) => qb.last_error = Some(e.to_string()),
        }
    }

    if let Some(result) = qb.add_job.take_result() {
        let pending = qb.pending_add.take();
        match (result, pending) {
            (Ok(schema), Some((name, alias))) => {
                let columns = schema
                    .columns
                    .iter()
                    .map(|c| (c.name.clone(), c.data_type.clone()))
                    .collect();
                let table = QueryTable::new(alias, qb.schema.clone(), name).with_columns(columns);
                qb.state.tables.push(table);
            }
            (Err(e), _) => qb.last_error = Some(e.to_string()),
            (Ok(_), None) => {}
        }
    }

    if let Some((result, elapsed)) = qb.run_job.take_result() {
        qb.elapsed_ms = Some(elapsed);
        match result {
            Ok(r) => {
                qb.grid = results_grid::GridState::default();
                qb.result = Some(r);
                qb.last_error = None;
            }
            Err(e) => {
                qb.result = None;
                qb.last_error = Some(e.to_string());
            }
        }
    }
}

fn start_pending(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    if !app.screens.query_builder.needs_catalog {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.needs_catalog = false;
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let qb = &mut app.screens.query_builder;
    qb.needs_catalog = false;
    let schema = qb.schema.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .query_builder
        .catalog_job
        .start(&handle, ctx, async move {
            let tables = d.get_tables(&schema).await?;
            // Suggestions are a nice-to-have; a driver that cannot report
            // foreign keys must not cost the user the table list.
            let keys = d.get_foreign_keys(&schema).await.unwrap_or_default();
            Ok((tables, keys))
        });
}

fn start_add_table(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str, table: String) {
    if app.screens.query_builder.add_job.is_running() {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let qb = &mut app.screens.query_builder;
    let alias = qb.state.free_alias(&table);
    qb.pending_add = Some((table.clone(), alias));
    let schema = qb.schema.clone();
    let handle = app.rt.handle().clone();
    qb.add_job.start(&handle, ctx, async move {
        d.get_table_schema(&schema, &table).await
    });
}

fn run(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let sql = app.screens.query_builder.preview.clone();
    if sql.trim().is_empty() || app.screens.query_builder.run_job.is_running() {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let handle = app.rt.handle().clone();
    app.screens
        .query_builder
        .run_job
        .start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = d.execute_query(&sql).await;
            (out, started.elapsed().as_millis())
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn users() -> QueryTable {
        QueryTable::new("users", "public", "users").with_columns(vec![
            ("id".into(), "integer".into()),
            ("name".into(), "text".into()),
            ("active".into(), "boolean".into()),
        ])
    }

    fn orders() -> QueryTable {
        QueryTable::new("orders", "public", "orders").with_columns(vec![
            ("id".into(), "integer".into()),
            ("user_id".into(), "integer".into()),
            ("total".into(), "numeric".into()),
        ])
    }

    /// users joined to orders, mirroring the fixture in the model's tests.
    fn joined() -> BuilderState {
        BuilderState {
            tables: vec![users(), orders()],
            joins: vec![Join {
                join_type: JoinType::Inner,
                left_table: "users".into(),
                left_column: "id".into(),
                right_table: "orders".into(),
                right_column: "user_id".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_completion_index_covers_tables_aliases_and_columns() {
        let index = schema_index(&joined());
        assert!(index.tables.contains("users"));
        assert!(index.tables.contains("orders"));
        assert!(index.columns.contains("user_id"));
    }
}
