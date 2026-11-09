//! The ER diagram's sizing, layout and hit testing. See lib.rs.
//!
//! The pure half of the entity-relationship diagram: table sizing, the
//! layout call, pinned-position lookup and hit testing, all in plain world
//! coordinates. The shell owns pan, zoom, drawing and the load job, and
//! converts to its own geometry types at the call site.

use std::collections::HashMap;

use dbhive_core::models::{ColumnInfo, ForeignKeyInfo};

use crate::graph_layout::{self, Edge, LayoutOptions, Node};

/// A point in diagram world space. Plain, so no UI toolkit type leaks in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Point) -> f32 {
        // Same distance formula as the canvas uses, so boundaries do not move.
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// Whether the point lies inside the box at `min` of size `(w, h)`,
    /// edges inclusive (matching the canvas rectangle test).
    pub fn in_box(self, min: Point, size: (f32, f32)) -> bool {
        min.x <= self.x && self.x <= min.x + size.0 && min.y <= self.y && self.y <= min.y + size.1
    }
}

/// A table as drawn: its columns and which are keys.
#[derive(Clone)]
pub struct DiagramTable {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

pub const HEADER_H: f32 = 26.0;
pub const ROW_H: f32 = 18.0;

/// Header height plus one row per column, used both for layout and drawing.
pub fn table_size(t: &DiagramTable, show_columns: bool) -> (f32, f32) {
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

/// Compute positions from the current tables and foreign keys.
pub fn relayout(
    tables: &[DiagramTable],
    foreign_keys: &[ForeignKeyInfo],
    show_columns: bool,
) -> graph_layout::Layout {
    let nodes: Vec<Node> = tables
        .iter()
        .map(|t| {
            let (width, height) = table_size(t, show_columns);
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
    let edges: Vec<Edge> = foreign_keys
        .iter()
        .map(|fk| Edge {
            from: fk.referenced_table.clone(),
            to: fk.table.clone(),
        })
        .collect();
    graph_layout::layout(&nodes, &edges, LayoutOptions::default())
}

/// Where a table sits in world space: a manual position if the user
/// dragged it, otherwise the computed layout.
pub fn position(
    layout: &graph_layout::Layout,
    pinned: &HashMap<String, Point>,
    name: &str,
) -> Point {
    if let Some(p) = pinned.get(name) {
        return *p;
    }
    layout
        .placements
        .get(name)
        .map(|p| Point::new(p.x, p.y))
        .unwrap_or(Point::ZERO)
}

/// The topmost table under a world-space point, if any.
pub fn hit_test(
    tables: &[DiagramTable],
    layout: &graph_layout::Layout,
    pinned: &HashMap<String, Point>,
    show_columns: bool,
    world: Point,
) -> Option<String> {
    for t in tables.iter().rev() {
        let size = table_size(t, show_columns);
        let pos = position(layout, pinned, &t.name);
        if world.in_box(pos, size) {
            return Some(t.name.clone());
        }
    }
    None
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
            columns: (0..n)
                .map(|i| col(&format!("c{}", i), "text", i == 0))
                .collect(),
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
        let tables = vec![table("users", 1)];
        let layout = relayout(&tables, &[], true);
        let mut pinned = HashMap::new();
        let auto = position(&layout, &pinned, "users");
        pinned.insert("users".to_string(), Point::new(500.0, 500.0));
        assert_ne!(auto, position(&layout, &pinned, "users"));
        assert_eq!(
            position(&layout, &pinned, "users"),
            Point::new(500.0, 500.0)
        );
    }

    #[test]
    fn hit_test_finds_the_table_under_the_point() {
        let tables = vec![table("users", 2)];
        let layout = relayout(&tables, &[], true);
        let pinned = HashMap::new();
        let p = position(&layout, &pinned, "users");
        assert_eq!(
            hit_test(
                &tables,
                &layout,
                &pinned,
                true,
                Point::new(p.x + 5.0, p.y + 5.0)
            ),
            Some("users".into())
        );
        assert_eq!(
            hit_test(
                &tables,
                &layout,
                &pinned,
                true,
                Point::new(p.x - 50.0, p.y - 50.0)
            ),
            None
        );
    }

    #[test]
    fn relayout_places_every_table() {
        let tables = vec![table("a", 1), table("b", 1), table("c", 1)];
        let foreign_keys = vec![ForeignKeyInfo {
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
        let layout = relayout(&tables, &foreign_keys, true);
        assert_eq!(layout.placements.len(), 3);
        // b holds a foreign key to a, so the parent a comes first.
        assert!(
            layout.placements["a"].layer < layout.placements["b"].layer,
            "parent should be laid out before the table referencing it"
        );
    }

    #[test]
    fn point_distance_and_box_containment() {
        assert_eq!(Point::new(0.0, 0.0).distance(Point::new(3.0, 4.0)), 5.0);
        let min = Point::new(10.0, 10.0);
        assert!(
            Point::new(10.0, 10.0).in_box(min, (5.0, 5.0)),
            "edges are inclusive"
        );
        assert!(Point::new(15.0, 15.0).in_box(min, (5.0, 5.0)));
        assert!(!Point::new(15.1, 12.0).in_box(min, (5.0, 5.0)));
        assert!(!Point::new(9.9, 12.0).in_box(min, (5.0, 5.0)));
    }
}
