//! Maps the ER diagram's scene (`dbhive_ui_model::erd_scene`) onto the canvas's
//! node and link types. Kept apart from the controller so the snapshot tool
//! builds the same pictures without a controller behind them.

use dbhive_ui_model::erd_scene::{
    minimap, zoom_label, Cardinality, ErdScene, MiniMap, HEADER_H, ROW_H,
};

use crate::diagram_model::{Diagram, Link, Node, Row};
use crate::ErdState;

/// One canvas node per table, in scene order.
pub fn nodes(scene: &ErdScene) -> Vec<Node> {
    let filtering = scene.filtering();
    (0..scene.len())
        .map(|i| {
            let r = scene.rect(i);
            Node {
                title: scene.tables[i].name.clone(),
                columns: Vec::new(),
                rows: scene
                    .rows(i)
                    .into_iter()
                    .map(|row| Row {
                        name: row.name,
                        detail: row.data_type,
                        badge: row.badge.label().to_string(),
                    })
                    .collect(),
                more: scene.more_label(i),
                tag: if scene.is_junction(i) {
                    "M:N".to_string()
                } else {
                    String::new()
                },
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                header: HEADER_H,
                row_height: ROW_H,
                detail_width: scene.type_width(i),
                junction: scene.is_junction(i),
                emphasis: scene.matches(i),
                dimmed: filtering && !scene.matches(i),
            }
        })
        .collect()
}

/// The minimap's largest size.
pub const MAP_MAX: (f32, f32) = (180.0, 120.0);

/// Everything the `ErdState` global shows, from the scene and the canvas
/// models. Shared by the controller and the snapshot tool, so a snapshot is
/// the picture the app draws. Returns the minimap's mapping, which the
/// controller keeps to turn a click on it into a world point.
pub fn project(
    st: &ErdState<'_>,
    scene: Option<&ErdScene>,
    diagram: &Diagram,
    zoom: f32,
    loading: bool,
    error: &str,
    schema_label: &str,
) -> Option<MiniMap> {
    st.set_loading(loading);
    st.set_error(error.into());
    st.set_nodes(diagram.node_model());
    st.set_edges(diagram.edge_model());
    st.set_zoom(zoom);
    st.set_zoom_label(zoom_label(zoom).into());

    let Some(scene) = scene else {
        st.set_empty(false);
        st.set_subtitle("".into());
        st.set_selection_label("".into());
        return None;
    };
    st.set_empty(scene.is_empty() && !loading && error.is_empty());
    st.set_subtitle(format!("{} \u{b7} {}", schema_label, scene.summary()).into());
    st.set_show_columns(scene.options.show_columns);
    st.set_show_relationships(scene.options.show_relationships);
    st.set_show_labels(scene.options.show_labels);
    st.set_snap(scene.options.snap);
    // Typing into the field already updated it; rewriting would move the caret.
    if st.get_filter() != scene.filter.as_str() {
        st.set_filter(scene.filter.as_str().into());
    }
    st.set_selection_label(
        scene
            .selected
            .map(|i| scene.describe(i))
            .unwrap_or_default()
            .into(),
    );

    let map = scene.bounds().map(|b| minimap(b, MAP_MAX.0, MAP_MAX.1));
    if let Some(m) = &map {
        st.set_map_scale(m.scale);
        st.set_map_origin_x(m.origin.x);
        st.set_map_origin_y(m.origin.y);
        st.set_map_width(m.width);
        st.set_map_height(m.height);
    }
    map
}

/// One canvas link per relationship, parent to child. Many-to-many links are
/// dashed; the label is always supplied and the canvas decides whether to
/// show it.
pub fn links(scene: &ErdScene) -> Vec<Link> {
    scene
        .links
        .iter()
        .map(|l| Link {
            from: l.parent,
            to: l.child,
            dashed: l.kind == Cardinality::ManyToMany,
            label: l.label.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{ColumnInfo, ForeignKeyInfo};
    use dbhive_ui_model::er_diagram::DiagramTable;
    use std::collections::HashMap;

    fn col(name: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: "integer".into(),
            nullable: !pk,
            default_value: None,
            is_primary_key: pk,
            is_auto_increment: false,
        }
    }

    fn scene() -> ErdScene {
        let tables = vec![
            DiagramTable {
                name: "users".into(),
                columns: vec![col("id", true)],
            },
            DiagramTable {
                name: "orders".into(),
                columns: vec![col("id", true), col("user_id", false)],
            },
        ];
        let fks = vec![ForeignKeyInfo {
            name: "fk".into(),
            table: "orders".into(),
            schema: "main".into(),
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_schema: "main".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }];
        ErdScene::new(tables, fks, &HashMap::new())
    }

    #[test]
    fn nodes_carry_rows_badges_and_geometry() {
        let s = scene();
        let n = nodes(&s);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].title, "orders");
        assert_eq!(n[1].rows[0].badge, "PK");
        assert_eq!(n[1].rows[1].badge, "FK");
        assert_eq!(n[1].rows[1].detail, "integer");
        let r = s.rect(1);
        assert_eq!(
            (n[1].x, n[1].y, n[1].width, n[1].height),
            (r.x, r.y, r.w, r.h)
        );
        assert!(!n[1].dimmed && !n[1].emphasis);
    }

    #[test]
    fn a_filter_emphasises_matches_and_dims_the_rest() {
        let mut s = scene();
        s.filter = "ord".into();
        let n = nodes(&s);
        assert!(n[1].emphasis && !n[1].dimmed);
        assert!(!n[0].emphasis && n[0].dimmed);
    }

    #[test]
    fn links_run_parent_to_child() {
        let l = links(&scene());
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].from, l[0].to), (0, 1));
        assert!(!l[0].dashed);
        assert!(l[0].label.starts_with("1:N"));
    }
}
