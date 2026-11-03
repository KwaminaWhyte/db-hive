//! A minimal chart of a result set: a bar or line series of one numeric
//! column grouped by another.
//!
//! Pure logic. It picks sensible default columns, sums the Y values per X
//! value (so a result with several rows per group still draws one mark per
//! group), and writes the SVG path data the Slint `ChartPlot` draws, in a
//! 1000 by 1000 space so the chart does not care how big it is on screen.
//!
//! Deliberately small: one series, Bar and Line, no PNG export.

use serde_json::Value;

use crate::format::thousands;
use crate::results_grid::cell_text;

/// Most groups drawn. More than this is unreadable as bars and a result that
/// large is better exported; the chart says how many were left out.
pub const MAX_POINTS: usize = 100;

/// Rows read to decide which columns are numeric.
const SAMPLE_ROWS: usize = 200;

/// The plot's own coordinate space.
const SPACE: f64 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    Bar,
    Line,
}

impl ChartKind {
    pub const ALL: [ChartKind; 2] = [ChartKind::Bar, ChartKind::Line];

    pub fn label(self) -> &'static str {
        match self {
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
        }
    }

    pub fn labels() -> Vec<String> {
        Self::ALL.iter().map(|k| k.label().to_string()).collect()
    }

    pub fn index(self) -> i32 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as i32
    }

    pub fn at(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(ChartKind::Bar)
    }
}

/// What to draw: the kind, and the column indices for X (the group) and Y
/// (the number summed per group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub x: usize,
    pub y: usize,
}

/// A value as a number: a JSON number, or a string that is entirely one
/// (drivers hand NUMERIC and DECIMAL columns back as strings).
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                t.parse::<f64>().ok().filter(|f| f.is_finite())
            }
        }
        _ => None,
    }
}

/// Columns that hold numbers: every non-null value in the first rows is a
/// number, and at least one is. Returns column indices in order.
pub fn numeric_columns(columns: &[String], rows: &[&Vec<Value>]) -> Vec<usize> {
    (0..columns.len())
        .filter(|&c| {
            let mut seen = false;
            for row in rows.iter().take(SAMPLE_ROWS) {
                match row.get(c) {
                    None | Some(Value::Null) => {}
                    Some(v) => match number(v) {
                        Some(_) => seen = true,
                        None => return false,
                    },
                }
            }
            seen
        })
        .collect()
}

/// A starting chart: Y is the first numeric column, X the first column that
/// is not numeric (else the first column that is not Y). `None` when there is
/// no numeric column to chart.
pub fn default_spec(columns: &[String], rows: &[&Vec<Value>]) -> Option<ChartSpec> {
    let numeric = numeric_columns(columns, rows);
    let y = *numeric.first()?;
    let x = (0..columns.len())
        .find(|c| !numeric.contains(c))
        .or_else(|| (0..columns.len()).find(|&c| c != y))
        .unwrap_or(y);
    Some(ChartSpec {
        kind: ChartKind::Bar,
        x,
        y,
    })
}

/// One group: its label and the sum of its Y values.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub label: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartData {
    /// At most [`MAX_POINTS`], in first-seen order.
    pub points: Vec<Point>,
    /// Groups in the result, before the cap.
    pub groups: usize,
    /// Rows whose Y was NULL or not a number, so they added nothing.
    pub skipped: usize,
}

/// Sum Y per X, keeping the order each X first appears in.
pub fn aggregate(rows: &[&Vec<Value>], spec: ChartSpec) -> ChartData {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut all: Vec<Point> = Vec::new();
    let mut skipped = 0;
    for row in rows {
        let Some(y) = row.get(spec.y).and_then(number) else {
            skipped += 1;
            continue;
        };
        let label = row.get(spec.x).map(|v| cell_text(v).0).unwrap_or_default();
        match index.get(&label) {
            Some(&i) => all[i].value += y,
            None => {
                index.insert(label.clone(), all.len());
                all.push(Point { label, value: y });
            }
        }
    }
    let groups = all.len();
    all.truncate(MAX_POINTS);
    ChartData {
        points: all,
        groups,
        skipped,
    }
}

/// A number short enough for an axis: `1.2k`, `3.4M`, `0.25`, `-7`.
pub fn axis_number(v: f64) -> String {
    let a = v.abs();
    let (scaled, suffix) = if a >= 1e9 {
        (v / 1e9, "B")
    } else if a >= 1e6 {
        (v / 1e6, "M")
    } else if a >= 1e3 {
        (v / 1e3, "k")
    } else {
        (v, "")
    };
    let text = if scaled.fract().abs() < 1e-9 {
        format!("{}", scaled.round() as i64)
    } else if scaled.abs() >= 100.0 {
        format!("{:.0}", scaled)
    } else if scaled.abs() >= 10.0 {
        format!("{:.1}", scaled)
    } else {
        format!("{:.2}", scaled)
    };
    // Trim "1.50" to "1.5" and "2.00" to "2".
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    format!("{text}{suffix}")
}

