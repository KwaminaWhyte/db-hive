//! The ER diagram's scene for the Slint shell: what each table shows, where
//! it sits, how relationships are routed and styled, zoom and pan maths, the
//! minimap, manual-layout persistence and SVG export.
//!
//! `er_diagram` owns table sizing and pinned-position lookup; this module
//! builds on its `DiagramTable` and `Point` and adds:
//! a column cap with "+N more", junction tables, 1:N versus M:N styling,
//! edge labels, snap-to-grid dragging, a per-schema saved arrangement in the
//! `erd-layouts.json` format, a minimap and vector export.
//!
//! No toolkit types. The shell converts a scene into its own node and edge
//! models and feeds pointer input back in as plain numbers.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use dbhive_core::models::ForeignKeyInfo;
use serde_json::{json, Value};

use crate::er_diagram::{DiagramTable, Point};
use crate::graph_layout::{self, Edge, LayoutOptions, Node};
use crate::tokens::Palette;

// -- Metrics ------------------------------------------------------------------

/// Header band of a table box, in world pixels.
pub const HEADER_H: f32 = 30.0;
/// One column row.
pub const ROW_H: f32 = 20.0;
/// Space under the last row.
pub const FOOT_H: f32 = 8.0;
/// A table shows at most this many columns, then a "+N more" line.
pub const MAX_COLUMNS: usize = 10;
/// Drag snapping step, the web diagram's 16px grid.
pub const GRID: f32 = 16.0;
/// Zoom limits: the web diagram allowed 5% to 150%.
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 1.5;

const MIN_WIDTH: f32 = 190.0;
const MAX_WIDTH: f32 = 360.0;
/// Average advance of the body and type faces, used to size boxes before any
/// text is measured. Boxes elide what does not fit, so an underestimate costs
/// a few characters, never a broken layout.
const NAME_CHAR: f32 = 7.1;
const TYPE_CHAR: f32 = 6.2;
/// Space taken by the PK/FK pill at the start of a row.
const BADGE_W: f32 = 28.0;
const SIDE_PAD: f32 = 12.0;

// -- Geometry -----------------------------------------------------------------

/// An axis-aligned box in world space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn union(self, o: Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }
    pub fn contains(&self, p: Point) -> bool {
        p.in_box(Point::new(self.x, self.y), (self.w, self.h))
    }
    pub fn inflate(&self, by: f32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }
}

/// Which way relationships are meant to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Parent above child (the ER diagram).
    Vertical,
    /// Parent left of child (the schema designer).
    Horizontal,
}

/// A cubic bezier between two boxes, with the direction each end leaves its
/// box in so end markers can be oriented.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Route {
    pub p: [Point; 4],
    /// Unit vector pointing away from the parent at the start.
    pub start_dir: Point,
    /// Unit vector pointing away from the child at the end.
    pub end_dir: Point,
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn curve(start: Point, sd: Point, end: Point, ed: Point, min_reach: f32) -> Route {
    // How far the tangents reach: a share of the span along the leaving
    // direction, never so small the curve kinks.
    let span = if sd.x != 0.0 {
        (end.x - start.x).abs()
    } else {
        (end.y - start.y).abs()
    };
    let reach = (span * 0.45).max(min_reach);
    Route {
        p: [
            start,
            pt(start.x + sd.x * reach, start.y + sd.y * reach),
            pt(end.x + ed.x * reach, end.y + ed.y * reach),
            end,
        ],
        start_dir: sd,
        end_dir: ed,
    }
}

/// Route a relationship from a parent box to a child box.
///
/// `head` is the header height, used as the anchor height for side-to-side
/// routes in a horizontal flow (the line meets the title row, not the middle
/// of a tall box).
pub fn route(parent: Rect, child: Rect, flow: Flow, head: f32) -> Route {
    const CLEAR: f32 = 16.0;
    match flow {
        Flow::Vertical => {
            if child.y >= parent.bottom() + CLEAR {
                return curve(
                    pt(parent.cx(), parent.bottom()),
                    pt(0.0, 1.0),
                    pt(child.cx(), child.y),
                    pt(0.0, -1.0),
                    40.0,
                );
            }
            if parent.y >= child.bottom() + CLEAR {
                return curve(
                    pt(parent.cx(), parent.y),
                    pt(0.0, -1.0),
                    pt(child.cx(), child.bottom()),
                    pt(0.0, 1.0),
                    40.0,
                );
            }
            sides(parent, child, head / 2.0, head / 2.0)
        }
        Flow::Horizontal => sides(parent, child, head / 2.0, head / 2.0),
    }
}

/// Right edge to left edge when the child is to the right, else mirrored.
fn sides(parent: Rect, child: Rect, parent_anchor: f32, child_anchor: f32) -> Route {
    if child.cx() >= parent.cx() {
        curve(
            pt(parent.right(), parent.y + parent_anchor),
            pt(1.0, 0.0),
            pt(child.x, child.y + child_anchor),
            pt(-1.0, 0.0),
            40.0,
        )
    } else {
        curve(
            pt(parent.x, parent.y + parent_anchor),
            pt(-1.0, 0.0),
            pt(child.right(), child.y + child_anchor),
            pt(1.0, 0.0),
            40.0,
        )
    }
}

/// A table that references itself: a loop out of and back into its right edge.
pub fn route_self(r: Rect) -> Route {
    let (y0, y1) = (r.y + r.h * 0.3, r.y + r.h * 0.7);
    Route {
        p: [
            pt(r.right(), y0),
            pt(r.right() + 64.0, y0),
            pt(r.right() + 64.0, y1),
            pt(r.right(), y1),
        ],
        start_dir: pt(1.0, 0.0),
        end_dir: pt(1.0, 0.0),
    }
}

impl Route {
    pub fn point_at(&self, t: f32) -> Point {
        let u = 1.0 - t;
        let [a, b, c, d] = self.p;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        pt(
            w[0] * a.x + w[1] * b.x + w[2] * c.x + w[3] * d.x,
            w[0] * a.y + w[1] * b.y + w[2] * c.y + w[3] * d.y,
        )
    }

    /// Where a label sits: the middle of the curve.
    pub fn midpoint(&self) -> Point {
        self.point_at(0.5)
    }

    /// The curve as a polyline with its cumulative length at each vertex.
    fn polyline(&self, steps: usize) -> (Vec<Point>, Vec<f32>) {
        let mut pts = Vec::with_capacity(steps + 1);
        let mut len = Vec::with_capacity(steps + 1);
        let mut acc = 0.0;
        for i in 0..=steps {
            let p = self.point_at(i as f32 / steps as f32);
            if let Some(prev) = pts.last() {
                acc += p.distance(*prev);
            }
            pts.push(p);
            len.push(acc);
        }
        (pts, len)
    }
}

/// An edge ready to draw: SVG path commands relative to their own bounding
/// box, plus that box in world space.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePath {
    pub commands: String,
    pub bounds: Rect,
}

const DASH_ON: f32 = 7.0;
const DASH_OFF: f32 = 5.0;
const MARK_PAD: f32 = 4.0;

