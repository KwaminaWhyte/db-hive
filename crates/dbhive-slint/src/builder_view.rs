//! Maps the query builder's plain view (`dbhive_ui_model::builder_editor`) to
//! the Slint structs the builder draws, and pushes it into `BuilderState`.
//!
//! Kept apart from the controller so the snapshot tool projects exactly what
//! the app does, with no controller behind it.

use dbhive_ui_model::builder_editor::{
    BuilderView, Change, ColumnView, FilterView, HavingView, JoinView, OrderView, SectionView,
    SourceView,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::history_view::strings_model;
use crate::{
    sql_highlight, AppWindow, BuilderState, QbAdded, QbColumn, QbFilter, QbHaving, QbJoin, QbOrder,
    QbSection, QbSource,
};

fn section(s: &SectionView) -> QbSection {
    QbSection {
        title: s.title.into(),
        open: s.open,
        count: s.count as i32,
    }
}

fn source(s: &SourceView) -> QbSource {
    QbSource {
        alias: s.alias.as_str().into(),
        columns: strings_model(&s.columns),
        star_columns: strings_model(&s.star_columns),
    }
}

fn join(j: &JoinView) -> QbJoin {
    QbJoin {
        type_label: j.type_label.as_str().into(),
        type_index: j.type_index,
        left_alias: j.left_alias.as_str().into(),
        left_alias_index: j.left_alias_index,
        left_column: j.left_column.as_str().into(),
        left_column_index: j.left_column_index,
        right_alias: j.right_alias.as_str().into(),
        right_alias_index: j.right_alias_index,
        right_column: j.right_column.as_str().into(),
        right_column_index: j.right_column_index,
        takes_on: j.takes_on,
        warning: j.warning.as_str().into(),
        incomplete: j.incomplete,
    }
}

fn column(c: &ColumnView) -> QbColumn {
    QbColumn {
        label: c.label.as_str().into(),
        aggregate: c.aggregate.as_str().into(),
        alias: c.alias.as_str().into(),
        distinct: c.distinct,
        distinct_enabled: c.distinct_enabled,
    }
}

fn filter(f: &FilterView) -> QbFilter {
    QbFilter {
        kind: f.kind,
        depth: f.depth,
        group_operator: f.group_operator.as_str().into(),
        can_nest: f.can_nest,
        is_root: f.is_root,
        alias: f.alias.as_str().into(),
        alias_index: f.alias_index,
        column: f.column.as_str().into(),
        column_index: f.column_index,
        op_label: f.op_label.as_str().into(),
        ops: strings_model(&f.ops),
        arity: f.arity,
        is_bool: f.is_bool,
        value: f.value.as_str().into(),
        value2: f.value2.as_str().into(),
        placeholder: f.placeholder.as_str().into(),
        hint: f.hint.as_str().into(),
    }
}

fn having(h: &HavingView) -> QbHaving {
    QbHaving {
        aggregate: h.aggregate.as_str().into(),
        alias: h.alias.as_str().into(),
        alias_index: h.alias_index,
        column: h.column.as_str().into(),
        column_index: h.column_index,
        op_label: h.op_label.as_str().into(),
        value: h.value.as_str().into(),
        hint: h.hint.as_str().into(),
    }
}

fn order(o: &OrderView) -> QbOrder {
    QbOrder {
        label: o.label.as_str().into(),
        descending: o.descending,
    }
}

/// Replace a list's model, or, when only text changed and the length is the
/// same, update its rows where they stand. Replacing recreates every row's
/// widgets, which would drop the caret of the field being typed into.
///
/// Returns the new model to install, or `None` when the rows were updated in
/// place and the existing model stays.
pub fn put<T: Clone + 'static>(
    current: ModelRc<T>,
    rows: Vec<T>,
    in_place: bool,
) -> Option<ModelRc<T>> {
    if in_place {
        if let Some(vm) = current.as_any().downcast_ref::<VecModel<T>>() {
            if vm.row_count() == rows.len() {
                for (i, r) in rows.into_iter().enumerate() {
                    vm.set_row_data(i, r);
                }
                return None;
            }
        }
    }
    Some(ModelRc::new(VecModel::from(rows)))
}