/// A label cut to fit under an axis.
fn short(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        label.to_string()
    } else {
        let cut: String = label.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Everything the plot draws.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartView {
    /// Path data: filled rectangles for bars, a polyline for a line.
    pub commands: String,
    /// A closed area under a line; empty for bars.
    pub area: String,
    pub y_top: String,
    pub y_mid: String,
    pub y_bottom: String,
    pub x_start: String,
    pub x_end: String,
    /// "Sum of revenue by name, 24 groups".
    pub caption: String,
    /// Set when groups were left out or rows added nothing.
    pub notice: String,
}

fn f(v: f64) -> String {
    // One decimal is plenty in a 1000 unit space.
    format!("{:.1}", v)
}

/// The value range: bars always include zero (a bar's length is its value);
/// a line spans its own values. Never zero wide.
fn range(points: &[Point], kind: ChartKind) -> (f64, f64) {
    let mut lo = points.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
    let mut hi = points
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max);
    if kind == ChartKind::Bar {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    if hi - lo < f64::EPSILON {
        hi = lo + 1.0;
    }
    (lo, hi)
}

pub fn build(columns: &[String], data: &ChartData, spec: ChartSpec) -> Option<ChartView> {
    if data.points.is_empty() {
        return None;
    }
    let (lo, hi) = range(&data.points, spec.kind);
    let y_at = |v: f64| SPACE - (v - lo) / (hi - lo) * SPACE;
    let n = data.points.len();
    let mut commands = String::new();
    let mut area = String::new();
    match spec.kind {
        ChartKind::Bar => {
            let slot = SPACE / n as f64;
            let width = (slot * 0.7).min(160.0);
            let zero = y_at(0.0);
            for (i, p) in data.points.iter().enumerate() {
                let x0 = i as f64 * slot + (slot - width) / 2.0;
                let x1 = x0 + width;
                let y = y_at(p.value);
                // A zero value still draws a hairline so the group is visible.
                let (top, bottom) = if (y - zero).abs() < 1.0 {
                    (zero - 1.0, zero)
                } else {
                    (y.min(zero), y.max(zero))
                };
                commands.push_str(&format!(
                    "M {} {} L {} {} L {} {} L {} {} Z ",
                    f(x0),
                    f(top),
                    f(x1),
                    f(top),
                    f(x1),
                    f(bottom),
                    f(x0),
                    f(bottom)
                ));
            }
        }
        ChartKind::Line => {
            let xs: Vec<f64> = if n == 1 {
                vec![500.0]
            } else {
                (0..n).map(|i| i as f64 / (n - 1) as f64 * SPACE).collect()
            };
            for (i, (p, x)) in data.points.iter().zip(&xs).enumerate() {
                let y = y_at(p.value);
                if n == 1 {
                    // One point is a short flat mark.
                    commands.push_str(&format!(
                        "M {} {} L {} {}",
                        f(x - 20.0),
                        f(y),
                        f(x + 20.0),
                        f(y)
                    ));
                } else {
                    commands.push_str(&format!(
                        "{} {} {} ",
                        if i == 0 { "M" } else { "L" },
                        f(*x),
                        f(y)
                    ));
                }
            }
            if n > 1 {
                area = format!(
                    "{}L {} {} L {} {} Z",
                    commands,
                    f(*xs.last().unwrap()),
                    f(SPACE),
                    f(xs[0]),
                    f(SPACE)
                );
            }
        }
    }
    let x_name = columns.get(spec.x).map(String::as_str).unwrap_or("");
    let y_name = columns.get(spec.y).map(String::as_str).unwrap_or("");
    // The caption carries the group cap (it is always on screen); the notice
    // is for rows that added nothing.
    let mut notice = Vec::new();
    if data.skipped > 0 {
        notice.push(format!(
            "{} {} with no number in {} {} left out.",
            thousands(data.skipped),
            if data.skipped == 1 { "row" } else { "rows" },
            y_name,
            if data.skipped == 1 { "was" } else { "were" }
        ));
    }
    Some(ChartView {
        commands: commands.trim_end().to_string(),
        area,
        y_top: axis_number(hi),
        y_mid: axis_number((hi + lo) / 2.0),
        y_bottom: axis_number(lo),
        x_start: short(&data.points[0].label, 24),
        x_end: if n > 1 {
            short(&data.points[n - 1].label, 24)
        } else {
            String::new()
        },
        caption: if data.groups > data.points.len() {
            format!(
                "Sum of {y_name} by {x_name}, first {} of {} groups",
                thousands(data.points.len()),
                thousands(data.groups)
            )
        } else {
            format!(
                "Sum of {y_name} by {x_name}, {} {}",
                thousands(data.groups),
                if data.groups == 1 { "group" } else { "groups" }
            )
        },
        notice: notice.join(" "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cols(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn refs(rows: &[Vec<Value>]) -> Vec<&Vec<Value>> {
        rows.iter().collect()
    }

    fn sales() -> (Vec<String>, Vec<Vec<Value>>) {
        (
            cols(&["id", "region", "amount", "note"]),
            vec![
                vec![json!(1), json!("north"), json!(10), json!("a")],
                vec![json!(2), json!("south"), json!("5.5"), json!("b")],
                vec![json!(3), json!("north"), json!(20), json!(null)],
                vec![json!(4), json!("east"), json!(null), json!("d")],
            ],
        )
    }

    #[test]
    fn numeric_columns_accept_numbers_and_numeric_strings_only() {
        let (c, rows) = sales();
        // id and amount are numeric; region and note are text.
        assert_eq!(numeric_columns(&c, &refs(&rows)), vec![0, 2]);
    }

    #[test]
    fn a_column_of_only_nulls_is_not_numeric() {
        let c = cols(&["a"]);
        let rows = vec![vec![json!(null)], vec![json!(null)]];
        assert!(numeric_columns(&c, &refs(&rows)).is_empty());
    }

    #[test]
    fn default_spec_puts_a_text_column_on_x_and_a_number_on_y() {
        let (c, rows) = sales();
        let s = default_spec(&c, &refs(&rows)).unwrap();
        assert_eq!((s.x, s.y, s.kind), (1, 0, ChartKind::Bar));
    }

    #[test]
    fn default_spec_with_only_numbers_uses_two_different_columns() {
        let c = cols(&["a", "b"]);
        let rows = vec![vec![json!(1), json!(2)]];
        let s = default_spec(&c, &refs(&rows)).unwrap();
        assert_eq!((s.x, s.y), (1, 0));
        assert_eq!(
            default_spec(&cols(&["t"]), &refs(&[vec![json!("x")]])),
            None
        );
    }

    #[test]
    fn aggregate_sums_per_group_in_first_seen_order_and_counts_skips() {
        let (_, rows) = sales();
        let d = aggregate(
            &refs(&rows),
            ChartSpec {
                kind: ChartKind::Bar,
                x: 1,
                y: 2,
            },
        );
        assert_eq!(
            d.points,
            vec![
                Point {
                    label: "north".into(),
                    value: 30.0
                },
                Point {
                    label: "south".into(),
                    value: 5.5
                },
            ]
        );
        assert_eq!((d.groups, d.skipped), (2, 1));
    }

    #[test]
    fn a_null_group_is_labelled_null() {
        let rows = vec![vec![json!(null), json!(3)]];
        let d = aggregate(
            &refs(&rows),
            ChartSpec {
                kind: ChartKind::Bar,
                x: 0,
                y: 1,
            },
        );
        assert_eq!(d.points[0].label, "NULL");
    }

    #[test]
    fn the_point_cap_keeps_the_total_group_count() {
        let rows: Vec<Vec<Value>> = (0..250)
            .map(|i| vec![json!(format!("g{i}")), json!(i)])
            .collect();
        let d = aggregate(
            &refs(&rows),
            ChartSpec {
                kind: ChartKind::Line,
                x: 0,
                y: 1,
            },
        );
        assert_eq!((d.points.len(), d.groups), (MAX_POINTS, 250));
        let v = build(
            &cols(&["g", "v"]),
            &d,
            ChartSpec {
                kind: ChartKind::Line,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        assert_eq!(v.caption, "Sum of v by g, first 100 of 250 groups");
        assert!(v.notice.is_empty());
    }

    #[test]
    fn bars_draw_one_rectangle_per_group_from_the_zero_line() {
        let d = ChartData {
            points: vec![
                Point {
                    label: "a".into(),
                    value: 10.0,
                },
                Point {
                    label: "b".into(),
                    value: 5.0,
                },
            ],
            groups: 2,
            skipped: 0,
        };
        let v = build(
            &cols(&["x", "y"]),
            &d,
            ChartSpec {
                kind: ChartKind::Bar,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        assert_eq!(v.commands.matches('M').count(), 2);
        assert_eq!(v.commands.matches('Z').count(), 2);
        // The tallest bar reaches the top (y = 0) and every bar sits on the bottom (zero) line.
        // Two groups: 500 wide slots, bars capped at 160 and centred in them.
        assert!(
            v.commands
                .starts_with("M 170.0 0.0 L 330.0 0.0 L 330.0 1000.0"),
            "{}",
            v.commands
        );
        // The half-height bar starts at the middle.
        assert!(
            v.commands.contains("M 670.0 500.0 L 830.0 500.0"),
            "{}",
            v.commands
        );
        assert!(v.commands.contains("1000.0"), "{}", v.commands);
        assert_eq!((v.y_top.as_str(), v.y_bottom.as_str()), ("10", "0"));
        assert!(v.area.is_empty());
    }

    #[test]
    fn negative_bars_hang_below_the_zero_line() {
        let d = ChartData {
            points: vec![
                Point {
                    label: "a".into(),
                    value: 10.0,
                },
                Point {
                    label: "b".into(),
                    value: -10.0,
                },
            ],
            groups: 2,
            skipped: 0,
        };
        let v = build(
            &cols(&["x", "y"]),
            &d,
            ChartSpec {
                kind: ChartKind::Bar,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        // The range is -10..10, so zero is the middle (500) and the labels say so.
        assert_eq!(
            (v.y_top.as_str(), v.y_mid.as_str(), v.y_bottom.as_str()),
            ("10", "0", "-10")
        );
        assert!(v.commands.contains("500.0"), "{}", v.commands);
    }

    #[test]
    fn a_line_is_a_polyline_with_a_closed_area_under_it() {
        let d = ChartData {
            points: vec![
                Point {
                    label: "a".into(),
                    value: 100.0,
                },
                Point {
                    label: "b".into(),
                    value: 200.0,
                },
                Point {
                    label: "c".into(),
                    value: 150.0,
                },
            ],
            groups: 3,
            skipped: 0,
        };
        let v = build(
            &cols(&["x", "y"]),
            &d,
            ChartSpec {
                kind: ChartKind::Line,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        assert_eq!(v.commands, "M 0.0 1000.0 L 500.0 0.0 L 1000.0 500.0");
        assert!(v.area.starts_with(&v.commands) && v.area.ends_with("Z"));
        // A line spans its own values, not zero to max.
        assert_eq!((v.y_top.as_str(), v.y_bottom.as_str()), ("200", "100"));
        assert_eq!((v.x_start.as_str(), v.x_end.as_str()), ("a", "c"));
    }

    #[test]
    fn single_values_and_flat_series_do_not_divide_by_zero() {
        let one = ChartData {
            points: vec![Point {
                label: "only".into(),
                value: 7.0,
            }],
            groups: 1,
            skipped: 0,
        };
        for kind in ChartKind::ALL {
            let v = build(&cols(&["x", "y"]), &one, ChartSpec { kind, x: 0, y: 1 }).unwrap();
            assert!(
                !v.commands.contains("NaN") && !v.commands.contains("inf"),
                "{}",
                v.commands
            );
        }
        let flat = ChartData {
            points: vec![
                Point {
                    label: "a".into(),
                    value: 3.0,
                },
                Point {
                    label: "b".into(),
                    value: 3.0,
                },
            ],
            groups: 2,
            skipped: 0,
        };
        let v = build(
            &cols(&["x", "y"]),
            &flat,
            ChartSpec {
                kind: ChartKind::Line,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        assert!(
            !v.commands.contains("NaN") && !v.commands.contains("inf"),
            "{}",
            v.commands
        );
    }

    #[test]
    fn nothing_to_draw_is_none() {
        assert_eq!(
            build(
                &cols(&["x", "y"]),
                &ChartData::default(),
                ChartSpec {
                    kind: ChartKind::Bar,
                    x: 0,
                    y: 1
                }
            ),
            None
        );
    }

    #[test]
    fn axis_numbers_are_short() {
        assert_eq!(axis_number(0.0), "0");
        assert_eq!(axis_number(7.0), "7");
        assert_eq!(axis_number(0.25), "0.25");
        assert_eq!(axis_number(1.5), "1.5");
        assert_eq!(axis_number(12.34), "12.3");
        assert_eq!(axis_number(-7.0), "-7");
        assert_eq!(axis_number(999.0), "999");
        assert_eq!(axis_number(8960.0), "8.96k");
        assert_eq!(axis_number(12_000.0), "12k");
        assert_eq!(axis_number(1_250_000.0), "1.25M");
        assert_eq!(axis_number(3_000_000_000.0), "3B");
    }

    #[test]
    fn long_labels_and_notices() {
        let d = ChartData {
            points: vec![
                Point {
                    label: "x".repeat(60),
                    value: 1.0,
                },
                Point {
                    label: "b".into(),
                    value: 2.0,
                },
            ],
            groups: 2,
            skipped: 3,
        };
        let v = build(
            &cols(&["name", "n"]),
            &d,
            ChartSpec {
                kind: ChartKind::Bar,
                x: 0,
                y: 1,
            },
        )
        .unwrap();
        assert_eq!(v.x_start.chars().count(), 24);
        assert!(v.x_start.ends_with('…'));
        assert_eq!(v.notice, "3 rows with no number in n were left out.");
        assert_eq!(v.caption, "Sum of n by name, 2 groups");
    }
}
