//! Geometry for the diagram canvas.
//!
//! The canvas draws; this owns positions and edge curves. Kept apart so the
//! curve maths and drag arithmetic are testable without a window. Routing,
//! dashing and end markers come from `dbhive_ui_model::erd_scene`, shared by
//! the ER diagram, the schema designer and the SVG export.

use dbhive_ui_model::er_diagram::Point;
use dbhive_ui_model::erd_scene::{edge_path, route, route_self, Flow, Route};
use slint::{Model, ModelRc, VecModel};

use crate::{DiagramEdge, DiagramNode, DiagramRow};

/// A drawn row: name on the left, detail (the type) on the right.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub name: String,
    pub detail: String,
    /// "PK", "FK" or empty.
    pub badge: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub title: String,
    /// Plain one-string rows, for the spikes. Used only when `rows` is empty;
    /// the first is drawn as a key.
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub more: String,
    /// "M:N" on a junction table.
    pub tag: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub header: f32,
    pub row_height: f32,
    pub detail_width: f32,
    pub junction: bool,
    pub emphasis: bool,
    pub dimmed: bool,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            title: String::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            more: String::new(),
            tag: String::new(),
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: Node::height_for(0),
            header: 30.0,
            row_height: 19.0,
            detail_width: 70.0,
            junction: false,
            emphasis: false,
            dimmed: false,
        }
    }
}

impl Node {
    /// Height for a table with `n` columns: header plus one row each.
    pub fn height_for(n: usize) -> f32 {
        30.0 + 12.0 + n as f32 * 19.0
    }

    fn rect(&self) -> dbhive_ui_model::erd_scene::Rect {
        dbhive_ui_model::erd_scene::Rect::new(self.x, self.y, self.width, self.height)
    }
}

/// A relationship from `from` (referenced, parent) to `to` (referencing).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    /// Drawn as dashes and in the lighter colour (a many-to-many link).
    pub dashed: bool,
    /// Shown at the middle of the line.
    pub label: String,
}

impl Link {
    pub fn new(from: usize, to: usize) -> Self {
        Self {
            from,
            to,
            ..Default::default()
        }
    }
}

fn route_of(a: &Node, b: &Node, flow: Flow) -> Route {
    route(a.rect(), b.rect(), flow, a.header)
}

/// SVG commands for the curve between two nodes, in canvas coordinates.
///
/// Leaves the right edge of the parent, enters the left edge of the child,
/// with horizontal tangents so the direction reads at a glance. Horizontal
/// control distance grows with the gap but never collapses to a kink.
pub fn edge_commands(a: &Node, b: &Node) -> String {
    let [p0, p1, p2, p3] = route_of(a, b, Flow::Horizontal).p;
    format!(
        "M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
        p0.x, p0.y, p1.x, p1.y, p2.x, p2.y, p3.x, p3.y
    )
}

/// The edge as a path local to its own bounding box, plus that box.
///
/// A cubic bezier never leaves the hull of its control points, so their
/// bounding box contains the whole curve. It is padded by the stroke so the
/// line is not clipped at the extremes.
pub fn edge_local(a: &Node, b: &Node) -> (String, f32, f32, f32, f32) {
    let e = edge_path(&route_of(a, b, Flow::Horizontal), false, false);
    (e.commands, e.bounds.x, e.bounds.y, e.bounds.w, e.bounds.h)
}

pub struct Diagram {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub selected: Option<usize>,
    /// Which way relationships read; horizontal for the spikes and the
    /// designer, vertical for the ER diagram.
    pub flow: Flow,
    /// Draw the bar and crow's foot at the ends of a relationship.
    pub markers: bool,
    /// Edges are not drawn at all (the relationships toggle).
    pub hide_edges: bool,
    node_model: std::rc::Rc<VecModel<DiagramNode>>,
    edge_model: std::rc::Rc<VecModel<DiagramEdge>>,
}

impl Diagram {
    pub fn new(nodes: Vec<Node>, links: Vec<Link>) -> Self {
        let d = Self {
            node_model: std::rc::Rc::new(VecModel::default()),
            edge_model: std::rc::Rc::new(VecModel::default()),
            nodes,
            links,
            selected: None,
            flow: Flow::Horizontal,
            markers: false,
            hide_edges: false,
        };
        d.rebuild();
        d
    }

    pub fn with_flow(mut self, flow: Flow, markers: bool) -> Self {
        self.flow = flow;
        self.markers = markers;
        self.rebuild();
        self
    }