/// The end markers as line pieces in world space: a bar across the line near
/// the parent (the "one" side) and three prongs fanning into the child (the
/// "many" side, a crow's foot).
pub fn marker_lines(route: &Route) -> Vec<Vec<Point>> {
    let perp = |d: Point| pt(-d.y, d.x);
    let mut out = Vec::with_capacity(4);
    let c = pt(
        route.p[0].x + route.start_dir.x * 10.0,
        route.p[0].y + route.start_dir.y * 10.0,
    );
    let n = perp(route.start_dir);
    out.push(vec![
        pt(c.x + n.x * 5.0, c.y + n.y * 5.0),
        pt(c.x - n.x * 5.0, c.y - n.y * 5.0),
    ]);
    let end = route.p[3];
    let tip = pt(
        end.x + route.end_dir.x * 12.0,
        end.y + route.end_dir.y * 12.0,
    );
    let n = perp(route.end_dir);
    out.push(vec![tip, pt(end.x + n.x * 6.0, end.y + n.y * 6.0)]);
    out.push(vec![tip, end]);
    out.push(vec![tip, pt(end.x - n.x * 6.0, end.y - n.y * 6.0)]);
    out
}

/// The line, and optionally its end markers: a bar at the parent (the "one"
/// side) and a crow's foot at the child (the "many" side). A dashed line is
/// drawn as many short subpaths because the canvas path has no dash setting.
pub fn edge_path(route: &Route, dashed: bool, markers: bool) -> EdgePath {
    // Collect absolute subpaths first; the box is only known afterwards.
    let mut subpaths: Vec<Vec<Point>> = Vec::new();

    if dashed {
        let (pts, cum) = route.polyline(96);
        let total = *cum.last().unwrap_or(&0.0);
        let at = |s: f32| -> Point {
            let s = s.clamp(0.0, total);
            let i = cum.partition_point(|l| *l < s).clamp(1, pts.len() - 1);
            let (l0, l1) = (cum[i - 1], cum[i]);
            let f = if l1 > l0 { (s - l0) / (l1 - l0) } else { 0.0 };
            pt(
                pts[i - 1].x + (pts[i].x - pts[i - 1].x) * f,
                pts[i - 1].y + (pts[i].y - pts[i - 1].y) * f,
            )
        };
        let mut s = 0.0;
        while s < total {
            let end = (s + DASH_ON).min(total);
            let mid = (s + end) / 2.0;
            subpaths.push(vec![at(s), at(mid), at(end)]);
            s += DASH_ON + DASH_OFF;
        }
        if subpaths.is_empty() {
            subpaths.push(vec![route.p[0], route.p[3]]);
        }
    }

    let mut curve_cmd: Option<[Point; 4]> = None;
    if !dashed {
        curve_cmd = Some(route.p);
    }

    let marks = if markers {
        marker_lines(route)
    } else {
        Vec::new()
    };

    // Bounding box over everything emitted. A curve stays inside the hull of
    // its control points.
    let mut all: Vec<Point> = subpaths.iter().flatten().copied().collect();
    if let Some(c) = curve_cmd {
        all.extend(c);
    }
    all.extend(marks.iter().flatten().copied());
    let min_x = all.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - MARK_PAD;
    let min_y = all.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - MARK_PAD;
    let max_x = all.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) + MARK_PAD;
    let max_y = all.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) + MARK_PAD;
    let l = |p: Point| (p.x - min_x, p.y - min_y);

    let mut s = String::new();
    if let Some(c) = curve_cmd {
        let [a, b, cc, d] = c.map(l);
        let _ = write!(
            s,
            "M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
            a.0, a.1, b.0, b.1, cc.0, cc.1, d.0, d.1
        );
    }
    for sp in subpaths.iter().chain(marks.iter()) {
        for (i, p) in sp.iter().enumerate() {
            let (x, y) = l(*p);
            if !s.is_empty() {
                s.push(' ');
            }
            let _ = write!(s, "{} {:.1} {:.1}", if i == 0 { "M" } else { "L" }, x, y);
        }
    }

    EdgePath {
        commands: s,
        bounds: Rect::new(min_x, min_y, max_x - min_x, max_y - min_y),
    }
}

// -- What a table shows -----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    Pk,
    Fk,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Badge::None => "",
            Badge::Pk => "PK",
            Badge::Fk => "FK",
        }
    }
}

/// One drawn column row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowView {
    pub name: String,
    pub data_type: String,
    pub badge: Badge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    OneToMany,
    ManyToMany,
}

impl Cardinality {
    pub fn label(self) -> &'static str {
        match self {
            Cardinality::OneToMany => "1:N",
            Cardinality::ManyToMany => "M:N",
        }
    }
}

/// A relationship between two tables of the scene, parent to child.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub parent: usize,
    pub child: usize,
    pub kind: Cardinality,
    /// "1:N  user_id → id".
    pub label: String,
}

/// Whether a table is a join table for a many-to-many relationship.
///
/// The web diagram called any table with `_has_` in its name, or two foreign
/// key columns and at most five columns, a junction. That also caught
/// `orders(id, user_id, store_id, total)`. This keeps the name rule and
/// tightens the shape rule: at least two foreign key columns, at most six
/// columns, and nothing else but the primary key and timestamps.
pub fn is_junction(table: &DiagramTable, fk_columns: &HashSet<String>) -> bool {
    if table.name.to_lowercase().contains("_has_") {
        return true;
    }
    if fk_columns.len() < 2 || table.columns.len() > 6 {
        return false;
    }
    table.columns.iter().all(|c| {
        fk_columns.contains(&c.name)
            || c.is_primary_key
            || c.name.to_lowercase().ends_with("_at")
            || {
                let t = c.data_type.to_lowercase();
                t.contains("timestamp") || t.contains("date") || t.contains("time")
            }
    })
}

/// Size of a table box given what it shows.
pub fn node_size(t: &DiagramTable, show_columns: bool, junction: bool) -> (f32, f32) {
    let shown = if show_columns {
        t.columns.len().min(MAX_COLUMNS)
    } else {
        0
    };
    let more = if show_columns && t.columns.len() > MAX_COLUMNS {
        1
    } else {
        0
    };
    let title_w = t.name.chars().count() as f32 * (NAME_CHAR + 0.9)
        + SIDE_PAD * 2.0
        + if junction { 52.0 } else { 0.0 };
    let body_w = t
        .columns
        .iter()
        .take(shown)
        .map(|c| {
            // Badge, name, a gap, the type, and the padding either side.
            c.name.chars().count() as f32 * NAME_CHAR
                + c.data_type.chars().count() as f32 * TYPE_CHAR
                + BADGE_W
                + SIDE_PAD * 2.0
                + 20.0
        })
        .fold(0.0, f32::max);
    let width = title_w.max(body_w).clamp(MIN_WIDTH, MAX_WIDTH);
    let height = if show_columns {
        HEADER_H + (shown + more) as f32 * ROW_H + FOOT_H
    } else {
        HEADER_H
    };
    (width, height)
}

/// Width reserved for the type column on the right of a row.
pub fn type_column_width(t: &DiagramTable, width: f32) -> f32 {
    let longest = t
        .columns
        .iter()
        .take(MAX_COLUMNS)
        .map(|c| c.data_type.chars().count())
        .max()
        .unwrap_or(0) as f32;
    let want = longest * TYPE_CHAR + 8.0;
    // Never more than 55% of the box, so names keep room.
    want.min(width * 0.55).max(40.0)
}

