//! Maps the schema designer's session (`dbhive_ui_model::designer_session`)
//! onto the canvas's node and link types and the panel's row structs. Kept
//! apart from the controller so the snapshot tool builds the same pictures
//! without a controller behind them.

use dbhive_ui_model::designer_session::{
    action_labels, type_labels, Designer, Outcome, TableResult,
};
use dbhive_ui_model::erd_scene::{minimap, zoom_label, MiniMap, HEADER_H, ROW_H};
use slint::{ModelRc, SharedString, VecModel};

use crate::diagram_model::{Diagram, Link, Node, Row};
use crate::{DesignerColumnRow, DesignerResultRow, DesignerState};

/// One canvas node per designed table.
pub fn nodes(d: &Designer) -> Vec<Node> {
    (0..d.len())
        .map(|i| {
            let r = d.rect(i);
            Node {
                title: d.title(i),
                columns: Vec::new(),
                rows: d
                    .rows(i)
                    .into_iter()
                    .map(|row| Row {
                        name: row.name,
                        detail: row.data_type,
                        badge: row.badge.label().to_string(),
                    })
                    .collect(),
                more: String::new(),
                tag: if d.is_created(i) {
                    "created".to_string()
                } else {
                    String::new()
                },
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                header: HEADER_H,
                row_height: ROW_H,
                detail_width: d.type_width(i),
                junction: d.is_created(i),
                emphasis: false,
                dimmed: false,
            }
        })
        .collect()
}

/// One link per resolved foreign key, referenced table to referencing table.
pub fn links(d: &Designer) -> Vec<Link> {
    d.links()
        .into_iter()
        .map(|l| Link {
            from: l.parent,
            to: l.child,
            dashed: false,
            label: l.label,
        })
        .collect()
}

/// The minimap's largest size.
pub const MAP_MAX: (f32, f32) = (180.0, 120.0);

fn strings(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|s| SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    ))
}

/// Set a text property only when it differs, so typing does not move the caret.
fn set_if_changed(current: SharedString, new: &str, set: impl FnOnce(SharedString)) {
    if current != new {
        set(new.into());
    }
}

/// Everything the `DesignerState` global shows, from the design and the
/// canvas models. Shared by the controller and the snapshot tool, so a
/// snapshot is the picture the app draws. Returns the minimap's mapping.
pub fn project(
    st: &DesignerState<'_>,
    d: &Designer,
    diagram: &Diagram,
    zoom: f32,
    applying: bool,
    location: &str,
) -> Option<MiniMap> {
    let n = d.len();
    st.set_subtitle(
        if n == 0 {
            location.to_string()
        } else {
            format!(
                "{} \u{b7} {} table{}",
                location,
                n,
                if n == 1 { "" } else { "s" }
            )
        }
        .into(),
    );
    st.set_empty(d.is_empty());
    st.set_applying(applying);
    st.set_snap(d.snap);
    st.set_nodes(diagram.node_model());
    st.set_edges(diagram.edge_model());
    st.set_zoom(zoom);
    st.set_zoom_label(zoom_label(zoom).into());

    // Problems, listed rather than hidden.
    let errors = d.errors();
    st.set_problems(
        match errors.len() {
            0 => String::new(),
            1 => errors[0].clone(),
            k => format!(
                "{}\n(and {} more; Preview SQL lists them all)",
                errors[0],
                k - 1
            ),
        }
        .into(),
    );
    let pending = d.pending_count();
    st.set_can_apply(pending > 0 && errors.is_empty());
    st.set_apply_label(
        match pending {
            0 => "Nothing to create".to_string(),
            1 => "Create 1 table".to_string(),
            k => format!("Create {} tables", k),
        }
        .into(),
    );

    let map = d.bounds().map(|b| minimap(b, MAP_MAX.0, MAP_MAX.1));
    if let Some(m) = &map {
        st.set_map_scale(m.scale);
        st.set_map_origin_x(m.origin.x);
        st.set_map_origin_y(m.origin.y);
        st.set_map_width(m.width);
        st.set_map_height(m.height);
    }

    // The selected table and column.
    st.set_type_labels(strings(&type_labels()));
    st.set_action_labels(strings(&action_labels()));
    let ti = d.selected_index();
    st.set_has_selection(ti.is_some());
    let Some(ti) = ti else {
        st.set_has_column(false);
        return map;
    };
    set_if_changed(st.get_table_name(), &d.tables[ti].name, |v| {
        st.set_table_name(v)
    });
    set_if_changed(st.get_table_schema(), &d.tables[ti].schema, |v| {
        st.set_table_schema(v)
    });
    st.set_columns(column_rows(d, ti));

    let detail = d.selected_column().and_then(|c| d.column_detail(ti, c));
    st.set_has_column(detail.is_some());
    let Some(c) = detail else { return map };
    set_if_changed(st.get_col_name(), &c.name, |v| st.set_col_name(v));
    st.set_col_type_label(c.type_label.as_str().into());
    st.set_col_takes_length(c.takes_length);
    set_if_changed(st.get_col_length(), &c.length, |v| st.set_col_length(v));
    st.set_col_takes_precision(c.takes_precision);
    set_if_changed(st.get_col_precision(), &c.precision, |v| {
        st.set_col_precision(v)
    });
    set_if_changed(st.get_col_scale(), &c.scale, |v| st.set_col_scale(v));
    st.set_col_takes_custom(c.takes_custom);
    set_if_changed(st.get_col_custom(), &c.custom, |v| st.set_col_custom(v));
    set_if_changed(st.get_col_default(), &c.default, |v| st.set_col_default(v));
    st.set_col_nullable(c.nullable);
    st.set_col_pk(c.primary_key);
    st.set_col_unique(c.unique);
    st.set_col_auto(c.auto_increment);
    st.set_col_can_auto(c.can_auto_increment);
    st.set_col_has_ref(c.has_reference);

    let mut tables = vec!["(none)".to_string()];
    tables.extend(c.ref_tables.iter().cloned());
    st.set_ref_table_options(strings(&tables));
    st.set_ref_table_value(
        if c.ref_table.trim().is_empty() {
            "(none)".to_string()
        } else {
            c.ref_table.clone()
        }
        .into(),
    );
    st.set_ref_column_options(strings(&c.ref_columns));
    st.set_ref_column_enabled(!c.ref_columns.is_empty());
    st.set_ref_column_value(
        if !c.ref_column.trim().is_empty() {
            c.ref_column.clone()
        } else if c.ref_table.trim().is_empty() {
            "(column)".to_string()
        } else if c.ref_columns.is_empty() {
            "no key columns".to_string()
        } else {
            "(choose)".to_string()
        }
        .into(),
    );
    let actions = action_labels();
    st.set_action_label(
        actions[c.on_delete_index.min(actions.len() - 1)]
            .as_str()
            .into(),
    );
    st.set_ref_problem(c.problem.as_str().into());
    map
}

