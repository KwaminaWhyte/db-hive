//! Maps the table dialogs' plain view (`dbhive_ui_model::table_dialog`) to the
//! Slint structs they draw, and pushes it into `TableDialogState`.
//!
//! Kept apart from the controller so the snapshot tool projects exactly what
//! the app does, with no controller behind it.

use dbhive_ui_model::builder_editor::Change;
use dbhive_ui_model::table_dialog::{
    report_ok, report_summary, ChipView, DialogView, EditRowView, FkView, StatementReport,
    StatementStatus, UniqueView, WizardColumnView,
};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::builder_view::put;
use crate::history_view::strings_model;
use crate::{
    sql_highlight, AppWindow, TableDialogState, TdChip, TdColumn, TdEditRow, TdForeignKey,
    TdStatement, TdUnique,
};

fn chips(list: &[ChipView]) -> ModelRc<TdChip> {
    ModelRc::new(VecModel::from(
        list.iter()
            .map(|c| TdChip {
                label: c.label.as_str().into(),
                on: c.on,
            })
            .collect::<Vec<_>>(),
    ))
}

fn column(c: &WizardColumnView) -> TdColumn {
    TdColumn {
        name: c.name.as_str().into(),
        preset_index: c.preset_index,
        preset_label: c.preset_label.as_str().into(),
        show_param: c.show_param,
        param: c.param.as_str().into(),
        param_hint: c.param_hint.as_str().into(),
        param_error: c.param_error,
        primary_key: c.primary_key,
        auto_increment: c.auto_increment,
        auto_enabled: c.auto_enabled,
        nullable: c.nullable,
        nullable_enabled: c.nullable_enabled,
    }
}

fn unique(u: &UniqueView) -> TdUnique {
    TdUnique {
        name: u.name.as_str().into(),
        chips: chips(&u.chips),
    }
}

fn foreign_key(f: &FkView) -> TdForeignKey {
    TdForeignKey {
        name: f.name.as_str().into(),
        chips: chips(&f.chips),
        ref_table: f.ref_table.as_str().into(),
        ref_table_index: f.ref_table_index,
        ref_chips: chips(&f.ref_chips),
        ref_note: f.ref_note.as_str().into(),
        on_delete: f.on_delete.as_str().into(),
        on_update: f.on_update.as_str().into(),
    }
}

fn edit_row(r: &EditRowView) -> TdEditRow {
    TdEditRow {
        name: r.name.as_str().into(),
        type_label: r.type_label.as_str().into(),
        nullable: r.nullable,
        nullable_enabled: r.nullable_enabled,
        primary_key: r.primary_key,
        is_new: r.is_new,
        removed: r.removed,
        badges: strings_model(&r.badges),
        can_remove: r.can_remove,
        remove_note: r.remove_note.as_str().into(),
        name_enabled: r.name_enabled,
        type_pickable: r.type_pickable,
    }
}

pub fn statements(report: &[StatementReport]) -> Vec<TdStatement> {
    report
        .iter()
        .map(|r| {
            let (status, message) = match &r.status {
                StatementStatus::Done => (0, String::new()),
                StatementStatus::Failed(m) => (1, m.clone()),
                StatementStatus::Skipped => (2, String::new()),
            };
            TdStatement {
                sql: r.sql.as_str().into(),
                status,
                message: message.into(),
            }
        })
        .collect()
}

/// Push a view into the window's `TableDialogState`. As with the builder,
/// `Change::Text` updates rows where they stand so the focused field keeps its
/// caret; anything else rebuilds the lists.
pub fn apply(
    w: &AppWindow,
    v: &DialogView,
    change: Change,
    report: &[StatementReport],
    dark: bool,
) {
    let d = w.global::<TableDialogState>();
    let in_place = change == Change::Text;

    d.set_mode(v.mode);
    d.set_title(v.title.as_str().into());
    d.set_subtitle(v.subtitle.as_str().into());
    d.set_steps(strings_model(&v.steps));
    d.set_step(v.step);
    d.set_step_errors(strings_model(&v.step_errors));
    d.set_can_next(v.can_next);
    d.set_if_not_exists(v.if_not_exists);
    d.set_pending(v.pending as i32);

    // The two basics fields belong to the user while they type.
    if !in_place {
        d.set_name(v.name.as_str().into());
        d.set_description(v.description.as_str().into());
    }

    macro_rules! list {
        ($get:ident, $set:ident, $items:expr, $map:expr) => {
            if let Some(m) = put(d.$get(), $items.iter().map($map).collect(), in_place) {
                d.$set(m);
            }
        };
    }
    list!(get_columns, set_columns, v.columns, column);
    list!(get_uniques, set_uniques, v.uniques, unique);
    list!(
        get_foreign_keys,
        set_foreign_keys,
        v.foreign_keys,
        foreign_key
    );
    list!(get_edit_rows, set_edit_rows, v.edit_rows, edit_row);

    d.set_preset_labels(strings_model(&v.preset_labels));
    d.set_fk_tables(strings_model(&v.fk_tables));
    d.set_action_labels(strings_model(&v.action_labels));

    d.set_sql(v.sql.as_str().into());
    d.set_sql_lines(sql_highlight::to_model_for(&v.sql, dark));
    d.set_sql_error(v.sql_error.as_str().into());
    d.set_statement_count(v.statement_count as i32);

    d.set_results(ModelRc::new(VecModel::from(statements(report))));
    d.set_result_ok(report_ok(report));
    d.set_result_summary(if report.is_empty() {
        "".into()
    } else {
        report_summary(report).into()
    });
}
