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
//!
//! The pure logic (cell text, filtering, sorting, `GridState`) lives in
//! `dbhive_ui_model::results_grid` and is re-exported here.

use dbhive_core::drivers::QueryResult;
use egui_extras::{Column, TableBuilder};

pub use dbhive_ui_model::results_grid::*;

use crate::theme::Palette;
use crate::widgets::ui_kit::thousands;

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
