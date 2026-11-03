//! SQL editor.
//!
//! Replaces Monaco, which was 4.1 MB of the old bundle and 75% of its JS.
//! Scope was set deliberately at "solid basics": syntax highlighting,
//! schema-aware autocomplete, find and replace, line numbers, and the
//! execute shortcut. No multi-cursor, folding or minimap.
//!
//! Highlighting runs through `egui_extras`' syntect integration, which
//! caches per string, so retyping a large statement does not re-highlight
//! the whole buffer every frame.
//!
//! The pure completion logic (`SchemaIndex`, `KEYWORDS`, `word_before`,
//! `candidates`, `replace_all`) lives in `dbhive_ui_model::sql_completion`
//! and is re-exported here, so `sql_editor::SchemaIndex` keeps working.

pub use dbhive_ui_model::sql_completion::*;

use crate::theme::Palette;

#[derive(Default)]
pub struct EditorState {
    pub find_open: bool,
    pub find: String,
    pub replace: String,
    /// Completion candidates for the word under the cursor.
    completions: Vec<String>,
    completion_selected: usize,
    /// Byte range of the word completion would replace.
    completion_range: Option<(usize, usize)>,
}

/// What the editor wants the host screen to do after this frame.
#[derive(Debug, PartialEq, Eq)]
pub enum EditorAction {
    None,
    Execute,
}

/// Draw the editor. `sql` is edited in place.
pub fn show(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut EditorState,
    schema: &SchemaIndex,
    sql: &mut String,
) -> EditorAction {
    let mut action = EditorAction::None;

    // Cmd/Ctrl+Enter executes, Cmd/Ctrl+F opens find. Checked before the
    // TextEdit consumes the keystroke.
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter) {
            action = EditorAction::Execute;
        }
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::F) {
            state.find_open = !state.find_open;
        }
    });

    if state.find_open {
        find_bar(ui, palette, state, sql);
    }

    let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
    let mut layouter = |ui: &egui::Ui, buf: &str, wrap_width: f32| {
        let mut job =
            egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, buf, "sql");
        job.wrap.max_width = wrap_width;
        ui.fonts(|f| f.layout_job(job))
    };

    let output = egui::ScrollArea::vertical()
        .id_salt("sql_editor_scroll")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                gutter(ui, palette, sql);
                egui::TextEdit::multiline(sql)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(12)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner
        })
        .inner;

    update_completions(state, schema, sql, &output);
    if let Some(chosen) = completion_popup(ui, palette, state, &output) {
        if let Some((start, end)) = state.completion_range {
            sql.replace_range(start..end, &chosen);
        }
        state.completions.clear();
        state.completion_range = None;
    }

    action
}

/// Line-number gutter, aligned to the editor's line height.
fn gutter(ui: &mut egui::Ui, palette: &Palette, sql: &str) {
    let lines = sql.lines().count().max(1);
    let width = (lines.to_string().len() as f32 * 8.0) + 10.0;
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.available_height()),
        egui::Layout::top_down(egui::Align::RIGHT),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for n in 1..=lines {
                ui.label(
                    egui::RichText::new(n.to_string())
                        .monospace()
                        .color(palette.muted_foreground),
                );
            }
        },
    );
}

fn find_bar(ui: &mut egui::Ui, palette: &Palette, state: &mut EditorState, sql: &mut String) {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .inner_margin(egui::Margin::symmetric(10.0_f32, 8.0_f32))
        .rounding(crate::theme::radius::CONTROL)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.find)
                        .hint_text("Find")
                        .desired_width(160.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut state.replace)
                        .hint_text("Replace with")
                        .desired_width(160.0),
                );
                let matches = if state.find.is_empty() {
                    0
                } else {
                    sql.matches(state.find.as_str()).count()
                };
                if ui
                    .add_enabled(matches > 0, egui::Button::new("Replace all"))
                    .clicked()
                {
                    replace_all(sql, &state.find, &state.replace);
                }
                crate::widgets::ui_kit::badge(
                    ui,
                    palette,
                    format!("{} matches", matches),
                    if matches > 0 {
                        crate::widgets::ui_kit::Tone::Accent
                    } else {
                        crate::widgets::ui_kit::Tone::Neutral
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(crate::theme::glyph::CLOSE)
                                    .size(9.0)
                                    .color(palette.dim_foreground),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .small(),
                        )
                        .on_hover_text("Close find")
                        .clicked()
                    {
                        state.find_open = false;
                    }
                });
            });
        });
    ui.add_space(4.0);
}

fn update_completions(
    state: &mut EditorState,
    schema: &SchemaIndex,
    sql: &str,
    output: &egui::text_edit::TextEditOutput,
) {
    let Some(cursor) = output.cursor_range.map(|r| r.primary.ccursor.index) else {
        state.completions.clear();
        state.completion_range = None;
        return;
    };
    // ccursor is a char index; the buffer is indexed in bytes.
    let byte_cursor = sql
        .char_indices()
        .nth(cursor)
        .map(|(b, _)| b)
        .unwrap_or(sql.len());

    match word_before(sql, byte_cursor) {
        Some((start, end, prefix)) => {
            state.completions = candidates(&prefix, schema);
            state.completion_range = Some((start, end));
            if state.completion_selected >= state.completions.len() {
                state.completion_selected = 0;
            }
        }
        None => {
            state.completions.clear();
            state.completion_range = None;
        }
    }
}

fn completion_popup(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut EditorState,
    output: &egui::text_edit::TextEditOutput,
) -> Option<String> {
    if state.completions.is_empty() {
        return None;
    }

    let mut chosen = None;
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
            state.completion_selected = (state.completion_selected + 1) % state.completions.len();
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
            state.completion_selected = state
                .completion_selected
                .checked_sub(1)
                .unwrap_or(state.completions.len() - 1);
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
            chosen = state.completions.get(state.completion_selected).cloned();
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            state.completions.clear();
        }
    });
    if chosen.is_some() || state.completions.is_empty() {
        return chosen;
    }

    let pos = output.response.rect.left_bottom();
    egui::Area::new(egui::Id::new("sql_completions"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(palette.popover)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .show(ui, |ui| {
                    ui.set_max_width(280.0);
                    for (i, c) in state.completions.iter().enumerate() {
                        let selected = i == state.completion_selected;
                        if ui.selectable_label(selected, c).clicked() {
                            chosen = Some(c.clone());
                        }
                    }
                    ui.label(crate::theme::text::muted(
                        palette,
                        "Tab to accept, Esc to dismiss",
                    ));
                });
        });
    chosen
}
