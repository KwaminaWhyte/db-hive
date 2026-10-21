//! Controller glue for the ER diagram overlay.
//!
//! A child module of `app` so it can reach the controller's private state.
//! The scene (what each table shows, where it sits, zoom and pan maths, the
//! saved arrangement, SVG) is `dbhive_ui_model::erd_scene`; this file starts
//! the schema load, lands it by ticket, moves values between the scene, the
//! canvas models and the `ErdState` global, and writes the saved arrangement
//! and exported file off the UI thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dbhive_core::drivers::DatabaseDriver;
use dbhive_core::models::ForeignKeyInfo;
use dbhive_core::storage::JsonStore;
use dbhive_ui_model::er_diagram::{DiagramTable, Point};
use dbhive_ui_model::erd_scene::{
    export_file_name, fit, layout_key, positions_from_json, positions_to_json, zoom_label,
    ErdScene, Flow, MiniMap, View,
};
use dbhive_ui_model::tokens::{DARK, LIGHT};
use slint::ComponentHandle;

use super::{with_ctx, Ctx};
use crate::diagram_model::Diagram;
use crate::{erd_view, runtime, AppWindow, ErdState, Screen, Theme, Tone};

/// The file the arrangement of every schema is kept in, shared with the web
/// shell's format.
const LAYOUT_STORE: &str = "erd-layouts.json";

type Loaded = (Vec<DiagramTable>, Vec<ForeignKeyInfo>);

/// The diagram's own state. The pan lives in `ErdState` (the canvas writes it
/// on every pointer move); everything else is here.
pub(super) struct ErdSession {
    scene: Option<ErdScene>,
    diagram: Diagram,
    loading: bool,
    error: String,
    latest: runtime::Latest,
    /// Store key of the schema on screen: `<connection>-<schema>`.
    key: String,
    schema_label: String,
    /// Fit as soon as the canvas has a size (after a load).
    want_fit: bool,
    /// The pointer moved a table since it was pressed.
    dragged: bool,
    zoom: f32,
    map: Option<MiniMap>,
    exporting: bool,
    /// A path already refused once because the file exists: saving the same
    /// path again confirms the overwrite.
    overwrite_ok: Option<String>,
}

impl Default for ErdSession {
    fn default() -> Self {
        Self {
            scene: None,
            diagram: Diagram::new(Vec::new(), Vec::new()).with_flow(Flow::Vertical, true),
            loading: false,
            error: String::new(),
            latest: runtime::Latest::default(),
            key: String::new(),
            schema_label: String::new(),
            want_fit: false,
            dragged: false,
            zoom: 1.0,
            map: None,
            exporting: false,
            overwrite_ok: None,
        }
    }
}

/// Read every table's columns and the foreign keys, eight tables at a time.
///
/// A table whose columns cannot be read still appears, without its columns,
/// which is far better than failing the whole picture.
async fn load_schema(d: Arc<dyn DatabaseDriver>, schema: String) -> Result<Loaded, String> {
    let tables = d.get_tables(&schema).await.map_err(|e| e.to_string())?;
    let foreign_keys = d.get_foreign_keys(&schema).await.unwrap_or_default();

    let mut out: Vec<DiagramTable> = Vec::with_capacity(tables.len());
    for chunk in tables.chunks(8) {
        let mut set = tokio::task::JoinSet::new();
        for (i, t) in chunk.iter().enumerate() {
            let (d, schema, name) = (d.clone(), schema.clone(), t.name.clone());
            set.spawn(async move {
                let cols = d
                    .get_table_schema(&schema, &name)
                    .await
                    .map(|s| s.columns)
                    .unwrap_or_default();
                (
                    i,
                    DiagramTable {
                        name,
                        columns: cols,
                    },
                )
            });
        }
        let mut part: Vec<(usize, DiagramTable)> = Vec::with_capacity(chunk.len());
        while let Some(done) = set.join_next().await {
            if let Ok(item) = done {
                part.push(item);
            }
        }
        part.sort_by_key(|(i, _)| *i);
        out.extend(part.into_iter().map(|(_, t)| t));
    }
    Ok((out, foreign_keys))
}

/// Where exports go by default: the downloads folder if there is one.
fn export_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    match home {
        Some(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
        Some(h) => h,
        None => std::env::temp_dir(),
    }
}

/// `~/x` to the home directory.
fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(path)
}

impl Ctx {
    // -- Opening and closing ---------------------------------------------------

