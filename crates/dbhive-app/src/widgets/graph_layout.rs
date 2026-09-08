//! Layered graph layout, replacing dagre.
//!
//! The ER diagram and the visual schema designer both need to place boxes
//! (tables) joined by edges (foreign keys) so the result is readable without
//! the user dragging anything. dagre did this in the web shell; there is no
//! equivalent Rust crate, so this is a compact Sugiyama-style layout:
//!
//!   1. break cycles, so the rest can assume a DAG
//!   2. assign each node a layer (longest path from a root)
//!   3. order nodes within layers to reduce edge crossings
//!   4. assign coordinates
//!
//! Step 3 is the one that decides whether the picture is legible. It uses
//! the barycentre heuristic, sweeping down and up a few times: each node is
//! pulled towards the average position of its neighbours in the adjacent
//! layer. It is not optimal (minimising crossings is NP-hard) but it is what
//! dagre does, and it is good enough that a schema of a few dozen tables
//! reads cleanly.
//!
//! Everything here is pure. No egui types, so it can be tested directly.

use std::collections::{HashMap, HashSet, VecDeque};

/// A node to lay out. Size is supplied by the caller because it depends on
/// the rendered text (a table with 30 columns is taller than one with 2).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub width: f32,
    pub height: f32,
}

/// A directed edge, by node id. Ids that do not match a node are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

/// Where a node ended up. Coordinates are the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub layer: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions {
    /// Gap between layers, along the layout direction.
    pub layer_gap: f32,
    /// Gap between nodes within a layer.
    pub node_gap: f32,
    /// Lay out left-to-right (layers as columns) instead of top-to-bottom.
    pub horizontal: bool,
    /// Barycentre sweeps. More is tidier but slower; dagre defaults to 4.
    pub sweeps: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            layer_gap: 90.0,
            node_gap: 32.0,
            horizontal: true,
            sweeps: 4,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub placements: HashMap<String, Placement>,
    pub width: f32,
    pub height: f32,
}

/// Lay out `nodes` connected by `edges`.
///
/// Self-edges (a table with a foreign key to itself, which is common enough:
/// think `employee.manager_id`) are dropped for layering: they carry no
/// ordering information and would otherwise look like a cycle.
pub fn layout(nodes: &[Node], edges: &[Edge], opts: LayoutOptions) -> Layout {
    if nodes.is_empty() {
        return Layout::default();
    }

    let ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.from != e.to && ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()))
        .collect();

    let acyclic = break_cycles(nodes, &edges);
    let layers = assign_layers(nodes, &acyclic);
    let ordered = order_within_layers(&layers, &acyclic, opts.sweeps);
    place(nodes, &ordered, opts)
}

/// Remove edges that close a cycle, found by depth-first search.
///
/// Schemas do contain cycles (two tables referencing each other), and layer
/// assignment would not terminate on one. The dropped edge is only dropped
/// for *layering*; the caller still draws it.
fn break_cycles<'a>(nodes: &[Node], edges: &[&'a Edge]) -> Vec<&'a Edge> {
    let mut adjacency: HashMap<&str, Vec<&Edge>> = HashMap::new();
    for e in edges {
        adjacency.entry(e.from.as_str()).or_default().push(e);
    }

    let mut kept = Vec::new();
    let mut colour: HashMap<&str, u8> = HashMap::new(); // 0 unseen, 1 on stack, 2 done

    // Iterative DFS: a deep schema would blow the stack on a recursive one.
    for start in nodes.iter().map(|n| n.id.as_str()) {
        if colour.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        colour.insert(start, 1);

        while let Some((node, idx)) = stack.pop() {
            let out = adjacency.get(node).map(|v| v.as_slice()).unwrap_or(&[]);
            if idx < out.len() {
                stack.push((node, idx + 1));
                let e = out[idx];
                let target = e.to.as_str();
                match colour.get(target).copied().unwrap_or(0) {
                    // Back edge: this one closes a cycle, so leave it out.
                    1 => {}
                    2 => kept.push(e),
                    _ => {
                        kept.push(e);
                        colour.insert(target, 1);
                        stack.push((target, 0));
                    }
                }
            } else {
                colour.insert(node, 2);
            }
        }
    }
    kept
}