// -- The scene --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub show_columns: bool,
    pub show_relationships: bool,
    pub show_labels: bool,
    pub snap: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_columns: true,
            show_relationships: true,
            show_labels: true,
            // The web diagram snapped by default.
            snap: true,
        }
    }
}

/// Derived facts about one table.
#[derive(Debug, Clone)]
struct Info {
    junction: bool,
    pk: Vec<bool>,
    fk: Vec<bool>,
    width: f32,
    height: f32,
}

pub struct ErdScene {
    pub tables: Vec<DiagramTable>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub links: Vec<Link>,
    pub options: Options,
    pub filter: String,
    pub selected: Option<usize>,
    info: Vec<Info>,
    layout: graph_layout::Layout,
    /// Positions the user chose (or loaded from the saved arrangement).
    pinned: HashMap<String, Point>,
    /// The unsnapped position of the table being dragged, so a snapped drag
    /// does not lose sub-grid movement.
    drag_raw: Option<(usize, Point)>,
}

impl ErdScene {
    /// Build a scene. `saved` is the arrangement stored for this schema; names
    /// that are no longer tables are ignored.
    pub fn new(
        tables: Vec<DiagramTable>,
        foreign_keys: Vec<ForeignKeyInfo>,
        saved: &HashMap<String, Point>,
    ) -> Self {
        let names: HashSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        let pinned = saved
            .iter()
            .filter(|(k, _)| names.contains(k.as_str()))
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let mut s = Self {
            tables,
            foreign_keys,
            links: Vec::new(),
            options: Options::default(),
            filter: String::new(),
            selected: None,
            info: Vec::new(),
            layout: graph_layout::Layout::default(),
            pinned,
            drag_raw: None,
        };
        s.rebuild();
        s
    }

