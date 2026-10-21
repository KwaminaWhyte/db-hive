//! Maps the schema tree's plain view (`dbhive_ui_model::schema_tree`) to the
//! Slint structs it draws, and pushes it into `TreeState`.
//!
//! Kept apart from the controller so the snapshot tool projects exactly what
//! the app does, with no controller behind it.

use dbhive_ui_model::schema_tree::{
    DropDialog, MenuEntry, PanelStatus, RowKind, TreeRow, TreeView,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{AppWindow, CtxItem, TreeNode, TreeRowKind, TreeState, TreeStatus};

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