    pub fn node_model(&self) -> ModelRc<DiagramNode> {
        ModelRc::from(self.node_model.clone())
    }

    pub fn edge_model(&self) -> ModelRc<DiagramEdge> {
        ModelRc::from(self.edge_model.clone())
    }

    fn to_slint(&self, i: usize) -> DiagramNode {
        let n = &self.nodes[i];
        let rows: Vec<DiagramRow> = if n.rows.is_empty() {
            n.columns
                .iter()
                .enumerate()
                .map(|(c, name)| DiagramRow {
                    name: name.as_str().into(),
                    detail: "".into(),
                    badge: if c == 0 { "PK".into() } else { "".into() },
                })
                .collect()
        } else {
            n.rows
                .iter()
                .map(|r| DiagramRow {
                    name: r.name.as_str().into(),
                    detail: r.detail.as_str().into(),
                    badge: r.badge.as_str().into(),
                })
                .collect()
        };
        DiagramNode {
            title: n.title.as_str().into(),
            tag: n.tag.as_str().into(),
            rows: ModelRc::new(VecModel::from(rows)),
            more: n.more.as_str().into(),
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            header_height: n.header,
            row_height: n.row_height,
            detail_width: n.detail_width,
            selected: self.selected == Some(i),
            emphasis: n.emphasis,
            dimmed: n.dimmed,
            junction: n.junction,
        }
    }

    fn route(&self, l: &Link) -> Route {
        if l.from == l.to {
            route_self(self.nodes[l.from].rect())
        } else {
            route_of(&self.nodes[l.from], &self.nodes[l.to], self.flow)
        }
    }

    fn edge(&self, l: &Link) -> DiagramEdge {
        let r = self.route(l);
        let path = edge_path(&r, l.dashed, self.markers);
        let mid: Point = r.midpoint();
        let touches_selected = self.selected.is_some_and(|s| s == l.from || s == l.to);
        let filtering = self.nodes.iter().any(|n| n.emphasis || n.dimmed);
        let involved = |i: usize| self.nodes[i].emphasis;
        DiagramEdge {
            commands: path.commands.into(),
            x: path.bounds.x,
            y: path.bounds.y,
            width: path.bounds.w,
            height: path.bounds.h,
            highlighted: touches_selected,
            muted: l.dashed,
            // While filtering, relationships of unmatched tables fade too.
            dimmed: filtering && !involved(l.from) && !involved(l.to),
            label: l.label.as_str().into(),
            label_x: mid.x,
            label_y: mid.y,
        }
    }

    fn rebuild(&self) {
        self.node_model.set_vec(
            (0..self.nodes.len())
                .map(|i| self.to_slint(i))
                .collect::<Vec<_>>(),
        );
        if self.hide_edges {
            self.edge_model.set_vec(Vec::new());
        } else {
            self.edge_model
                .set_vec(self.links.iter().map(|l| self.edge(l)).collect::<Vec<_>>());
        }
    }

    /// Replace the whole picture, keeping the same models so whatever is bound
    /// to them keeps following.
    pub fn replace(&mut self, nodes: Vec<Node>, links: Vec<Link>) {
        self.nodes = nodes;
        self.links = links;
        if self.selected.is_some_and(|s| s >= self.nodes.len()) {
            self.selected = None;
        }
        self.rebuild();
    }

    /// Apply a drag delta to one node.
    ///
    /// Slint reports the pointer relative to the element being dragged, and
    /// that element moves with the node, so each event carries only the
    /// movement since the last one. The delta is added, not assigned.
    ///
    /// Only the dragged node's row and the edges touching it are updated, so
    /// a drag does not rebuild the whole diagram on every pointer event.
    pub fn drag(&mut self, i: usize, dx: f32, dy: f32) {
        if i >= self.nodes.len() {
            return;
        }
        let (x, y) = (self.nodes[i].x + dx, self.nodes[i].y + dy);
        self.move_to(i, x, y);
    }

    /// Put one node at an absolute position (the scene decided where, for
    /// instance after snapping to the grid).
    pub fn move_to(&mut self, i: usize, x: f32, y: f32) {
        if i >= self.nodes.len() {
            return;
        }
        self.nodes[i].x = x;
        self.nodes[i].y = y;
        self.node_model.set_row_data(i, self.to_slint(i));
        if self.hide_edges {
            return;
        }
        for (e, l) in self.links.iter().enumerate() {
            if l.from == i || l.to == i {
                self.edge_model.set_row_data(e, self.edge(l));
            }
        }
    }

