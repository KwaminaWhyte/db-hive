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

use std::collections::BTreeSet;

use crate::theme::Palette;

/// Table and column names used to drive completion.
///
/// Rebuilt when the schema is (re)loaded, not per keystroke.
#[derive(Default, Clone)]
pub struct SchemaIndex {
    pub tables: BTreeSet<String>,
    pub columns: BTreeSet<String>,
}

impl SchemaIndex {
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.columns.is_empty()
    }
}

/// SQL keywords offered by completion.
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE",
    "TABLE", "ALTER", "DROP", "INDEX", "VIEW", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "FULL",
    "ON", "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "OFFSET", "DISTINCT", "AS", "AND", "OR",
    "NOT", "NULL", "IS", "IN", "LIKE", "BETWEEN", "EXISTS", "UNION", "ALL", "CASE", "WHEN",
    "THEN", "ELSE", "END", "ASC", "DESC", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "DEFAULT",
    "UNIQUE", "CONSTRAINT", "WITH", "RETURNING", "EXPLAIN", "ANALYZE",
];

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

/// The identifier being typed immediately before `cursor`.
///
/// Returns the byte range and the text. Only `[A-Za-z0-9_]` counts, so a
/// cursor sitting after a space or a comma yields nothing and no popup.
fn word_before(text: &str, cursor: usize) -> Option<(usize, usize, String)> {
    let cursor = cursor.min(text.len());
    let bytes = text.as_bytes();
    let mut start = cursor;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_ascii_alphanumeric() || c == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == cursor {
        return None;
    }
    Some((start, cursor, text[start..cursor].to_string()))
}

/// Candidates for `prefix`, tables first, then columns, then keywords.
///
/// Ordering is deliberate: in a database client the identifier you want is
/// far more often a table or column than a keyword.
fn candidates(prefix: &str, schema: &SchemaIndex) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let lower = prefix.to_lowercase();
    let mut out = Vec::new();
    let mut push_matching = |it: &mut dyn Iterator<Item = String>, out: &mut Vec<String>| {
        for s in it {
            if s.to_lowercase().starts_with(&lower) && !out.contains(&s) {
                out.push(s);
            }
        }
    };
    push_matching(&mut schema.tables.iter().cloned(), &mut out);
    push_matching(&mut schema.columns.iter().cloned(), &mut out);
    push_matching(
        &mut KEYWORDS.iter().map(|k| k.to_string()),
        &mut out,
    );
    out.truncate(12);
    out
}

/// Replace every occurrence of `find` with `replace`. Returns the count.
fn replace_all(text: &mut String, find: &str, replace: &str) -> usize {
    if find.is_empty() {
        return 0;
    }
    let n = text.matches(find).count();
    if n > 0 {
        *text = text.replace(find, replace);
    }
    n
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
        .fill(palette.secondary)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .rounding(egui::Rounding::same(6.0))
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
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} match(es)", matches),
                ));
                if ui.small_button("✕").clicked() {
                    state.find_open = false;
                }
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
            state.completion_selected =
                (state.completion_selected + 1) % state.completions.len();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaIndex {
        let mut s = SchemaIndex::default();
        s.tables.insert("users".into());
        s.tables.insert("user_roles".into());
        s.columns.insert("user_id".into());
        s
    }

    #[test]
    fn word_before_finds_the_identifier_under_the_cursor() {
        let sql = "SELECT * FROM use";
        let (start, end, w) = word_before(sql, sql.len()).unwrap();
        assert_eq!(w, "use");
        assert_eq!(&sql[start..end], "use");
    }

    #[test]
    fn word_before_yields_nothing_after_whitespace() {
        assert!(word_before("SELECT ", 7).is_none());
    }

    #[test]
    fn completion_prefers_schema_over_keywords() {
        let c = candidates("use", &schema());
        // Tables and columns come before the SQL keywords.
        assert_eq!(c[0], "user_roles");
        assert_eq!(c[1], "users");
        assert!(c.contains(&"user_id".to_string()));
    }

    #[test]
    fn completion_is_case_insensitive() {
        assert!(candidates("USE", &schema()).contains(&"users".to_string()));
    }

    #[test]
    fn completion_matches_keywords_too() {
        assert!(candidates("sel", &SchemaIndex::default()).contains(&"SELECT".to_string()));
    }

    #[test]
    fn empty_prefix_offers_nothing() {
        assert!(candidates("", &schema()).is_empty());
    }

    #[test]
    fn replace_all_counts_and_replaces() {
        let mut s = "a b a".to_string();
        assert_eq!(replace_all(&mut s, "a", "z"), 2);
        assert_eq!(s, "z b z");
    }

    #[test]
    fn replace_all_ignores_empty_needle() {
        let mut s = "abc".to_string();
        assert_eq!(replace_all(&mut s, "", "z"), 0);
        assert_eq!(s, "abc");
    }
}