    /// Recompute everything derived from the tables and options.
    fn rebuild(&mut self) {
        let index: HashMap<&str, usize> = self
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.as_str(), i))
            .collect();

        // Foreign key columns per table.
        let mut fk_cols: Vec<HashSet<String>> = vec![HashSet::new(); self.tables.len()];
        for fk in &self.foreign_keys {
            if let Some(&i) = index.get(fk.table.as_str()) {
                fk_cols[i].extend(fk.columns.iter().cloned());
            }
        }

        self.info = self
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let junction = is_junction(t, &fk_cols[i]);
                let (width, height) = node_size(t, self.options.show_columns, junction);
                Info {
                    junction,
                    pk: t.columns.iter().map(|c| c.is_primary_key).collect(),
                    fk: t
                        .columns
                        .iter()
                        .map(|c| fk_cols[i].contains(&c.name))
                        .collect(),
                    width,
                    height,
                }
            })
            .collect();

        self.links = self
            .foreign_keys
            .iter()
            .filter_map(|fk| {
                let child = *index.get(fk.table.as_str())?;
                let parent = *index.get(fk.referenced_table.as_str())?;
                let pairs = fk
                    .columns
                    .iter()
                    .zip(fk.referenced_columns.iter())
                    .map(|(a, b)| format!("{} \u{2192} {}", a, b))
                    .collect::<Vec<_>>()
                    .join(", ");
                let kind = if self.info[child].junction {
                    Cardinality::ManyToMany
                } else {
                    Cardinality::OneToMany
                };
                Some(Link {
                    parent,
                    child,
                    kind,
                    label: format!("{}  {}", kind.label(), pairs),
                })
            })
            .collect();

        self.relayout();
    }

    fn relayout(&mut self) {
        let nodes: Vec<Node> = self
            .tables
            .iter()
            .zip(self.info.iter())
            .map(|(t, i)| Node {
                id: t.name.clone(),
                width: i.width,
                height: i.height,
            })
            .collect();
        // Parent first, so referenced tables sit above the ones referencing them.
        let edges: Vec<Edge> = self
            .links
            .iter()
            .map(|l| Edge {
                from: self.tables[l.parent].name.clone(),
                to: self.tables[l.child].name.clone(),
            })
            .collect();
        self.layout = graph_layout::layout(
            &nodes,
            &edges,
            LayoutOptions {
                layer_gap: 80.0,
                node_gap: 40.0,
                horizontal: false,
                sweeps: 4,
            },
        );
    }

    // -- Reading ------------------------------------------------------------

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    pub fn is_junction(&self, i: usize) -> bool {
        self.info[i].junction
    }

    pub fn position(&self, i: usize) -> Point {
        let name = &self.tables[i].name;
        if let Some(p) = self.pinned.get(name) {
            return *p;
        }
        self.layout
            .placements
            .get(name)
            .map(|p| Point::new(p.x, p.y))
            .unwrap_or(Point::ZERO)
    }

    pub fn size(&self, i: usize) -> (f32, f32) {
        (self.info[i].width, self.info[i].height)
    }

    pub fn rect(&self, i: usize) -> Rect {
        let p = self.position(i);
        let (w, h) = self.size(i);
        Rect::new(p.x, p.y, w, h)
    }

    /// Every table's box, or `None` for an empty scene.
    pub fn bounds(&self) -> Option<Rect> {
        (0..self.len()).map(|i| self.rect(i)).reduce(Rect::union)
    }

    /// The rows drawn for table `i`: at most `MAX_COLUMNS`, none when columns
    /// are hidden.
    pub fn rows(&self, i: usize) -> Vec<RowView> {
        if !self.options.show_columns {
            return Vec::new();
        }
        let info = &self.info[i];
        self.tables[i]
            .columns
            .iter()
            .enumerate()
            .take(MAX_COLUMNS)
            .map(|(c, col)| RowView {
                name: col.name.clone(),
                data_type: col.data_type.clone(),
                badge: if info.pk[c] {
                    Badge::Pk
                } else if info.fk[c] {
                    Badge::Fk
                } else {
                    Badge::None
                },
            })
            .collect()
    }

    /// "+3 more columns", or empty when everything is shown.
    pub fn more_label(&self, i: usize) -> String {
        let n = self.tables[i].columns.len();
        if self.options.show_columns && n > MAX_COLUMNS {
            format!("+{} more columns", n - MAX_COLUMNS)
        } else {
            String::new()
        }
    }

    pub fn type_width(&self, i: usize) -> f32 {
        type_column_width(&self.tables[i], self.info[i].width)
    }

    /// Whether the table matches the highlight filter. An empty filter
    /// matches nothing: it highlights, it does not hide.
    pub fn matches(&self, i: usize) -> bool {
        let needle = self.filter.trim().to_lowercase();
        !needle.is_empty() && self.tables[i].name.to_lowercase().contains(&needle)
    }

    pub fn filtering(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    /// The route of link `l` in the current arrangement.
    pub fn route_of(&self, l: &Link) -> Route {
        if l.parent == l.child {
            return route_self(self.rect(l.parent));
        }
        route(
            self.rect(l.parent),
            self.rect(l.child),
            Flow::Vertical,
            HEADER_H,
        )
    }

    /// A line about one table for the status strip:
    /// "orders - 4 columns - references users, roles - referenced by 1".
    pub fn describe(&self, i: usize) -> String {
        let name = &self.tables[i].name;
        let n = self.tables[i].columns.len();
        let mut parts = vec![format!("{} column{}", n, if n == 1 { "" } else { "s" })];
        let mut parents: Vec<&str> = Vec::new();
        for l in self.links.iter().filter(|l| l.child == i && l.parent != i) {
            let p = self.tables[l.parent].name.as_str();
            if !parents.contains(&p) {
                parents.push(p);
            }
        }
        if !parents.is_empty() {
            parts.push(format!("references {}", parents.join(", ")));
        }
        let mut children = 0;
        let mut seen: Vec<usize> = Vec::new();
        for l in self.links.iter().filter(|l| l.parent == i && l.child != i) {
            if !seen.contains(&l.child) {
                seen.push(l.child);
                children += 1;
            }
        }
        if children > 0 {
            parts.push(format!("referenced by {}", children));
        }
        if self.info[i].junction {
            parts.push("junction table".to_string());
        }
        format!("{} \u{b7} {}", name, parts.join(" \u{b7} "))
    }

    /// "12 tables, 14 relationships".
    pub fn summary(&self) -> String {
        let t = self.tables.len();
        let r = self.links.len();
        format!(
            "{} {}, {} {}",
            t,
            if t == 1 { "table" } else { "tables" },
            r,
            if r == 1 {
                "relationship"
            } else {
                "relationships"
            }
        )
    }

    // -- Editing ------------------------------------------------------------

    /// Start (or continue) a drag of table `i`.
    pub fn begin_drag(&mut self, i: usize) {
        if i < self.len() && self.drag_raw.map(|(d, _)| d) != Some(i) {
            self.drag_raw = Some((i, self.position(i)));
        }
    }

    /// Move table `i` by a world-space delta. Returns its new position, or
    /// `None` for an unknown table.
    pub fn drag(&mut self, i: usize, dx: f32, dy: f32) -> Option<Point> {
        if i >= self.len() {
            return None;
        }
        self.begin_drag(i);
        let (_, raw) = self.drag_raw.as_mut()?;
        raw.x += dx;
        raw.y += dy;
        let eff = if self.options.snap {
            Point::new(snap(raw.x), snap(raw.y))
        } else {
            *raw
        };
        self.pinned.insert(self.tables[i].name.clone(), eff);
        Some(eff)
    }

    pub fn end_drag(&mut self) {
        self.drag_raw = None;
    }

    /// Forget every manual position and lay the schema out again.
    pub fn auto_layout(&mut self) {
        self.pinned.clear();
        self.drag_raw = None;
        self.relayout();
    }

    /// Show or hide the column rows. Box sizes change, so the automatic layout
    /// is recomputed; manual positions are kept.
    pub fn set_show_columns(&mut self, on: bool) {
        if self.options.show_columns != on {
            self.options.show_columns = on;
            self.rebuild();
        }
    }

    /// The arrangement to persist: only what the user placed.
    pub fn pinned(&self) -> &HashMap<String, Point> {
        &self.pinned
    }

    /// The index of the topmost table under a world point.
    pub fn hit(&self, world: Point) -> Option<usize> {
        (0..self.len())
            .rev()
            .find(|i| self.rect(*i).contains(world))
    }

    // -- Export -------------------------------------------------------------

    /// The diagram as a standalone SVG document in the given palette.
    pub fn to_svg(&self, palette: &Palette) -> String {
        const MARGIN: f32 = 32.0;
        let bounds = self
            .bounds()
            .unwrap_or(Rect::new(0.0, 0.0, 200.0, 100.0))
            .inflate(MARGIN);
        let mut s = String::new();
        let _ = writeln!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"{x:.1} {y:.1} {w:.1} {h:.1}\" font-family=\"Helvetica, Arial, sans-serif\">",
            x = bounds.x, y = bounds.y, w = bounds.w, h = bounds.h
        );
        let _ = writeln!(
            s,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/>",
            bounds.x,
            bounds.y,
            bounds.w,
            bounds.h,
            palette.background.hex()
        );

        if self.options.show_relationships {
            for l in &self.links {
                let r = self.route_of(l);
                let m = l.kind == Cardinality::ManyToMany;
                let stroke = if m {
                    palette.muted_foreground
                } else {
                    palette.border_strong
                };
                let [a, b, c, d] = r.p;
                let _ = writeln!(
                    s,
                    "<path d=\"M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"{}/>",
                    a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y,
                    stroke.hex(),
                    if m { " stroke-dasharray=\"7 5\"" } else { "" }
                );
                // End markers as one path, so the file stays small.
                let mut marks = String::new();
                for line in marker_lines(&r) {
                    for (k, p) in line.iter().enumerate() {
                        let _ = write!(
                            marks,
                            "{} {:.1} {:.1} ",
                            if k == 0 { "M" } else { "L" },
                            p.x,
                            p.y
                        );
                    }
                }
                let _ = writeln!(
                    s,
                    "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"/>",
                    marks.trim(),
                    stroke.hex()
                );
                if self.options.show_labels {
                    let mid = r.midpoint();
                    let _ = writeln!(
                        s,
                        "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
                        mid.x, mid.y - 4.0, palette.muted_foreground.hex(), xml_escape(&l.label)
                    );
                }
            }
        }

        for i in 0..self.len() {
            let r = self.rect(i);
            let junction = self.info[i].junction;
            let _ = writeln!(
                s,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" fill=\"{}\" stroke=\"{}\"/>",
                r.x, r.y, r.w, r.h, palette.card.hex(), palette.border_strong.hex()
            );
            let _ = writeln!(
                s,
                "<path d=\"M {x:.1} {y1:.1} L {x:.1} {y0:.1} Q {x:.1} {y:.1} {xr:.1} {y:.1} L {xe:.1} {y:.1} Q {xw:.1} {y:.1} {xw:.1} {y0:.1} L {xw:.1} {y1:.1} Z\" fill=\"{}\"/>",
                if junction { palette.secondary.hex() } else { palette.elevated.hex() },
                x = r.x, y = r.y, y0 = r.y + 8.0, y1 = r.y + HEADER_H,
                xr = r.x + 8.0, xe = r.right() - 8.0, xw = r.right()
            );
            let _ = writeln!(
                s,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"13\" font-weight=\"600\" fill=\"{}\">{}</text>",
                r.x + SIDE_PAD, r.y + 20.0, palette.foreground.hex(), xml_escape(&self.tables[i].name)
            );
            if junction {
                let _ = writeln!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">M:N</text>",
                    r.right() - SIDE_PAD, r.y + 19.0, palette.muted_foreground.hex()
                );
            }
            for (k, row) in self.rows(i).iter().enumerate() {
                let y = r.y + HEADER_H + k as f32 * ROW_H + 14.0;
                let (colour, weight) = match row.badge {
                    Badge::Pk => (palette.primary, "600"),
                    _ => (palette.foreground, "400"),
                };
                if row.badge != Badge::None {
                    let _ = writeln!(
                        s,
                        "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"9\" font-weight=\"700\" fill=\"{}\">{}</text>",
                        r.x + SIDE_PAD, y, palette.muted_foreground.hex(), row.badge.label()
                    );
                }
                let _ = writeln!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" font-weight=\"{}\" fill=\"{}\">{}</text>",
                    r.x + SIDE_PAD + BADGE_W - 4.0, y, weight, colour.hex(), xml_escape(&row.name)
                );
                let _ = writeln!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
                    r.right() - SIDE_PAD, y, palette.dim_foreground.hex(), xml_escape(&row.data_type)
                );
            }
            let more = self.more_label(i);
            if !more.is_empty() {
                let y = r.y + HEADER_H + MAX_COLUMNS as f32 * ROW_H + 14.0;
                let _ = writeln!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" font-style=\"italic\" fill=\"{}\">{}</text>",
                    r.x + SIDE_PAD, y, palette.dim_foreground.hex(), xml_escape(&more)
                );
            }
        }
        s.push_str("</svg>\n");
        s
    }
}