/// Longest-path layering: a node sits one layer below its deepest parent.
fn assign_layers(nodes: &[Node], edges: &[&Edge]) -> Vec<Vec<String>> {
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for n in nodes {
        in_degree.insert(n.id.as_str(), 0);
    }
    for e in edges {
        outgoing.entry(e.from.as_str()).or_default().push(e.to.as_str());
        incoming.entry(e.to.as_str()).or_default().push(e.from.as_str());
        *in_degree.entry(e.to.as_str()).or_insert(0) += 1;
    }

    // Kahn's algorithm, tracking depth as we go.
    let mut depth: HashMap<&str, usize> = HashMap::new();
    let mut queue: VecDeque<&str> = nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| in_degree.get(id).copied().unwrap_or(0) == 0)
        .collect();
    for id in &queue {
        depth.insert(id, 0);
    }

    while let Some(node) = queue.pop_front() {
        let d = depth.get(node).copied().unwrap_or(0);
        for &next in outgoing.get(node).map(|v| v.as_slice()).unwrap_or(&[]) {
            let entry = depth.entry(next).or_insert(0);
            *entry = (*entry).max(d + 1);
            let deg = in_degree.get_mut(next).unwrap();
            *deg -= 1;
            if *deg == 0 {
                queue.push_back(next);
            }
        }
    }

    // Anything still unassigned was in a component the traversal could not
    // reach; park it on layer 0 rather than dropping it from the diagram.
    let max_layer = depth.values().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<String>> = vec![Vec::new(); max_layer + 1];
    for n in nodes {
        let l = depth.get(n.id.as_str()).copied().unwrap_or(0);
        layers[l].push(n.id.clone());
    }
    layers
}

/// Barycentre ordering: repeatedly pull each node towards the mean position
/// of its neighbours in the previous (then next) layer.
fn order_within_layers(
    layers: &[Vec<String>],
    edges: &[&Edge],
    sweeps: usize,
) -> Vec<Vec<String>> {
    let mut layers: Vec<Vec<String>> = layers.to_vec();
    if layers.len() < 2 {
        return layers;
    }

    let mut preds: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        preds.entry(e.to.as_str()).or_default().push(e.from.as_str());
        succs.entry(e.from.as_str()).or_default().push(e.to.as_str());
    }

    for sweep in 0..sweeps {
        let downward = sweep % 2 == 0;
        let order: Vec<usize> = if downward {
            (1..layers.len()).collect()
        } else {
            (0..layers.len() - 1).rev().collect()
        };

        for li in order {
            let reference: HashMap<&str, f32> = {
                let ref_layer = if downward { &layers[li - 1] } else { &layers[li + 1] };
                ref_layer
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.as_str(), i as f32))
                    .collect()
            };
            let neighbours = if downward { &preds } else { &succs };

            let mut scored: Vec<(f32, usize, String)> = layers[li]
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let bary = neighbours
                        .get(id.as_str())
                        .map(|ns| {
                            let vals: Vec<f32> =
                                ns.iter().filter_map(|n| reference.get(n).copied()).collect();
                            if vals.is_empty() {
                                // No neighbour to follow: hold position, so
                                // unconnected nodes do not jump around.
                                i as f32
                            } else {
                                vals.iter().sum::<f32>() / vals.len() as f32
                            }
                        })
                        .unwrap_or(i as f32);
                    (bary, i, id.clone())
                })
                .collect();

            // Ties broken by existing index keeps the layout stable between
            // runs; an unstable diagram that reshuffles on every refresh is
            // worse than a slightly less tidy one.
            scored.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.cmp(&b.1))
            });
            layers[li] = scored.into_iter().map(|(_, _, id)| id).collect();
        }
    }
    layers
}

