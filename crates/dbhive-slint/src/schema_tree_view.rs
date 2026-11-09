//! Maps the schema tree's plain view (`dbhive_ui_model::schema_tree`) to the
//! Slint structs it draws, and pushes it into `TreeState`.
//!
//! Kept apart from the controller so the snapshot tool projects exactly what
//! the app does, with no controller behind it.

use dbhive_ui_model::routine_list::{RoutineRow, RoutineStatus};
use dbhive_ui_model::routine_run::RunView;
use dbhive_ui_model::schema_tree::{
    CreateDatabaseDialog, DropDialog, MenuEntry, PanelStatus, RowKind, TreeRow, TreeView,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    AppWindow, CtxItem, TreeNode, TreeRoutine, TreeRoutineArg, TreeRowKind, TreeState, TreeStatus,
};

fn kind(k: RowKind) -> TreeRowKind {
    match k {
        RowKind::Database => TreeRowKind::Database,
        RowKind::Schema => TreeRowKind::Schema,
        RowKind::Table => TreeRowKind::Table,
        RowKind::View => TreeRowKind::View,
        RowKind::Loading => TreeRowKind::Loading,
        RowKind::Error => TreeRowKind::Error,
        RowKind::Empty => TreeRowKind::Empty,
    }
}

fn node(r: &TreeRow) -> TreeNode {
    TreeNode {
        kind: kind(r.kind),
        depth: i32::from(r.depth),
        label: r.label.as_str().into(),
        detail: r.detail.as_str().into(),
        expandable: r.expandable,
        expanded: r.expanded,
        active: r.active,
        dim: r.dim,
        busy: r.busy,
    }
}

fn status(s: PanelStatus) -> TreeStatus {
    match s {
        PanelStatus::Loading => TreeStatus::Loading,
        PanelStatus::Error => TreeStatus::Error,
        PanelStatus::Empty => TreeStatus::Empty,
        PanelStatus::NoMatches => TreeStatus::NoMatches,
        PanelStatus::Populated => TreeStatus::Populated,
    }
}

/// Put `nodes` in the rows model, changing it in place.
///
/// A fresh model every time would destroy every row's element, and a click
/// that redraws the tree would destroy the row it was delivered to before
/// Slint could deliver the second half of a double click to it. So the model
/// is kept, and only the rows that differ are written.
fn sync_rows(s: &TreeState<'_>, nodes: Vec<TreeNode>) {
    let current = s.get_rows();
    let Some(model) = current.as_any().downcast_ref::<VecModel<TreeNode>>() else {
        s.set_rows(ModelRc::new(VecModel::from(nodes)));
        return;
    };
    for (i, n) in nodes.iter().enumerate() {
        if i < model.row_count() {
            if model.row_data(i).as_ref() != Some(n) {
                model.set_row_data(i, n.clone());
            }
        } else {
            model.push(n.clone());
        }
    }
    while model.row_count() > nodes.len() {
        model.remove(model.row_count() - 1);
    }
}

/// Draw the tree: its rows, which panel state it is in, and the cursor.
pub fn apply(w: &AppWindow, view: &TreeView, can_create: bool) {
    let s = w.global::<TreeState>();
    sync_rows(&s, view.rows.iter().map(node).collect());
    s.set_status(status(view.status));
    s.set_error(view.error.as_str().into());
    s.set_cursor(view.cursor.map_or(-1, |i| i as i32));
    s.set_table_count(view.table_count as i32);
    s.set_can_create(can_create);
}

pub fn menu_items(entries: &[MenuEntry]) -> ModelRc<CtxItem> {
    ModelRc::new(VecModel::from(
        entries
            .iter()
            .map(|e| CtxItem {
                label: e.label.into(),
                icon: e.icon.into(),
                enabled: e.enabled,
                danger: e.danger,
                divider: e.divider_above,
            })
            .collect::<Vec<_>>(),
    ))
}

/// Show the menu at a window position.
pub fn show_menu(w: &AppWindow, entries: &[MenuEntry], x: f32, y: f32) {
    let s = w.global::<TreeState>();
    s.set_menu_items(menu_items(entries));
    s.set_menu_x(x);
    s.set_menu_y(y);
    s.set_menu_visible(true);
}

/// Draw the drop-table confirmation. `sql` is the statement it would run.
pub fn apply_drop(w: &AppWindow, d: &DropDialog, sql: &str) {
    let s = w.global::<TreeState>();
    s.set_drop_table(
        dbhive_ui_model::table_editor::qualified_display(&d.qualifier, &d.table).into(),
    );
    s.set_drop_warning(d.warning().into());
    s.set_drop_checking(d.dependents == dbhive_ui_model::schema_tree::Dependents::Loading);
    s.set_drop_offers_cascade(d.cascade_offered());
    s.set_drop_cascade(d.cascade);
    s.set_drop_sql(sql.into());
    s.set_drop_hint(d.typed_hint().into());
    if s.get_drop_typed() != d.typed.as_str() {
        s.set_drop_typed(d.typed.as_str().into());
    }
    s.set_drop_error(d.error.as_str().into());
    s.set_drop_running(d.running);
    s.set_drop_can_drop(d.can_drop());
    s.set_drop_visible(true);
}

