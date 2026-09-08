//! Entity-relationship diagram.
//!
//! Replaces the reactflow + dagre stack. Layout comes from
//! `crate::widgets::graph_layout` (a layered Sugiyama layout); everything
//! here is drawing and interaction on an egui painter.
//!
//! Pan and zoom are handled by hand rather than with a scene widget, which
//! egui 0.29 does not have. All world coordinates go through `to_screen`,
//! so there is exactly one place that knows about the transform.

use std::collections::HashMap;

use dbhive_core::drivers::DatabaseDriver;
use dbhive_core::models::{ColumnInfo, DbError, ForeignKeyInfo, TableInfo};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::graph_layout::{self, Edge, LayoutOptions, Node};

/// A table as drawn: its columns and which are keys.
#[derive(Clone)]
pub struct DiagramTable {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

type Loaded = (Vec<DiagramTable>, Vec<ForeignKeyInfo>);

pub struct ErDiagram {
    pub open: bool,
    pub connection_id: Option<String>,
    pub schema: String,

    pub tables: Vec<DiagramTable>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub layout: graph_layout::Layout,

    /// Pan offset in screen pixels, and zoom factor.
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub selected: Option<String>,
    /// Tables the user has dragged, in world coordinates. These override the
    /// computed layout so a manual arrangement survives a re-layout.
    pub pinned: HashMap<String, egui::Pos2>,
    pub filter: String,
    pub show_columns: bool,

    pub load_job: Job<Result<Loaded, DbError>>,
    pub last_error: Option<String>,
    requested_for: Option<(String, String)>,
}

impl Default for ErDiagram {
    fn default() -> Self {
        Self {
            open: false,
            connection_id: None,
            schema: String::new(),
            tables: Vec::new(),
            foreign_keys: Vec::new(),
            layout: graph_layout::Layout::default(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            selected: None,
            pinned: HashMap::new(),
            filter: String::new(),
            show_columns: true,
            load_job: Job::default(),
            last_error: None,
            requested_for: None,
        }
    }
}

/// Header height plus one row per column, used both for layout and drawing.
fn table_size(t: &DiagramTable, show_columns: bool) -> (f32, f32) {
    let longest = t
        .columns
        .iter()
        .map(|c| c.name.len() + c.data_type.len() + 3)
        .max()
        .unwrap_or(0)
        .max(t.name.len());
    let width = (longest as f32 * 7.0 + 24.0).clamp(140.0, 320.0);
    let rows = if show_columns { t.columns.len() } else { 0 };
    let height = HEADER_H + rows as f32 * ROW_H + 6.0;
    (width, height)
}

const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 18.0;

impl ErDiagram {
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            self.open = true;
            return;
        }
        self.connection_id = Some(c);
        self.schema = s;
        self.open = true;
        self.tables.clear();
        self.foreign_keys.clear();
        self.pinned.clear();
        self.pan = egui::Vec2::ZERO;
        self.zoom = 1.0;
        self.requested_for = None;
        self.last_error = None;
    }

    /// Recompute positions from the current tables and foreign keys.
    fn relayout(&mut self) {
        let nodes: Vec<Node> = self
            .tables
            .iter()
            .map(|t| {
                let (width, height) = table_size(t, self.show_columns);
                Node {
                    id: t.name.clone(),
                    width,
                    height,
                }
            })
            .collect();
        // Parent before child: the edge runs from the referenced table to the
        // one holding the foreign key, so `users` is laid out before `orders`
        // rather than after it. Getting this backwards produces a diagram
        // that reads inside out.
        let edges: Vec<Edge> = self
            .foreign_keys
            .iter()
            .map(|fk| Edge {
                from: fk.referenced_table.clone(),
                to: fk.table.clone(),
            })
            .collect();
        self.layout = graph_layout::layout(&nodes, &edges, LayoutOptions::default());
    }

