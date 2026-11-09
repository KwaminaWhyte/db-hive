//! What the SQL editor takes from Settings > Appearance beyond font size and
//! line numbers: the font family, word wrap and the minimap.
//!
//! The editor draws its syntax colours as a separate layer under the text,
//! placed by column times one measured glyph advance. Two consequences live
//! here:
//!
//! - The font must be monospace. The setting is a CSS-style list
//!   (`Monaco, 'Courier New', monospace`); the editor takes the first named
//!   family and a platform fallback, and the view switches to the fallback
//!   when the first one measures as proportional (which is also what a
//!   missing font looks like, since the renderer substitutes its default).
//! - Wrapping is done by character, at a column count the view reports, and
//!   the colour layer is wrapped here at exactly the same columns. That is
//!   only exact for single-width characters, the same limit the unwrapped
//!   editor has.

/// A monospace family that ships with each desktop platform.
pub fn platform_monospace(os: &str) -> &'static str {
    match os {
        "macos" => "Menlo",
        "windows" => "Consolas",
        _ => "DejaVu Sans Mono",
    }
}

/// The family to ask for, and the one to use if that is not monospace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontChoice {
    pub family: String,
    pub fallback: String,
}

/// Generic CSS families: they name no font, so they mean "the fallback".
const GENERIC: [&str; 6] = [
    "monospace",
    "ui-monospace",
    "serif",
    "sans-serif",
    "system-ui",
    "cursive",
];

/// The editor's font from the setting's text, for the platform `os`
/// (`std::env::consts::OS`).
pub fn font_choice(setting: &str, os: &str) -> FontChoice {
    let fallback = platform_monospace(os).to_string();
    let family = setting
        .split(',')
        .map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"').trim())
        .find(|f| !f.is_empty() && !GENERIC.iter().any(|g| g.eq_ignore_ascii_case(f)))
        .map(str::to_string)
        .unwrap_or_else(|| fallback.clone());
    FontChoice { family, fallback }
}

/// One row of a wrapped line: its pieces and the column each starts at,
/// counted from the row's left edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedRow<K> {
    /// 1-based number of the line this row belongs to, on its first row
    /// only; 0 on continuation rows (the gutter leaves those blank).
    pub number: usize,
    pub pieces: Vec<(String, usize, K)>,
}

/// Lay out lines of coloured runs as display rows. `width` is the number of
/// columns a row holds; 0 means no wrapping (one row per line). Runs that
/// cross a row boundary are split there, so every character lands in the row
/// and column the editor's own text puts it in.
pub fn wrap_lines<K: Copy>(lines: &[Vec<(String, K)>], width: usize) -> Vec<WrappedRow<K>> {
    let mut out = Vec::new();
    for (index, runs) in lines.iter().enumerate() {
        let mut rows: Vec<WrappedRow<K>> = vec![WrappedRow {
            number: index + 1,
            pieces: Vec::new(),
        }];
        let mut col = 0usize;
        for (text, kind) in runs {
            let mut piece = String::new();
            let mut piece_col = col;
            for ch in text.chars() {
                if width > 0 && col > 0 && col.is_multiple_of(width) && col != piece_col {
                    // The row is full: close the piece and start a new row.
                    push_piece(&mut rows, &mut piece, piece_col, *kind, width);
                    piece_col = col;
                }
                piece.push(ch);
                col += 1;
            }
            push_piece(&mut rows, &mut piece, piece_col, *kind, width);
        }
        out.extend(rows);
    }
    out
}

fn push_piece<K: Copy>(
    rows: &mut Vec<WrappedRow<K>>,
    piece: &mut String,
    start: usize,
    kind: K,
    width: usize,
) {
    if piece.is_empty() {
        return;
    }
    let (row, col) = match std::num::NonZeroUsize::new(width) {
        Some(width) => (start / width, start % width),
        None => (0, start),
    };
    while rows.len() <= row {
        rows.push(WrappedRow {
            number: 0,
            pieces: Vec::new(),
        });
    }
    rows[row].pieces.push((std::mem::take(piece), col, kind));
}

/// Display rows: lines wrapped at `width`, or one per line when 0.
pub fn row_count<K: Copy>(lines: &[Vec<(String, K)>], width: usize) -> usize {
    wrap_lines(lines, width).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(parts: &[(&str, u8)]) -> Vec<(String, u8)> {
        parts.iter().map(|(t, k)| (t.to_string(), *k)).collect()
    }

    #[test]
    fn the_first_named_family_wins_and_generics_fall_back() {
        let f = font_choice("Monaco, 'Courier New', monospace", "macos");
        assert_eq!(f.family, "Monaco");
        assert_eq!(f.fallback, "Menlo");
        assert_eq!(
            font_choice("\"JetBrains Mono\"", "linux").family,
            "JetBrains Mono"
        );
        assert_eq!(font_choice("monospace", "windows").family, "Consolas");
        assert_eq!(font_choice("  ", "linux").family, "DejaVu Sans Mono");
        assert_eq!(
            font_choice("ui-monospace, Fira Code", "macos").family,
            "Fira Code"
        );
    }

    #[test]
    fn without_wrapping_every_line_is_one_row_at_its_own_columns() {
        let rows = wrap_lines(&[line(&[("SELECT", 1), (" ", 0), ("x", 2)]), vec![]], 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].number, 1);
        assert_eq!(
            rows[0].pieces,
            vec![
                ("SELECT".into(), 0, 1),
                (" ".into(), 6, 0),
                ("x".into(), 7, 2)
            ]
        );
        assert_eq!(rows[1].number, 2);
        assert!(rows[1].pieces.is_empty());
    }

    #[test]
    fn a_run_across_the_edge_is_split_at_the_exact_column() {
        // Width 4: "SELECT" (0..6) crosses the edge at 4.
        let rows = wrap_lines(&[line(&[("SELECT", 1), (" ab", 0)])], 4);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(rows[0].pieces, vec![("SELE".into(), 0, 1)]);
        assert_eq!(rows[0].number, 1);
        assert_eq!(
            rows[1].pieces,
            vec![("CT".into(), 0, 1), (" a".into(), 2, 0)]
        );
        assert_eq!(rows[1].number, 0);
        assert_eq!(rows[2].pieces, vec![("b".into(), 0, 0)]);
    }

    #[test]
    fn a_line_exactly_as_wide_as_a_row_stays_one_row() {
        let rows = wrap_lines(&[line(&[("abcd", 0)])], 4);
        assert_eq!(rows.len(), 1);
        assert_eq!(row_count(&[line(&[("abcde", 0)])], 4), 2);
    }

    #[test]
    fn wrapped_rows_reassemble_to_the_line() {
        let src = line(&[("select", 1), ("  ", 0), ("'a long string'", 3), (";", 4)]);
        for width in 1..30 {
            let rows = wrap_lines(std::slice::from_ref(&src), width);
            let text: String = rows
                .iter()
                .flat_map(|r| r.pieces.iter().map(|p| p.0.clone()))
                .collect();
            assert_eq!(text, "select  'a long string';", "width {width}");
            for r in &rows {
                let used: usize = r.pieces.iter().map(|p| p.0.chars().count()).sum();
                assert!(used <= width, "width {width}: row {r:?}");
                for p in &r.pieces {
                    assert!(p.1 + p.0.chars().count() <= width, "width {width}: {p:?}");
                }
            }
        }
    }
}
