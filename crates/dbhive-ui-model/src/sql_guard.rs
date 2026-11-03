//! Warns before a statement that can destroy data runs.
//!
//! Covers `DELETE` or `UPDATE` with no `WHERE`, `DROP TABLE/DATABASE/SCHEMA`,
//! `TRUNCATE` and `ALTER TABLE ... DROP COLUMN`. Keywords that appear inside
//! string literals or comments never count: `SELECT 'DROP TABLE x'` is safe.
//!
//! This is a speed bump, not a parser. It reads one statement at a time as a
//! stream of words, which is enough to catch the mistakes people actually
//! make (forgetting a `WHERE`) without pretending to understand every dialect.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    DeleteWithoutWhere,
    UpdateWithoutWhere,
    DropObject,
    Truncate,
    DropColumn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub kind: Kind,
    /// One line saying what will happen, e.g. "DELETE without WHERE removes every row of users".
    pub description: String,
    /// The statement, trimmed, for showing in the confirmation.
    pub statement: String,
}

/// Replace string literals (`'...'`, with `''` as an escaped quote) and
/// comments (`-- ...`, `/* ... */`) with spaces, keeping byte offsets, so the
/// result can be scanned for keywords and `;` without false hits.
fn mask(sql: &str) -> String {
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' {
            out.push(' ');
            i += 1;
            while i < b.len() {
                if b[i] == b'\'' {
                    if i + 1 < b.len() && b[i + 1] == b'\'' {
                        out.push_str("  ");
                        i += 2;
                        continue;
                    }
                    out.push(' ');
                    i += 1;
                    break;
                }
                // One space per byte keeps offsets aligned with the original.
                out.push(' ');
                i += 1;
            }
        } else if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            out.push_str("  ");
            i += 2;
            while i < b.len() && !(b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/') {
                out.push(if b[i] == b'\n' { '\n' } else { ' ' });
                i += 1;
            }
            if i < b.len() {
                out.push_str("  ");
                i += 2;
            }
        } else {
            // Copy whole characters, not bytes, so multi-byte text survives.
            let ch = sql[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// `sql` with string literals and comments blanked out, byte offsets intact.
/// Shared with the completion logic so both agree on what is code.
pub fn masked(sql: &str) -> String {
    mask(sql)
}

/// Statements of `sql`, split on `;` outside literals and comments. Empty
/// statements are dropped.
pub fn split_statements(sql: &str) -> Vec<String> {
    let masked = mask(sql);
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in masked.char_indices() {
        if c == ';' {
            push_trimmed(&mut out, &sql[start..i]);
            start = i + 1;
        }
    }
    push_trimmed(&mut out, &sql[start..]);
    out
}

fn push_trimmed(out: &mut Vec<String>, s: &str) {
    let t = s.trim();
    // A statement that is only a comment has nothing to run.
    if !mask(t).trim().is_empty() {
        out.push(t.to_string());
    }
}

fn words(statement: &str) -> Vec<String> {
    mask(statement)
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

fn upper(w: &str) -> String {
    w.to_ascii_uppercase()
}

fn table_after(ws: &[String], keyword: &str) -> String {
    ws.iter()
        .position(|w| upper(w) == keyword)
        .and_then(|i| ws.get(i + 1))
        .map(|t| {
            t.trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']')
                .to_string()
        })
        .unwrap_or_default()
}

/// Everything in `sql` that deserves a second look, one entry per risky
/// statement, in the order they appear.
pub fn warnings(sql: &str) -> Vec<Warning> {
    let mut out = Vec::new();
    for statement in split_statements(sql) {
        let ws = words(&statement);
        let up: Vec<String> = ws.iter().map(|w| upper(w)).collect();
        let Some(first) = up.first() else { continue };
        let has = |kw: &str| up.iter().any(|w| w == kw);
        let found = match first.as_str() {
            "DELETE" if !has("WHERE") => Some((
                Kind::DeleteWithoutWhere,
                format!(
                    "DELETE without WHERE removes every row of {}",
                    table_after(&ws, "FROM")
                ),
            )),
            "UPDATE" if !has("WHERE") => Some((
                Kind::UpdateWithoutWhere,
                format!(
                    "UPDATE without WHERE changes every row of {}",
                    table_after(&ws, "UPDATE")
                ),
            )),
            "DROP" => match up.get(1).map(String::as_str) {
                Some("TABLE") | Some("DATABASE") | Some("SCHEMA") => Some((
                    Kind::DropObject,
                    format!("DROP {} permanently deletes it and its data", up[1]),
                )),
                _ => None,
            },
            "TRUNCATE" => Some((
                Kind::Truncate,
                "TRUNCATE removes every row and cannot be rolled back on most databases"
                    .to_string(),
            )),
            "ALTER" if has("TABLE") => up
                .windows(2)
                .any(|p| p[0] == "DROP" && p[1] == "COLUMN")
                .then(|| {
                    (
                        Kind::DropColumn,
                        "ALTER TABLE ... DROP COLUMN deletes the column and its data".to_string(),
                    )
                }),
            _ => None,
        };
        if let Some((kind, description)) = found {
            out.push(Warning {
                kind,
                description,
                statement,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<Kind> {
        warnings(sql).into_iter().map(|w| w.kind).collect()
    }

    #[test]
    fn delete_and_update_without_where_warn_but_with_where_do_not() {
        assert_eq!(kinds("DELETE FROM users"), vec![Kind::DeleteWithoutWhere]);
        assert_eq!(
            kinds("update users set active = false"),
            vec![Kind::UpdateWithoutWhere]
        );
        assert!(kinds("DELETE FROM users WHERE id = 1").is_empty());
        assert!(kinds("UPDATE users SET a = 1 WHERE id = 2").is_empty());
    }

    #[test]
    fn the_description_names_the_table() {
        let w = warnings("DELETE FROM \"audit_log\"");
        assert!(
            w[0].description.contains("audit_log"),
            "{}",
            w[0].description
        );
        let w = warnings("UPDATE orders SET x = 1");
        assert!(w[0].description.contains("orders"));
    }

    #[test]
    fn drops_truncate_and_drop_column_warn() {
        assert_eq!(kinds("DROP TABLE t"), vec![Kind::DropObject]);
        assert_eq!(kinds("drop database shop"), vec![Kind::DropObject]);
        assert_eq!(
            kinds("DROP SCHEMA IF EXISTS s CASCADE"),
            vec![Kind::DropObject]
        );
        assert_eq!(kinds("TRUNCATE TABLE logs"), vec![Kind::Truncate]);
        assert_eq!(kinds("ALTER TABLE t DROP COLUMN c"), vec![Kind::DropColumn]);
    }

    #[test]
    fn dropping_an_index_or_view_is_not_flagged() {
        assert!(kinds("DROP INDEX idx").is_empty());
        assert!(kinds("DROP VIEW v").is_empty());
    }

    #[test]
    fn keywords_inside_strings_and_comments_do_not_count() {
        assert!(kinds("SELECT 'DROP TABLE x'").is_empty());
        assert!(kinds("SELECT 'it''s; DELETE FROM t'").is_empty());
        assert!(kinds("-- DROP TABLE t\nSELECT 1").is_empty());
        assert!(kinds("/* TRUNCATE t; */ SELECT 1").is_empty());
        // A WHERE hidden in a comment does not rescue the DELETE.
        assert_eq!(
            kinds("DELETE FROM t -- WHERE id = 1"),
            vec![Kind::DeleteWithoutWhere]
        );
        assert_eq!(
            kinds("DELETE FROM t WHERE_ok"),
            vec![Kind::DeleteWithoutWhere]
        );
    }

    #[test]
    fn every_risky_statement_in_a_script_is_reported_in_order() {
        let w = warnings("SELECT 1; DELETE FROM a; UPDATE b SET x=1 WHERE y=2; TRUNCATE c;");
        let k: Vec<Kind> = w.iter().map(|w| w.kind).collect();
        assert_eq!(k, vec![Kind::DeleteWithoutWhere, Kind::Truncate]);
        assert_eq!(w[0].statement, "DELETE FROM a");
    }

    #[test]
    fn splitting_ignores_semicolons_in_literals_and_comment_only_tails() {
        assert_eq!(
            split_statements("SELECT 'a;b'; SELECT 2;"),
            vec!["SELECT 'a;b'", "SELECT 2"]
        );
        assert_eq!(split_statements("SELECT 1; -- done"), vec!["SELECT 1"]);
        assert!(split_statements("   ").is_empty());
    }

    #[test]
    fn non_ascii_text_survives_masking() {
        assert!(kinds("SELECT 'café'; SELECT 'naïve -- x'").is_empty());
        assert_eq!(kinds("DELETE FROM résumés"), vec![Kind::DeleteWithoutWhere]);
    }
}