    /// Where a table sits in world space: a manual position if the user
    /// dragged it, otherwise the computed layout.
    fn position(&self, name: &str) -> egui::Pos2 {
        if let Some(p) = self.pinned.get(name) {
            return *p;
        }
        self.layout
            .placements
            .get(name)
            .map(|p| egui::pos2(p.x, p.y))
            .unwrap_or(egui::Pos2::ZERO)
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();

    let Some(connection_id) = app.screens.er_diagram.connection_id.clone() else {
        ui.label(crate::theme::text::muted(
            palette,
            "Open a connection to see its diagram",
        ));
        return;
    };
    let schema = app.screens.er_diagram.schema.clone();

    // Load once per (connection, schema).
    let key = (connection_id.clone(), schema.clone());
    if app.screens.er_diagram.requested_for.as_ref() != Some(&key) {
        app.screens.er_diagram.requested_for = Some(key);
        load(app, ui.ctx(), connection_id, schema.clone());
    }

    if let Some(result) = app.screens.er_diagram.load_job.take_result() {
        match result {
            Ok((tables, fks)) => {
                let d = &mut app.screens.er_diagram;
                d.tables = tables;
                d.foreign_keys = fks;
                d.last_error = None;
                d.relayout();
            }
            Err(e) => app.screens.er_diagram.last_error = Some(e.to_string()),
        }
    }

    toolbar(app, ui, palette);

    if app.screens.er_diagram.load_job.is_running() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading schema"));
        });
        return;
    }
    if let Some(err) = app.screens.er_diagram.last_error.clone() {
        ui.colored_label(palette.destructive, err);
        return;
    }
    if app.screens.er_diagram.tables.is_empty() {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::heading(palette, "Nothing to diagram"));
            ui.label(crate::theme::text::muted(
                palette,
                "This schema has no tables.",
            ));
        });
        return;
    }

    canvas(app, ui, palette);
}

fn toolbar(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(
            palette,
            format!("ER Diagram · {}", app.screens.er_diagram.schema),
        ));
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut app.screens.er_diagram.filter)
                .hint_text("Highlight tables...")
                .desired_width(180.0),
        );
        let mut show_columns = app.screens.er_diagram.show_columns;
        if ui.checkbox(&mut show_columns, "Columns").changed() {
            app.screens.er_diagram.show_columns = show_columns;
            app.screens.er_diagram.relayout();
        }
        if ui.button("Auto layout").clicked() {
            app.screens.er_diagram.pinned.clear();
            app.screens.er_diagram.relayout();
        }
        if ui.button("Fit").clicked() {
            app.screens.er_diagram.pan = egui::Vec2::ZERO;
            app.screens.er_diagram.zoom = 1.0;
        }
        ui.label(crate::theme::text::muted(
            palette,
            format!("{:.0}%", app.screens.er_diagram.zoom * 100.0),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                app.screens.er_diagram.open = false;
            }
        });
    });
    ui.add_space(4.0);
}