/// The selected table's column list.
pub fn column_rows(d: &Designer, table: usize) -> ModelRc<DesignerColumnRow> {
    let selected = d.selected_column();
    let rows: Vec<DesignerColumnRow> = d
        .rows(table)
        .into_iter()
        .enumerate()
        .map(|(c, r)| DesignerColumnRow {
            name: r.name.as_str().into(),
            type_label: r.data_type.as_str().into(),
            badge: r.badge.label().into(),
            selected: selected == Some(c),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// The per-table report of an apply.
pub fn result_rows(results: &[TableResult]) -> ModelRc<DesignerResultRow> {
    ModelRc::new(VecModel::from(
        results
            .iter()
            .map(|r| {
                let (state, message) = match &r.outcome {
                    Outcome::Created => (0, "Created".to_string()),
                    Outcome::Failed(m) => (1, m.clone()),
                    Outcome::Skipped(m) => (2, format!("Skipped: {}", m)),
                };
                DesignerResultRow {
                    name: r.name.as_str().into(),
                    state,
                    message: message.into(),
                }
            })
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    fn design() -> Designer {
        let mut d = Designer::new();
        let u = d.add_table();
        d.rename_table(u, "users");
        let o = d.add_table();
        d.rename_table(o, "orders");
        let c = d.add_column(o).unwrap();
        d.rename_column(o, c, "user_id");
        d.set_reference_table(o, c, "users");
        d
    }

    #[test]
    fn nodes_and_links_follow_the_design() {
        let d = design();
        let n = nodes(&d);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].title, "orders");
        assert_eq!(n[1].rows[0].badge, "PK");
        assert_eq!(n[1].rows[1].badge, "FK");
        let l = links(&d);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].from, l[0].to), (0, 1), "users is the parent");
    }

    #[test]
    fn the_column_list_marks_the_chosen_column() {
        let mut d = design();
        d.select_index(Some(1));
        d.select_column(Some(1));
        let rows = column_rows(&d, 1);
        assert_eq!(rows.row_count(), 2);
        assert!(!rows.row_data(0).unwrap().selected);
        assert!(rows.row_data(1).unwrap().selected);
        assert_eq!(rows.row_data(1).unwrap().name, "user_id");
    }

    #[test]
    fn results_map_to_states_with_messages() {
        let rows = result_rows(&[
            TableResult {
                name: "a".into(),
                outcome: Outcome::Created,
            },
            TableResult {
                name: "b".into(),
                outcome: Outcome::Failed("no such table".into()),
            },
            TableResult {
                name: "c".into(),
                outcome: Outcome::Skipped("b was not created".into()),
            },
        ]);
        let states: Vec<i32> = (0..3).map(|i| rows.row_data(i).unwrap().state).collect();
        assert_eq!(states, vec![0, 1, 2]);
        assert_eq!(rows.row_data(1).unwrap().message, "no such table");
        assert_eq!(
            rows.row_data(2).unwrap().message,
            "Skipped: b was not created"
        );
    }

    #[test]
    fn a_created_table_is_tagged() {
        let mut d = design();
        d.set_results(vec![TableResult {
            name: "users".into(),
            outcome: Outcome::Created,
        }]);
        d.mark_created();
        let n = nodes(&d);
        assert_eq!(n[0].tag, "created");
        assert_eq!(n[1].tag, "");
    }
}