    /// Select a node (or none).
    ///
    /// Only the rows that change are rewritten, never the whole model: a
    /// press on a table selects it, and replacing the model at that moment
    /// would destroy the very element the pointer is dragging.
    pub fn select(&mut self, i: Option<usize>) {
        let old = self.selected;
        self.selected = i;
        let touched: Vec<usize> = [old, i]
            .into_iter()
            .flatten()
            .filter(|n| *n < self.nodes.len())
            .collect();
        for n in &touched {
            self.node_model.set_row_data(*n, self.to_slint(*n));
        }
        if !self.hide_edges {
            for (e, l) in self.links.iter().enumerate() {
                if touched.iter().any(|n| l.from == *n || l.to == *n) {
                    self.edge_model.set_row_data(e, self.edge(l));
                }
            }
        }
    }

    pub fn edge_count(&self) -> usize {
        self.edge_model.row_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f32, y: f32) -> Node {
        Node {
            title: "t".into(),
            columns: vec!["id".into()],
            x,
            y,
            width: 200.0,
            height: Node::height_for(1),
            ..Default::default()
        }
    }

    #[test]
    fn edge_runs_from_parent_right_edge_to_child_left_edge() {
        let cmd = edge_commands(&node(0.0, 0.0), &node(400.0, 100.0));
        assert!(cmd.starts_with("M 200.0 15.0 C "), "{}", cmd);
        assert!(cmd.ends_with(" 400.0 115.0"), "{}", cmd);
    }

    #[test]
    fn close_nodes_still_get_a_visible_curve() {
        // With no horizontal gap a proportional control distance is zero and
        // the curve degenerates into a kink.
        let cmd = edge_commands(&node(0.0, 0.0), &node(200.0, 0.0));
        assert!(
            cmd.contains("240.0"),
            "control point did not respect the minimum: {}",
            cmd
        );
    }

