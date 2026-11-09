//! Exporting a result set, and the JSON / Text views of one.
//!
//! Three jobs, all pure so they test without a window or a database:
//!
//! 1. **Export**: turn a result into CSV or JSON bytes. The grid hands over
//!    the rows it shows, in the order it shows them; nothing here drops,
//!    rewrites or "sanitises" a value. In particular a cell that starts with
//!    `=`, `+`, `-` or `@` is written exactly as it is (a spreadsheet may run
//!    it as a formula, and the export dialog says so) and a NULL is written as
//!    an empty CSV field or a JSON `null`.
//! 2. **Views**: the results area can show the same rows as pretty JSON or as
//!    aligned text. Both are capped at [`VIEW_ROW_CAP`] rows and say so.
//! 3. **Errors**: split a driver's error string into a headline and a detail.
//!
//! The JSON writer is hand written rather than `serde_json::to_string_pretty`
//! on a `Map`: without serde_json's `preserve_order` feature a `Map` sorts its
//! keys, which would silently reorder the user's columns.

use std::path::Path;

use serde_json::Value;

use crate::data_forms::{expand_home, output_path_problem, suggest_path};
use crate::format::thousands;

/// Rows the JSON and Text views draw. Past this the view says how many more
/// there are and points at Export.
pub const VIEW_ROW_CAP: usize = 2000;

/// A text column is padded to its widest cell, but never wider than this, so
/// one long value cannot push every other column off screen.
pub const TEXT_COLUMN_CAP: usize = 40;

// ---------------------------------------------------------------------------
// Formats and scope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFormat {
    Csv,
    Json,
}

impl ResultFormat {
    pub const ALL: [ResultFormat; 2] = [ResultFormat::Csv, ResultFormat::Json];

    pub fn label(self) -> &'static str {
        match self {
            ResultFormat::Csv => "CSV",
            ResultFormat::Json => "JSON",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ResultFormat::Csv => "csv",
            ResultFormat::Json => "json",
        }
    }

    pub fn labels() -> Vec<String> {
        Self::ALL.iter().map(|f| f.label().to_string()).collect()
    }

    pub fn index(self) -> i32 {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0) as i32
    }

    /// Out-of-range indices fall back to CSV.
    pub fn at(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(ResultFormat::Csv)
    }
}

/// What to know about a format before writing it, shown under the path.
pub fn format_hint(format: ResultFormat) -> &'static str {
    match format {
        ResultFormat::Csv => {
            "NULL becomes an empty field. Values are written as they are, so a spreadsheet may run a cell that starts with =, +, - or @ as a formula."
        }
        ResultFormat::Json => {
            "An array of objects, one per row, keyed by column name. NULL becomes null; repeated column names get _2, _3."
        }
    }
}

/// What an export covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScope {
    /// A query tab: every row of the current result.
    Query,
    /// A table tab: the one page the inspector has loaded.
    TablePage,
}

/// One sentence that says which rows the file will hold, shown in the dialog
/// before anything is written.
///
/// `exported` is the number of rows going into the file, `held` the number the
/// result holds (they differ when a grid filter is active).
pub fn scope_note(scope: ExportScope, exported: usize, held: usize, columns: usize) -> String {
    let cols = format!("{} {}", thousands(columns), plural(columns, "column"));
    match scope {
        ExportScope::Query if exported == held => format!(
            "All {} {} of this result, {}.",
            thousands(exported),
            plural(exported, "row"),
            cols
        ),
        ExportScope::Query => format!(
            "The {} {} the grid shows after your filter (of {}), in grid order, {}.",
            thousands(exported),
            plural(exported, "row"),
            thousands(held),
            cols
        ),
        ExportScope::TablePage => format!(
            "This page only: {} {} of the table, {}. To export the whole table use Import and Export.",
            thousands(exported),
            plural(exported, "row"),
            cols
        ),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

// ---------------------------------------------------------------------------
// The file name and path
// ---------------------------------------------------------------------------

/// `<dir>/<stem>_<today>.<ext>`, with the stem made safe for a file name.
pub fn suggested_path(dir: &Path, stem: &str, format: ResultFormat, today: &str) -> String {
    suggest_path(dir, stem, format.extension(), today)
}

/// Swap the extension when the format changes, but only on a path that ends
/// in one of ours, so a name the user typed is never mangled.
pub fn retarget_extension(path: &str, format: ResultFormat) -> String {
    let p = Path::new(path.trim());
    let known = ResultFormat::ALL.map(|f| f.extension());
    if p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| known.iter().any(|k| k.eq_ignore_ascii_case(e)))
    {
        p.with_extension(format.extension())
            .to_string_lossy()
            .to_string()
    } else {
        path.to_string()
    }
}

/// Whether a path can be written, and what writing it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathCheck {
    /// Cannot write; the text says why, ready to show next to the field.
    Problem(String),
    /// A file is already there and would be replaced.
    Replaces,
    /// A new file.
    Free,
}

