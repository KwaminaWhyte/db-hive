//! Virtualised result set grid.
//!
//! Replaces the TanStack Table + react-virtual stack. Backed by
//! `egui_extras::TableBuilder`, which only lays out the rows currently on
//! screen, so a 50,000 row result (the driver cap, `MAX_RESULT_ROWS`) costs
//! the same to draw as ten rows.
//!
//! Values arrive as `serde_json::Value` because that is what every driver
//! normalises to. Rendering is deliberately explicit about NULL: a database
//! NULL and the string "NULL" are different things and must not look alike.

use dbhive_core::drivers::QueryResult;
use egui_extras::{Column, TableBuilder};

use crate::theme::Palette;
use crate::widgets::ui_kit::thousands;

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
fn cell_text(value: &serde_json::Value) -> (String, bool) {
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
fn visible_rows(result: &QueryResult, state: &GridState) -> Vec<usize> {
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
/// NULL sorts before everything, matching the grid the web shell had.
fn compare(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> std::cmp::Ordering {
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
        (Some(x), Some(y)) => cell_text(x).0.to_lowercase().cmp(&cell_text(y).0.to_lowercase()),
    }
}

/// Draw the grid. Returns the row index the user clicked this frame, if any.
pub fn show(
    ui: &mut egui::Ui,
    palette: &Palette,
    result: &QueryResult,
    state: &mut GridState,
) -> Option<usize> {
    if result.columns.is_empty() {
        if let Some(affected) = result.rows_affected {
            ui.label(crate::theme::text::muted(
                palette,
                format!("{} row(s) affected", affected),
            ));
        } else {
            ui.label(crate::theme::text::muted(palette, "No results"));
        }
        return None;
    }

    let rows = visible_rows(result, state);
    let mut clicked = None;

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text(format!("{}  Filter rows", crate::theme::glyph::SEARCH))
                .desired_width(220.0)
                .margin(egui::Margin::symmetric(8.0_f32, 5.0_f32)),
        );
        // Showing "40 of 40" on an unfiltered grid is noise; only say it
        // when filtering actually removed something.
        let label = if rows.len() == result.rows.len() {
            format!("{} rows", thousands(result.rows.len()))
        } else {
            format!(
                "{} of {} rows",
                thousands(rows.len()),
                thousands(result.rows.len())
            )
        };
        ui.label(crate::theme::text::muted(palette, label));
        if state.sort.is_some()
            && crate::widgets::ui_kit::ghost_button(ui, palette, "Clear sort").clicked()
        {
            state.sort = None;
        }
    });
    ui.add_space(6.0);

    let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;

    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0);
    for _ in &result.columns {
        builder = builder.column(Column::auto().at_least(80.0).clip(true));
    }

    builder
        .header(text_height + 4.0, |mut header| {
            for (i, name) in result.columns.iter().enumerate() {
                header.col(|ui| {
                    let sorted_here = matches!(state.sort, Some((c, _)) if c == i);
                    let arrow = match state.sort {
                        Some((c, false)) if c == i => " ⏶",
                        Some((c, true)) if c == i => " ⏷",
                        _ => "",
                    };
                    let label = format!("{}{}", name, arrow);
                    // Column names are uppercase and tracked out, so the
                    // header reads as a header and not as another data row.
                    let text = egui::RichText::new(label.to_uppercase())
                        .size(crate::theme::size::SMALL)
                        .strong()
                        .color(if sorted_here {
                            palette.primary
                        } else {
                            palette.muted_foreground
                        });
                    if ui
                        .add(egui::Label::new(text).sense(egui::Sense::click()))
                        .on_hover_text("Sort by this column")
                        .clicked()
                    {
                        state.sort = match state.sort {
                            Some((c, false)) if c == i => Some((i, true)),
                            Some((c, true)) if c == i => None,
                            _ => Some((i, false)),
                        };
                    }
                });
            }
        })
        .body(|body| {
            body.rows(text_height, rows.len(), |mut row| {
                let row_index = rows[row.index()];
                row.set_selected(state.selected_row == Some(row_index));

                for col in 0..result.columns.len() {
                    row.col(|ui| {
                        let value = result.rows[row_index].get(col);
                        match value {
                            Some(v) => {
                                let (text, is_null) = cell_text(v);
                                let rich = if is_null {
                                    egui::RichText::new("NULL")
                                        .italics()
                                        .color(palette.muted_foreground)
                                } else {
                                    egui::RichText::new(text.clone()).color(palette.foreground)
                                };
                                // Long values are clipped by the column; the
                                // full text is available on hover rather than
                                // wrapping and blowing up the row height.
                                ui.add(egui::Label::new(rich).truncate())
                                    .on_hover_text(text);
                            }
                            None => {
                                ui.label("");
                            }
                        }
                    });
                }

                if row.response().clicked() {
                    clicked = Some(row_index);
                }
            });
        });

    if let Some(r) = clicked {
        state.selected_row = Some(r);
    }
    clicked
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