/// Turn layer/position into pixel coordinates, centring each layer.
fn place(nodes: &[Node], layers: &[Vec<String>], opts: LayoutOptions) -> Layout {
    let size: HashMap<&str, (f32, f32)> = nodes
        .iter()
        .map(|n| (n.id.as_str(), (n.width, n.height)))
        .collect();

    // Extent of each layer along the cross axis, to centre them against
    // each other.
    let cross_extents: Vec<f32> = layers
        .iter()
        .map(|layer| {
            let sum: f32 = layer
                .iter()
                .map(|id| {
                    let (w, h) = size.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
                    if opts.horizontal {
                        h
                    } else {
                        w
                    }
                })
                .sum();
            sum + opts.node_gap * layer.len().saturating_sub(1) as f32
        })
        .collect();
    let max_cross = cross_extents.iter().copied().fold(0.0_f32, f32::max);

    let mut placements = HashMap::new();
    let mut along = 0.0_f32;

    for (li, layer) in layers.iter().enumerate() {
        let layer_thickness = layer
            .iter()
            .map(|id| {
                let (w, h) = size.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
                if opts.horizontal {
                    w
                } else {
                    h
                }
            })
            .fold(0.0_f32, f32::max);

        let mut cross = (max_cross - cross_extents[li]) / 2.0;
        for id in layer {
            let (w, h) = size.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
            let (x, y) = if opts.horizontal {
                (along, cross)
            } else {
                (cross, along)
            };
            placements.insert(id.clone(), Placement { x, y, layer: li });
            cross += if opts.horizontal { h } else { w } + opts.node_gap;
        }
        along += layer_thickness + opts.layer_gap;
    }

    let total_along = (along - opts.layer_gap).max(0.0);
    let (width, height) = if opts.horizontal {
        (total_along, max_cross)
    } else {
        (max_cross, total_along)
    };
    Layout {
        placements,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str) -> Node {
        Node {
            id: id.into(),
            width: 100.0,
            height: 50.0,
        }
    }
    fn e(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn empty_graph_is_empty() {
        let l = layout(&[], &[], LayoutOptions::default());
        assert!(l.placements.is_empty());
        assert_eq!(l.width, 0.0);
    }

    #[test]
    fn chain_gets_one_node_per_layer() {
        let nodes = vec![n("a"), n("b"), n("c")];
        let edges = vec![e("a", "b"), e("b", "c")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert_eq!(l.placements["a"].layer, 0);
        assert_eq!(l.placements["b"].layer, 1);
        assert_eq!(l.placements["c"].layer, 2);
    }

    #[test]
    fn every_node_is_placed_even_when_disconnected() {
        let nodes = vec![n("a"), n("b"), n("island")];
        let edges = vec![e("a", "b")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert_eq!(l.placements.len(), 3);
        assert!(l.placements.contains_key("island"));
    }

    #[test]
    fn cycles_terminate_and_place_everything() {
        // Two tables referencing each other, which real schemas do.
        let nodes = vec![n("a"), n("b"), n("c")];
        let edges = vec![e("a", "b"), e("b", "c"), e("c", "a")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert_eq!(l.placements.len(), 3);
    }

    #[test]
    fn self_reference_does_not_create_a_layer() {
        // employee.manager_id -> employee
        let nodes = vec![n("employee")];
        let edges = vec![e("employee", "employee")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert_eq!(l.placements["employee"].layer, 0);
    }

    #[test]
    fn diamond_puts_the_join_below_both_branches() {
        let nodes = vec![n("a"), n("b"), n("c"), n("d")];
        let edges = vec![e("a", "b"), e("a", "c"), e("b", "d"), e("c", "d")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        // Longest path, so d sits below b and c rather than beside them.
        assert_eq!(l.placements["a"].layer, 0);
        assert_eq!(l.placements["b"].layer, 1);
        assert_eq!(l.placements["c"].layer, 1);
        assert_eq!(l.placements["d"].layer, 2);
    }

    #[test]
    fn edges_to_unknown_nodes_are_ignored() {
        let nodes = vec![n("a")];
        let edges = vec![e("a", "ghost")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert_eq!(l.placements.len(), 1);
        assert_eq!(l.placements["a"].layer, 0);
    }

    #[test]
    fn layout_is_stable_across_runs() {
        let nodes = vec![n("a"), n("b"), n("c"), n("d")];
        let edges = vec![e("a", "c"), e("b", "c"), e("b", "d")];
        let first = layout(&nodes, &edges, LayoutOptions::default());
        let second = layout(&nodes, &edges, LayoutOptions::default());
        for id in ["a", "b", "c", "d"] {
            assert_eq!(first.placements[id], second.placements[id], "{} moved", id);
        }
    }

    #[test]
    fn horizontal_and_vertical_swap_the_axes() {
        let nodes = vec![n("a"), n("b")];
        let edges = vec![e("a", "b")];
        let h = layout(&nodes, &edges, LayoutOptions { horizontal: true, ..Default::default() });
        let v = layout(&nodes, &edges, LayoutOptions { horizontal: false, ..Default::default() });
        // Laid out horizontally the second node advances in x, vertically in y.
        assert!(h.placements["b"].x > h.placements["a"].x);
        assert_eq!(h.placements["a"].y, h.placements["b"].y);
        assert!(v.placements["b"].y > v.placements["a"].y);
        assert_eq!(v.placements["a"].x, v.placements["b"].x);
    }

    #[test]
    fn crossing_reduction_orders_by_barycentre() {
        // a->y, b->x. Without reordering the edges cross; the sweep should
        // swap x and y in the second layer.
        let nodes = vec![n("a"), n("b"), n("x"), n("y")];
        let edges = vec![e("a", "y"), e("b", "x")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        let cross_of = |id: &str| l.placements[id].y;
        // y follows a (index 0), x follows b (index 1), so y sits above x.
        assert!(cross_of("y") < cross_of("x"));
    }

    #[test]
    fn bounds_cover_the_placed_nodes() {
        let nodes = vec![n("a"), n("b")];
        let edges = vec![e("a", "b")];
        let l = layout(&nodes, &edges, LayoutOptions::default());
        assert!(l.width >= 200.0, "width {} should span two layers", l.width);
        assert!(l.height >= 50.0);
    }
}
