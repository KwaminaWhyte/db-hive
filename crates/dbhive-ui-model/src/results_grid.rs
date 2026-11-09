//! Result grid logic: cell formatting, filtering and sorting.
//!
//! `widgets::results_grid`. Values arrive as
//! `serde_json::Value` because that is what every driver normalises to.
//! Rendering is deliberately explicit about NULL: a database NULL and the
//! string "NULL" are different things and must not look alike.

use dbhive_core::drivers::QueryResult;

/// Grid state the caller owns across frames.
#[derive(Default)]
pub struct GridState {
    /// Column index the user sorted by, and whether it is descending.
    pub sort: Option<(usize, bool)>,
    /// Row the user last clicked, for selection highlighting.
    pub selected_row: Option<usize>,
    /// Free-text filter applied across all columns.
    pub filter: String,
}

/// Format one cell. NULL is rendered distinctly and never as the text "NULL".
pub fn cell_text(value: &serde_json::Value) -> (String, bool) {
    match value {
        serde_json::Value::Null => ("NULL".to_string(), true),
        serde_json::Value::String(s) => (s.clone(), false),
        serde_json::Value::Bool(b) => (b.to_string(), false),
        serde_json::Value::Number(n) => (n.to_string(), false),
        other => (other.to_string(), false),
    }
}

/// Row indices to display, after filtering and sorting.
///
/// Returned as indices rather than cloned rows so a large result set is not
/// duplicated every frame.
pub fn visible_rows(result: &QueryResult, state: &GridState) -> Vec<usize> {
    let mut idx: Vec<usize> = if state.filter.is_empty() {
        (0..result.rows.len()).collect()
    } else {
        let needle = state.filter.to_lowercase();
        (0..result.rows.len())
            .filter(|&r| {
                result.rows[r]
                    .iter()
                    .any(|v| cell_text(v).0.to_lowercase().contains(&needle))
            })
            .collect()
    };

    if let Some((col, desc)) = state.sort {
        idx.sort_by(|&a, &b| {
            let av = result.rows[a].get(col);
            let bv = result.rows[b].get(col);
            let ord = compare(av, bv);
            if desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }
    idx
}

/// Compare two cells for sorting.
///
/// Numbers compare numerically rather than lexically, so 10 sorts after 9.
/// NULL sorts before everything, matching the driver order.
pub fn compare(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> std::cmp::Ordering {
    use serde_json::Value;
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) | (Some(Value::Null), _) => Ordering::Less,
        (_, None) | (_, Some(Value::Null)) => Ordering::Greater,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(x), Some(y)) => cell_text(x)
            .0
            .to_lowercase()
            .cmp(&cell_text(y).0.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> QueryResult {
        QueryResult::with_data(
            vec!["id".into(), "name".into()],
            vec![
                vec![json!(10), json!("beta")],
                vec![json!(9), json!("alpha")],
                vec![json!(null), json!("gamma")],
            ],
        )
    }

    #[test]
    fn numbers_sort_numerically_not_lexically() {
        let r = sample();
        let state = GridState {
            sort: Some((0, false)),
            ..Default::default()
        };
        let order = visible_rows(&r, &state);
        // NULL first, then 9, then 10. Lexical order would put 10 before 9.
        assert_eq!(order, vec![2, 1, 0]);
    }

    #[test]
    fn descending_reverses() {
        let r = sample();
        let state = GridState {
            sort: Some((0, true)),
            ..Default::default()
        };
        assert_eq!(visible_rows(&r, &state), vec![0, 1, 2]);
    }

    #[test]
    fn filter_matches_any_column_case_insensitively() {
        let r = sample();
        let state = GridState {
            filter: "ALPHA".into(),
            ..Default::default()
        };
        assert_eq!(visible_rows(&r, &state), vec![1]);
    }

    #[test]
    fn null_renders_as_null_and_is_flagged() {
        assert_eq!(cell_text(&json!(null)), ("NULL".to_string(), true));
        // A literal string "NULL" must not be mistaken for a database NULL.
        assert_eq!(cell_text(&json!("NULL")), ("NULL".to_string(), false));
    }

    #[test]
    fn unfiltered_keeps_original_order() {
        let r = sample();
        assert_eq!(visible_rows(&r, &GridState::default()), vec![0, 1, 2]);
    }
}