fn snap(v: f32) -> f32 {
    (v / GRID).round() * GRID
}

/// Escape text for an SVG element body.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// -- View: zoom, pan, fit, minimap -------------------------------------------

/// Pan and zoom of the canvas. Pan is the screen position of the world
/// origin; a world point `w` is drawn at `pan + w * zoom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

impl View {
    /// Zoom to `new_zoom` keeping the world point under screen point
    /// `(ax, ay)` where it is.
    pub fn zoom_to(self, new_zoom: f32, ax: f32, ay: f32) -> View {
        let z = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let wx = (ax - self.pan_x) / self.zoom;
        let wy = (ay - self.pan_y) / self.zoom;
        View {
            zoom: z,
            pan_x: ax - wx * z,
            pan_y: ay - wy * z,
        }
    }

    /// A mouse wheel or trackpad scroll of `delta` about a screen point. Up
    /// (positive delta) zooms in.
    pub fn wheel(self, delta: f32, ax: f32, ay: f32) -> View {
        let factor = (delta * 0.0015).exp().clamp(0.8, 1.25);
        self.zoom_to(self.zoom * factor, ax, ay)
    }

    /// One toolbar step in or out about the viewport centre.
    pub fn step(self, zoom_in: bool, vw: f32, vh: f32) -> View {
        let f = if zoom_in { 1.25 } else { 0.8 };
        self.zoom_to(self.zoom * f, vw / 2.0, vh / 2.0)
    }

    /// Pan so world point `(wx, wy)` is in the middle of the viewport.
    pub fn center_on(self, wx: f32, wy: f32, vw: f32, vh: f32) -> View {
        View {
            zoom: self.zoom,
            pan_x: vw / 2.0 - wx * self.zoom,
            pan_y: vh / 2.0 - wy * self.zoom,
        }
    }

    /// The world rectangle the viewport shows.
    pub fn visible(self, vw: f32, vh: f32) -> Rect {
        Rect::new(
            -self.pan_x / self.zoom,
            -self.pan_y / self.zoom,
            vw / self.zoom,
            vh / self.zoom,
        )
    }
}

/// A view that shows all of `bounds` centred in a `vw` by `vh` viewport. It
/// never zooms in past 100%, so a small schema is not blown up.
pub fn fit(bounds: Rect, vw: f32, vh: f32) -> View {
    const PAD: f32 = 40.0;
    if bounds.w <= 0.0 || bounds.h <= 0.0 || vw <= 0.0 || vh <= 0.0 {
        return View::default();
    }
    let z = ((vw - 2.0 * PAD) / bounds.w)
        .min((vh - 2.0 * PAD) / bounds.h)
        .min(1.0)
        .clamp(MIN_ZOOM, MAX_ZOOM);
    View {
        zoom: z,
        pan_x: (vw - bounds.w * z) / 2.0 - bounds.x * z,
        pan_y: (vh - bounds.h * z) / 2.0 - bounds.y * z,
    }
}

/// "100%".
pub fn zoom_label(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// How the minimap maps world space to its own box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniMap {
    /// Minimap pixels per world pixel.
    pub scale: f32,
    /// The world point at the minimap's top-left.
    pub origin: Point,
    pub width: f32,
    pub height: f32,
}

/// Fit the diagram, with some margin, into at most `max_w` by `max_h`.
pub fn minimap(bounds: Rect, max_w: f32, max_h: f32) -> MiniMap {
    let cover = bounds.inflate((bounds.w.max(bounds.h) * 0.15).max(80.0));
    let scale = (max_w / cover.w).min(max_h / cover.h);
    MiniMap {
        scale,
        origin: Point::new(cover.x, cover.y),
        width: cover.w * scale,
        height: cover.h * scale,
    }
}

impl MiniMap {
    /// The world point under a point of the minimap.
    pub fn to_world(&self, mx: f32, my: f32) -> Point {
        Point::new(
            self.origin.x + mx / self.scale,
            self.origin.y + my / self.scale,
        )
    }
}

/// A file name for an exported diagram: `public-erd.svg`. Anything that is
/// not a letter, digit, dash or underscore becomes a dash, so a schema called
/// `my schema/v2` cannot name a path.
pub fn export_file_name(schema: &str) -> String {
    let stem: String = schema
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        "diagram-erd.svg".to_string()
    } else {
        format!("{}-erd.svg", stem)
    }
}

// -- Saved arrangement (erd-layouts.json) ------------------------------------

/// The store key for one schema: `<connection id>-<schema>`, as the web
/// diagram wrote it.
pub fn layout_key(connection_id: &str, schema: &str) -> String {
    format!("{}-{}", connection_id, schema)
}

/// `{ "table": { "x": 1.0, "y": 2.0 } }`.
pub fn positions_to_json(p: &HashMap<String, Point>) -> Value {
    let mut m = serde_json::Map::new();
    let mut names: Vec<&String> = p.keys().collect();
    names.sort();
    for k in names {
        let v = p[k];
        m.insert(k.clone(), json!({ "x": v.x, "y": v.y }));
    }
    Value::Object(m)
}

