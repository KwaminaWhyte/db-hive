//! Geometry for the diagram canvas.
//!
//! The canvas draws; this owns positions and edge curves. Kept apart so the
//! curve maths and drag arithmetic are testable without a window.

use slint::{Model, ModelRc, VecModel};

use crate::{DiagramEdge, DiagramNode};

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub title: String,
    pub columns: Vec<String>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Node {
    /// Height for a table with `n` columns: header plus one row each.
    pub fn height_for(n: usize) -> f32 {
        30.0 + 12.0 + n as f32 * 19.0
    }
}

/// A relationship from `from` (referenced, parent) to `to` (referencing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub from: usize,
    pub to: usize,
}

/// SVG commands for the curve between two nodes.
///
/// Leaves the right edge of the parent, enters the left edge of the child,
/// with horizontal tangents so the direction reads at a glance. Horizontal
/// control distance grows with the gap but never collapses to a kink.
pub fn edge_commands(a: &Node, b: &Node) -> String {
    let [p0, p1, p2, p3] = edge_points(a, b);
    format!(
        "M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
        p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
    )
}

/// Start, two control points, end, in canvas coordinates.
fn edge_points(a: &Node, b: &Node) -> [(f32, f32); 4] {
    let (sx, sy) = (a.x + a.width, a.y + 15.0);
    let (ex, ey) = (b.x, b.y + 15.0);
    let dx = ((ex - sx).abs() * 0.45).max(40.0);
    [(sx, sy), (sx + dx, sy), (ex - dx, ey), (ex, ey)]
}

/// The edge as a path local to its own bounding box, plus that box.
///
/// A cubic bezier never leaves the hull of its control points, so their
/// bounding box contains the whole curve. It is padded by the stroke so the
/// line is not clipped at the extremes.
pub fn edge_local(a: &Node, b: &Node) -> (String, f32, f32, f32, f32) {
    const PAD: f32 = 3.0;
    let pts = edge_points(a, b);
    let min_x = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) - PAD;
    let min_y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) - PAD;
    let max_x = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) + PAD;
    let max_y = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) + PAD;
    let l = |p: (f32, f32)| (p.0 - min_x, p.1 - min_y);
    let [p0, p1, p2, p3] = pts.map(l);
    let cmd = format!(
        "M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
        p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
    );
    (cmd, min_x, min_y, max_x - min_x, max_y - min_y)
}

pub struct Diagram {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub selected: Option<usize>,
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
        };
        d.rebuild();
        d
    }

    pub fn node_model(&self) -> ModelRc<DiagramNode> {
        ModelRc::from(self.node_model.clone())
    }

    pub fn edge_model(&self) -> ModelRc<DiagramEdge> {
        ModelRc::from(self.edge_model.clone())
    }

    fn to_slint(&self, i: usize) -> DiagramNode {
        let n = &self.nodes[i];
        DiagramNode {
            title: n.title.as_str().into(),
            columns: ModelRc::new(VecModel::from(
                n.columns
                    .iter()
                    .map(|c| slint::SharedString::from(c.as_str()))
                    .collect::<Vec<_>>(),
            )),
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            selected: self.selected == Some(i),
        }
    }

    fn edge(&self, l: &Link) -> DiagramEdge {
        let (cmd, x, y, width, height) = edge_local(&self.nodes[l.from], &self.nodes[l.to]);
        DiagramEdge {
            commands: cmd.into(),
            x,
            y,
            width,
            height,
            highlighted: self.selected.is_some_and(|s| s == l.from || s == l.to),
        }
    }

    fn rebuild(&self) {
        self.node_model
            .set_vec((0..self.nodes.len()).map(|i| self.to_slint(i)).collect::<Vec<_>>());
        self.edge_model
            .set_vec(self.links.iter().map(|l| self.edge(l)).collect::<Vec<_>>());
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
        self.nodes[i].x += dx;
        self.nodes[i].y += dy;
        self.node_model.set_row_data(i, self.to_slint(i));
        for (e, l) in self.links.iter().enumerate() {
            if l.from == i || l.to == i {
                self.edge_model.set_row_data(e, self.edge(l));
            }
        }
    }

    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i;
        self.rebuild();
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
        assert!(cmd.contains("240.0"), "control point did not respect the minimum: {}", cmd);
    }

    #[test]
    fn dragging_adds_the_delta_and_moves_only_touching_edges() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0), node(800.0, 0.0)],
            vec![Link { from: 0, to: 1 }, Link { from: 1, to: 2 }],
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
        assert_eq!(e.x, 220.0 - 3.0);
    }

    #[test]
    fn selection_highlights_only_connected_edges() {
        let mut d = Diagram::new(
            vec![node(0.0, 0.0), node(400.0, 0.0), node(800.0, 0.0)],
            vec![Link { from: 0, to: 1 }, Link { from: 1, to: 2 }],
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
            assert!(pair[0] >= 0.0 && pair[0] <= w, "x {} outside 0..{}", pair[0], w);
            assert!(pair[1] >= 0.0 && pair[1] <= h, "y {} outside 0..{}", pair[1], h);
        }
    }

    #[test]
    fn dragging_an_unknown_node_is_ignored() {
        let mut d = Diagram::new(vec![node(0.0, 0.0)], vec![]);
        d.drag(9, 10.0, 10.0);
        assert_eq!(d.nodes[0].x, 0.0);
    }
}
