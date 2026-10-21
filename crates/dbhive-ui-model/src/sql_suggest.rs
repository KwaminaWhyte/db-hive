//! Context-aware SQL completion.
//!
//! Given the text and the cursor, decide what the user is probably typing and
//! offer matching names: tables after `FROM`/`JOIN`/`INTO`/`UPDATE`, columns
//! (with their types) after `SELECT`/`WHERE`/`ON`/`SET`/`GROUP BY`/`ORDER BY`,
//! the columns of an alias or table after a dot, and keywords and functions
//! otherwise. Each category is ranked by what is likely in that spot.
//!
//! Pure: it reads the schema from [`Metadata`] and never touches a database.
//! When a table that the statement refers to has no column list loaded yet it
//! reports the name in `missing_columns`, so the shell can fetch it and ask
//! again. Strings and comments are never completed inside.

use crate::sql_guard::masked;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Table,
    Column,
    Keyword,
    Function,
    Schema,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Table => "table",
            Kind::Column => "column",
            Kind::Keyword => "keyword",
            Kind::Function => "function",
            Kind::Schema => "schema",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// What the list shows.
    pub label: String,
    /// What replaces the word under the cursor.
    pub insert: String,
    pub kind: Kind,
    /// A column's type, a table's schema, "function", and so on.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMeta {
    pub schema: Option<String>,
    pub name: String,
    /// `None` until the columns have been fetched.
    pub columns: Option<Vec<ColumnMeta>>,
}

/// What the connection has, as far as completion knows.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub schemas: Vec<String>,
    pub tables: Vec<TableMeta>,
}

impl Metadata {
    pub fn find_table(&self, name: &str) -> Option<&TableMeta> {
        self.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Record the columns of a table once they have been fetched.
    pub fn set_columns(&mut self, table: &str, columns: Vec<ColumnMeta>) {
        if let Some(t) = self
            .tables
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(table))
        {
            t.columns = Some(columns);
        }
    }

