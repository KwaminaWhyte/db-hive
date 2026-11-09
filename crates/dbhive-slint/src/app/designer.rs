//! Controller glue for the visual schema designer overlay.
//!
//! A child module of `app`. The design itself (edits, validation, the
//! creation plan, what a failed table does to the ones that depend on it) is
//! `dbhive_ui_model::designer_session`, and the DDL comes from the driver's
//! generator in `dbhive_core::ddl`; this file moves values between that
//! logic, the canvas models and the `DesignerState` global, and runs the
//! generated statements against the open connection.

use std::collections::HashSet;
use std::rc::Rc;

use dbhive_ui_model::designer_session::{
    action_labels, results_summary, script, skip_reason, type_labels, Designer, Outcome,
    PlannedTable, TableResult,
};
use dbhive_ui_model::erd_scene::{fit, zoom_label, Flow, MiniMap, View};
use dbhive_ui_model::table_view::sql_qualifier;
use slint::ComponentHandle;

use super::{with_ctx, Ctx};
use crate::diagram_model::Diagram;
use crate::{designer_view, runtime, AppWindow, DesignerState, Screen, Tone};

pub(super) struct DesignerSession {
    design: Designer,
    diagram: Diagram,
    zoom: f32,
    map: Option<MiniMap>,
    dragged: bool,
    applying: bool,
}

impl Default for DesignerSession {
    fn default() -> Self {
        Self {
            design: Designer::new(),
            diagram: Diagram::new(Vec::new(), Vec::new()).with_flow(Flow::Horizontal, true),
            zoom: 1.0,
            map: None,
            dragged: false,
            applying: false,
        }
    }
}

impl Ctx {
    // -- Opening and closing ---------------------------------------------------

    pub(super) fn open_designer(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if w.get_screen() != Screen::Workspace {
            self.toast("Open a connection to design tables.", Tone::Neutral);
            return;
        }
        self.hide_history_panel();
        self.hide_erd();
        w.global::<DesignerState>().set_visible(true);
        self.project_designer();
    }

    pub(super) fn hide_designer(&self) {
        if let Some(w) = self.window.upgrade() {
            let st = w.global::<DesignerState>();
            st.set_visible(false);
            st.set_preview_open(false);
        }
    }

    // -- Where the tables will be created ------------------------------------------

    fn designer_target(&self) -> Option<(String, dbhive_core::models::DbDriver, String)> {
        let s = self.session.borrow();
        let ws = &s.workspace;
        let driver = ws.driver.clone()?;
        let schema = sql_qualifier(&driver, ws.schema.as_deref().unwrap_or(""));
        Some((ws.connection_id.clone(), driver, schema))
    }

    // -- Projection ---------------------------------------------------------------

    /// Rebuild the canvas models from the design (after any edit).
    fn designer_rebuild(&self) {
        {
            let mut s = self.session.borrow_mut();
            let dz = &mut s.designer;
            let (nodes, links) = (
                designer_view::nodes(&dz.design),
                designer_view::links(&dz.design),
            );
            dz.diagram.selected = dz.design.selected_index();
            dz.diagram.replace(nodes, links);
        }
        self.project_designer();
    }

    /// Push the session into the `DesignerState` global.
    pub(super) fn project_designer(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let st = w.global::<DesignerState>();
        let location = self
            .designer_target()
            .map(|(_, driver, schema)| {
                let label = crate::connection_list::driver_label(&driver);
                if schema.is_empty() {
                    label.to_string()
                } else {
                    format!("{} \u{b7} {}", label, schema)
                }
            })
            .unwrap_or_default();
        let mut s = self.session.borrow_mut();
        let dz = &mut s.designer;
        dz.map = designer_view::project(
            &st,
            &dz.design,
            &dz.diagram,
            dz.zoom,
            dz.applying,
            &location,
        );
    }

    fn designer_view(&self) -> View {
        let Some(w) = self.window.upgrade() else {
            return View::default();
        };
        let st = w.global::<DesignerState>();
        View {
            zoom: self.session.borrow().designer.zoom,
            pan_x: st.get_pan_x(),
            pan_y: st.get_pan_y(),
        }
    }

    fn designer_viewport(&self) -> (f32, f32) {
        let Some(w) = self.window.upgrade() else {
            return (1000.0, 600.0);
        };
        let st = w.global::<DesignerState>();
        let (vw, vh) = (st.get_viewport_width(), st.get_viewport_height());
        if vw > 1.0 && vh > 1.0 {
            (vw, vh)
        } else {
            (700.0, 560.0)
        }
    }