    #[test]
    fn dragging_adds_the_delta_and_moves_only_touching_edges() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0), node(800.0, 0.0)],
            vec![Link::new(0, 1), Link::new(1, 2)],
        );
        let untouched_before = d.edge_model.row_data(1).unwrap().commands;

        d.drag(0, 10.0, 5.0);
        d.drag(0, 10.0, 5.0);

        assert_eq!(d.nodes[0].x, 20.0, "deltas must accumulate");
        assert_eq!(d.nodes[0].y, 10.0);
        assert_eq!(
            d.edge_model.row_data(1).unwrap().commands,
            untouched_before,
            "an edge not touching the dragged node changed"
        );
        // The moved edge's box follows the node: it now starts at x=220.
        let e = d.edge_model.row_data(0).unwrap();
        assert_eq!(e.x, 220.0 - 4.0);
    }

    #[test]
    fn selecting_rewrites_only_the_old_and_new_selection() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0), node(800.0, 0.0)],
            vec![Link::new(0, 1), Link::new(1, 2)],
        );
        d.select(Some(0));
        assert!(d.node_model.row_data(0).unwrap().selected);
        d.select(Some(2));
        assert!(
            !d.node_model.row_data(0).unwrap().selected,
            "the old selection stayed selected"
        );
        assert!(d.node_model.row_data(2).unwrap().selected);
        assert!(
            !d.edge_model.row_data(0).unwrap().highlighted,
            "edge 0 touches neither selected table now"
        );
        assert!(d.edge_model.row_data(1).unwrap().highlighted);
        d.select(None);
        assert!(!d.node_model.row_data(2).unwrap().selected);
        assert!(!d.edge_model.row_data(1).unwrap().highlighted);
        // Out of range is ignored rather than panicking.
        d.select(Some(9));
        assert_eq!(d.selected, Some(9));
    }

    #[test]
    fn selection_highlights_only_connected_edges() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0), node(800.0, 0.0)],
            vec![Link::new(0, 1), Link::new(1, 2)],
        );
        d.select(Some(0));
        assert!(d.edge_model.row_data(0).unwrap().highlighted);
        assert!(!d.edge_model.row_data(1).unwrap().highlighted);
    }

    #[test]
    fn local_path_fits_inside_its_box() {
        let (cmd, _x, _y, w, h) = edge_local(&node(0.0, 0.0), &node(400.0, 300.0));
        // Every coordinate in the local commands lies within the box.
        let nums: Vec<f32> = cmd
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        for pair in nums.chunks(2) {
            assert!(
                pair[0] >= 0.0 && pair[0] <= w,
                "x {} outside 0..{}",
                pair[0],
                w
            );
            assert!(
                pair[1] >= 0.0 && pair[1] <= h,
                "y {} outside 0..{}",
                pair[1],
                h
            );
        }
    }

    #[test]
    fn dragging_an_unknown_node_is_ignored() {
        let mut d = Diagram::new(vec![node(0.0, 0.0)], vec![]);
        d.drag(9, 10.0, 10.0);
        assert_eq!(d.nodes[0].x, 0.0);
    }

    #[test]
    fn plain_columns_become_rows_with_the_first_as_a_key() {
        let d = Diagram::new(vec![node(0.0, 0.0)], vec![]);
        let n = d.node_model.row_data(0).unwrap();
        assert_eq!(n.rows.row_count(), 1);
        assert_eq!(n.rows.row_data(0).unwrap().badge, "PK");
    }

    #[test]
    fn a_vertical_flow_with_markers_joins_bottom_to_top_and_labels_the_middle() {
        let mut a = node(0.0, 0.0);
        a.height = 100.0;
        let b = node(0.0, 300.0);
        let mut link = Link::new(0, 1);
        link.label = "1:N  a_id \u{2192} id".into();
        let d = Diagram::new(vec![a, b], vec![link]).with_flow(Flow::Vertical, true);
        let e = d.edge_model.row_data(0).unwrap();
        assert_eq!(e.label, "1:N  a_id \u{2192} id");
        // The curve is vertical, so its middle sits halfway between the boxes.
        assert!((e.label_y - 200.0).abs() < 1.0, "{}", e.label_y);
        assert!((e.label_x - 100.0).abs() < 1.0, "{}", e.label_x);
        // Bar plus three prongs after the curve.
        assert_eq!(e.commands.matches('M').count(), 5, "{}", e.commands);
    }

    #[test]
    fn a_dashed_link_is_muted_and_drawn_in_pieces() {
        let mut link = Link::new(0, 1);
        link.dashed = true;
        let d = Diagram::new(vec![node(0.0, 0.0), node(400.0, 0.0)], vec![link]);
        let e = d.edge_model.row_data(0).unwrap();
        assert!(e.muted);
        assert!(e.commands.matches('M').count() > 5, "{}", e.commands);
    }

    #[test]
    fn a_self_reference_is_drawn_as_a_loop_beside_its_table() {
        let d = Diagram::new(vec![node(0.0, 0.0)], vec![Link::new(0, 0)])
            .with_flow(Flow::Vertical, true);
        let e = d.edge_model.row_data(0).unwrap();
        assert!(
            e.x >= 190.0,
            "the loop should start at the table's right edge: {}",
            e.x
        );
        assert!(e.width > 60.0);
    }

    #[test]
    fn hidden_edges_leave_the_model_empty_and_come_back() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0)],
            vec![Link::new(0, 1)],
        );
        assert_eq!(d.edge_count(), 1);
        let again = |d: &mut Diagram| {
            let (n, l) = (d.nodes.clone(), d.links.clone());
            d.replace(n, l);
        };
        d.hide_edges = true;
        again(&mut d);
        assert_eq!(d.edge_count(), 0);
        d.hide_edges = false;
        again(&mut d);
        assert_eq!(d.edge_count(), 1);
    }

    #[test]
    fn filtering_fades_relationships_of_unmatched_tables() {
        let mut a = node(0.0, 0.0);
        a.emphasis = true;
        let mut b = node(400.0, 0.0);
        b.dimmed = true;
        let mut c = node(800.0, 0.0);
        c.dimmed = true;
        let d = Diagram::new(vec![a, b, c], vec![Link::new(0, 1), Link::new(1, 2)]);
        assert!(!d.edge_model.row_data(0).unwrap().dimmed, "touches a match");
        assert!(
            d.edge_model.row_data(1).unwrap().dimmed,
            "touches only unmatched tables"
        );
    }

    #[test]
    fn move_to_places_a_node_absolutely() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0)],
            vec![Link::new(0, 1)],
        );
        d.move_to(0, 32.0, 48.0);
        assert_eq!((d.nodes[0].x, d.nodes[0].y), (32.0, 48.0));
        assert_eq!(d.node_model.row_data(0).unwrap().x, 32.0);
    }
}