    /// Forget every column list, so the next completion fetches them afresh.
    pub fn clear_columns(&mut self) {
        for t in &mut self.tables {
            t.columns = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestions {
    /// Byte range of the word the choice replaces.
    pub start: usize,
    pub end: usize,
    pub items: Vec<Suggestion>,
    /// Tables the statement refers to whose columns are not loaded yet.
    pub missing_columns: Vec<String>,
}

const MAX_ITEMS: usize = 10;

/// Roughly most to least common, which is also the order they are offered in.
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "JOIN", "LEFT", "INNER", "ON", "GROUP", "BY", "ORDER",
    "LIMIT", "AS", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "HAVING", "DISTINCT",
    "NOT", "NULL", "IS", "IN", "LIKE", "ILIKE", "BETWEEN", "EXISTS", "CASE", "WHEN", "THEN", "ELSE",
    "END", "ASC", "DESC", "OFFSET", "UNION", "ALL", "RIGHT", "OUTER", "FULL", "CROSS", "CREATE",
    "TABLE", "ALTER", "DROP", "INDEX", "VIEW", "PRIMARY", "KEY", "FOREIGN", "REFERENCES",
    "DEFAULT", "UNIQUE", "CONSTRAINT", "WITH", "RETURNING", "EXPLAIN", "ANALYZE", "TRUNCATE",
    "TRUE", "FALSE",
];

const FUNCTIONS: &[&str] = &[
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NULLIF", "CAST", "UPPER", "LOWER", "LENGTH",
    "TRIM", "SUBSTRING", "CONCAT", "REPLACE", "ROUND", "ABS", "NOW", "CURRENT_DATE",
    "CURRENT_TIMESTAMP", "DATE_TRUNC", "EXTRACT", "ROW_NUMBER", "RANK", "STRING_AGG",
];

/// What kind of name is due at the cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Table,
    Column,
    /// A keyword is what usually comes next (start of a statement, or right
    /// after a table or column name).
    Keyword,
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Words of `text` as `(start, end, word)`: identifier runs, plus every other
/// non-space character as its own one-character token.
fn tokens(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap();
        if is_ident_char(c) {
            let start = i;
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap();
                if is_ident_char(ch) {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push((start, i, text[start..i].to_string()));
        } else {
            if !c.is_whitespace() {
                out.push((i, i + c.len_utf8(), c.to_string()));
            }
            i += c.len_utf8();
        }
    }
    out
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// Byte range of the statement that contains `cursor`, in `masked` text.
fn statement_bounds(masked: &str, cursor: usize) -> (usize, usize) {
    let start = masked[..cursor].rfind(';').map(|p| p + 1).unwrap_or(0);
    let end = masked[cursor..]
        .find(';')
        .map(|p| cursor + p)
        .unwrap_or(masked.len());
    (start, end)
}

fn is_ident_word(w: &str) -> bool {
    w.chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false)
        && w.chars().all(is_ident_char)
}

fn is_keyword(w: &str) -> bool {
    let u = upper(w);
    KEYWORDS.iter().any(|k| *k == u)
}

fn clause_expectation(w: &str) -> Option<Expect> {
    match upper(w).as_str() {
        "FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "TRUNCATE" => Some(Expect::Table),
        "SELECT" | "WHERE" | "ON" | "SET" | "HAVING" | "AND" | "OR" | "BY" | "DISTINCT" => {
            Some(Expect::Column)
        }
        _ => None,
    }
}

/// `FROM`/`JOIN`/`UPDATE`/`INTO` table references in a statement:
/// `(table, alias)`.
fn table_references(stmt: &[(usize, usize, String)]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stmt.len() {
        let w = upper(&stmt[i].2);
        if !matches!(w.as_str(), "FROM" | "JOIN" | "UPDATE" | "INTO") {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        loop {
            let Some(name) = stmt.get(j) else { break };
            if !is_ident_word(&name.2) || is_keyword(&name.2) {
                break;
            }
            let mut table = name.2.clone();
            j += 1;
            // schema.table
            if stmt.get(j).map(|t| t.2.as_str()) == Some(".") {
                if let Some(t) = stmt.get(j + 1) {
                    if is_ident_word(&t.2) {
                        table = t.2.clone();
                        j += 2;
                    }
                }
            }
            let mut alias = None;
            if let Some(t) = stmt.get(j) {
                if upper(&t.2) == "AS" {
                    if let Some(a) = stmt.get(j + 1) {
                        if is_ident_word(&a.2) {
                            alias = Some(a.2.clone());
                            j += 2;
                        }
                    }
                } else if is_ident_word(&t.2) && !is_keyword(&t.2) {
                    alias = Some(t.2.clone());
                    j += 1;
                }
            }
            out.push((table, alias));
            // `FROM a, b c`: a comma continues the list.
            if w == "FROM" && stmt.get(j).map(|t| t.2.as_str()) == Some(",") {
                j += 1;
                continue;
            }
            break;
        }
        i = j.max(i + 1);
    }
    out
}

/// What kind of name is due at the word that starts at token `idx`.
fn expectation(stmt: &[(usize, usize, String)], idx: usize) -> Expect {
    let Some(prev) = idx.checked_sub(1).and_then(|p| stmt.get(p)) else {
        return Expect::Keyword;
    };
    let pw = prev.2.as_str();
    // Straight after a clause keyword a name is due.
    if let Some(e) = clause_expectation(pw) {
        return e;
    }
    // After a name, a number or a closing paren, a keyword is due.
    if is_ident_word(pw) || pw == ")" || pw.chars().all(|c| c.is_ascii_digit()) {
        return Expect::Keyword;
    }
    // After a comma, an operator or '(' the nearest clause continues.
    for t in stmt[..idx].iter().rev() {
        if let Some(e) = clause_expectation(&t.2) {
            return e;
        }
    }
    Expect::Keyword
}

fn matches_prefix(label: &str, prefix_lower: &str) -> bool {
    prefix_lower.is_empty() || label.to_lowercase().starts_with(prefix_lower)
}

/// Rank names: shorter first, then alphabetical.
fn sort_names(items: &mut [Suggestion]) {
    items.sort_by(|a, b| {
        a.label
            .len()
            .cmp(&b.label.len())
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
}

/// Suggestions for the word ending at `cursor`, or `None` when nothing should
/// pop up (inside a string or comment, or an empty unqualified word).
pub fn suggest(text: &str, cursor: usize, meta: &Metadata) -> Option<Suggestions> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let masked = masked(text);
    // Inside a literal or comment the masker blanked what the text has.
    if cursor > 0 {
        let before = text[..cursor].chars().next_back().unwrap();
        let blanked = masked[..cursor].chars().next_back().unwrap();
        if before != blanked && !before.is_whitespace() {
            return None;
        }
    }

    let (s_start, s_end) = statement_bounds(&masked, cursor);
    let stmt: Vec<(usize, usize, String)> = tokens(&masked[s_start..s_end])
        .into_iter()
        .map(|(a, b, w)| (a + s_start, b + s_start, w))
        .collect();

    // The word under the cursor, if any.
    let mut word_start = cursor;
    while word_start > s_start {
        let c = masked[..word_start].chars().next_back().unwrap();
        if is_ident_char(c) {
            word_start -= c.len_utf8();
        } else {
            break;
        }
    }
    let prefix = &text[word_start..cursor];
    let prefix_lower = prefix.to_lowercase();
    // A word that starts with a digit is a number, not a name.
    if prefix.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return None;
    }

    // Index of the first token at or after the word's start.
    let idx = stmt
        .iter()
        .position(|t| t.0 >= word_start)
        .unwrap_or(stmt.len());

    let references = table_references(&stmt);
    let mut missing: Vec<String> = Vec::new();
    let mut note_missing = |name: &str, missing: &mut Vec<String>| {
        if !missing.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            missing.push(name.to_string());
        }
    };
    for (t, _) in &references {
        if let Some(tm) = meta.find_table(t) {
            if tm.columns.is_none() {
                note_missing(&tm.name, &mut missing);
            }
        }
    }

    // Qualified: `x.` or `x.pre`.
    let qualified = idx >= 2 && stmt.get(idx - 1).map(|t| t.2.as_str()) == Some(".");
    if qualified {
        let qualifier = stmt[idx - 2].2.clone();
        let mut items: Vec<Suggestion> = Vec::new();
        // An alias or a table name: its columns.
        let table_name = references
            .iter()
            .find(|(t, a)| {
                a.as_deref()
                    .map(|a| a.eq_ignore_ascii_case(&qualifier))
                    .unwrap_or(false)
                    || t.eq_ignore_ascii_case(&qualifier)
            })
            .map(|(t, _)| t.clone())
            .or_else(|| meta.find_table(&qualifier).map(|t| t.name.clone()));
        if let Some(t) = table_name {
            if let Some(tm) = meta.find_table(&t) {
                match &tm.columns {
                    Some(cols) => {
                        for c in cols {
                            if matches_prefix(&c.name, &prefix_lower) {
                                items.push(Suggestion {
                                    label: c.name.clone(),
                                    insert: c.name.clone(),
                                    kind: Kind::Column,
                                    detail: c.data_type.clone(),
                                });
                            }
                        }
                    }
                    None => note_missing(&tm.name, &mut missing),
                }
            }
        } else if meta.schemas.iter().any(|s| s.eq_ignore_ascii_case(&qualifier)) {
            // A schema: its tables.
            for t in &meta.tables {
                let in_schema = t
                    .schema
                    .as_deref()
                    .map(|s| s.eq_ignore_ascii_case(&qualifier))
                    .unwrap_or(false);
                if in_schema && matches_prefix(&t.name, &prefix_lower) {
                    items.push(Suggestion {
                        label: t.name.clone(),
                        insert: t.name.clone(),
                        kind: Kind::Table,
                        detail: qualifier.clone(),
                    });
                }
            }
        }
        sort_names(&mut items);
        items.truncate(MAX_ITEMS);
        if items.is_empty() && missing.is_empty() {
            return None;
        }
        return Some(Suggestions {
            start: word_start,
            end: cursor,
            items,
            missing_columns: missing,
        });
    }

    // An unqualified empty word is not worth a popup.
    if prefix.is_empty() {
        return None;
    }

    let expect = expectation(&stmt, idx);

    let mut tables: Vec<Suggestion> = meta
        .tables
        .iter()
        .filter(|t| matches_prefix(&t.name, &prefix_lower))
        .map(|t| Suggestion {
            label: t.name.clone(),
            insert: t.name.clone(),
            kind: Kind::Table,
            detail: t.schema.clone().unwrap_or_else(|| "table".to_string()),
        })
        .collect();
    sort_names(&mut tables);
    let mut schemas: Vec<Suggestion> = meta
        .schemas
        .iter()
        .filter(|s| matches_prefix(s, &prefix_lower))
        .map(|s| Suggestion {
            label: s.clone(),
            insert: s.clone(),
            kind: Kind::Schema,
            detail: "schema".to_string(),
        })
        .collect();
    sort_names(&mut schemas);

    // Columns of the tables the statement refers to; if it refers to none,
    // none at all (every column of the database would be noise).
    let mut columns: Vec<Suggestion> = Vec::new();
    for (t, _) in &references {
        if let Some(cols) = meta.find_table(t).and_then(|tm| tm.columns.as_ref()) {
            for c in cols {
                if matches_prefix(&c.name, &prefix_lower)
                    && !columns.iter().any(|s| s.label == c.name)
                {
                    columns.push(Suggestion {
                        label: c.name.clone(),
                        insert: c.name.clone(),
                        kind: Kind::Column,
                        detail: c.data_type.clone(),
                    });
                }
            }
        }
    }
    sort_names(&mut columns);

    // Keywords and functions keep their list order (most common first) and
    // follow the user's casing.
    let lowercase = prefix.chars().any(|c| c.is_ascii_lowercase())
        && !prefix.chars().any(|c| c.is_ascii_uppercase());
    let case = |s: &str| if lowercase { s.to_lowercase() } else { s.to_string() };
    let keywords: Vec<Suggestion> = KEYWORDS
        .iter()
        .filter(|k| matches_prefix(k, &prefix_lower))
        .map(|k| Suggestion {
            label: case(k),
            insert: case(k),
            kind: Kind::Keyword,
            detail: "keyword".to_string(),
        })
        .collect();
    let functions: Vec<Suggestion> = FUNCTIONS
        .iter()
        .filter(|f| matches_prefix(f, &prefix_lower))
        .map(|f| Suggestion {
            label: case(f),
            insert: format!("{}(", case(f)),
            kind: Kind::Function,
            detail: "function".to_string(),
        })
        .collect();

    let groups: Vec<Vec<Suggestion>> = match expect {
        Expect::Table => vec![tables, schemas, keywords],
        Expect::Column => vec![columns, functions, keywords, tables],
        Expect::Keyword => vec![keywords, functions, tables, columns],
    };
    let mut items: Vec<Suggestion> = Vec::new();
    for g in groups {
        for s in g {
            // The word is already complete: no point offering it back.
            if s.label == prefix {
                continue;
            }
            if !items.iter().any(|i| i.label == s.label && i.kind == s.kind) {
                items.push(s);
            }
        }
    }
    items.truncate(MAX_ITEMS);

    if items.is_empty() && missing.is_empty() {
        return None;
    }
    Some(Suggestions {
        start: word_start,
        end: cursor,
        items,
        missing_columns: missing,
    })
}

/// Apply a chosen suggestion to `text`. Returns the new text and where the
/// cursor goes (just after the inserted text).
pub fn apply(text: &str, suggestions: &Suggestions, choice: usize) -> Option<(String, usize)> {
    let item = suggestions.items.get(choice)?;
    if suggestions.start > suggestions.end
        || suggestions.end > text.len()
        || !text.is_char_boundary(suggestions.start)
        || !text.is_char_boundary(suggestions.end)
    {
        return None;
    }
    let mut out = String::with_capacity(text.len() + item.insert.len());
    out.push_str(&text[..suggestions.start]);
    out.push_str(&item.insert);
    out.push_str(&text[suggestions.end..]);
    Some((out, suggestions.start + item.insert.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: ty.into(),
        }
    }

    fn table(name: &str, cols: Option<Vec<ColumnMeta>>) -> TableMeta {
        TableMeta {
            schema: Some("public".into()),
            name: name.into(),
            columns: cols,
        }
    }

    fn meta() -> Metadata {
        Metadata {
            schemas: vec!["public".into(), "audit".into()],
            tables: vec![
                table(
                    "customers",
                    Some(vec![
                        col("id", "integer"),
                        col("name", "text"),
                        col("created_at", "timestamp"),
                    ]),
                ),
                table(
                    "orders",
                    Some(vec![
                        col("id", "integer"),
                        col("customer_id", "integer"),
                        col("total", "numeric"),
                    ]),
                ),
                table("order_items", None),
            ],
        }
    }

    fn labels(text: &str, m: &Metadata) -> Vec<String> {
        suggest(text, text.len(), m)
            .map(|s| s.items.into_iter().map(|i| i.label).collect())
            .unwrap_or_default()
    }

    #[test]
    fn tables_are_offered_after_from_join_into_and_update() {
        let m = meta();
        for sql in [
            "SELECT * FROM cu",
            "select * from a join cu",
            "INSERT INTO cu",
            "UPDATE cu",
        ] {
            let l = labels(sql, &m);
            assert_eq!(l.first().map(String::as_str), Some("customers"), "{sql}: {l:?}");
        }
    }

    #[test]
    fn columns_with_types_are_offered_after_select_where_on_set_group_order() {
        let m = meta();
        for sql in [
            "SELECT na FROM customers",
            "SELECT * FROM customers WHERE na",
            "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id AND na",
            "UPDATE customers SET na",
            "SELECT * FROM customers GROUP BY na",
            "SELECT * FROM customers ORDER BY na",
        ] {
            // The cursor sits right after the " na" being typed.
            let cursor = sql.rfind(" na").unwrap() + 3;
            let s = suggest(sql, cursor, &m).unwrap_or_else(|| panic!("{sql}"));
            let first = &s.items[0];
            assert_eq!((first.label.as_str(), first.kind), ("name", Kind::Column), "{sql}");
            assert_eq!(first.detail, "text");
        }
    }

    #[test]
    fn a_dot_after_an_alias_or_table_lists_that_tables_columns() {
        let m = meta();
        let sql = "SELECT c. FROM customers c";
        let s = suggest(sql, "SELECT c.".len(), &m).unwrap();
        let l: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(l, vec!["id", "name", "created_at"]);
        let sql = "SELECT customers.cr FROM customers";
        let s = suggest(sql, "SELECT customers.cr".len(), &m).unwrap();
        assert_eq!(s.items[0].label, "created_at");
    }

    #[test]
    fn an_aliased_join_resolves_each_alias_to_its_own_table() {
        let m = meta();
        let sql = "SELECT * FROM customers c JOIN orders AS o ON o.cu";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert_eq!(s.items[0].label, "customer_id");
        // And the other alias gets the other table's columns.
        let sql = "SELECT * FROM customers c JOIN orders AS o ON c.na";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert_eq!(s.items[0].label, "name");
    }

    #[test]
    fn a_schema_qualifier_lists_its_tables() {
        let m = meta();
        let sql = "SELECT * FROM public.o";
        let s = suggest(sql, sql.len(), &m).unwrap();
        let l: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(l, vec!["orders", "order_items"]);
    }

    #[test]
    fn tables_with_unloaded_columns_are_reported_so_the_shell_can_fetch_them() {
        let m = meta();
        let sql = "SELECT it FROM order_items";
        let s = suggest(sql, "SELECT it".len(), &m).unwrap();
        assert_eq!(s.missing_columns, vec!["order_items".to_string()]);
        // After loading, the columns show up.
        let mut m2 = m.clone();
        m2.set_columns("order_items", vec![col("item_id", "integer")]);
        let s = suggest(sql, "SELECT it".len(), &m2).unwrap();
        assert!(s.missing_columns.is_empty());
        assert_eq!(s.items[0].label, "item_id");
        // And clearing forgets them.
        m2.clear_columns();
        assert!(m2.find_table("customers").unwrap().columns.is_none());
    }

    #[test]
    fn keywords_lead_at_the_start_of_a_statement_and_after_a_table_name() {
        let m = meta();
        assert_eq!(labels("sel", &m).first().map(String::as_str), Some("select"));
        // After a table name the next word is a keyword (WHERE, JOIN, ...), not another table.
        let l = labels("SELECT * FROM customers wh", &m);
        assert_eq!(l.first().map(String::as_str), Some("where"), "{l:?}");
        let l = labels("SELECT * FROM customers WH", &m);
        assert_eq!(l.first().map(String::as_str), Some("WHERE"), "{l:?}");
    }

    #[test]
    fn keyword_case_follows_what_the_user_typed() {
        let m = meta();
        assert_eq!(labels("SEL", &m).first().map(String::as_str), Some("SELECT"));
        assert_eq!(labels("sel", &m).first().map(String::as_str), Some("select"));
    }

    #[test]
    fn functions_insert_with_an_open_paren() {
        let m = meta();
        let s = suggest("SELECT cou FROM customers", "SELECT cou".len(), &m).unwrap();
        let f = s.items.iter().find(|i| i.kind == Kind::Function).unwrap();
        assert_eq!(f.label, "count");
        assert_eq!(f.insert, "count(");
    }

    #[test]
    fn nothing_pops_up_inside_strings_comments_numbers_or_on_an_empty_word() {
        let m = meta();
        assert!(suggest("SELECT 'cu", "SELECT 'cu".len(), &m).is_none());
        assert!(suggest("-- cu", "-- cu".len(), &m).is_none());
        let sql = "SELECT * FROM customers WHERE id = 12";
        assert!(suggest(sql, sql.len(), &m).is_none());
        assert!(suggest("SELECT ", "SELECT ".len(), &m).is_none());
        // A literal that has closed is code again.
        let sql = "SELECT 'a' || na FROM customers";
        assert!(suggest(sql, "SELECT 'a' || na".len(), &m).is_some());
    }

    #[test]
    fn an_already_complete_word_is_not_offered_back() {
        let m = meta();
        let l = labels("SELECT * FROM customers", &m);
        assert!(!l.contains(&"customers".to_string()), "{l:?}");
    }

    #[test]
    fn only_the_current_statement_counts() {
        let m = meta();
        // The previous statement's tables must not leak into this one's columns.
        let sql = "SELECT * FROM orders; SELECT cust";
        let l = labels(sql, &m);
        assert!(!l.contains(&"customer_id".to_string()), "{l:?}");
    }

    #[test]
    fn at_most_ten_suggestions_and_none_for_an_unknown_prefix() {
        let m = meta();
        assert!(suggest("SELECT zzzz", "SELECT zzzz".len(), &m).is_none());
        assert!(labels("S", &m).len() <= 10);
    }

    #[test]
    fn applying_a_choice_replaces_the_word_and_places_the_cursor_after_it() {
        let m = meta();
        let sql = "SELECT * FROM cu WHERE 1";
        let cursor = "SELECT * FROM cu".len();
        let s = suggest(sql, cursor, &m).unwrap();
        let (out, at) = apply(sql, &s, 0).unwrap();
        assert_eq!(out, "SELECT * FROM customers WHERE 1");
        assert_eq!(at, "SELECT * FROM customers".len());
        assert!(apply(sql, &s, 99).is_none());
    }

    #[test]
    fn multibyte_text_does_not_break_offsets() {
        let m = meta();
        let sql = "SELECT 'café' FROM cu";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert_eq!(s.items[0].label, "customers");
        let (out, _) = apply(sql, &s, 0).unwrap();
        assert_eq!(out, "SELECT 'café' FROM customers");
    }
}
