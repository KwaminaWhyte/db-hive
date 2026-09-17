//! SQL completion logic: the schema index, keyword list, the word under the
//! cursor, candidate ranking, and find-and-replace.
//!
//! Lifted from the egui `widgets::sql_editor`, which keeps the drawing.

use std::collections::BTreeSet;

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
pub const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "ALTER",
    "DROP",
    "INDEX",
    "VIEW",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "FULL",
    "ON",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "AS",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "BETWEEN",
    "EXISTS",
    "UNION",
    "ALL",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "DEFAULT",
    "UNIQUE",
    "CONSTRAINT",
    "WITH",
    "RETURNING",
    "EXPLAIN",
    "ANALYZE",
];

/// The identifier being typed immediately before `cursor`.
///
/// Returns the byte range and the text. Only `[A-Za-z0-9_]` counts, so a
/// cursor sitting after a space or a comma yields nothing and no popup.
pub fn word_before(text: &str, cursor: usize) -> Option<(usize, usize, String)> {
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
pub fn candidates(prefix: &str, schema: &SchemaIndex) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let lower = prefix.to_lowercase();
    let mut out = Vec::new();
    let push_matching = |it: &mut dyn Iterator<Item = String>, out: &mut Vec<String>| {
        for s in it {
            if s.to_lowercase().starts_with(&lower) && !out.contains(&s) {
                out.push(s);
            }
        }
    };
    push_matching(&mut schema.tables.iter().cloned(), &mut out);
    push_matching(&mut schema.columns.iter().cloned(), &mut out);
    push_matching(&mut KEYWORDS.iter().map(|k| k.to_string()), &mut out);
    out.truncate(12);
    out
}

/// Replace every occurrence of `find` with `replace`. Returns the count.
pub fn replace_all(text: &mut String, find: &str, replace: &str) -> usize {
    if find.is_empty() {
        return 0;
    }
    let n = text.matches(find).count();
    if n > 0 {
        *text = text.replace(find, replace);
    }
    n
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