fn canvas(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.background);

    let d = &mut app.screens.er_diagram;

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

    // Dragging empty space pans; dragging a table moves that table.
    let mut dragged_table: Option<String> = None;
    if response.dragged() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let world = (pointer - rect.min.to_vec2() - d.pan) / d.zoom;
            let hit = hit_test(d, egui::pos2(world.x, world.y));
            match hit {
                Some(name) => dragged_table = Some(name),
                None => d.pan += response.drag_delta(),
            }
        }
    }
    if let Some(name) = dragged_table {
        let delta = response.drag_delta() / d.zoom;
        let current = d.position(&name);
        d.pinned.insert(name.clone(), current + delta);
        d.selected = Some(name);
    }

    if response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let world = (pointer - rect.min.to_vec2() - d.pan) / d.zoom;
            d.selected = hit_test(d, egui::pos2(world.x, world.y));
        }
    }

    let to_screen = |p: egui::Pos2| rect.min + d.pan + p.to_vec2() * d.zoom;

    // Sizes are needed for both edges and boxes; compute once.
    let sizes: HashMap<String, (f32, f32)> = d
        .tables
        .iter()
        .map(|t| (t.name.clone(), table_size(t, d.show_columns)))
        .collect();

    // Relationships first, so boxes sit on top of the lines.
    for fk in &d.foreign_keys {
        let (Some(from), Some(to)) = (
            sizes.get(&fk.table).map(|s| (d.position(&fk.table), *s)),
            sizes
                .get(&fk.referenced_table)
                .map(|s| (d.position(&fk.referenced_table), *s)),
        ) else {
            continue;
        };
        let a = to_screen(egui::pos2(
            from.0.x + from.1 .0,
            from.0.y + from.1 .1 / 2.0,
        ));
        let b = to_screen(egui::pos2(to.0.x, to.0.y + to.1 .1 / 2.0));

        let involved = d.selected.as_deref() == Some(fk.table.as_str())
            || d.selected.as_deref() == Some(fk.referenced_table.as_str());
        let colour = if involved {
            palette.primary
        } else {
            palette.muted_foreground
        };
        let width = if involved { 2.0 } else { 1.0 };

        // A horizontal-tangent bezier reads better than a straight line when
        // layers are stacked; it makes the direction of the relationship
        // obvious without arrowheads at every crossing.
        let dx = ((b.x - a.x).abs() * 0.4).max(24.0);
        let curve = egui::epaint::CubicBezierShape::from_points_stroke(
            [a, a + egui::vec2(dx, 0.0), b - egui::vec2(dx, 0.0), b],
            false,
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(width, colour),
        );
        painter.add(curve);
        painter.circle_filled(b, 3.0 * d.zoom.min(1.5), colour);
    }

    let needle = d.filter.to_lowercase();
    for t in &d.tables {
        let (w, h) = sizes[&t.name];
        let pos = d.position(&t.name);
        let top_left = to_screen(pos);
        let size = egui::vec2(w, h) * d.zoom;
        let box_rect = egui::Rect::from_min_size(top_left, size);
        if !rect.intersects(box_rect) {
            continue; // off-screen, skip the text shaping entirely
        }

        let selected = d.selected.as_deref() == Some(t.name.as_str());
        let matched = !needle.is_empty() && t.name.to_lowercase().contains(&needle);
        let border = if selected || matched {
            palette.primary
        } else {
            palette.border
        };

        painter.rect_filled(box_rect, 6.0, palette.card);
        painter.rect_stroke(
            box_rect,
            6.0,
            egui::Stroke::new(if selected { 2.0 } else { 1.0 }, border),
        );

        let header = egui::Rect::from_min_size(top_left, egui::vec2(size.x, HEADER_H * d.zoom));
        painter.rect_filled(header, 6.0, palette.secondary);
        painter.text(
            header.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &t.name,
            egui::FontId::proportional(13.0 * d.zoom),
            palette.foreground,
        );

        if d.show_columns {
            for (i, col) in t.columns.iter().enumerate() {
                let y = top_left.y + (HEADER_H + i as f32 * ROW_H + ROW_H / 2.0) * d.zoom;
                let key = col.is_primary_key;
                let label = if key {
                    format!("PK  {}", col.name)
                } else {
                    col.name.clone()
                };
                painter.text(
                    egui::pos2(top_left.x + 8.0 * d.zoom, y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(11.0 * d.zoom),
                    if key {
                        palette.primary
                    } else {
                        palette.foreground
                    },
                );
                painter.text(
                    egui::pos2(top_left.x + size.x - 8.0 * d.zoom, y),
                    egui::Align2::RIGHT_CENTER,
                    &col.data_type,
                    egui::FontId::proportional(10.0 * d.zoom),
                    palette.muted_foreground,
                );
            }
        }
    }
}

/// The topmost table under a world-space point, if any.
fn hit_test(d: &ErDiagram, world: egui::Pos2) -> Option<String> {
    for t in d.tables.iter().rev() {
        let (w, h) = table_size(t, d.show_columns);
        let pos = d.position(&t.name);
        if egui::Rect::from_min_size(pos, egui::vec2(w, h)).contains(world) {
            return Some(t.name.clone());
        }
    }
    None
}