    fn designer_apply_view(&self, v: View) {
        self.session.borrow_mut().designer.zoom = v.zoom;
        if let Some(w) = self.window.upgrade() {
            let st = w.global::<DesignerState>();
            st.set_zoom(v.zoom);
            st.set_zoom_label(zoom_label(v.zoom).into());
            st.set_pan_x(v.pan_x);
            st.set_pan_y(v.pan_y);
        }
    }

    fn designer_fit(&self) {
        let bounds = self.session.borrow().designer.design.bounds();
        let Some(b) = bounds else { return };
        let (vw, vh) = self.designer_viewport();
        self.designer_apply_view(fit(b, vw, vh));
    }

    /// Pan to a table that is off screen, without changing the zoom.
    fn designer_reveal(&self, table: usize) {
        let rect = self.session.borrow().designer.design.rect(table);
        let (vw, vh) = self.designer_viewport();
        let v = self.designer_view();
        let visible = v.visible(vw, vh);
        let inside = rect.x >= visible.x
            && rect.y >= visible.y
            && rect.right() <= visible.right()
            && rect.bottom() <= visible.bottom();
        if !inside {
            self.designer_apply_view(v.center_on(rect.cx(), rect.cy(), vw, vh));
        }
    }

    // -- Editing ----------------------------------------------------------------------

    /// Run `f` on the design, then rebuild the canvas and the panel.
    fn designer_edit(&self, f: impl FnOnce(&mut Designer)) {
        {
            let mut s = self.session.borrow_mut();
            f(&mut s.designer.design);
        }
        self.designer_rebuild();
    }

    /// Run `f` with the selected table's id and column index, if there is a
    /// chosen column.
    fn designer_edit_column(&self, f: impl FnOnce(&mut Designer, u64, usize)) {
        let target = {
            let s = self.session.borrow();
            let d = &s.designer.design;
            d.selected().zip(d.selected_column())
        };
        if let Some((id, c)) = target {
            self.designer_edit(|d| f(d, id, c));
        }
    }

    fn designer_edit_table(&self, f: impl FnOnce(&mut Designer, u64)) {
        let id = self.session.borrow().designer.design.selected();
        if let Some(id) = id {
            self.designer_edit(|d| f(d, id));
        }
    }

    fn designer_select(&self, i: Option<usize>) {
        {
            let mut s = self.session.borrow_mut();
            let dz = &mut s.designer;
            dz.design.select_index(i);
            dz.diagram.select(dz.design.selected_index());
        }
        self.project_designer();
    }

    fn designer_dragged(&self, i: usize, dx: f32, dy: f32) {
        let selection_changed = {
            let mut s = self.session.borrow_mut();
            let dz = &mut s.designer;
            let zoom = dz.zoom.max(0.01);
            let Some(p) = dz.design.drag(i, dx / zoom, dy / zoom) else {
                return;
            };
            dz.dragged = true;
            dz.diagram.move_to(i, p.x, p.y);
            let changed = dz.design.selected_index() != Some(i);
            if changed {
                dz.design.select_index(Some(i));
                dz.diagram.select(Some(i));
            }
            changed
        };
        if selection_changed {
            self.project_designer();
        }
    }

    fn designer_released(&self) {
        let moved = {
            let mut s = self.session.borrow_mut();
            s.designer.design.end_drag();
            std::mem::take(&mut s.designer.dragged)
        };
        if moved {
            self.project_designer();
        }
    }

    // -- Preview and apply ------------------------------------------------------------

    fn designer_preview(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let st = w.global::<DesignerState>();
        let Some((_, driver, schema)) = self.designer_target() else {
            return;
        };
        let (sql, problems) = {
            let s = self.session.borrow();
            match s.designer.design.plan(&driver, &schema) {
                Ok(plan) => (script(&plan), String::new()),
                Err(errors) => (String::new(), errors.join("\n")),
            }
        };
        st.set_preview_sql(sql.into());
        st.set_preview_problems(problems.into());
        // A fresh preview is not a report of an earlier apply.
        st.set_results(designer_view::result_rows(&[]));
        st.set_results_summary("".into());
        st.set_preview_open(true);
    }