    /// Show the diagram for the workspace's schema, loading it if it is not
    /// already the one on screen.
    pub(super) fn open_erd(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if w.get_screen() != Screen::Workspace {
            self.toast("Open a connection to see its diagram.", Tone::Neutral);
            return;
        }
        let (connection, schema) = {
            let s = self.session.borrow();
            (
                s.workspace.connection_id.clone(),
                s.workspace.schema.clone(),
            )
        };
        let Some(schema) = schema else {
            self.toast(
                "The table list is still loading. Try again in a moment.",
                Tone::Neutral,
            );
            return;
        };
        self.hide_history_panel();
        self.hide_designer();

        let key = layout_key(&connection, &schema);
        let loaded = {
            let s = self.session.borrow();
            s.erd.key == key && s.erd.scene.is_some() && s.erd.error.is_empty()
        };
        w.global::<ErdState>().set_visible(true);
        if loaded {
            self.project_erd();
        } else {
            self.session.borrow_mut().erd.key = key;
            self.session.borrow_mut().erd.schema_label = schema;
            self.load_erd();
        }
    }

    /// The schema changed under the diagram (tables were created); the next
    /// open reads it again.
    pub(super) fn invalidate_erd(&self) {
        let mut s = self.session.borrow_mut();
        s.erd.scene = None;
        s.erd.key.clear();
    }

    pub(super) fn hide_erd(&self) {
        if let Some(w) = self.window.upgrade() {
            let st = w.global::<ErdState>();
            st.set_visible(false);
            st.set_export_open(false);
        }
    }