fn load(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: String, schema: String) {
    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.er_diagram.last_error = Some("Connection is no longer open".into());
        return;
    };
    let handle = app.rt.handle().clone();
    app.screens
        .er_diagram
        .load_job
        .start(&handle, ctx, async move {
            let tables: Vec<TableInfo> = driver.get_tables(&schema).await?;
            let foreign_keys = driver.get_foreign_keys(&schema).await.unwrap_or_default();

            // Columns are fetched per table. A table whose schema cannot be
            // read still appears in the diagram, just without its columns,
            // which is far better than failing the whole picture.
            let mut out = Vec::with_capacity(tables.len());
            for t in tables {
                let columns = match driver.get_table_schema(&schema, &t.name).await {
                    Ok(s) => s.columns,
                    Err(_) => Vec::new(),
                };
                out.push(DiagramTable {
                    name: t.name,
                    columns,
                });
            }
            Ok((out, foreign_keys))
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable: true,
            default_value: None,
            is_primary_key: pk,
            is_auto_increment: false,
        }
    }

    fn table(name: &str, n: usize) -> DiagramTable {
        DiagramTable {
            name: name.into(),
            columns: (0..n).map(|i| col(&format!("c{}", i), "text", i == 0)).collect(),
        }
    }

    #[test]
    fn height_grows_with_columns_and_collapses_when_hidden() {
        let t = table("users", 4);
        let (_, tall) = table_size(&t, true);
        let (_, short) = table_size(&t, false);
        assert!(tall > short);
        assert!((short - (HEADER_H + 6.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn width_is_clamped_for_absurd_names() {
        let mut t = table("x", 1);
        t.columns[0].name = "a".repeat(500);
        let (w, _) = table_size(&t, true);
        assert!(w <= 320.0, "width {} escaped the clamp", w);
        let narrow = table_size(&table("a", 0), true).0;
        assert!(narrow >= 140.0);
    }

    #[test]
    fn pinned_position_overrides_layout() {
        let mut d = ErDiagram::default();
        d.tables = vec![table("users", 1)];
        d.relayout();
        let auto = d.position("users");
        d.pinned.insert("users".into(), egui::pos2(500.0, 500.0));
        assert_ne!(auto, d.position("users"));
        assert_eq!(d.position("users"), egui::pos2(500.0, 500.0));
    }

    #[test]
    fn hit_test_finds_the_table_under_the_point() {
        let mut d = ErDiagram::default();
        d.tables = vec![table("users", 2)];
        d.relayout();
        let p = d.position("users");
        assert_eq!(hit_test(&d, p + egui::vec2(5.0, 5.0)), Some("users".into()));
        assert_eq!(hit_test(&d, p - egui::vec2(50.0, 50.0)), None);
    }

    #[test]
    fn relayout_places_every_table() {
        let mut d = ErDiagram::default();
        d.tables = vec![table("a", 1), table("b", 1), table("c", 1)];
        d.foreign_keys = vec![ForeignKeyInfo {
            name: "fk".into(),
            table: "b".into(),
            schema: "public".into(),
            columns: vec!["c0".into()],
            referenced_table: "a".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["c0".into()],
            on_delete: None,
            on_update: None,
        }];
        d.relayout();
        assert_eq!(d.layout.placements.len(), 3);
        // b holds a foreign key to a, so the parent a comes first.
        assert!(
            d.layout.placements["a"].layer < d.layout.placements["b"].layer,
            "parent should be laid out before the table referencing it"
        );
    }

    #[test]
    fn reopening_the_same_schema_keeps_manual_positions() {
        let mut d = ErDiagram::default();
        d.open_for("conn", "public");
        d.pinned.insert("users".into(), egui::pos2(10.0, 10.0));
        d.open_for("conn", "public");
        assert_eq!(d.pinned.len(), 1, "a no-op reopen discarded the arrangement");

        // A different schema is a different diagram, so it does reset.
        d.open_for("conn", "other");
        assert!(d.pinned.is_empty());
    }
}