pub fn check_path(path: &str, format: ResultFormat) -> PathCheck {
    if let Some(p) = output_path_problem(path, format.extension()) {
        return PathCheck::Problem(p);
    }
    if Path::new(&expand_home(path)).exists() {
        PathCheck::Replaces
    } else {
        PathCheck::Free
    }
}

/// Write `body` to `path` without ever leaving a half-written file: the text
/// goes to a sibling temp file first and is renamed over the target, so a
/// failed write cannot clobber an existing export.
pub fn write_export(path: &str, body: &str) -> Result<(), String> {
    let target = std::path::PathBuf::from(expand_home(path));
    let mut tmp = target.clone().into_os_string();
    tmp.push(".dbhive-partial");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not write {}: {e}", target.display())
    })
}

/// The program and arguments that show `path` selected in the file manager.
/// Returned rather than run so it can be tested.
pub fn reveal_command(path: &str, os: &str) -> (String, Vec<String>) {
    match os {
        "macos" => ("open".into(), vec!["-R".into(), path.into()]),
        "windows" => ("explorer".into(), vec![format!("/select,{path}")]),
        _ => {
            // No portable "select this file" on Linux: open its folder.
            let dir = Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".into());
            ("xdg-open".into(), vec![dir])
        }
    }
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

/// A cell as CSV text. NULL is empty; a nested value keeps its JSON form.
fn csv_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Quote a CSV field when it has to be, doubling embedded quotes (RFC 4180).
///
/// Quoted when it holds a comma, a double quote or a line break, when it has
/// leading or trailing whitespace (so a tolerant reader cannot trim it), and
/// when it is empty in a one-column file (otherwise the record is a blank line
/// that readers skip).
fn csv_field(value: &str, only_column: bool) -> String {
    let needs = value.contains([',', '"', '\n', '\r'])
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
        || (only_column && value.is_empty());
    if needs {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// The whole file: a header row, then one record per row, each ended by `\n`.
/// A row shorter than the header is padded with empty fields.
pub fn render_csv(columns: &[String], rows: &[Vec<Value>]) -> String {
    let only = columns.len() == 1;
    let mut out = String::new();
    let header: Vec<String> = columns.iter().map(|c| csv_field(c, only)).collect();
    out.push_str(&header.join(","));
    out.push('\n');
    for row in rows {
        let fields: Vec<String> = (0..columns.len())
            .map(|i| csv_field(&row.get(i).map(csv_text).unwrap_or_default(), only))
            .collect();
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Object keys for the columns. Two columns with one name (`SELECT a.id,
/// b.id`) would overwrite each other in an object, so the later ones become
/// `id_2`, `id_3`, and so on, skipping any name already taken.
pub fn unique_keys(columns: &[String]) -> Vec<String> {
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut keys = Vec::with_capacity(columns.len());
    for c in columns {
        let mut key = c.clone();
        let mut n = 2;
        while taken.contains(&key) {
            key = format!("{c}_{n}");
            n += 1;
        }
        taken.insert(key.clone());
        keys.push(key);
    }
    keys
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// One value at nesting `depth` (0 is the array's own level). Scalars are
/// compact; objects and arrays inside a cell are indented to match.
fn json_value(value: &Value, indent: usize) -> String {
    match value {
        Value::Array(_) | Value::Object(_) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
            let pad = " ".repeat(indent);
            pretty.replace('\n', &format!("\n{pad}"))
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
    }
}

/// An array of objects keyed by column, in column order, two-space indented.
/// A row shorter than the header gets `null` for the missing cells; cells past
/// the header are ignored. No rows is `[]`.
pub fn render_json(columns: &[String], rows: &[Vec<Value>]) -> String {
    if rows.is_empty() {
        return "[]\n".to_string();
    }
    let keys: Vec<String> = unique_keys(columns)
        .iter()
        .map(|k| json_string(k))
        .collect();
    let mut out = String::from("[\n");
    for (r, row) in rows.iter().enumerate() {
        if keys.is_empty() {
            out.push_str("  {}");
        } else {
            out.push_str("  {\n");
            for (i, key) in keys.iter().enumerate() {
                let value = row.get(i).unwrap_or(&Value::Null);
                out.push_str("    ");
                out.push_str(key);
                out.push_str(": ");
                out.push_str(&json_value(value, 4));
                if i + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  }");
        }
        if r + 1 < rows.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("]\n");
    out
}

pub fn render(format: ResultFormat, columns: &[String], rows: &[Vec<Value>]) -> String {
    match format {
        ResultFormat::Csv => render_csv(columns, rows),
        ResultFormat::Json => render_json(columns, rows),
    }
}

// ---------------------------------------------------------------------------
// The JSON and Text views
// ---------------------------------------------------------------------------

/// The results area's three ways to look at a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Table,
    Json,
    Text,
    /// A bar or line chart (`result_chart`).
    Chart,
    /// The statement's query plan (`query_plan`).
    Plan,
}

impl ViewMode {
    pub const ALL: [ViewMode; 5] = [
        ViewMode::Table,
        ViewMode::Json,
        ViewMode::Text,
        ViewMode::Chart,
        ViewMode::Plan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Table => "Table",
            ViewMode::Json => "JSON",
            ViewMode::Text => "Text",
            ViewMode::Chart => "Chart",
            ViewMode::Plan => "Plan",
        }
    }

    pub fn labels() -> Vec<String> {
        Self::ALL.iter().map(|m| m.label().to_string()).collect()
    }

    pub fn index(self) -> i32 {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as i32
    }

    pub fn at(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or_default()
    }
}

/// What a view draws: its lines, and how much of the result they cover.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewText {
    pub lines: Vec<String>,
    pub shown_rows: usize,
    pub total_rows: usize,
}

impl ViewText {
    pub fn truncated(&self) -> bool {
        self.shown_rows < self.total_rows
    }

    /// The longest line, in characters, for sizing the horizontal scroll.
    pub fn max_columns(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
    }

    /// The lines as one string, for the Copy button.
    pub fn joined(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

/// The notice under a capped view, or `None` when everything is shown.
pub fn truncation_notice(shown: usize, total: usize) -> Option<String> {
    (shown < total).then(|| {
        format!(
            "Showing the first {} of {} rows. Export writes all of them.",
            thousands(shown),
            thousands(total)
        )
    })
}

/// Pretty JSON for the first `cap` rows.
pub fn json_view(columns: &[String], rows: &[Vec<Value>], cap: usize) -> ViewText {
    let shown = rows.len().min(cap);
    let body = render_json(columns, &rows[..shown]);
    ViewText {
        lines: body.lines().map(str::to_string).collect(),
        shown_rows: shown,
        total_rows: rows.len(),
    }
}

/// A cell on one line: a database NULL reads `NULL`, and tabs and line breaks
/// inside a value are shown as `\t`, `\n`, `\r` so a row is always one line.
fn text_cell(value: &Value) -> String {
    let raw = match value {
        Value::Null => return "NULL".to_string(),
        other => csv_text(other),
    };
    raw.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// A header and one line per row, columns padded into a grid so it reads like
/// a terminal result. [`text_tsv`] is the same rows with real tabs, which is
/// what the Copy button puts on the clipboard.
pub fn text_view(columns: &[String], rows: &[Vec<Value>], cap: usize) -> ViewText {
    let shown = rows.len().min(cap);
    let cells: Vec<Vec<String>> = rows[..shown]
        .iter()
        .map(|row| {
            (0..columns.len())
                .map(|i| row.get(i).map(text_cell).unwrap_or_default())
                .collect()
        })
        .collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            cells
                .iter()
                .map(|r| r[i].chars().count())
                .chain(std::iter::once(columns[i].chars().count()))
                .max()
                .unwrap_or(0)
                .min(TEXT_COLUMN_CAP)
        })
        .collect();
    let pad_line = |fields: Vec<&str>| -> String {
        let mut line = String::new();
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            line.push_str(f);
            // The last column needs no trailing spaces.
            if i + 1 < fields.len() {
                let len = f.chars().count();
                if len < widths[i] {
                    line.push_str(&" ".repeat(widths[i] - len));
                }
            }
        }
        line
    };
    let mut lines = Vec::with_capacity(shown + 1);
    lines.push(pad_line(columns.iter().map(String::as_str).collect()));
    for row in &cells {
        lines.push(pad_line(row.iter().map(String::as_str).collect()));
    }
    ViewText {
        lines,
        shown_rows: shown,
        total_rows: rows.len(),
    }
}

/// The Text view as real tab-separated values (same cap, same escaping).
pub fn text_tsv(columns: &[String], rows: &[Vec<Value>], cap: usize) -> String {
    let shown = rows.len().min(cap);
    let mut out = columns.join("\t");
    out.push('\n');
    for row in &rows[..shown] {
        let fields: Vec<String> = (0..columns.len())
            .map(|i| row.get(i).map(text_cell).unwrap_or_default())
            .collect();
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// JSON highlighting
// ---------------------------------------------------------------------------

/// What a run of a JSON line is. The numbers are what the Slint viewer maps to
/// theme colours, so they are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonKind {
    /// Indentation and other whitespace.
    Plain = 0,
    Key = 1,
    String = 2,
    Number = 3,
    Bool = 4,
    Null = 5,
    /// Braces, brackets, commas and colons.
    Punct = 6,
}

/// Split one line of pretty JSON into coloured runs. Lines come from
/// [`render_json`], where a string never spans lines (newlines are escaped),
/// so each line can be read alone. Unknown text becomes `Plain`; the runs
/// always join back to the line.
pub fn json_runs(line: &str) -> Vec<(JsonKind, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut runs: Vec<(JsonKind, String)> = Vec::new();
    let mut push = |kind: JsonKind, text: String| {
        if text.is_empty() {
            return;
        }
        match runs.last_mut() {
            Some((k, t)) if *k == kind && kind != JsonKind::Punct => t.push_str(&text),
            _ => runs.push((kind, text)),
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => {
                let start = i;
                while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                push(JsonKind::Plain, chars[start..i].iter().collect());
            }
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == '"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let end = i.min(chars.len());
                // A string followed by a colon is an object key.
                let mut j = end;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                let kind = if chars.get(j) == Some(&':') {
                    JsonKind::Key
                } else {
                    JsonKind::String
                };
                push(kind, chars[start..end].iter().collect());
            }
            '{' | '}' | '[' | ']' | ',' | ':' => {
                push(JsonKind::Punct, c.to_string());
                i += 1;
            }
            '-' | '0'..='9' => {
                let start = i;
                while i < chars.len() && matches!(chars[i], '-' | '+' | '.' | 'e' | 'E' | '0'..='9')
                {
                    i += 1;
                }
                push(JsonKind::Number, chars[start..i].iter().collect());
            }
            _ => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i == start {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let kind = match word.as_str() {
                    "true" | "false" => JsonKind::Bool,
                    "null" => JsonKind::Null,
                    _ => JsonKind::Plain,
                };
                push(kind, word);
            }
        }
    }
    runs
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error ready to draw: what went wrong in a few words, the driver's own
/// message, and a code when the message carried one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorView {
    pub headline: String,
    pub detail: String,
    pub code: String,
}

/// `DbError`'s Display prefixes, with the headline each one gets.
const PREFIXES: [(&str, &str); 7] = [
    ("Query execution failed: ", "Query failed"),
    ("Connection failed: ", "Connection failed"),
    ("Authentication failed: ", "Authentication failed"),
    ("Operation timed out: ", "Query timed out"),
    ("Invalid input: ", "Invalid input"),
    ("Not found: ", "Not found"),
    ("Internal error: ", "Something went wrong"),
];

/// Wording the sqlx drivers put in front of the server's own message.
const NOISE: [&str; 2] = ["error returned from database: ", "error occurred while "];

/// A more useful headline than "Query failed", chosen from the message.
fn refine(lower: &str) -> Option<&'static str> {
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if has(&[
        "syntax error",
        "you have an error in your sql syntax",
        "incorrect syntax",
    ]) {
        Some("Syntax error")
    } else if has(&["unique constraint", "duplicate key", "duplicate entry"]) {
        Some("Duplicate value")
    } else if has(&["foreign key constraint", "violates foreign key"]) {
        Some("Foreign key violation")
    } else if has(&[
        "not null constraint",
        "null value in column",
        "cannot be null",
    ]) {
        Some("Missing required value")
    } else if has(&[
        "no such table",
        "no such column",
        "unknown column",
        "unknown table",
    ]) || (lower.contains("does not exist") && has(&["relation", "column", "table"]))
        || (lower.contains("doesn't exist") && lower.contains("table"))
        || lower.contains("invalid object name")
    {
        Some("Table or column not found")
    } else if has(&["permission denied", "access denied", "not authorized"]) {
        Some("Permission denied")
    } else if has(&["timed out", "timeout"]) {
        Some("Query timed out")
    } else if has(&[
        "connection refused",
        "connection reset",
        "broken pipe",
        "connection closed",
    ]) {
        Some("Connection lost")
    } else {
        None
    }
}

/// The error code in a driver message: SQLite's `(code: 1)`, MySQL's
/// `1064 (42000)`, or a Postgres `SQLSTATE 42601`. Empty when there is none.
fn find_code(message: &str) -> String {
    if let Some(i) = message.find("(code: ") {
        let rest = &message[i + 7..];
        if let Some(end) = rest.find(')') {
            return rest[..end].trim().to_string();
        }
    }
    if let Some(i) = message.find("SQLSTATE") {
        let rest = message[i + 8..].trim_start_matches([' ', ':']);
        let code: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if code.len() == 5 {
            return code;
        }
    }
    // MySQL: "1064 (42000): You have ...".
    let digits: String = message.chars().take_while(char::is_ascii_digit).collect();
    if (3..=5).contains(&digits.len()) {
        let rest = &message[digits.len()..];
        if let Some(inner) = rest.strip_prefix(" (") {
            if let Some(end) = inner.find("):") {
                return format!("{digits} ({})", &inner[..end]);
            }
        }
    }
    String::new()
}

pub fn describe_error(raw: &str) -> ErrorView {
    // A stopped query is not a failure. Callers show their own neutral state
    // for it; this only makes sure the sentinel never reaches the user raw.
    if crate::format::is_cancelled(raw) {
        return ErrorView {
            headline: "Cancelled".to_string(),
            detail: "You stopped this query before it finished.".to_string(),
            code: String::new(),
        };
    }
    let raw = raw.trim();
    let mut headline = "Query failed";
    let mut rest = raw;
    for (prefix, title) in PREFIXES {
        if let Some(r) = raw.strip_prefix(prefix) {
            headline = title;
            rest = r.trim();
            break;
        }
    }
    let mut detail = rest.to_string();
    for noise in NOISE {
        if let Some(r) = detail.strip_prefix(noise) {
            detail = r.to_string();
        }
    }
    let code = find_code(&detail);
    if let Some(better) = refine(&detail.to_lowercase()) {
        headline = better;
    }
    if detail.is_empty() {
        detail = "The database returned no further detail.".to_string();
    }
    ErrorView {
        headline: headline.to_string(),
        detail,
        code,
    }
}

/// What the Copy button puts on the clipboard: the error, its code, and the
/// query that caused it.
pub fn error_clipboard(view: &ErrorView, sql: &str) -> String {
    let mut out = format!("{}\n{}", view.headline, view.detail);
    if !view.code.is_empty() {
        out.push_str(&format!("\nCode: {}", view.code));
    }
    if !sql.trim().is_empty() {
        out.push_str(&format!("\n\nQuery:\n{}", sql.trim_end()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // -- CSV ------------------------------------------------------------------

    #[test]
    fn csv_quotes_commas_quotes_and_line_breaks() {
        let rows = vec![vec![
            json!("a,b"),
            json!("say \"hi\""),
            json!("line1\nline2"),
            json!("cr\rhere"),
            json!("plain"),
        ]];
        let out = render_csv(&cols(&["c1", "c2", "c3", "c4", "c5"]), &rows);
        assert_eq!(
            out,
            "c1,c2,c3,c4,c5\n\"a,b\",\"say \"\"hi\"\"\",\"line1\nline2\",\"cr\rhere\",plain\n"
        );
    }

    #[test]
    fn csv_header_names_are_quoted_like_values() {
        let out = render_csv(&cols(&["a,b", "say \"x\"", "ok"]), &[]);
        assert_eq!(out, "\"a,b\",\"say \"\"x\"\"\",ok\n");
    }

    #[test]
    fn csv_writes_formula_looking_values_untouched() {
        // Nothing is neutralised silently: the dialog tells the user instead.
        let rows = vec![vec![
            json!("=1+1"),
            json!("+SUM(A1)"),
            json!("-2"),
            json!("@cmd"),
        ]];
        let out = render_csv(&cols(&["a", "b", "c", "d"]), &rows);
        assert_eq!(out, "a,b,c,d\n=1+1,+SUM(A1),-2,@cmd\n");
    }

    #[test]
    fn csv_null_is_empty_and_distinct_types_keep_their_text() {
        let rows = vec![vec![
            json!(null),
            json!(true),
            json!(1.5),
            json!(42),
            json!({"k": [1, 2]}),
        ]];
        let out = render_csv(&cols(&["n", "b", "f", "i", "o"]), &rows);
        // The object contains a comma and quotes, so it is quoted.
        assert_eq!(out, "n,b,f,i,o\n,true,1.5,42,\"{\"\"k\"\":[1,2]}\"\n");
    }

    #[test]
    fn csv_keeps_unicode_and_leading_trailing_space() {
        let rows = vec![vec![json!("héllo 世界 🐝"), json!("  padded  "), json!("")]];
        let out = render_csv(&cols(&["u", "p", "e"]), &rows);
        assert_eq!(out, "u,p,e\nhéllo 世界 🐝,\"  padded  \",\n");
    }

    #[test]
    fn csv_one_column_empty_value_is_quoted_so_the_row_survives() {
        let rows = vec![vec![json!("")], vec![json!(null)], vec![json!("x")]];
        let out = render_csv(&cols(&["only"]), &rows);
        assert_eq!(out, "only\n\"\"\n\"\"\nx\n");
    }

    #[test]
    fn csv_short_rows_are_padded_and_long_rows_trimmed() {
        let rows = vec![vec![json!(1)], vec![json!(1), json!(2), json!(3)]];
        let out = render_csv(&cols(&["a", "b"]), &rows);
        assert_eq!(out, "a,b\n1,\n1,2\n");
    }

    #[test]
    fn csv_with_no_rows_is_just_the_header() {
        assert_eq!(render_csv(&cols(&["a", "b"]), &[]), "a,b\n");
    }

    // -- JSON -----------------------------------------------------------------

    #[test]
    fn json_is_an_array_of_objects_in_column_order() {
        let rows = vec![vec![json!(2), json!("b")], vec![json!(1), json!(null)]];
        let out = render_json(&cols(&["zeta", "alpha"]), &rows);
        // Keys keep the column order: zeta before alpha, not sorted.
        assert_eq!(
            out,
            "[\n  {\n    \"zeta\": 2,\n    \"alpha\": \"b\"\n  },\n  {\n    \"zeta\": 1,\n    \"alpha\": null\n  }\n]\n"
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[1]["alpha"], Value::Null);
    }

    #[test]
    fn json_escapes_nasty_strings_and_round_trips() {
        let nasty = "q\"uote \\ back\nline\ttab \u{1} ctl 世界 🐝 </script>";
        let rows = vec![vec![json!(nasty), json!("=1+1")]];
        let out = render_json(&cols(&["a\"b", "f"]), &rows);
        let parsed: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed[0]["a\"b"], json!(nasty));
        assert_eq!(parsed[0]["f"], json!("=1+1"));
    }

    #[test]
    fn json_duplicate_column_names_do_not_overwrite_each_other() {
        let rows = vec![vec![json!(1), json!(2), json!(3)]];
        let out = render_json(&cols(&["id", "id", "id"]), &rows);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["id"], json!(1));
        assert_eq!(parsed[0]["id_2"], json!(2));
        assert_eq!(parsed[0]["id_3"], json!(3));
    }

    #[test]
    fn unique_keys_skip_a_name_that_already_exists() {
        assert_eq!(
            unique_keys(&cols(&["id", "id_2", "id"])),
            vec!["id", "id_2", "id_3"]
        );
    }

    #[test]
    fn json_nested_values_stay_valid_and_indented() {
        let rows = vec![vec![json!({"a": [1, {"b": null}]}), json!([])]];
        let out = render_json(&cols(&["doc", "list"]), &rows);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["doc"]["a"][1]["b"], Value::Null);
        assert!(out.contains("    \"doc\": {\n      \"a\": ["));
    }

    #[test]
    fn json_numbers_keep_their_precision() {
        let rows = vec![vec![
            json!(9007199254740993_i64),
            json!(u64::MAX),
            json!(0.1),
        ]];
        let out = render_json(&cols(&["i", "u", "f"]), &rows);
        assert!(out.contains("9007199254740993"));
        assert!(out.contains("18446744073709551615"));
        assert!(out.contains("0.1"));
    }

    #[test]
    fn json_empty_and_ragged_inputs() {
        assert_eq!(render_json(&cols(&["a"]), &[]), "[]\n");
        let out = render_json(&cols(&["a", "b"]), &[vec![json!(1)]]);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["b"], Value::Null);
        let none = render_json(&[], &[vec![]]);
        assert!(serde_json::from_str::<Value>(&none).is_ok());
    }

    // -- Views ------------------------------------------------------------------

    fn numbered(n: usize) -> Vec<Vec<Value>> {
        (0..n)
            .map(|i| vec![json!(i), json!(format!("row {i}"))])
            .collect()
    }

    #[test]
    fn views_cap_at_the_row_limit_and_report_the_rest() {
        let rows = numbered(2500);
        let j = json_view(&cols(&["id", "name"]), &rows, VIEW_ROW_CAP);
        assert_eq!((j.shown_rows, j.total_rows), (2000, 2500));
        assert!(j.truncated());
        // 4 lines per row plus the brackets.
        assert_eq!(j.lines.len(), 2000 * 4 + 2);
        let t = text_view(&cols(&["id", "name"]), &rows, VIEW_ROW_CAP);
        assert_eq!((t.shown_rows, t.total_rows), (2000, 2500));
        assert_eq!(t.lines.len(), 2001);
        assert_eq!(
            truncation_notice(t.shown_rows, t.total_rows).unwrap(),
            "Showing the first 2,000 of 2,500 rows. Export writes all of them."
        );
    }

    #[test]
    fn no_notice_when_everything_fits() {
        let rows = numbered(2000);
        let t = text_view(&cols(&["id", "name"]), &rows, VIEW_ROW_CAP);
        assert!(!t.truncated());
        assert_eq!(truncation_notice(t.shown_rows, t.total_rows), None);
    }

    #[test]
    fn text_view_aligns_columns_and_marks_null() {
        let rows = vec![
            vec![json!(1), json!("alpha")],
            vec![json!(null), json!("b")],
        ];
        let t = text_view(&cols(&["id", "name"]), &rows, 10);
        assert_eq!(t.lines, vec!["id    name", "1     alpha", "NULL  b"]);
    }

    #[test]
    fn text_view_escapes_tabs_and_newlines_so_a_row_is_one_line() {
        let rows = vec![vec![json!("a\tb\nc"), json!("back\\slash")]];
        let t = text_view(&cols(&["x", "y"]), &rows, 10);
        assert_eq!(t.lines.len(), 2);
        assert!(t.lines[1].starts_with("a\\tb\\nc"));
        assert!(t.lines[1].contains("back\\\\slash"));
        let tsv = text_tsv(&cols(&["x", "y"]), &rows, 10);
        assert_eq!(tsv, "x\ty\na\\tb\\nc\tback\\\\slash\n");
    }

    #[test]
    fn text_view_caps_column_padding() {
        let long = "x".repeat(100);
        let rows = vec![
            vec![json!(long.clone()), json!("b")],
            vec![json!("s"), json!("b")],
        ];
        let t = text_view(&cols(&["a", "b"]), &rows, 10);
        // The long cell is not cut, and the short row pads to the cap only.
        assert!(t.lines[1].starts_with(&long));
        assert_eq!(
            t.lines[2],
            format!("s{}  b", " ".repeat(TEXT_COLUMN_CAP - 1))
        );
    }

    #[test]
    fn view_mode_round_trips_through_its_index() {
        for m in ViewMode::ALL {
            assert_eq!(ViewMode::at(m.index()), m);
        }
        assert_eq!(ViewMode::at(-1), ViewMode::Table);
        assert_eq!(ViewMode::at(9), ViewMode::Table);
        assert_eq!(ViewMode::at(3), ViewMode::Chart);
    }

    // -- Highlighting -------------------------------------------------------------

    fn kinds(line: &str) -> Vec<(JsonKind, String)> {
        json_runs(line)
    }

    #[test]
    fn highlight_splits_a_key_value_line() {
        use JsonKind::*;
        assert_eq!(
            kinds("    \"name\": \"Ada\","),
            vec![
                (Plain, "    ".into()),
                (Key, "\"name\"".into()),
                (Punct, ":".into()),
                (Plain, " ".into()),
                (String, "\"Ada\"".into()),
                (Punct, ",".into()),
            ]
        );
    }

    #[test]
    fn highlight_tells_scalars_apart() {
        use JsonKind::*;
        let k: Vec<JsonKind> = kinds("\"a\": -1.5e3").iter().map(|r| r.0).collect();
        assert_eq!(k, vec![Key, Punct, Plain, Number]);
        assert_eq!(kinds("true").first().unwrap().0, Bool);
        assert_eq!(kinds("null").first().unwrap().0, Null);
        assert_eq!(kinds("  {").last().unwrap().0, Punct);
    }

    #[test]
    fn highlight_handles_escapes_and_colons_inside_strings() {
        use JsonKind::*;
        // A quote and a colon inside a value do not end it or make it a key.
        let runs = kinds("\"v\": \"a\\\"b: c\"");
        assert_eq!(runs.last().unwrap(), &(String, "\"a\\\"b: c\"".to_string()));
        assert_eq!(runs.first().unwrap().0, Key);
    }

    #[test]
    fn highlight_runs_always_join_back_to_the_line() {
        let lines = [
            "[",
            "  {",
            "    \"k\": \"unterminated",
            "    \"emoji 🐝\": [1, 2.5, true, null],",
            "    garbage ~ here",
            "",
        ];
        for line in lines {
            let joined: String = json_runs(line).iter().map(|r| r.1.as_str()).collect();
            assert_eq!(joined, line);
        }
    }

    #[test]
    fn every_line_of_a_rendered_view_highlights_without_loss() {
        let rows = vec![vec![json!("x\"y"), json!({"a": [1, null]}), json!(false)]];
        let v = json_view(&cols(&["s", "o", "b"]), &rows, 10);
        for line in &v.lines {
            let joined: String = json_runs(line).iter().map(|r| r.1.as_str()).collect();
            assert_eq!(&joined, line);
        }
    }

    // -- Paths, scope, reveal -----------------------------------------------------

    #[test]
    fn suggested_path_is_dated_and_safe() {
        let p = suggested_path(
            Path::new("/tmp/dl"),
            "my table/1",
            ResultFormat::Json,
            "2026-10-21",
        );
        assert_eq!(p, "/tmp/dl/my_table_1_2026-10-21.json");
    }

    #[test]
    fn changing_format_retargets_our_extensions_only() {
        assert_eq!(
            retarget_extension("/x/a.csv", ResultFormat::Json),
            "/x/a.json"
        );
        assert_eq!(
            retarget_extension("/x/a.JSON", ResultFormat::Csv),
            "/x/a.csv"
        );
        assert_eq!(
            retarget_extension("/x/notes.txt", ResultFormat::Json),
            "/x/notes.txt"
        );
        assert_eq!(
            retarget_extension("/x/noext", ResultFormat::Json),
            "/x/noext"
        );
    }

    #[test]
    fn check_path_separates_problems_from_replace_and_free() {
        let dir = std::env::temp_dir().join(format!("dbhive-result-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let free = dir.join("new.csv");
        let taken = dir.join("old.csv");
        std::fs::write(&taken, "x").unwrap();
        assert_eq!(
            check_path(free.to_str().unwrap(), ResultFormat::Csv),
            PathCheck::Free
        );
        assert_eq!(
            check_path(taken.to_str().unwrap(), ResultFormat::Csv),
            PathCheck::Replaces
        );
        assert!(matches!(
            check_path("", ResultFormat::Csv),
            PathCheck::Problem(_)
        ));
        assert!(matches!(
            check_path("relative.csv", ResultFormat::Csv),
            PathCheck::Problem(_)
        ));
        assert!(matches!(
            check_path(free.to_str().unwrap(), ResultFormat::Json),
            PathCheck::Problem(_)
        ));
        assert!(matches!(
            check_path(
                dir.join("missing").join("a.csv").to_str().unwrap(),
                ResultFormat::Csv
            ),
            PathCheck::Problem(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_export_replaces_atomically_and_reports_failure() {
        let dir = std::env::temp_dir().join(format!("dbhive-write-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.csv");
        std::fs::write(&file, "old").unwrap();
        write_export(file.to_str().unwrap(), "a,b\n1,2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a,b\n1,2\n");
        // No temp file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1);
        // A target in a missing folder fails and says where.
        let bad = dir.join("missing").join("x.csv");
        let err = write_export(bad.to_str().unwrap(), "x").unwrap_err();
        assert!(err.contains("Could not write"), "{err}");
        // The earlier file is untouched.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a,b\n1,2\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scope_notes_say_which_rows_land_in_the_file() {
        assert_eq!(
            scope_note(ExportScope::Query, 1204, 1204, 6),
            "All 1,204 rows of this result, 6 columns."
        );
        assert_eq!(
            scope_note(ExportScope::Query, 12, 1204, 1),
            "The 12 rows the grid shows after your filter (of 1,204), in grid order, 1 column."
        );
        let page = scope_note(ExportScope::TablePage, 50, 50, 4);
        assert!(page.starts_with("This page only: 50 rows of the table"));
        assert!(page.contains("whole table"));
    }

    #[test]
    fn reveal_selects_the_file_where_the_os_can() {
        assert_eq!(
            reveal_command("/Users/a/Downloads/x.csv", "macos"),
            (
                "open".to_string(),
                vec!["-R".to_string(), "/Users/a/Downloads/x.csv".to_string()]
            )
        );
        assert_eq!(
            reveal_command("C:\\a\\x.csv", "windows"),
            (
                "explorer".to_string(),
                vec!["/select,C:\\a\\x.csv".to_string()]
            )
        );
        assert_eq!(
            reveal_command("/home/a/x.csv", "linux"),
            ("xdg-open".to_string(), vec!["/home/a".to_string()])
        );
    }

    // -- Errors ---------------------------------------------------------------------

    #[test]
    fn a_sqlite_syntax_error_gets_a_headline_detail_and_code() {
        let v = describe_error(
            "Query execution failed: error returned from database: (code: 1) near \"FORM\": syntax error",
        );
        assert_eq!(v.headline, "Syntax error");
        assert_eq!(v.detail, "(code: 1) near \"FORM\": syntax error");
        assert_eq!(v.code, "1");
    }

    #[test]
    fn a_missing_table_is_named_as_such() {
        let v = describe_error("Query execution failed: error returned from database: (code: 1) no such table: scratch");
        assert_eq!(v.headline, "Table or column not found");
        let v = describe_error("Query execution failed: relation \"nope\" does not exist");
        assert_eq!(v.headline, "Table or column not found");
    }

    #[test]
    fn constraint_and_permission_errors_are_recognised() {
        let h = |m: &str| describe_error(m).headline;
        assert_eq!(
            h("UNIQUE constraint failed: users.email"),
            "Duplicate value"
        );
        assert_eq!(
            h("duplicate key value violates unique constraint \"x\""),
            "Duplicate value"
        );
        assert_eq!(h("FOREIGN KEY constraint failed"), "Foreign key violation");
        assert_eq!(
            h("NOT NULL constraint failed: t.c"),
            "Missing required value"
        );
        assert_eq!(h("permission denied for table users"), "Permission denied");
    }

    #[test]
    fn mysql_codes_and_timeouts() {
        let v = describe_error("Query execution failed: error returned from database: 1064 (42000): You have an error in your SQL syntax");
        assert_eq!(v.code, "1064 (42000)");
        assert_eq!(v.headline, "Syntax error");
        let t = describe_error("Operation timed out: Query timeout after 30s");
        assert_eq!(t.headline, "Query timed out");
        assert_eq!(t.detail, "Query timeout after 30s");
    }

    #[test]
    fn a_cancelled_query_is_described_without_the_sentinel() {
        let v = describe_error(crate::format::CANCELLED);
        assert_eq!(v.headline, "Cancelled");
        assert!(!v.detail.contains("dbhive:"), "{:?}", v.detail);
        assert!(v.code.is_empty());
    }

    #[test]
    fn an_unrecognised_message_is_kept_whole() {
        let v = describe_error("something odd happened");
        assert_eq!(v.headline, "Query failed");
        assert_eq!(v.detail, "something odd happened");
        assert_eq!(v.code, "");
        assert_eq!(
            describe_error("").detail,
            "The database returned no further detail."
        );
    }

    #[test]
    fn the_clipboard_text_carries_the_query() {
        let v = describe_error("Query execution failed: (code: 1) near \"FORM\": syntax error");
        let text = error_clipboard(&v, "SELECT * FORM t;\n");
        assert_eq!(
            text,
            "Syntax error\n(code: 1) near \"FORM\": syntax error\nCode: 1\n\nQuery:\nSELECT * FORM t;"
        );
        assert!(!error_clipboard(&v, "  ").contains("Query:"));
    }
}