    /// Read the schema again (first open, Refresh, Retry).
    fn load_erd(self: &Rc<Self>) {
        let (connection, schema, key, ticket) = {
            let mut s = self.session.borrow_mut();
            let ticket = s.erd.latest.begin();
            s.erd.loading = true;
            s.erd.error.clear();
            (
                s.workspace.connection_id.clone(),
                s.erd.schema_label.clone(),
                s.erd.key.clone(),
                ticket,
            )
        };
        let Some(d) = self.open_driver(&connection) else {
            {
                let mut s = self.session.borrow_mut();
                s.erd.loading = false;
                s.erd.error = "This connection is no longer open.".to_string();
            }
            self.project_erd();
            return;
        };
        self.project_erd();

        runtime::spawn(
            async move {
                // The saved arrangement is a small file; reading it here keeps
                // the UI thread out of it.
                let saved: HashMap<String, Point> = JsonStore::open(LAYOUT_STORE)
                    .ok()
                    .and_then(|st| st.get(&key).map(positions_from_json))
                    .unwrap_or_default();
                (saved, load_schema(d, schema).await)
            },
            move |(saved, loaded)| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.erd.loading = false;
                        match loaded {
                            Ok((tables, fks)) => {
                                let scene = ErdScene::new(tables, fks, &saved);
                                s.erd.diagram.selected = None;
                                s.erd
                                    .diagram
                                    .replace(erd_view::nodes(&scene), erd_view::links(&scene));
                                s.erd.scene = Some(scene);
                                s.erd.error.clear();
                                s.erd.want_fit = true;
                            }
                            Err(e) => s.erd.error = format!("Could not read the schema: {e}"),
                        }
                    }
                    ctx.project_erd();
                    // The canvas may already have a size (a refresh reuses it).
                    ctx.erd_canvas_ready();
                })
            },
        );
    }

    // -- Projection ---------------------------------------------------------------

    /// Push the session into the `ErdState` global.
    pub(super) fn project_erd(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let st = w.global::<ErdState>();
        let mut s = self.session.borrow_mut();
        let erd = &mut s.erd;
        erd.map = erd_view::project(
            &st,
            erd.scene.as_ref(),
            &erd.diagram,
            erd.zoom,
            erd.loading,
            &erd.error,
            &erd.schema_label,
        );
    }

    fn erd_view(&self) -> View {
        let Some(w) = self.window.upgrade() else {
            return View::default();
        };
        let st = w.global::<ErdState>();
        View {
            zoom: self.session.borrow().erd.zoom,
            pan_x: st.get_pan_x(),
            pan_y: st.get_pan_y(),
        }
    }

    fn erd_viewport(&self) -> (f32, f32) {
        let Some(w) = self.window.upgrade() else {
            return (1000.0, 600.0);
        };
        let st = w.global::<ErdState>();
        let (vw, vh) = (st.get_viewport_width(), st.get_viewport_height());
        if vw > 1.0 && vh > 1.0 {
            (vw, vh)
        } else {
            (1000.0, 600.0)
        }
    }

    fn erd_apply_view(&self, v: View) {
        self.session.borrow_mut().erd.zoom = v.zoom;
        if let Some(w) = self.window.upgrade() {
            let st = w.global::<ErdState>();
            st.set_zoom(v.zoom);
            st.set_zoom_label(zoom_label(v.zoom).into());
            st.set_pan_x(v.pan_x);
            st.set_pan_y(v.pan_y);
        }
    }

    /// Show the whole diagram.
    fn erd_fit(&self) {
        let bounds = self
            .session
            .borrow()
            .erd
            .scene
            .as_ref()
            .and_then(|s| s.bounds());
        let Some(b) = bounds else { return };
        let (vw, vh) = self.erd_viewport();
        self.erd_apply_view(fit(b, vw, vh));
    }

    /// The canvas exists and knows its size.
    pub(super) fn erd_canvas_ready(&self) {
        let want = {
            let mut s = self.session.borrow_mut();
            std::mem::take(&mut s.erd.want_fit)
        };
        if want {
            let ready = self
                .window
                .upgrade()
                .is_some_and(|w| w.global::<ErdState>().get_viewport_width() > 1.0);
            if ready {
                self.erd_fit();
                self.project_erd();
            } else {
                // Not laid out yet: the canvas calls this again when it is.
                self.session.borrow_mut().erd.want_fit = true;
            }
        }
    }

    // -- Editing --------------------------------------------------------------------

    /// Run `f` on the scene, then rebuild the canvas models from it.
    fn erd_edit(&self, f: impl FnOnce(&mut ErdScene)) {
        {
            let mut s = self.session.borrow_mut();
            let erd = &mut s.erd;
            let Some(scene) = erd.scene.as_mut() else {
                return;
            };
            f(scene);
            erd.diagram.hide_edges = !scene.options.show_relationships;
            let (nodes, links) = (erd_view::nodes(scene), erd_view::links(scene));
            erd.diagram.replace(nodes, links);
        }
        self.project_erd();
    }

    fn erd_select(&self, i: Option<usize>) {
        {
            let mut s = self.session.borrow_mut();
            let erd = &mut s.erd;
            let Some(scene) = erd.scene.as_mut() else {
                return;
            };
            scene.selected = i.filter(|i| *i < scene.len());
            erd.diagram.select(scene.selected);
        }
        self.project_erd();
    }

    fn erd_dragged(&self, i: usize, dx: f32, dy: f32) {
        let selection_changed = {
            let mut s = self.session.borrow_mut();
            let erd = &mut s.erd;
            let zoom = erd.zoom.max(0.01);
            let Some(scene) = erd.scene.as_mut() else {
                return;
            };
            let Some(p) = scene.drag(i, dx / zoom, dy / zoom) else {
                return;
            };
            erd.dragged = true;
            erd.diagram.move_to(i, p.x, p.y);
            let changed = scene.selected != Some(i);
            if changed {
                scene.selected = Some(i);
                erd.diagram.select(Some(i));
            }
            changed
        };
        if selection_changed {
            self.project_erd();
        }
    }

    fn erd_released(self: &Rc<Self>) {
        let moved = {
            let mut s = self.session.borrow_mut();
            let moved = std::mem::take(&mut s.erd.dragged);
            if let Some(scene) = s.erd.scene.as_mut() {
                scene.end_drag();
            }
            moved
        };
        if moved {
            self.erd_persist();
            self.project_erd();
        }
    }

    /// Write the arrangement for this schema to `erd-layouts.json`.
    fn erd_persist(self: &Rc<Self>) {
        let (key, value, empty) = {
            let s = self.session.borrow();
            let Some(scene) = s.erd.scene.as_ref() else {
                return;
            };
            (
                s.erd.key.clone(),
                positions_to_json(scene.pinned()),
                scene.pinned().is_empty(),
            )
        };
        runtime::spawn(
            async move {
                let mut store = JsonStore::open(LAYOUT_STORE).map_err(|e| e.to_string())?;
                if empty {
                    store.delete(&key);
                } else {
                    store.set(key, value);
                }
                store.save().map_err(|e| e.to_string())
            },
            |saved| {
                with_ctx(|ctx| {
                    if let Err(e) = saved {
                        ctx.toast(
                            format!("Could not save the diagram layout: {e}"),
                            Tone::Danger,
                        );
                    }
                })
            },
        );
    }

    // -- Export ---------------------------------------------------------------------

    fn erd_export_open(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let st = w.global::<ErdState>();
        if st.get_export_path().is_empty() {
            let name = export_file_name(&self.session.borrow().erd.schema_label);
            st.set_export_path(export_dir().join(name).to_string_lossy().to_string().into());
        }
        st.set_export_message("".into());
        st.set_export_ok(false);
        st.set_export_open(true);
        self.session.borrow_mut().erd.overwrite_ok = None;
    }

    fn erd_export_save(self: &Rc<Self>, typed: &str) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let st = w.global::<ErdState>();
        let typed = typed.trim().to_string();
        let fail = |m: String| {
            st.set_export_ok(false);
            st.set_export_message(m.into());
        };
        if typed.is_empty() {
            return fail("Type a file path first.".into());
        }
        let path = expand_home(&typed);
        if path.is_dir() {
            return fail("That is a folder. Add a file name, such as diagram.svg.".into());
        }
        if path.exists()
            && self.session.borrow().erd.overwrite_ok.as_deref() != Some(typed.as_str())
        {
            self.session.borrow_mut().erd.overwrite_ok = Some(typed);
            return fail("That file already exists. Save again to replace it.".into());
        }
        let svg = {
            let s = self.session.borrow();
            let Some(scene) = s.erd.scene.as_ref() else {
                return;
            };
            scene.to_svg(if w.global::<Theme>().get_dark() {
                &DARK
            } else {
                &LIGHT
            })
        };
        {
            let mut s = self.session.borrow_mut();
            if s.erd.exporting {
                return;
            }
            s.erd.exporting = true;
        }
        st.set_exporting(true);
        st.set_export_message("".into());
        let shown = path.to_string_lossy().to_string();
        runtime::spawn(
            async move { std::fs::write(&path, svg).map_err(|e| e.to_string()) },
            move |written| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().erd.exporting = false;
                    ctx.session.borrow_mut().erd.overwrite_ok = None;
                    let Some(w) = ctx.window.upgrade() else {
                        return;
                    };
                    let st = w.global::<ErdState>();
                    st.set_exporting(false);
                    match written {
                        Ok(()) => {
                            st.set_export_ok(true);
                            st.set_export_message(format!("Saved {shown}").into());
                            ctx.toast("Diagram exported as SVG", Tone::Success);
                        }
                        Err(e) => {
                            st.set_export_ok(false);
                            st.set_export_message(format!("Could not save: {e}").into());
                        }
                    }
                })
            },
        );
    }
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let st = window.global::<ErdState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            st.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_erd());
    on!(on_close, |c| c.hide_erd());
    on!(on_retry, |c| c.load_erd());
    on!(on_refresh, |c| c.load_erd());
    on!(on_canvas_ready, |c| c.erd_canvas_ready());
    on!(on_auto_layout, |c| {
        c.erd_edit(|s| s.auto_layout());
        c.erd_persist();
        c.erd_fit();
        c.project_erd();
    });
    on!(on_fit, |c| {
        c.erd_fit();
        c.project_erd();
    });
    on!(on_zoom_in, |c| {
        let (vw, vh) = c.erd_viewport();
        c.erd_apply_view(c.erd_view().step(true, vw, vh));
    });
    on!(on_zoom_out, |c| {
        let (vw, vh) = c.erd_viewport();
        c.erd_apply_view(c.erd_view().step(false, vw, vh));
    });
    on!(on_wheel, |c, d, x, y| c
        .erd_apply_view(c.erd_view().wheel(d, x, y)));
    on!(on_node_dragged, |c, i, dx, dy| c.erd_dragged(
        i.max(0) as usize,
        dx,
        dy
    ));
    on!(on_node_clicked, |c, i| c
        .erd_select(Some(i.max(0) as usize)));
    on!(on_node_released, |c, _i| c.erd_released());
    on!(on_background_clicked, |c| c.erd_select(None));
    on!(on_minimap_navigate, |c, x, y| {
        let (vw, vh) = c.erd_viewport();
        let map = c.session.borrow().erd.map;
        if let Some(m) = map {
            let w = m.to_world(x, y);
            c.erd_apply_view(c.erd_view().center_on(w.x, w.y, vw, vh));
        }
    });
    on!(on_filter_edited, |c, t| c
        .erd_edit(|s| s.filter = t.to_string()));
    on!(on_toggle_columns, |c| c.erd_edit(|s| {
        let on = !s.options.show_columns;
        s.set_show_columns(on);
    }));
    on!(on_toggle_relationships, |c| c.erd_edit(|s| {
        s.options.show_relationships = !s.options.show_relationships
    }));
    on!(on_toggle_labels, |c| {
        c.erd_edit(|s| s.options.show_labels = !s.options.show_labels)
    });
    on!(on_toggle_snap, |c| c
        .erd_edit(|s| s.options.snap = !s.options.snap));
    on!(on_export_open_requested, |c| c.erd_export_open());
    on!(on_export_save, |c, p| c.erd_export_save(&p));
    on!(on_export_cancel, |c| {
        if let Some(w) = c.window.upgrade() {
            w.global::<ErdState>().set_export_open(false);
        }
    });
    on!(on_open_designer, |c| {
        c.hide_erd();
        c.open_designer();
    });
}
