//! SQL syntax highlighting as coloured runs per line.
//!
//! Slint's editable text cannot style spans (`TextInput` is one colour;
//! `StyledText` can colour spans but is read-only). The editor therefore
//! draws a layer of coloured runs underneath a transparent `TextInput`, and
//! this module produces that layer.
//!
//! Output is per line, never spanning a newline, because each line is laid
//! out as its own row at the input's exact line height. That is what keeps
//! the two layers aligned.

/// Token classes the palette distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    String,
    Number,
    Comment,
    Operator,
    Identifier,
    Whitespace,
}

/// A run of text on one line with one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub kind: Kind,
}

const KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CHECK",
    "COLUMN",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IF",
    "ILIKE",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TRUE",
    "TRUNCATE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WITH",
];

fn is_keyword(word: &str) -> bool {
    let upper = word.to_ascii_uppercase();
    KEYWORDS.binary_search(&upper.as_str()).is_ok()
}

/// Split SQL into lines of runs.
///
/// Block comments and quoted strings may span lines; their state carries
/// across the newline so the continuation is coloured correctly.
pub fn highlight(sql: &str) -> Vec<Vec<Run>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Carry {
        None,
        BlockComment,
        Quote(char),
    }

    let mut lines = Vec::new();
    let mut carry = Carry::None;

    for raw_line in sql.split('\n') {
        let chars: Vec<char> = raw_line.chars().collect();
        let mut runs: Vec<Run> = Vec::new();
        let mut i = 0;

        let mut push = |runs: &mut Vec<Run>, text: String, kind: Kind| {
            if text.is_empty() {
                return;
            }
            match runs.last_mut() {
                Some(last) if last.kind == kind => last.text.push_str(&text),
                _ => runs.push(Run { text, kind }),
            }
        };

        while i < chars.len() {
            match carry {
                Carry::BlockComment => {
                    let start = i;
                    while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 2;
                        carry = Carry::None;
                    }
                    push(&mut runs, chars[start..i].iter().collect(), Kind::Comment);
                    continue;
                }
                Carry::Quote(q) => {
                    let start = i;
                    while i < chars.len() {
                        if chars[i] == q {
                            // A doubled quote is an escaped quote, not the end.
                            if chars.get(i + 1) == Some(&q) {
                                i += 2;
                                continue;
                            }
                            i += 1;
                            carry = Carry::None;
                            break;
                        }
                        i += 1;
                    }
                    push(&mut runs, chars[start..i].iter().collect(), Kind::String);
                    continue;
                }
                Carry::None => {}
            }

            let c = chars[i];
            if c == '-' && chars.get(i + 1) == Some(&'-') {
                push(&mut runs, chars[i..].iter().collect(), Kind::Comment);
                break;
            }
            if c == '/' && chars.get(i + 1) == Some(&'*') {
                carry = Carry::BlockComment;
                push(&mut runs, "/*".into(), Kind::Comment);
                i += 2;
                continue;
            }
            if c == '\'' || c == '"' || c == '`' {
                carry = Carry::Quote(c);
                push(&mut runs, c.to_string(), Kind::String);
                i += 1;
                continue;
            }
            if c.is_whitespace() {
                let start = i;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                push(
                    &mut runs,
                    chars[start..i].iter().collect(),
                    Kind::Whitespace,
                );
                continue;
            }
            if c.is_ascii_digit() {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                push(&mut runs, chars[start..i].iter().collect(), Kind::Number);
                continue;
            }
            if c.is_alphanumeric() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let kind = if is_keyword(&word) {
                    Kind::Keyword
                } else {
                    Kind::Identifier
                };
                push(&mut runs, word, kind);
                continue;
            }
            push(&mut runs, c.to_string(), Kind::Operator);
            i += 1;
        }

        lines.push(runs);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &[Run]) -> Vec<(String, Kind)> {
        line.iter().map(|r| (r.text.clone(), r.kind)).collect()
    }

    #[test]
    fn keyword_list_is_sorted_for_binary_search() {
        let mut sorted = KEYWORDS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, KEYWORDS, "KEYWORDS must stay sorted");
    }

    #[test]
    fn classifies_a_simple_select() {
        let lines = highlight("SELECT id FROM users WHERE n = 10;");
        assert_eq!(lines.len(), 1);
        let k = kinds(&lines[0]);
        assert_eq!(k[0], ("SELECT".into(), Kind::Keyword));
        assert!(k.contains(&("users".into(), Kind::Identifier)));
        assert!(k.contains(&("10".into(), Kind::Number)));
    }

    #[test]
    fn runs_reassemble_to_exactly_the_input() {
        // The overlay only aligns if the coloured layer draws the same
        // characters as the input, in the same places.
        let sql = "select *\n  from t -- note\n\n/* a\n b */ where s = 'it''s'";
        let rebuilt: Vec<String> = highlight(sql)
            .iter()
            .map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>())
            .collect();
        assert_eq!(rebuilt.join("\n"), sql);
    }

    #[test]
    fn empty_lines_are_kept_as_lines() {
        assert_eq!(highlight("a\n\nb").len(), 3);
        assert!(highlight("a\n\nb")[1].is_empty());
    }

    #[test]
    fn block_comments_and_strings_carry_across_lines() {
        let lines = highlight("/* one\ntwo */ x\n'multi\nline' y");
        assert_eq!(lines[1][0].kind, Kind::Comment);
        assert_eq!(lines[1][0].text, "two */");
        assert_eq!(lines[3][0].kind, Kind::String);
        assert_eq!(lines[3][0].text, "line'");
    }

    #[test]
    fn doubled_quotes_do_not_end_a_string() {
        let k = kinds(&highlight("'it''s' x")[0]);
        assert_eq!(k[0], ("'it''s'".into(), Kind::String));
    }

    #[test]
    fn keywords_are_case_insensitive() {
        assert_eq!(highlight("select")[0][0].kind, Kind::Keyword);
    }
}

