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

use dbhive_core::models::{DbError, ForeignKeyInfo, TableSchema};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::graph_layout;

// Pure logic moved to dbhive-ui-model; re-exported so existing paths keep
// working (the table editor calls `schema_designer::parse_type`).
pub use dbhive_ui_model::schema_designer::{
    column_display_label, column_display_suffix, display_name, fk_action_label, from_metadata,
    generate_sql, parse_type, relationships, table_size, validate, DesignColumn, DesignTable,
    FK_ACTIONS, HEADER_H, ROW_H,
};

use crate::screens::table_editor::TypeKind;
use dbhive_ui_model::er_diagram::Point;
use dbhive_ui_model::schema_designer as model;

fn to_pos2(p: Point) -> egui::Pos2 {
    egui::pos2(p.x, p.y)
}

fn to_point(p: egui::Pos2) -> Point {
    Point::new(p.x, p.y)
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
    pub pinned: HashMap<u64, Point>,

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
        let n = self.tables.len();
        self.tables.push(model::new_table(id));
        self.pinned.insert(id, model::cascade_position(n));
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
        self.layout = model::relayout(&self.tables);
    }

    fn position(&self, id: u64) -> egui::Pos2 {
        to_pos2(model::position(&self.layout, &self.pinned, id))
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
                        if ui.small_button("✖").on_hover_text("Remove table").clicked() {
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
                                if ui.small_button("✖").on_hover_text("Remove column").clicked() {
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

// ─── canvas ───────────────────────────────────────────────────────────────

/// The table under a world-space point, topmost first.
fn table_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<usize> {
    model::table_at(&d.tables, &d.layout, &d.pinned, to_point(world))
}

/// The column row under a world-space point.
fn column_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<(usize, usize)> {
    model::column_at(&d.tables, &d.layout, &d.pinned, to_point(world))
}

/// Where a column's connector sits, in world space.
fn handle_pos(d: &SchemaDesigner, table: usize, column: usize, right: bool) -> egui::Pos2 {
    to_pos2(model::handle_pos(
        &d.tables, &d.layout, &d.pinned, table, column, right,
    ))
}

/// A connector near a world-space point, if the press should start a
/// connection rather than move the table.
fn handle_at(d: &SchemaDesigner, world: egui::Pos2) -> Option<(usize, usize)> {
    model::handle_at(&d.tables, &d.layout, &d.pinned, to_point(world))
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
                    d.pinned.insert(id, to_point(current + delta));
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
                match model::resolve_connection(&d.tables, source, column, target) {
                    Some(Ok(made)) => connect_made = Some(made),
                    Some(Err(msg)) => connect_failed = Some(msg),
                    None => {}
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
        // Say why it did not take rather than silently dropping the gesture.
        match model::link(&mut d.tables, si, sc, ti, tc) {
            Err(msg) => app.toasts.error(format!("Cannot link: {}", msg)),
            Ok((rt, rc)) => app.toasts.success(format!("Linked to {}.{}", rt, rc)),
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
            painter.text(
                egui::pos2(top_left.x + 8.0 * d.zoom, y),
                egui::Align2::LEFT_CENTER,
                column_display_label(col),
                egui::FontId::proportional(11.0 * d.zoom),
                if col.base.primary_key {
                    palette.primary
                } else {
                    palette.foreground
                },
            );

            painter.text(
                egui::pos2(top_left.x + size.x - 8.0 * d.zoom, y),
                egui::Align2::RIGHT_CENTER,
                column_display_suffix(col),
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
    // The design model, validation, DDL, loading and canvas geometry are
    // tested in dbhive-ui-model. What stays here is the screen state and the
    // egui conversions around it.
    use super::*;
    use crate::screens::table_editor::ColumnRow;

    fn pk(name: &str) -> DesignColumn {
        DesignColumn {
            base: ColumnRow {
                name: name.into(),
                type_kind: TypeKind::BigInt,
                primary_key: true,
                nullable: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// users(id PK) and orders(id PK, user_id -> users.id).
    fn users_and_orders() -> Vec<DesignTable> {
        let users = DesignTable {
            id: 1,
            name: "users".into(),
            schema: String::new(),
            columns: vec![pk("id")],
        };
        let mut fk = pk("user_id");
        fk.base.primary_key = false;
        fk.ref_table = "users".into();
        fk.ref_column = "id".into();
        let orders = DesignTable {
            id: 2,
            name: "orders".into(),
            schema: String::new(),
            columns: vec![pk("id"), fk],
        };
        vec![users, orders]
    }

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
        d.pinned.insert(1, Point::new(500.0, 500.0));
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

    #[test]
    fn screen_hit_testing_converts_to_and_from_world_points() {
        let mut d = SchemaDesigner::default();
        d.tables = users_and_orders();
        d.pinned.insert(1, Point::new(0.0, 0.0));
        d.pinned.insert(2, Point::new(1000.0, 0.0));
        assert_eq!(table_at(&d, egui::pos2(5.0, 5.0)), Some(0));
        assert_eq!(
            column_at(&d, egui::pos2(1005.0, HEADER_H + ROW_H + 1.0)),
            Some((1, 1))
        );
        let h = handle_pos(&d, 1, 1, true);
        assert_eq!(handle_at(&d, h), Some((1, 1)));
    }
}
