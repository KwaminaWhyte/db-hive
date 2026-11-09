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
    Database,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Table => "table",
            Kind::Column => "column",
            Kind::Keyword => "keyword",
            Kind::Function => "function",
            Kind::Schema => "schema",
            Kind::Database => "database",
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
    /// Every schema completion knows of, the current one included.
    pub schemas: Vec<String>,
    pub tables: Vec<TableMeta>,
    /// Databases on the server. Offered after `USE` and `DATABASE`, and as
    /// table containers when `databases_hold_tables`.
    pub databases: Vec<String>,
    /// MySQL-style: `db.table` names a table in another database, so a
    /// database qualifier lists that database's tables.
    pub databases_hold_tables: bool,
    /// The schema unqualified names resolve to. A table in any other schema
    /// is offered (and inserted) as `schema.table`. `None` means no table is
    /// qualified.
    pub current_schema: Option<String>,
    /// Schemas (or table-holding databases) whose tables are in `tables`.
    /// A qualifier naming one that is not is reported in
    /// [`Suggestions::missing_schemas`] so the shell can load it.
    pub loaded: Vec<String>,
}

impl Metadata {
    pub fn find_table(&self, name: &str) -> Option<&TableMeta> {
        self.find_table_in(None, name)
    }

    /// A table by name, in `schema` when one is given. Without a schema the
    /// current schema's table wins over a same-named one elsewhere.
    pub fn find_table_in(&self, schema: Option<&str>, name: &str) -> Option<&TableMeta> {
        let named = |t: &&TableMeta| t.name.eq_ignore_ascii_case(name);
        match schema {
            Some(s) => self.tables.iter().filter(named).find(|t| {
                t.schema
                    .as_deref()
                    .is_some_and(|ts| ts.eq_ignore_ascii_case(s))
            }),
            None => self
                .tables
                .iter()
                .filter(named)
                .find(|t| !self.is_elsewhere(t))
                .or_else(|| self.tables.iter().find(named)),
        }
    }

    /// Whether a table lives outside the current schema.
    fn is_elsewhere(&self, t: &TableMeta) -> bool {
        match (&self.current_schema, &t.schema) {
            (Some(current), Some(s)) => !s.eq_ignore_ascii_case(current),
            _ => false,
        }
    }

    /// Whether `name` is a schema, or a database that holds tables.
    fn is_container(&self, name: &str) -> bool {
        self.schemas.iter().any(|s| s.eq_ignore_ascii_case(name))
            || (self.databases_hold_tables
                && self.databases.iter().any(|d| d.eq_ignore_ascii_case(name)))
    }

    fn is_loaded(&self, container: &str) -> bool {
        self.loaded
            .iter()
            .any(|s| s.eq_ignore_ascii_case(container))
            || self.tables.iter().any(|t| {
                t.schema
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(container))
            })
    }

    /// Add a schema's tables once they have been fetched, replacing any it
    /// had, and mark it loaded.
    pub fn add_schema_tables(&mut self, schema: &str, names: Vec<String>) {
        self.tables.retain(|t| {
            !t.schema
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(schema))
        });
        self.tables.extend(names.into_iter().map(|name| TableMeta {
            schema: Some(schema.to_string()),
            name,
            columns: None,
        }));
        if !self.loaded.iter().any(|s| s.eq_ignore_ascii_case(schema)) {
            self.loaded.push(schema.to_string());
        }
    }

    /// Record the columns of a table once they have been fetched.
    pub fn set_columns(&mut self, table: &str, columns: Vec<ColumnMeta>) {
        self.set_columns_in(None, table, columns);
    }

    /// Like `set_columns`, for the table of that name in `schema`.
    pub fn set_columns_in(&mut self, schema: Option<&str>, table: &str, columns: Vec<ColumnMeta>) {
        let current = self.current_schema.clone();
        let target = self.tables.iter_mut().find(|t| {
            t.name.eq_ignore_ascii_case(table)
                && match (schema, &t.schema) {
                    (Some(s), Some(ts)) => ts.eq_ignore_ascii_case(s),
                    (Some(_), None) => false,
                    // No schema: the current schema's table, as `find_table`.
                    (None, Some(ts)) => current
                        .as_deref()
                        .is_none_or(|c| ts.eq_ignore_ascii_case(c)),
                    (None, None) => true,
                }
        });
        if let Some(t) = target {
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
    /// Tables the statement refers to whose columns are not loaded yet, as
    /// `(schema, table)`. The schema is `None` for the current schema.
    pub missing_columns: Vec<(Option<String>, String)>,
    /// Schemas (or table-holding databases) a qualifier names whose tables
    /// are not loaded yet.
    pub missing_schemas: Vec<String>,
}

const MAX_ITEMS: usize = 10;

/// Roughly most to least common, which is also the order they are offered in.
const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "JOIN",
    "LEFT",
    "INNER",
    "ON",
    "GROUP",
    "BY",
    "ORDER",
    "LIMIT",
    "AS",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "HAVING",
    "DISTINCT",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "EXISTS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    "OFFSET",
    "UNION",
    "ALL",
    "RIGHT",
    "OUTER",
    "FULL",
    "CROSS",
    "CREATE",
    "TABLE",
    "ALTER",
    "DROP",
    "INDEX",
    "VIEW",
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
    "TRUNCATE",
    "USE",
    "DATABASE",
    "SCHEMA",
    "TRUE",
    "FALSE",
];

const FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "NULLIF",
    "CAST",
    "UPPER",
    "LOWER",
    "LENGTH",
    "TRIM",
    "SUBSTRING",
    "CONCAT",
    "REPLACE",
    "ROUND",
    "ABS",
    "NOW",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "DATE_TRUNC",
    "EXTRACT",
    "ROW_NUMBER",
    "RANK",
    "STRING_AGG",
];

/// What kind of name is due at the cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Table,
    Column,
    /// A keyword is what usually comes next (start of a statement, or right
    /// after a table or column name).
    Keyword,
    /// After `USE` or `DATABASE`.
    Database,
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
        // `USE shop`, `DROP DATABASE shop`, `\c`-less Postgres equivalents.
        "USE" | "DATABASE" => Some(Expect::Database),
        _ => None,
    }
}

/// A table a statement names, with the schema it was qualified with.
struct Reference {
    schema: Option<String>,
    table: String,
    alias: Option<String>,
}

/// `FROM`/`JOIN`/`UPDATE`/`INTO` table references in a statement.
fn table_references(stmt: &[(usize, usize, String)]) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stmt.len() {
        let w = upper(&stmt[i].2);
        if !matches!(w.as_str(), "FROM" | "JOIN" | "UPDATE" | "INTO") {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while let Some(name) = stmt.get(j) {
            if !is_ident_word(&name.2) || is_keyword(&name.2) {
                break;
            }
            let mut table = name.2.clone();
            let mut schema = None;
            j += 1;
            // schema.table
            if stmt.get(j).map(|t| t.2.as_str()) == Some(".") {
                if let Some(t) = stmt.get(j + 1) {
                    if is_ident_word(&t.2) {
                        schema = Some(std::mem::replace(&mut table, t.2.clone()));
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
            out.push(Reference {
                schema,
                table,
                alias,
            });
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
    if prefix
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return None;
    }

    // Index of the first token at or after the word's start.
    let idx = stmt
        .iter()
        .position(|t| t.0 >= word_start)
        .unwrap_or(stmt.len());

    let references = table_references(&stmt);
    let mut missing: Vec<(Option<String>, String)> = Vec::new();
    let mut missing_schemas: Vec<String> = Vec::new();
    // The key a table's columns are fetched by: `None` for the current
    // schema, so a driver with no schema level keeps working as before.
    let fetch_key = |tm: &TableMeta| -> (Option<String>, String) {
        let schema = if meta.is_elsewhere(tm) {
            tm.schema.clone()
        } else {
            None
        };
        (schema, tm.name.clone())
    };
    let note_missing = |key: (Option<String>, String),
                        missing: &mut Vec<(Option<String>, String)>| {
        if !missing.contains(&key) {
            missing.push(key);
        }
    };
    let resolve = |r: &Reference| meta.find_table_in(r.schema.as_deref(), &r.table);
    for r in &references {
        if let Some(tm) = resolve(r) {
            if tm.columns.is_none() {
                note_missing(fetch_key(tm), &mut missing);
            }
        } else if let Some(s) = &r.schema {
            // `FROM other.t` where `other` was never loaded: load it, so the
            // table (and then its columns) can be found.
            if meta.is_container(s) && !meta.is_loaded(s) && !missing_schemas.contains(s) {
                missing_schemas.push(s.clone());
            }
        }
    }

    // Qualified: `x.` or `x.pre`.
    let qualified = idx >= 2 && stmt.get(idx - 1).map(|t| t.2.as_str()) == Some(".");
    if qualified {
        let qualifier = stmt[idx - 2].2.clone();
        // `schema.table.` names the table in that schema.
        let outer = (idx >= 4 && stmt.get(idx - 3).map(|t| t.2.as_str()) == Some("."))
            .then(|| stmt[idx - 4].2.clone());
        let mut items: Vec<Suggestion> = Vec::new();
        // An alias or a table name: its columns.
        let table = references
            .iter()
            .find(|r| {
                r.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&qualifier))
                    || (r.table.eq_ignore_ascii_case(&qualifier)
                        && (outer.is_none() || r.schema == outer))
            })
            .and_then(resolve)
            .or_else(|| meta.find_table_in(outer.as_deref(), &qualifier));
        if let Some(tm) = table {
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
                None => note_missing(fetch_key(tm), &mut missing),
            }
        } else if outer.is_none() && meta.is_container(&qualifier) {
            // A schema (or a database that holds tables): its tables.
            for t in &meta.tables {
                let in_schema = t
                    .schema
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&qualifier));
                if in_schema && matches_prefix(&t.name, &prefix_lower) {
                    items.push(Suggestion {
                        label: t.name.clone(),
                        insert: t.name.clone(),
                        kind: Kind::Table,
                        detail: qualifier.clone(),
                    });
                }
            }
            if !meta.is_loaded(&qualifier) && !missing_schemas.contains(&qualifier) {
                missing_schemas.push(qualifier.clone());
            }
        }
        sort_names(&mut items);
        items.truncate(MAX_ITEMS);
        if items.is_empty() && missing.is_empty() && missing_schemas.is_empty() {
            return None;
        }
        return Some(Suggestions {
            start: word_start,
            end: cursor,
            items,
            missing_columns: missing,
            missing_schemas,
        });
    }

    // An unqualified empty word is not worth a popup.
    if prefix.is_empty() {
        return None;
    }

    let expect = expectation(&stmt, idx);

    // Tables of the current schema by name; tables elsewhere by
    // `schema.table`, matched on either part, so they are found and inserted
    // in a form that resolves.
    let mut tables: Vec<Suggestion> = meta
        .tables
        .iter()
        .filter_map(|t| {
            if meta.is_elsewhere(t) {
                let schema = t.schema.clone().unwrap_or_default();
                let full = format!("{schema}.{}", t.name);
                (matches_prefix(&t.name, &prefix_lower) || matches_prefix(&full, &prefix_lower))
                    .then(|| Suggestion {
                        label: full.clone(),
                        insert: full,
                        kind: Kind::Table,
                        detail: schema,
                    })
            } else {
                matches_prefix(&t.name, &prefix_lower).then(|| Suggestion {
                    label: t.name.clone(),
                    insert: t.name.clone(),
                    kind: Kind::Table,
                    detail: t.schema.clone().unwrap_or_else(|| "table".to_string()),
                })
            }
        })
        .collect();
    // The current schema's tables first, then the rest.
    tables.sort_by(|a, b| {
        a.label
            .contains('.')
            .cmp(&b.label.contains('.'))
            .then_with(|| a.label.len().cmp(&b.label.len()))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
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
    let mut databases: Vec<Suggestion> = meta
        .databases
        .iter()
        .filter(|d| matches_prefix(d, &prefix_lower))
        .map(|d| Suggestion {
            label: d.clone(),
            insert: d.clone(),
            kind: Kind::Database,
            detail: "database".to_string(),
        })
        .collect();
    sort_names(&mut databases);
    // Where a table is due, a database is only a sensible word on a server
    // where it qualifies one (`db.table`).
    let table_databases = if meta.databases_hold_tables {
        databases.clone()
    } else {
        Vec::new()
    };

    // Columns of the tables the statement refers to; if it refers to none,
    // none at all (every column of the database would be noise).
    let mut columns: Vec<Suggestion> = Vec::new();
    for r in &references {
        if let Some(cols) = resolve(r).and_then(|tm| tm.columns.as_ref()) {
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
    let case = |s: &str| {
        if lowercase {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
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
        Expect::Table => vec![tables, schemas, table_databases, keywords],
        Expect::Column => vec![columns, functions, keywords, tables],
        Expect::Keyword => vec![keywords, functions, tables, columns],
        Expect::Database => vec![databases, keywords],
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

    if items.is_empty() && missing.is_empty() && missing_schemas.is_empty() {
        return None;
    }
    Some(Suggestions {
        start: word_start,
        end: cursor,
        items,
        missing_columns: missing,
        missing_schemas,
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
            ..Metadata::default()
        }
    }

    /// Postgres-like: `public` is current and loaded, `audit` known but not
    /// loaded, `billing` loaded with one table, plus three databases.
    fn multi() -> Metadata {
        let mut m = meta();
        m.current_schema = Some("public".into());
        m.schemas.push("billing".into());
        m.loaded = vec!["public".into()];
        m.add_schema_tables("billing", vec!["invoices".into(), "orders".into()]);
        m.databases = vec!["app".into(), "analytics".into(), "postgres".into()];
        m
    }

    #[test]
    fn databases_are_offered_after_use_and_database() {
        let m = multi();
        for sql in ["USE ana", "DROP DATABASE ana", "use ana"] {
            let s = suggest(sql, sql.len(), &m).unwrap_or_else(|| panic!("{sql}"));
            assert_eq!(
                (s.items[0].label.as_str(), s.items[0].kind),
                ("analytics", Kind::Database),
                "{sql}"
            );
            assert_eq!(s.items[0].detail, "database");
        }
        // Where a table is due, a Postgres database is no valid word.
        let l = labels("SELECT * FROM ana", &m);
        assert!(!l.contains(&"analytics".to_string()), "{l:?}");
    }

    #[test]
    fn a_mysql_database_qualifies_tables_and_is_loaded_on_demand() {
        let mut m = meta();
        m.databases = vec!["shop".into(), "warehouse".into()];
        m.databases_hold_tables = true;
        // Offered where a table is due.
        let s = suggest("SELECT * FROM wa", "SELECT * FROM wa".len(), &m).unwrap();
        assert!(
            s.items
                .iter()
                .any(|i| i.label == "warehouse" && i.kind == Kind::Database),
            "{:?}",
            s.items
        );
        // `warehouse.` asks for its tables, then lists them.
        let sql = "SELECT * FROM warehouse.";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert!(s.items.is_empty());
        assert_eq!(s.missing_schemas, vec!["warehouse".to_string()]);
        m.add_schema_tables("warehouse", vec!["bins".into(), "stock".into()]);
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert!(s.missing_schemas.is_empty());
        let l: Vec<&str> = s.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(l, vec!["bins", "stock"]);
    }

    #[test]
    fn other_schemas_are_listed_loaded_on_demand_and_inserted_qualified() {
        let m = multi();
        // Every known schema is offered, not just the current one.
        let l = labels("SELECT * FROM bil", &m);
        assert!(l.contains(&"billing".to_string()), "{l:?}");
        // A loaded schema's tables show qualified, after the current schema's.
        let s = suggest("SELECT * FROM inv", "SELECT * FROM inv".len(), &m).unwrap();
        assert_eq!(s.items[0].label, "billing.invoices");
        assert_eq!(s.items[0].insert, "billing.invoices");
        let l = labels("SELECT * FROM ord", &m);
        let current = l.iter().position(|x| x == "orders").unwrap();
        let other = l.iter().position(|x| x == "billing.orders").unwrap();
        assert!(current < other, "{l:?}");
        // An unloaded schema is asked for.
        let sql = "SELECT * FROM audit.";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert_eq!(s.missing_schemas, vec!["audit".to_string()]);
        // A loaded one lists its tables straight away.
        let sql = "SELECT * FROM billing.";
        let s = suggest(sql, sql.len(), &m).unwrap();
        assert!(s.missing_schemas.is_empty());
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn a_qualified_reference_fetches_and_uses_that_schemas_columns() {
        let mut m = multi();
        let sql = "SELECT o.to FROM billing.orders o";
        let cursor = "SELECT o.to".len();
        let s = suggest(sql, cursor, &m);
        let missing = s.map(|s| s.missing_columns).unwrap_or_default();
        assert_eq!(
            missing,
            vec![(Some("billing".to_string()), "orders".to_string())]
        );
        m.set_columns_in(Some("billing"), "orders", vec![col("total_due", "numeric")]);
        let s = suggest(sql, cursor, &m).unwrap();
        assert_eq!(s.items[0].label, "total_due");
        // The current schema's `orders` kept its own columns.
        let s = suggest("SELECT o.cu FROM orders o", "SELECT o.cu".len(), &m).unwrap();
        assert_eq!(s.items[0].label, "customer_id");
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
            assert_eq!(
                l.first().map(String::as_str),
                Some("customers"),
                "{sql}: {l:?}"
            );
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
            assert_eq!(
                (first.label.as_str(), first.kind),
                ("name", Kind::Column),
                "{sql}"
            );
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
        assert_eq!(s.missing_columns, vec![(None, "order_items".to_string())]);
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
        assert_eq!(
            labels("sel", &m).first().map(String::as_str),
            Some("select")
        );
        // After a table name the next word is a keyword (WHERE, JOIN, ...), not another table.
        let l = labels("SELECT * FROM customers wh", &m);
        assert_eq!(l.first().map(String::as_str), Some("where"), "{l:?}");
        let l = labels("SELECT * FROM customers WH", &m);
        assert_eq!(l.first().map(String::as_str), Some("WHERE"), "{l:?}");
    }

    #[test]
    fn keyword_case_follows_what_the_user_typed() {
        let m = meta();
        assert_eq!(
            labels("SEL", &m).first().map(String::as_str),
            Some("SELECT")
        );
        assert_eq!(
            labels("sel", &m).first().map(String::as_str),
            Some("select")
        );
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