/// Draw the create-database dialog.
pub fn apply_create_db(w: &AppWindow, d: &CreateDatabaseDialog) {
    let s = w.global::<TreeState>();
    s.set_create_db_sql(d.sql().into());
    s.set_create_db_error(d.error().into());
    s.set_create_db_running(d.running);
    s.set_create_db_can_create(d.can_create());
    if s.get_create_db_name() != d.name.as_str() {
        s.set_create_db_name(d.name.as_str().into());
    }
    s.set_create_db_visible(true);
}

/// Everything the Procedures & Functions panel shows.
pub struct RoutinesView {
    pub status: RoutineStatus,
    pub rows: Vec<RoutineRow>,
    pub count: usize,
    pub error: String,
    pub message: String,
    pub picked: Option<PickedRoutine>,
    pub def_loading: bool,
    pub def_error: String,
}

/// The routine whose source is on show.
pub struct PickedRoutine {
    pub title: String,
    pub signature: String,
    pub meta: String,
    pub definition: String,
}

fn routine_status(s: RoutineStatus) -> TreeStatus {
    match s {
        RoutineStatus::Loading => TreeStatus::Loading,
        RoutineStatus::Error => TreeStatus::Error,
        RoutineStatus::Empty | RoutineStatus::Unsupported => TreeStatus::Empty,
        RoutineStatus::NoMatches => TreeStatus::NoMatches,
        RoutineStatus::Populated => TreeStatus::Populated,
    }
}

/// Draw the Procedures & Functions panel. Rows are updated in place, like the
/// tree's, so a click does not destroy the row it landed on.
pub fn apply_routines(w: &AppWindow, v: &RoutinesView) {
    let s = w.global::<TreeState>();
    let nodes: Vec<TreeRoutine> = v
        .rows
        .iter()
        .map(|r| TreeRoutine {
            header: r.header,
            label: r.label.as_str().into(),
            detail: r.detail.as_str().into(),
            kind: r.kind.into(),
            selected: r.selected,
        })
        .collect();
    let current = s.get_routines();
    match current.as_any().downcast_ref::<VecModel<TreeRoutine>>() {
        Some(model) => {
            for (i, n) in nodes.iter().enumerate() {
                if i < model.row_count() {
                    if model.row_data(i).as_ref() != Some(n) {
                        model.set_row_data(i, n.clone());
                    }
                } else {
                    model.push(n.clone());
                }
            }
            while model.row_count() > nodes.len() {
                model.remove(model.row_count() - 1);
            }
        }
        None => s.set_routines(ModelRc::new(VecModel::from(nodes))),
    }
    s.set_routines_status(routine_status(v.status));
    s.set_routines_count(v.count as i32);
    s.set_routines_error(v.error.as_str().into());
    s.set_routines_message(v.message.as_str().into());
    s.set_routine_picked(v.picked.is_some());
    let p = v.picked.as_ref();
    s.set_routine_title(p.map_or("", |p| p.title.as_str()).into());
    s.set_routine_signature(p.map_or("", |p| p.signature.as_str()).into());
    s.set_routine_meta(p.map_or("", |p| p.meta.as_str()).into());
    s.set_routine_definition(p.map_or("", |p| p.definition.as_str()).into());
    s.set_routine_def_loading(v.def_loading);
    s.set_routine_def_error(v.def_error.as_str().into());
}

/// Draw the routine run form, or clear it when no routine is picked.
///
/// The boxes hold what the user typed, so while the same routine is on show
/// the rows are updated in place (a new model would rebuild every box under
/// the user's fingers). `fresh` says the form belongs to a newly picked
/// routine: then the model is replaced, so no box keeps the last routine's
/// text.
pub fn apply_run_form(w: &AppWindow, view: Option<&RunView>, fresh: bool) {
    let s = w.global::<TreeState>();
    let rows: Vec<TreeRoutineArg> = view
        .map(|v| {
            v.fields
                .iter()
                .map(|f| TreeRoutineArg {
                    label: f.label.as_str().into(),
                    hint: f.hint.as_str().into(),
                    value: f.value.as_str().into(),
                    error: f.error.as_str().into(),
                    input: f.input,
                })
                .collect()
        })
        .unwrap_or_default();
    let current = s.get_run_args();
    match current.as_any().downcast_ref::<VecModel<TreeRoutineArg>>() {
        Some(model) if !fresh && model.row_count() == rows.len() => {
            for (i, r) in rows.into_iter().enumerate() {
                if model.row_data(i).as_ref() != Some(&r) {
                    model.set_row_data(i, r);
                }
            }
        }
        _ => s.set_run_args(ModelRc::new(VecModel::from(rows))),
    }
    s.set_run_sql(view.map_or("", |v| v.sql.as_str()).into());
    s.set_run_can_run(view.is_some_and(|v| v.can_run));
    s.set_run_confirming(view.is_some_and(|v| v.confirming));
    s.set_run_label(view.map_or("Run", |v| v.run_label.as_str()).into());
    s.set_run_confirm_text(view.map_or("", |v| v.confirm_text.as_str()).into());
}