/// Colour for each token kind, dark palette.
pub fn colour(kind: Kind) -> slint::Color {
    colour_for(kind, true)
}

/// Colour for each token kind on a dark or light editor background. The
/// light palette is the dark one darkened until text stays readable on a
/// white card.
pub fn colour_for(kind: Kind, dark: bool) -> slint::Color {
    let (r, g, b) = match (kind, dark) {
        (Kind::Keyword, true) => (248, 176, 42),
        (Kind::String, true) => (126, 211, 142),
        (Kind::Number, true) => (240, 144, 110),
        (Kind::Comment, true) => (104, 108, 118),
        (Kind::Operator, true) => (150, 153, 162),
        (Kind::Identifier | Kind::Whitespace, true) => (240, 241, 244),
        (Kind::Keyword, false) => (160, 82, 0),
        (Kind::String, false) => (22, 112, 54),
        (Kind::Number, false) => (176, 62, 16),
        (Kind::Comment, false) => (104, 110, 122),
        (Kind::Operator, false) => (84, 88, 98),
        (Kind::Identifier | Kind::Whitespace, false) => (24, 26, 32),
    };
    slint::Color::from_rgb_u8(r, g, b)
}

/// Build the editor's colour layer for `sql`, dark palette.
pub fn to_model(sql: &str) -> slint::ModelRc<crate::HighlightLine> {
    to_model_for(sql, true)
}

/// Build the editor's colour layer for `sql` for the given theme.
pub fn to_model_for(sql: &str, dark: bool) -> slint::ModelRc<crate::HighlightLine> {
    use slint::{ModelRc, VecModel};
    let lines: Vec<crate::HighlightLine> = highlight(sql)
        .into_iter()
        .map(|runs| {
            let mut col = 0i32;
            let items: Vec<crate::HighlightRun> = runs
                .into_iter()
                .map(|r| {
                    let run = crate::HighlightRun {
                        col,
                        color: colour_for(r.kind, dark).into(),
                        text: r.text.as_str().into(),
                    };
                    // Columns count characters, not bytes: the layer is
                    // positioned per glyph.
                    col += r.text.chars().count() as i32;
                    run
                })
                .collect();
            crate::HighlightLine {
                runs: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(lines))
}

#[cfg(test)]
mod contrast_tests {
    use super::*;
    use dbhive_ui_model::tokens::{DARK, LIGHT};

    fn lum(r: u8, g: u8, b: u8) -> f64 {
        let f = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
    }

    fn ratio(fg: slint::Color, bg: (u8, u8, u8)) -> f64 {
        let a = lum(fg.red(), fg.green(), fg.blue());
        let b = lum(bg.0, bg.1, bg.2);
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_token_kind_is_readable_on_the_card_in_both_themes() {
        let kinds = [
            Kind::Keyword,
            Kind::String,
            Kind::Number,
            Kind::Comment,
            Kind::Operator,
            Kind::Identifier,
        ];
        for (dark, card) in [(true, DARK.card), (false, LIGHT.card)] {
            for kind in kinds {
                // Comments are meant to recede, so they only need to be legible.
                let floor = if kind == Kind::Comment { 3.0 } else { 4.5 };
                let r = ratio(colour_for(kind, dark), (card.0, card.1, card.2));
                assert!(r >= floor, "{kind:?} dark={dark}: {r:.2} < {floor}");
            }
        }
    }
}