/// Read a saved arrangement. Entries that are not `{x, y}` numbers are
/// skipped, so one damaged entry does not lose the rest.
pub fn positions_from_json(v: &Value) -> HashMap<String, Point> {
    let mut out = HashMap::new();
    if let Some(obj) = v.as_object() {
        for (k, p) in obj {
            if let (Some(x), Some(y)) = (
                p.get("x").and_then(Value::as_f64),
                p.get("y").and_then(Value::as_f64),
            ) {
                if x.is_finite() && y.is_finite() {
                    out.insert(k.clone(), Point::new(x as f32, y as f32));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::DARK;
    use dbhive_core::models::ColumnInfo;

    fn col(name: &str, ty: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable: !pk,
            default_value: None,
            is_primary_key: pk,
            is_auto_increment: false,
        }
    }

    fn table(name: &str, cols: &[(&str, &str, bool)]) -> DiagramTable {
        DiagramTable {
            name: name.into(),
            columns: cols.iter().map(|(n, t, pk)| col(n, t, *pk)).collect(),
        }
    }

    fn fk(table: &str, cols: &[&str], parent: &str, parent_cols: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: format!("fk_{}_{}", table, parent),
            table: table.into(),
            schema: "public".into(),
            columns: cols.iter().map(|s| s.to_string()).collect(),
            referenced_table: parent.into(),
            referenced_schema: "public".into(),
            referenced_columns: parent_cols.iter().map(|s| s.to_string()).collect(),
            on_delete: None,
            on_update: None,
        }
    }

    /// users, roles, user_roles (junction), orders (not a junction).
    fn shop() -> ErdScene {
        let tables = vec![
            table(
                "users",
                &[("id", "integer", true), ("email", "text", false)],
            ),
            table("roles", &[("id", "integer", true), ("name", "text", false)]),
            table(
                "user_roles",
                &[
                    ("user_id", "integer", true),
                    ("role_id", "integer", true),
                    ("granted_at", "timestamp", false),
                ],
            ),
            table(
                "orders",
                &[
                    ("id", "integer", true),
                    ("user_id", "integer", false),
                    ("role_id", "integer", false),
                    ("total", "numeric", false),
                ],
            ),
        ];
        let fks = vec![
            fk("user_roles", &["user_id"], "users", &["id"]),
            fk("user_roles", &["role_id"], "roles", &["id"]),
            fk("orders", &["user_id"], "users", &["id"]),
            fk("orders", &["role_id"], "roles", &["id"]),
        ];
        ErdScene::new(tables, fks, &HashMap::new())
    }

    // -- junctions and relationship kinds ------------------------------------

    #[test]
    fn a_table_of_only_keys_and_timestamps_is_a_junction() {
        let s = shop();
        assert!(s.is_junction(2), "user_roles should be a junction");
    }

    #[test]
    fn a_table_with_its_own_data_is_not_a_junction_even_with_two_foreign_keys() {
        let s = shop();
        assert!(
            !s.is_junction(3),
            "orders has a total and must not be a junction"
        );
    }

    #[test]
    fn the_has_naming_convention_marks_a_junction() {
        let t = table(
            "posts_has_tags",
            &[
                ("a", "int", false),
                ("b", "int", false),
                ("c", "text", false),
                ("d", "text", false),
                ("e", "text", false),
                ("f", "text", false),
                ("g", "text", false),
            ],
        );
        assert!(is_junction(&t, &HashSet::new()));
    }

    #[test]
    fn relationships_through_a_junction_are_many_to_many_and_others_one_to_many() {
        let s = shop();
        let kinds: Vec<_> = s.links.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                Cardinality::ManyToMany,
                Cardinality::ManyToMany,
                Cardinality::OneToMany,
                Cardinality::OneToMany
            ]
        );
        assert!(s.links[0].label.starts_with("M:N"), "{}", s.links[0].label);
        assert!(s.links[2].label.starts_with("1:N"), "{}", s.links[2].label);
        assert!(s.links[2].label.contains("user_id \u{2192} id"));
    }

    #[test]
    fn links_run_from_parent_to_child() {
        let s = shop();
        let l = &s.links[2];
        assert_eq!(s.tables[l.parent].name, "users");
        assert_eq!(s.tables[l.child].name, "orders");
    }

    #[test]
    fn a_foreign_key_to_a_table_outside_the_scene_is_dropped() {
        let tables = vec![table("a", &[("id", "int", true)])];
        let s = ErdScene::new(
            tables,
            vec![fk("a", &["x"], "elsewhere", &["id"])],
            &HashMap::new(),
        );
        assert!(s.links.is_empty());
    }

    // -- rows, badges, the column cap ---------------------------------------

    #[test]
    fn badges_mark_primary_and_foreign_key_columns() {
        let s = shop();
        let rows = s.rows(3);
        assert_eq!(rows[0].badge, Badge::Pk);
        assert_eq!(rows[1].badge, Badge::Fk, "orders.user_id");
        assert_eq!(rows[3].badge, Badge::None);
        // A column that is both keeps the PK marker.
        assert_eq!(s.rows(2)[0].badge, Badge::Pk);
    }

    #[test]
    fn a_wide_table_shows_ten_columns_and_a_more_line() {
        let cols: Vec<(String, &str, bool)> = (0..14)
            .map(|i| (format!("c{}", i), "text", i == 0))
            .collect();
        let refs: Vec<(&str, &str, bool)> =
            cols.iter().map(|(n, t, p)| (n.as_str(), *t, *p)).collect();
        let s = ErdScene::new(vec![table("wide", &refs)], vec![], &HashMap::new());
        assert_eq!(s.rows(0).len(), MAX_COLUMNS);
        assert_eq!(s.more_label(0), "+4 more columns");
        // Height counts the ten rows plus the more line.
        assert_eq!(s.size(0).1, HEADER_H + 11.0 * ROW_H + FOOT_H);
    }

    #[test]
    fn exactly_ten_columns_has_no_more_line() {
        let cols: Vec<(String, &str, bool)> = (0..10)
            .map(|i| (format!("c{}", i), "text", false))
            .collect();
        let refs: Vec<(&str, &str, bool)> =
            cols.iter().map(|(n, t, p)| (n.as_str(), *t, *p)).collect();
        let s = ErdScene::new(vec![table("ten", &refs)], vec![], &HashMap::new());
        assert_eq!(s.more_label(0), "");
    }

    #[test]
    fn hiding_columns_collapses_boxes_and_keeps_manual_positions() {
        let mut s = shop();
        s.drag(0, 500.0, 500.0);
        let pos = s.position(0);
        let tall = s.size(0).1;
        s.set_show_columns(false);
        assert_eq!(s.size(0).1, HEADER_H);
        assert!(s.size(0).1 < tall);
        assert!(s.rows(0).is_empty());
        assert_eq!(s.position(0), pos, "a manual position was lost");
    }

    #[test]
    fn box_width_stays_inside_its_limits_for_absurd_names() {
        let t = table(&"x".repeat(400), &[(&"y".repeat(400), "text", false)]);
        let (w, _) = node_size(&t, true, false);
        assert!(w <= MAX_WIDTH && w >= MIN_WIDTH);
    }

    // -- layout, dragging, snapping -----------------------------------------

    #[test]
    fn parents_are_laid_out_above_children() {
        let s = shop();
        assert!(
            s.position(0).y < s.position(3).y,
            "users should be above orders"
        );
        assert!(
            s.position(1).y < s.position(2).y,
            "roles should be above user_roles"
        );
    }

    #[test]
    fn dragging_adds_the_delta_and_pins_the_table() {
        let mut s = shop();
        s.options.snap = false;
        let p0 = s.position(1);
        s.drag(1, 10.0, 5.0);
        let p = s.drag(1, 10.0, 5.0).unwrap();
        assert_eq!(
            p,
            Point::new(p0.x + 20.0, p0.y + 10.0),
            "deltas must accumulate"
        );
        assert!(s.pinned().contains_key("roles"));
        assert_eq!(s.pinned().len(), 1, "only the dragged table is pinned");
    }

    #[test]
    fn snapping_rounds_to_the_grid_without_losing_small_moves() {
        let mut s = shop();
        s.options.snap = true;
        s.begin_drag(0);
        // Four 5px nudges: each alone would round back, together they cross.
        let mut last = s.position(0);
        for _ in 0..4 {
            last = s.drag(0, 5.0, 0.0).unwrap();
        }
        s.end_drag();
        assert_eq!(last.x % GRID, 0.0, "{} is off the grid", last.x);
        let start = shop().position(0).x;
        assert_eq!(last.x, snap(start + 20.0), "sub-grid movement was lost");
    }

    #[test]
    fn dragging_an_unknown_table_does_nothing() {
        let mut s = shop();
        assert_eq!(s.drag(99, 1.0, 1.0), None);
        assert!(s.pinned().is_empty());
    }

    #[test]
    fn auto_layout_forgets_manual_positions() {
        let mut s = shop();
        let auto = s.position(0);
        s.drag(0, 300.0, 300.0);
        assert_ne!(s.position(0), auto);
        s.auto_layout();
        assert_eq!(s.position(0), auto);
        assert!(s.pinned().is_empty());
    }

    #[test]
    fn a_saved_arrangement_is_applied_and_unknown_names_are_ignored() {
        let mut saved = HashMap::new();
        saved.insert("users".to_string(), Point::new(640.0, 320.0));
        saved.insert("dropped_table".to_string(), Point::new(1.0, 1.0));
        let s = ErdScene::new(
            vec![
                table("users", &[("id", "int", true)]),
                table("roles", &[("id", "int", true)]),
            ],
            vec![],
            &saved,
        );
        assert_eq!(s.position(0), Point::new(640.0, 320.0));
        assert_eq!(s.pinned().len(), 1);
    }

    #[test]
    fn hit_finds_the_topmost_table() {
        let s = shop();
        let r = s.rect(2);
        assert_eq!(s.hit(Point::new(r.x + 3.0, r.y + 3.0)), Some(2));
        assert_eq!(s.hit(Point::new(-9999.0, -9999.0)), None);
    }

    #[test]
    fn the_filter_highlights_and_never_hides() {
        let mut s = shop();
        assert!(!s.matches(0) && !s.filtering());
        s.filter = " ROLE ".into();
        assert!(s.filtering());
        assert!(s.matches(1) && s.matches(2), "roles and user_roles");
        assert!(!s.matches(0));
    }

    #[test]
    fn describe_names_parents_children_and_junctions() {
        let s = shop();
        assert_eq!(
            s.describe(3),
            "orders \u{b7} 4 columns \u{b7} references users, roles"
        );
        assert_eq!(
            s.describe(0),
            "users \u{b7} 2 columns \u{b7} referenced by 2"
        );
        assert!(
            s.describe(2).ends_with("junction table"),
            "{}",
            s.describe(2)
        );
    }

    #[test]
    fn export_names_cannot_escape_a_directory() {
        assert_eq!(export_file_name("public"), "public-erd.svg");
        assert_eq!(export_file_name("my schema/v2"), "my-schema-v2-erd.svg");
        assert_eq!(export_file_name("../../etc"), "etc-erd.svg");
        assert_eq!(export_file_name("  "), "diagram-erd.svg");
        assert!(
            !export_file_name("a/b\\c").contains('/') && !export_file_name("a/b\\c").contains('\\')
        );
    }

    #[test]
    fn summary_pluralises() {
        assert_eq!(shop().summary(), "4 tables, 4 relationships");
        let one = ErdScene::new(
            vec![table("a", &[("id", "int", true)])],
            vec![],
            &HashMap::new(),
        );
        assert_eq!(one.summary(), "1 table, 0 relationships");
    }

    // -- routing --------------------------------------------------------------

    #[test]
    fn a_child_below_its_parent_is_joined_bottom_to_top() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(20.0, 300.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        assert_eq!(r.p[0], Point::new(100.0, 100.0));
        assert_eq!(r.p[3], Point::new(120.0, 300.0));
        assert_eq!(r.start_dir, Point::new(0.0, 1.0));
        assert_eq!(r.end_dir, Point::new(0.0, -1.0));
    }

    #[test]
    fn a_child_dragged_above_its_parent_still_connects_sensibly() {
        let r = route(
            Rect::new(0.0, 400.0, 200.0, 100.0),
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        assert_eq!(
            r.p[0],
            Point::new(100.0, 400.0),
            "leaves the top of the parent"
        );
        assert_eq!(
            r.p[3],
            Point::new(100.0, 100.0),
            "enters the bottom of the child"
        );
    }

    #[test]
    fn overlapping_rows_fall_back_to_side_to_side() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(260.0, 20.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        assert_eq!(r.start_dir, Point::new(1.0, 0.0));
        assert_eq!(r.end_dir, Point::new(-1.0, 0.0));
    }

    #[test]
    fn horizontal_flow_runs_right_edge_to_left_edge_at_header_height() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 61.0),
            Rect::new(400.0, 100.0, 200.0, 61.0),
            Flow::Horizontal,
            30.0,
        );
        assert_eq!(r.p[0], Point::new(200.0, 15.0));
        assert_eq!(r.p[3], Point::new(400.0, 115.0));
    }

    #[test]
    fn touching_boxes_still_get_a_curve_that_does_not_collapse() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 60.0),
            Rect::new(200.0, 0.0, 200.0, 60.0),
            Flow::Horizontal,
            30.0,
        );
        assert!(r.p[1].x - r.p[0].x >= 40.0 - f32::EPSILON);
    }

    #[test]
    fn a_self_reference_loops_out_of_the_right_edge() {
        let r = route_self(Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(r.p[0].x, 200.0);
        assert_eq!(r.p[3].x, 200.0);
        assert!(r.p[1].x > 200.0 && r.p[2].x > 200.0);
        assert!(r.p[3].y > r.p[0].y);
    }

    // -- edge paths -----------------------------------------------------------

    fn numbers(cmd: &str) -> Vec<f32> {
        cmd.split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect()
    }

    #[test]
    fn every_coordinate_of_a_path_lies_inside_its_box() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(300.0, 400.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        for (dashed, markers) in [(false, false), (false, true), (true, true)] {
            let e = edge_path(&r, dashed, markers);
            for pair in numbers(&e.commands).chunks(2) {
                assert!(
                    pair[0] >= 0.0 && pair[0] <= e.bounds.w,
                    "x {} outside 0..{} ({dashed} {markers})",
                    pair[0],
                    e.bounds.w
                );
                assert!(
                    pair[1] >= 0.0 && pair[1] <= e.bounds.h,
                    "y {} outside 0..{}",
                    pair[1],
                    e.bounds.h
                );
            }
        }
    }

    #[test]
    fn a_solid_edge_is_one_curve_and_a_dashed_edge_is_many_short_pieces() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(0.0, 400.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        let solid = edge_path(&r, false, false);
        assert_eq!(solid.commands.matches('C').count(), 1);
        assert_eq!(solid.commands.matches('M').count(), 1);
        let dashed = edge_path(&r, true, false);
        assert!(
            dashed.commands.matches('M').count() > 10,
            "{}",
            dashed.commands
        );
        assert_eq!(dashed.commands.matches('C').count(), 0);
    }

    #[test]
    fn markers_add_a_bar_and_three_prongs() {
        let r = route(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(0.0, 400.0, 200.0, 100.0),
            Flow::Vertical,
            HEADER_H,
        );
        let plain = edge_path(&r, false, false);
        let marked = edge_path(&r, false, true);
        assert_eq!(
            marked.commands.matches('M').count(),
            plain.commands.matches('M').count() + 4
        );
    }

    // -- view -------------------------------------------------------------------

    #[test]
    fn zooming_about_a_point_keeps_the_world_point_under_it() {
        let v = View {
            zoom: 1.0,
            pan_x: 30.0,
            pan_y: -20.0,
        };
        let (ax, ay) = (400.0, 300.0);
        let wx = (ax - v.pan_x) / v.zoom;
        let wy = (ay - v.pan_y) / v.zoom;
        let z = v.zoom_to(2.0, ax, ay);
        assert!((z.pan_x + wx * z.zoom - ax).abs() < 0.001);
        assert!((z.pan_y + wy * z.zoom - ay).abs() < 0.001);
    }

    #[test]
    fn zoom_is_clamped() {
        let v = View::default();
        assert_eq!(v.zoom_to(100.0, 0.0, 0.0).zoom, MAX_ZOOM);
        assert_eq!(v.zoom_to(0.0001, 0.0, 0.0).zoom, MIN_ZOOM);
    }

    #[test]
    fn a_wheel_up_zooms_in_and_down_zooms_out_by_a_bounded_step() {
        let v = View::default();
        assert!(v.wheel(10.0, 0.0, 0.0).zoom > 1.0);
        assert!(v.wheel(-10.0, 0.0, 0.0).zoom < 1.0);
        let big = v.wheel(100_000.0, 0.0, 0.0).zoom;
        assert!(big <= 1.25 + 1e-4, "one event zoomed by {}", big);
    }

    #[test]
    fn fit_shows_everything_centred_and_never_zooms_past_100_percent() {
        let b = Rect::new(100.0, 50.0, 2000.0, 1000.0);
        let v = fit(b, 1000.0, 600.0);
        assert!(v.zoom < 1.0);
        let vis = v.visible(1000.0, 600.0);
        assert!(
            vis.x <= b.x && vis.y <= b.y && vis.right() >= b.right() && vis.bottom() >= b.bottom(),
            "{:?} does not contain {:?}",
            vis,
            b
        );
        // Centred: equal slack either side.
        assert!(((b.x - vis.x) - (vis.right() - b.right())).abs() < 0.01);

        let small = fit(Rect::new(0.0, 0.0, 100.0, 100.0), 1000.0, 600.0);
        assert_eq!(small.zoom, 1.0);
    }

    #[test]
    fn fit_with_nothing_or_no_room_is_the_identity() {
        assert_eq!(fit(Rect::default(), 1000.0, 600.0), View::default());
        assert_eq!(
            fit(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, 0.0),
            View::default()
        );
    }

    #[test]
    fn center_on_puts_the_world_point_mid_viewport() {
        let v = View {
            zoom: 0.5,
            pan_x: 0.0,
            pan_y: 0.0,
        }
        .center_on(1000.0, 400.0, 800.0, 600.0);
        assert_eq!(v.pan_x + 1000.0 * v.zoom, 400.0);
        assert_eq!(v.pan_y + 400.0 * v.zoom, 300.0);
    }

    #[test]
    fn step_zooms_about_the_viewport_centre() {
        let v = View::default();
        let z = v.step(true, 800.0, 600.0);
        assert!(z.zoom > 1.0);
        // The centre stays fixed.
        assert!((z.pan_x + (400.0 - v.pan_x) * z.zoom - 400.0).abs() < 0.001);
    }

    #[test]
    fn zoom_label_is_a_percentage() {
        assert_eq!(zoom_label(1.0), "100%");
        assert_eq!(zoom_label(0.254), "25%");
    }

    #[test]
    fn the_minimap_covers_the_diagram_and_maps_back_to_world() {
        let b = Rect::new(0.0, 0.0, 2000.0, 1000.0);
        let m = minimap(b, 200.0, 140.0);
        assert!(m.width <= 200.0 + 0.01 && m.height <= 140.0 + 0.01);
        // The diagram is inside the map, margin on every side.
        assert!(m.origin.x < 0.0 && m.origin.y < 0.0);
        let w = m.to_world(m.width / 2.0, m.height / 2.0);
        assert!(
            (w.x - 1000.0).abs() < 1.0 && (w.y - 500.0).abs() < 1.0,
            "{:?}",
            w
        );
    }

    // -- persistence -----------------------------------------------------------

    #[test]
    fn layouts_are_keyed_the_way_the_web_diagram_keyed_them() {
        assert_eq!(layout_key("conn-1", "public"), "conn-1-public");
    }

    #[test]
    fn positions_round_trip_through_json() {
        let mut p = HashMap::new();
        p.insert("users".to_string(), Point::new(16.0, 32.5));
        p.insert("orders".to_string(), Point::new(-48.0, 0.0));
        let v = positions_to_json(&p);
        assert_eq!(v["users"]["x"], 16.0);
        assert_eq!(positions_from_json(&v), p);
    }

    #[test]
    fn the_web_diagrams_file_format_is_read() {
        let v: Value =
            serde_json::from_str(r#"{"users":{"x":120,"y":40.5},"orders":{"x":300,"y":40}}"#)
                .unwrap();
        let p = positions_from_json(&v);
        assert_eq!(p["users"], Point::new(120.0, 40.5));
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn a_damaged_entry_is_skipped_without_losing_the_rest() {
        let v: Value =
            serde_json::from_str(r#"{"ok":{"x":1,"y":2},"bad":{"x":"one"},"worse":7}"#).unwrap();
        let p = positions_from_json(&v);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("ok"));
        assert!(positions_from_json(&Value::Null).is_empty());
    }

    // -- svg --------------------------------------------------------------------

    #[test]
    fn svg_is_a_document_with_every_table_and_relationship() {
        let s = shop();
        let svg = s.to_svg(&DARK);
        assert!(svg.starts_with("<svg "), "{}", &svg[..40]);
        assert!(svg.trim_end().ends_with("</svg>"));
        for t in ["users", "roles", "user_roles", "orders"] {
            assert!(svg.contains(&format!(">{}</text>", t)), "missing {}", t);
        }
        assert_eq!(
            svg.matches("stroke-dasharray").count(),
            2,
            "the two M:N edges are dashed"
        );
        assert!(svg.contains("1:N"));
        assert!(svg.contains("M:N"));
    }

    #[test]
    fn svg_escapes_names_and_respects_the_relationship_toggle() {
        let s = ErdScene::new(
            vec![table("a<b>&\"c\"", &[("x&y", "text", false)])],
            vec![],
            &HashMap::new(),
        );
        let svg = s.to_svg(&DARK);
        assert!(svg.contains("a&lt;b&gt;&amp;&quot;c&quot;"), "{}", svg);
        assert!(!svg.contains("a<b>"), "name was not escaped");

        let mut shop = shop();
        shop.options.show_relationships = false;
        let without = shop.to_svg(&DARK);
        assert!(
            !without.contains("stroke-width=\"1.5\""),
            "relationships should be omitted"
        );
        assert!(!without.contains("1:N"), "labels belong to relationships");
    }

    #[test]
    fn svg_of_an_empty_scene_is_still_a_valid_document() {
        let s = ErdScene::new(vec![], vec![], &HashMap::new());
        let svg = s.to_svg(&DARK);
        assert!(svg.starts_with("<svg ") && svg.contains("</svg>"));
    }
}