    fn designer_apply(self: &Rc<Self>) {
        let Some((connection, driver, schema)) = self.designer_target() else {
            return;
        };
        if self.session.borrow().designer.applying {
            return;
        }
        let plan = {
            let s = self.session.borrow();
            s.designer.design.plan(&driver, &schema)
        };
        let plan = match plan {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => {
                self.toast(
                    "Every table in the design has already been created.",
                    Tone::Neutral,
                );
                return;
            }
            Err(errors) => {
                // Show why, in the preview.
                self.designer_preview();
                self.toast(
                    format!("The design has problems: {}", errors[0]),
                    Tone::Danger,
                );
                return;
            }
        };
        let Some(d) = self.open_driver(&connection) else {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        self.session.borrow_mut().designer.applying = true;
        self.project_designer();

        runtime::spawn(self.ui_id, apply_plan(d, plan), move |results| {
            with_ctx(|ctx| ctx.designer_applied(results))
        });
    }

    fn designer_applied(self: &Rc<Self>, results: Vec<TableResult>) {
        let created = results.iter().any(|r| r.outcome == Outcome::Created);
        let summary = results_summary(&results);
        let failed = results
            .iter()
            .any(|r| matches!(r.outcome, Outcome::Failed(_)));
        {
            let mut s = self.session.borrow_mut();
            let dz = &mut s.designer;
            dz.applying = false;
            dz.design.set_results(results.clone());
            dz.design.mark_created();
        }
        self.designer_rebuild();
        if let Some(w) = self.window.upgrade() {
            let st = w.global::<DesignerState>();
            st.set_preview_sql("".into());
            st.set_preview_problems("".into());
            st.set_results(designer_view::result_rows(&results));
            st.set_results_summary(summary.as_str().into());
            st.set_preview_open(true);
        }
        if created {
            // The new tables belong in the sidebar, and the diagram is stale.
            self.invalidate_erd();
            self.load_tables();
        }
        self.toast(
            format!("Schema designer: {}", summary),
            if failed { Tone::Danger } else { Tone::Success },
        );
    }
}

fn strings(items: &[String]) -> slint::ModelRc<slint::SharedString> {
    slint::ModelRc::new(slint::VecModel::from(
        items
            .iter()
            .map(|s| slint::SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    ))
}

/// Run the plan in order. A table that fails is reported and its dependants
/// are skipped; independent tables carry on (there is no transaction across
/// tables, as in the web designer).
async fn apply_plan(
    d: std::sync::Arc<dyn dbhive_core::drivers::DatabaseDriver>,
    plan: Vec<PlannedTable>,
) -> Vec<TableResult> {
    let mut results = Vec::with_capacity(plan.len());
    let mut not_created: HashSet<String> = HashSet::new();
    for t in plan {
        if let Some(why) = skip_reason(&t, &not_created) {
            not_created.insert(t.name.clone());
            results.push(TableResult {
                name: t.name,
                outcome: Outcome::Skipped(why),
            });
            continue;
        }
        let mut outcome = Outcome::Created;
        for stmt in &t.statements {
            if let Err(e) = d.execute_query(stmt).await {
                outcome = Outcome::Failed(e.to_string());
                break;
            }
        }
        if outcome != Outcome::Created {
            not_created.insert(t.name.clone());
        }
        results.push(TableResult {
            name: t.name,
            outcome,
        });
    }
    results
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let st = window.global::<DesignerState>();
    st.set_type_labels(strings(&type_labels()));
    st.set_action_labels(strings(&action_labels()));

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            st.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_designer());
    on!(on_close, |c| c.hide_designer());
    on!(on_open_erd, |c| {
        c.hide_designer();
        c.open_erd();
    });
    on!(on_canvas_ready, |c| c.project_designer());

    on!(on_add_table, |c| {
        c.designer_edit(|d| {
            d.add_table();
        });
        let sel = c.session.borrow().designer.design.selected_index();
        if let Some(i) = sel {
            c.designer_reveal(i);
        }
    });
    on!(on_remove_table, |c| c
        .designer_edit_table(|d, id| d.remove_table(id)));
    on!(on_table_name_edited, |c, t| c
        .designer_edit_table(|d, id| d.rename_table(id, &t)));
    on!(on_table_schema_edited, |c, t| c
        .designer_edit_table(|d, id| d.set_table_schema(id, &t)));
    on!(on_add_column, |c| c.designer_edit_table(|d, id| {
        d.add_column(id);
    }));
    on!(on_remove_column, |c| c
        .designer_edit_column(|d, id, col| d.remove_column(id, col)));
    on!(on_column_selected, |c, i| {
        c.session
            .borrow_mut()
            .designer
            .design
            .select_column(Some(i.max(0) as usize));
        c.project_designer();
    });
    on!(on_col_name_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.rename_column(id, col, &t)
    ));
    on!(on_col_type_picked, |c, i| c.designer_edit_column(
        |d, id, col| d.set_column_type(id, col, i.max(0) as usize)
    ));
    on!(on_col_length_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.set_column_length(id, col, &t)
    ));
    on!(on_col_precision_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.set_column_precision(id, col, &t)
    ));
    on!(on_col_scale_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.set_column_scale(id, col, &t)
    ));
    on!(on_col_custom_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.set_column_custom(id, col, &t)
    ));
    on!(on_col_default_edited, |c, t| c.designer_edit_column(
        |d, id, col| d.set_column_default(id, col, &t)
    ));
    on!(on_toggle_pk, |c| c.designer_edit_column(
        |d, id, col| d.toggle_primary_key(id, col)
    ));
    on!(on_toggle_nullable, |c| c.designer_edit_column(
        |d, id, col| d.toggle_nullable(id, col)
    ));
    on!(on_toggle_unique, |c| c
        .designer_edit_column(|d, id, col| d.toggle_unique(id, col)));
    on!(on_toggle_auto, |c| c.designer_edit_column(
        |d, id, col| d.toggle_auto_increment(id, col)
    ));
    on!(on_ref_table_picked, |c, i| c.designer_edit_column(
        |d, id, col| {
            // Index 0 is "(none)".
            if i <= 0 {
                d.clear_reference(id, col);
            } else if let Some(t) = d.reference_table_options().get(i as usize - 1).cloned() {
                d.set_reference_table(id, col, &t);
            }
        }
    ));
    on!(on_ref_column_picked, |c, i| c.designer_edit_column(
        |d, id, col| {
            let table = d
                .tables
                .iter()
                .find(|t| t.id == id)
                .and_then(|t| t.columns.get(col))
                .map(|c| c.ref_table.clone())
                .unwrap_or_default();
            if let Some(name) = d
                .reference_column_options(&table)
                .get(i.max(0) as usize)
                .cloned()
            {
                d.set_reference_column(id, col, &name);
            }
        }
    ));
    on!(on_on_delete_picked, |c, i| c.designer_edit_column(
        |d, id, col| d.set_on_delete(id, col, i.max(0) as usize)
    ));

    on!(on_auto_layout, |c| {
        c.designer_edit(|d| d.auto_layout());
        c.designer_fit();
        c.project_designer();
    });
    on!(on_fit, |c| {
        c.designer_fit();
        c.project_designer();
    });
    on!(on_zoom_in, |c| {
        let (vw, vh) = c.designer_viewport();
        c.designer_apply_view(c.designer_view().step(true, vw, vh));
    });
    on!(on_zoom_out, |c| {
        let (vw, vh) = c.designer_viewport();
        c.designer_apply_view(c.designer_view().step(false, vw, vh));
    });
    on!(on_toggle_snap, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.designer.design.snap = !s.designer.design.snap;
        }
        c.project_designer();
    });
    on!(on_wheel, |c, d, x, y| c
        .designer_apply_view(c.designer_view().wheel(d, x, y)));
    on!(on_node_dragged, |c, i, dx, dy| c.designer_dragged(
        i.max(0) as usize,
        dx,
        dy
    ));
    on!(on_node_clicked, |c, i| c
        .designer_select(Some(i.max(0) as usize)));
    on!(on_node_released, |c, _i| c.designer_released());
    on!(on_background_clicked, |c| c.designer_select(None));
    on!(on_minimap_navigate, |c, x, y| {
        let (vw, vh) = c.designer_viewport();
        let map = c.session.borrow().designer.map;
        if let Some(m) = map {
            let w = m.to_world(x, y);
            c.designer_apply_view(c.designer_view().center_on(w.x, w.y, vw, vh));
        }
    });

    on!(on_preview, |c| c.designer_preview());
    on!(on_preview_close, |c| {
        if let Some(w) = c.window.upgrade() {
            w.global::<DesignerState>().set_preview_open(false);
        }
    });
    on!(on_apply, |c| c.designer_apply());
}