/// Push a view into the window's `BuilderState`. `change` says how much of it
/// moved: `Structure` rebuilds the row lists, `Text` keeps them and updates
/// rows in place, so a field being typed into keeps its caret.
pub fn apply(w: &AppWindow, view: &BuilderView, change: Change, can_use: bool, dark: bool) {
    let b = w.global::<BuilderState>();
    let in_place = change == Change::Text;

    macro_rules! list {
        ($get:ident, $set:ident, $items:expr, $map:expr) => {
            if let Some(m) = put(b.$get(), $items.iter().map($map).collect(), in_place) {
                b.$set(m);
            }
        };
    }

    list!(get_sections, set_sections, view.sections, section);
    list!(get_sources, set_sources, view.sources, source);
    let aliases: Vec<String> = view.sources.iter().map(|s| s.alias.clone()).collect();
    b.set_aliases(strings_model(&aliases));
    list!(get_columns, set_columns, view.columns, column);
    list!(get_filters, set_filters, view.filters, filter);
    list!(get_having, set_having, view.having, having);
    list!(get_order_by, set_order_by, view.order_by, order);
    list!(get_joins, set_joins, view.joins, join);
    list!(get_added, set_added, view.added, |t| QbAdded {
        alias: t.alias.as_str().into(),
        detail: t.detail.as_str().into(),
    });

    // Lists of plain strings: cheap, and none hold a text field.
    b.set_catalog(strings_model(&view.catalog));
    b.set_catalog_truncated(view.catalog_truncated);
    b.set_join_types(strings_model(&view.join_type_labels));
    b.set_join_suggestions(strings_model(&view.join_suggestions));
    b.set_aggregates(strings_model(&view.aggregate_labels));
    b.set_having_aggregates(strings_model(&view.having_aggregate_labels));
    b.set_having_ops(strings_model(&view.having_op_labels));
    b.set_group_by(strings_model(&view.group_by));
    b.set_group_issues(strings_model(&view.group_issues));
    b.set_query_distinct(view.query_distinct);

    // `table-search`, `limit-text` and `offset-text` are two-way bound to
    // their fields and left alone while the user types; a reset or a fresh
    // open writes them explicitly (see `reset_fields`).
    b.set_limit_error(view.limit_error.as_str().into());
    b.set_offset_error(view.offset_error.as_str().into());
    b.set_pagination(view.pagination.as_str().into());

    b.set_sql(view.sql.as_str().into());
    b.set_sql_lines(sql_highlight::to_model_for(&view.sql, dark));
    b.set_warnings(strings_model(&view.warnings));
    b.set_can_use(can_use);
}

/// Write the three typed fields from the editor, for a reset or an open.
pub fn reset_fields(w: &AppWindow, view: &BuilderView) {
    let b = w.global::<BuilderState>();
    b.set_table_search(view.table_search.as_str().into());
    b.set_limit_text(view.limit_text.as_str().into());
    b.set_offset_text(view.offset_text.as_str().into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(items: &[i32]) -> ModelRc<i32> {
        ModelRc::new(VecModel::from(items.to_vec()))
    }

    #[test]
    fn typing_updates_rows_where_they_stand_and_keeps_the_model() {
        let current = model(&[1, 2, 3]);
        let replaced = put(current.clone(), vec![1, 20, 3], true);
        assert!(
            replaced.is_none(),
            "the existing model must stay, or the focused field is recreated"
        );
        assert_eq!(current.row_data(1), Some(20));
        assert_eq!(current.row_count(), 3);
    }

    #[test]
    fn a_structural_change_always_gets_a_fresh_model() {
        let current = model(&[1, 2, 3]);
        // Same length, but not typing: rows must be rebuilt so every field
        // is bound to the new state again.
        let fresh = put(current.clone(), vec![1, 2, 3], false).expect("a new model");
        assert!(
            current != fresh,
            "same model means the rows were not rebuilt"
        );
    }

    #[test]
    fn a_length_change_while_typing_gets_a_fresh_model() {
        let current = model(&[1, 2]);
        let fresh = put(current, vec![1, 2, 3], true).expect("a new model");
        assert_eq!(fresh.row_count(), 3);
    }

    #[test]
    fn a_model_that_is_not_a_vec_model_is_replaced() {
        let current: ModelRc<i32> = ModelRc::default();
        assert!(put(current, vec![1], true).is_some());
    }
}
