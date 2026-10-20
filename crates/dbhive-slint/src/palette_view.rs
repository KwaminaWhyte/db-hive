//! The command palette and shortcuts help, projected into the window.
//!
//! The state and the rows are `dbhive_ui_model::palette_session`; this turns
//! rows into Slint structs and writes them to the window. The snapshot tool
//! calls the same functions, so what it renders is what the app shows.

use dbhive_ui_model::command_palette::IS_MAC;
use dbhive_ui_model::palette_session::{
    PaletteRow, PaletteView, ShortcutRow, HEADER_HEIGHT, LIST_MAX_HEIGHT, ROW_HEIGHT,
};
use slint::{Model, ModelRc, SharedString, VecModel};

use crate::{AppWindow, PaletteItem, ShortcutItem};

fn keys(keys: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        keys.iter()
            .map(|k| SharedString::from(k.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn palette_item(row: &PaletteRow) -> PaletteItem {
    PaletteItem {
        header: row.header,
        id: row.slug.into(),
        // Headers are drawn in capitals; Slint has no text-transform.
        title: if row.header {
            row.title.to_uppercase()
        } else {
            row.title.clone()
        }
        .into(),
        description: row.description.into(),
        keys: keys(&row.keys),
        enabled: row.enabled,
        reason: row.reason.into(),
        selected: row.selected,
    }
}

fn shortcut_item(row: &ShortcutRow) -> ShortcutItem {
    ShortcutItem {
        header: row.header,
        title: if row.header {
            row.title.to_uppercase()
        } else {
            row.title.clone()
        }
        .into(),
        description: row.description.into(),
        keys: keys(&row.keys),
    }
}

/// Write `new` into `current`. When the list has the same shape, rows are
/// updated in place, so the rows under the pointer are not destroyed and
/// recreated by every arrow key or hover; otherwise the model is replaced.
fn replace_rows<T: Clone + PartialEq + 'static>(
    current: ModelRc<T>,
    new: Vec<T>,
) -> Option<ModelRc<T>> {
    if let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() {
        if model.row_count() == new.len() {
            for (i, row) in new.into_iter().enumerate() {
                if model.row_data(i).as_ref() != Some(&row) {
                    model.set_row_data(i, row);
                }
            }
            return None;
        }
    }
    Some(ModelRc::new(VecModel::from(new)))
}

/// Hand the window the row heights the scroll maths assumes.
pub fn set_metrics(w: &AppWindow) {
    w.set_palette_row_height(ROW_HEIGHT);
    w.set_palette_header_height(HEADER_HEIGHT);
}

/// Show or hide the palette and draw `view`. `query` is only written when it
/// differs, so the field's caret is left alone while typing.
pub fn apply_palette(w: &AppWindow, open: bool, query: &str, view: &PaletteView) {
    set_metrics(w);
    let items: Vec<PaletteItem> = view.rows.iter().map(palette_item).collect();
    if let Some(model) = replace_rows(w.get_palette_rows(), items) {
        w.set_palette_rows(model);
    }
    w.set_palette_list_height(view.content_height().min(LIST_MAX_HEIGHT));
    if w.get_palette_query() != query {
        w.set_palette_query(query.into());
    }
    w.set_show_palette(open);
}

/// Show or hide the shortcuts help and draw `rows`.
pub fn apply_shortcuts(w: &AppWindow, open: bool, query: &str, rows: &[ShortcutRow]) {
    let items: Vec<ShortcutItem> = rows.iter().map(shortcut_item).collect();
    if let Some(model) = replace_rows(w.get_shortcuts_rows(), items) {
        w.set_shortcuts_rows(model);
    }
    w.set_shortcuts_platform(if IS_MAC { "macOS" } else { "Windows / Linux" }.into());
    if w.get_shortcuts_query() != query {
        w.set_shortcuts_query(query.into());
    }
    w.set_show_shortcuts(open);
}
