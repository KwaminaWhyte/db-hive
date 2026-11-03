//! Schema migrations view logic, lifted from the egui `migrations` screen.
//! See lib.rs.
//!
//! The wizard's plain-data state ([`Wizard`], [`Step`], [`DiffView`]), diff
//! emptiness and change counting, selection validation, the statement text
//! round trip ([`statements_to_text`] and [`split_statements`]), executable
//! counting, the diff projected into a `QueryResult` for the results grid,
//! and apply-outcome formatting.
//!
//! [`split_statements`] decides what is sent to the target database after
//! the user edits the generated SQL. Getting it wrong half-executes a DDL
//! migration, so its tests are the ones that matter most here.
//!
//! The operations themselves (diff, generate, apply) stay in
//! `dbhive_core::service_migrations`.

use std::collections::HashSet;

use dbhive_core::drivers::QueryResult;
use dbhive_core::migrations::{SchemaDiff, TableDiff};
use dbhive_core::models::ConnectionProfile;
use dbhive_core::service_migrations::ApplyResult;

// ===========================================================================
// State
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Select,
    Diff,
    Sql,
}

impl Step {
    /// Every step, in wizard order.
    pub const ALL: [Step; 3] = [Step::Select, Step::Diff, Step::Sql];

    pub fn index(self) -> usize {
        match self {
            Step::Select => 0,
            Step::Diff => 1,
            Step::Sql => 2,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Select => "Select",
            Step::Diff => "Diff",
            Step::Sql => "SQL",
        }
    }
}

/// How the diff is presented.
///
/// `Tree` is the web shell's nested, expandable view. `Grid` flattens every
/// change into one row and hands it to the results grid, which brings
/// filtering and sorting for free; a hundred-table diff is unreadable as a
/// tree and this is the cheap way to search it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffView {
    #[default]
    Tree,
    Grid,
}

/// The wizard's state that is plain data. A shell wraps this with its own
/// widget state (grid, editor) and background jobs.
#[derive(Debug, Clone)]
pub struct Wizard {
    pub step: Step,
    /// Profile id of the source (desired) schema.
    pub source_id: Option<String>,
    /// Profile id of the target, which is what gets modified.
    pub target_id: Option<String>,
    pub schema: String,

    pub diff: Option<SchemaDiff>,
    pub diff_view: DiffView,
    /// Flattened diff, rebuilt only when the diff changes rather than every
    /// frame. Set through [`Wizard::set_diff`].
    pub diff_rows: Option<QueryResult>,

    /// Statements exactly as the generator produced them, kept so an edited
    /// buffer can be reset.
    pub generated: Vec<String>,
    /// The editable buffer actually applied.
    pub sql: String,

    pub use_transaction: bool,
    pub acknowledged: bool,
    /// True while the confirmation dialog is up.
    pub confirming: bool,

    pub last_error: Option<String>,
    pub outcome: Option<ApplyResult>,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            step: Step::Select,
            source_id: None,
            target_id: None,
            // The web shell prefilled "public"; anything else is a first
            // keystroke every single time for the Postgres majority.
            schema: "public".to_string(),
            diff: None,
            diff_view: DiffView::Tree,
            diff_rows: None,
            generated: Vec::new(),
            sql: String::new(),
            use_transaction: true,
            acknowledged: false,
            confirming: false,
            last_error: None,
            outcome: None,
        }
    }
}

impl Wizard {
    /// Clear everything derived from a previous run, keeping the connection
    /// and schema choices so reopening after a failure is not a retype.
    pub fn reset(&mut self) {
        self.step = Step::Select;
        self.diff = None;
        self.diff_rows = None;
        self.generated.clear();
        self.sql.clear();
        self.acknowledged = false;
        self.confirming = false;
        self.last_error = None;
        self.outcome = None;
    }

    /// Store a computed diff together with its flattened projection. The
    /// shell is responsible for clearing its grid filter.
    pub fn set_diff(&mut self, diff: SchemaDiff) {
        self.diff_rows = Some(diff_rows(&diff));
        self.diff = Some(diff);
    }
}

// ===========================================================================
// Pure logic
// ===========================================================================

/// Nothing to migrate.
pub fn is_empty_diff(diff: &SchemaDiff) -> bool {
    diff.added_tables.is_empty()
        && diff.removed_tables.is_empty()
        && diff.modified_tables.is_empty()
}

/// Number of individual changes inside one modified table.
pub fn change_count(td: &TableDiff) -> usize {
    td.added_columns.len()
        + td.removed_columns.len()
        + td.modified_columns.len()
        + td.added_indexes.len()
        + td.removed_indexes.len()
        + td.added_fks.len()
        + td.removed_fks.len()
}

/// Why the Select step cannot proceed, or `None` if it can.
pub fn selection_problem(
    source: Option<&str>,
    target: Option<&str>,
    schema: &str,
) -> Option<&'static str> {
    match (source, target) {
        (None, _) | (_, None) => return Some("Select both a source and a target connection"),
        (Some(s), Some(t)) if s == t => {
            return Some("Source and target are the same connection; the diff would be empty")
        }
        _ => {}
    }
    if schema.trim().is_empty() {
        return Some("Enter a schema name, for example \"public\"");
    }
    None
}

/// Which of the chosen connections are not currently open.
///
/// Both have to be live: the diff reads metadata from each, and apply writes
/// to the target. Named rather than counted so the message can say which.
pub fn offline_selection(
    source: Option<&ConnectionProfile>,
    target: Option<&ConnectionProfile>,
    connected: &HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for p in [source, target].into_iter().flatten() {
        if !connected.contains(&p.id) && !out.contains(&p.name) {
            out.push(p.name.clone());
        }
    }
    out
}

/// Render statements for the editor, the way the web shell did: one blank
/// line between statements and an explicit terminator on each.
pub fn statements_to_text(statements: &[String]) -> String {
    if statements.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, s) in statements.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        // A comment-only marker gets no terminator: the `;` would land
        // inside the comment, where it is both meaningless and invisible to
        // the splitter that has to read this text back.
        if dbhive_core::service_migrations::is_noop_statement(s) {
            out.push_str(s.trim_end());
            continue;
        }
        out.push_str(s.trim_end().trim_end_matches(';'));
        out.push(';');
    }
    out
}

/// Split an edited buffer back into statements.
///
/// Splitting on `;` alone is wrong the moment a default value, a comment or a
/// dollar-quoted function body contains one, and the result of getting it
/// wrong is a half-executed DDL migration. Quoted strings (`'`, `"`, backtick,
/// with doubled-quote and backslash escapes), line comments, block comments
/// and Postgres dollar quoting are all skipped over.
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Line comment: runs to the newline, which is kept.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                cur.push(chars[i]);
                i += 1;
            }
            // A comment on lines of its own is a statement of its own. The
            // generator emits comment-only markers without a terminator, and
            // left attached to the statement after it, the pair would start
            // with `--` and be skipped as a no-op, silently dropping the real
            // statement from the migration.
            if cur.trim().lines().all(|l| l.trim_start().starts_with("--")) {
                push_statement(&mut out, &mut cur);
            }
            continue;
        }

        // Block comment.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            cur.push('/');
            cur.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    cur.push('*');
                    cur.push('/');
                    i += 2;
                    break;
                }
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Quoted string or quoted identifier.
        if c == '\'' || c == '"' || c == '`' {
            cur.push(c);
            i += 1;
            while i < chars.len() {
                // Backslash escape, as MySQL and Postgres E'' strings use.
                if chars[i] == '\\' && i + 1 < chars.len() {
                    cur.push(chars[i]);
                    cur.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == c {
                    // A doubled quote is an escaped quote, not the end.
                    if chars.get(i + 1) == Some(&c) {
                        cur.push(c);
                        cur.push(c);
                        i += 2;
                        continue;
                    }
                    cur.push(c);
                    i += 1;
                    break;
                }
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Dollar-quoted body: $$...$$ or $tag$...$tag$.
        if c == '$' {
            if let Some(tag) = dollar_tag(&chars, i) {
                let close: Vec<char> = tag.chars().collect();
                cur.push_str(&tag);
                i += close.len();
                while i < chars.len() {
                    if chars[i..].starts_with(close.as_slice()) {
                        cur.push_str(&tag);
                        i += close.len();
                        break;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
                continue;
            }
        }

        if c == ';' {
            push_statement(&mut out, &mut cur);
            i += 1;
            continue;
        }

        cur.push(c);
        i += 1;
    }

    push_statement(&mut out, &mut cur);
    out
}

/// The `$tag$` opener starting at `at`, if there is one.
fn dollar_tag(chars: &[char], at: usize) -> Option<String> {
    if chars.get(at) != Some(&'$') {
        return None;
    }
    let mut j = at + 1;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    if chars.get(j) == Some(&'$') {
        Some(chars[at..=j].iter().collect())
    } else {
        None
    }
}

fn push_statement(out: &mut Vec<String>, cur: &mut String) {
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    cur.clear();
}

/// How many statements would actually reach the server.
///
/// The generator emits comment-only lines for changes it cannot express as
/// DDL; those are skipped at execution time, so counting them here would
/// promise more than apply delivers.
pub fn executable_count(statements: &[String]) -> usize {
    statements
        .iter()
        .filter(|s| !dbhive_core::service_migrations::is_noop_statement(s))
        .count()
}

/// Flatten a diff into one row per change, for `widgets::results_grid`.
pub fn diff_rows(diff: &SchemaDiff) -> QueryResult {
    let columns = vec![
        "Change".to_string(),
        "Object".to_string(),
        "Schema".to_string(),
        "Table".to_string(),
        "Name".to_string(),
        "Detail".to_string(),
    ];
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();

    let row =
        |change: &str, object: &str, schema: &str, table: &str, name: &str, detail: String| {
            vec![
                serde_json::Value::String(change.to_string()),
                serde_json::Value::String(object.to_string()),
                serde_json::Value::String(schema.to_string()),
                serde_json::Value::String(table.to_string()),
                serde_json::Value::String(name.to_string()),
                serde_json::Value::String(detail),
            ]
        };

    for t in &diff.added_tables {
        rows.push(row(
            "Added",
            "table",
            &t.table.schema,
            &t.table.name,
            "",
            format!("{} column(s)", t.columns.len()),
        ));
    }
    for t in &diff.removed_tables {
        rows.push(row(
            "Removed",
            "table",
            &t.table.schema,
            &t.table.name,
            "",
            String::new(),
        ));
    }
    for td in &diff.modified_tables {
        for c in &td.added_columns {
            rows.push(row(
                "Added",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                c.data_type.clone(),
            ));
        }
        for c in &td.removed_columns {
            rows.push(row(
                "Removed",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                String::new(),
            ));
        }
        for c in &td.modified_columns {
            rows.push(row(
                "Modified",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                format!("{} -> {}", c.target.data_type, c.source.data_type),
            ));
        }
        for idx in &td.added_indexes {
            rows.push(row(
                "Added",
                "index",
                &td.schema,
                &td.name,
                &idx.name,
                idx.columns.join(", "),
            ));
        }
        for idx in &td.removed_indexes {
            rows.push(row(
                "Removed",
                "index",
                &td.schema,
                &td.name,
                &idx.name,
                String::new(),
            ));
        }
        for fk in &td.added_fks {
            rows.push(row(
                "Added",
                "foreign key",
                &td.schema,
                &td.name,
                &fk.name,
                format!(
                    "{} -> {}({})",
                    fk.columns.join(", "),
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                ),
            ));
        }
        for fk in &td.removed_fks {
            rows.push(row(
                "Removed",
                "foreign key",
                &td.schema,
                &td.name,
                &fk.name,
                String::new(),
            ));
        }
    }

    QueryResult::with_data(columns, rows)
}

/// One-line summary of an apply, for the toast and the inline banner.
pub fn apply_summary(result: &ApplyResult) -> String {
    match &result.error {
        Some(e) => format!(
            "Failed after {}/{} statement(s): {}",
            result.succeeded, result.executed, e
        ),
        None if result.executed == 0 => {
            "Nothing to apply: every statement was a comment".to_string()
        }
        None => format!("Applied {} statement(s)", result.succeeded),
    }
}

/// Collapse whitespace to single spaces and elide past `max` characters.
pub fn truncate(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let head: String = one_line.chars().take(max).collect();
        format!("{}…", head)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::migrations::ColumnChange;
    use dbhive_core::models::{
        ColumnInfo, DbDriver, ForeignKeyInfo, IndexInfo, TableInfo, TableSchema,
    };

    fn column(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo::new(name.to_string(), ty.to_string(), true)
    }

    fn table(schema: &str, name: &str, columns: Vec<ColumnInfo>) -> TableSchema {
        TableSchema::new(
            TableInfo::new(name.to_string(), schema.to_string(), "TABLE".to_string()),
            columns,
            Vec::new(),
        )
    }

    fn table_diff(schema: &str, name: &str) -> TableDiff {
        TableDiff {
            schema: schema.to_string(),
            name: name.to_string(),
            added_columns: Vec::new(),
            removed_columns: Vec::new(),
            modified_columns: Vec::new(),
            added_indexes: Vec::new(),
            removed_indexes: Vec::new(),
            added_fks: Vec::new(),
            removed_fks: Vec::new(),
        }
    }

    fn profile(id: &str, name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            id.to_string(),
            name.to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        )
    }

    // --- diff emptiness and counting -------------------------------------

    #[test]
    fn empty_diff_is_detected() {
        assert!(is_empty_diff(&SchemaDiff::default()));

        let mut d = SchemaDiff::default();
        d.added_tables.push(table("public", "users", vec![]));
        assert!(!is_empty_diff(&d));

        let mut d = SchemaDiff::default();
        d.modified_tables.push(table_diff("public", "users"));
        assert!(!is_empty_diff(&d));
    }

    #[test]
    fn change_count_sums_every_category() {
        let mut td = table_diff("public", "users");
        td.added_columns.push(column("a", "int"));
        td.removed_columns.push(column("b", "int"));
        td.modified_columns.push(ColumnChange {
            name: "c".into(),
            source: column("c", "bigint"),
            target: column("c", "int"),
        });
        td.added_indexes
            .push(IndexInfo::new("i1".into(), vec!["a".into()], false, false));
        td.removed_indexes
            .push(IndexInfo::new("i2".into(), vec!["b".into()], false, false));
        td.added_fks.push(ForeignKeyInfo {
            name: "f1".into(),
            table: "users".into(),
            schema: "public".into(),
            columns: vec!["a".into()],
            referenced_table: "orgs".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });
        assert_eq!(change_count(&td), 6);
        assert_eq!(change_count(&table_diff("public", "empty")), 0);
    }

    // --- selection validation --------------------------------------------

    #[test]
    fn selection_requires_two_distinct_connections_and_a_schema() {
        assert!(selection_problem(None, Some("b"), "public").is_some());
        assert!(selection_problem(Some("a"), None, "public").is_some());
        assert!(selection_problem(Some("a"), Some("a"), "public").is_some());
        assert!(selection_problem(Some("a"), Some("b"), "   ").is_some());
        assert!(selection_problem(Some("a"), Some("b"), "public").is_none());
    }

    #[test]
    fn offline_selection_names_each_disconnected_profile_once() {
        let src = profile("1", "Prod");
        let tgt = profile("2", "Staging");
        let mut connected = HashSet::new();

        assert_eq!(
            offline_selection(Some(&src), Some(&tgt), &connected),
            vec!["Prod".to_string(), "Staging".to_string()]
        );

        connected.insert("1".to_string());
        assert_eq!(
            offline_selection(Some(&src), Some(&tgt), &connected),
            vec!["Staging".to_string()]
        );

        connected.insert("2".to_string());
        assert!(offline_selection(Some(&src), Some(&tgt), &connected).is_empty());

        // Same profile picked twice must not be listed twice.
        assert_eq!(
            offline_selection(Some(&src), Some(&src), &HashSet::new()),
            vec!["Prod".to_string()]
        );
    }

    // --- statement text round trip ---------------------------------------

    #[test]
    fn statements_render_with_terminators_and_blank_lines() {
        let text = statements_to_text(&[
            "CREATE TABLE a (id int)".to_string(),
            "DROP TABLE b".to_string(),
        ]);
        assert_eq!(text, "CREATE TABLE a (id int);\n\nDROP TABLE b;");
        assert_eq!(statements_to_text(&[]), "");
    }

    #[test]
    fn already_terminated_statements_are_not_double_terminated() {
        let text = statements_to_text(&["SELECT 1;".to_string()]);
        assert_eq!(text, "SELECT 1;");
    }

    #[test]
    fn comment_markers_render_without_a_terminator() {
        // A `;` appended here would end up inside the comment, where the
        // splitter could never see it again.
        let text = statements_to_text(&[
            "-- unsupported change".to_string(),
            "DROP TABLE b".to_string(),
        ]);
        assert_eq!(text, "-- unsupported change\n\nDROP TABLE b;");
    }

    #[test]
    fn rendering_then_splitting_is_a_round_trip() {
        let statements = vec![
            "CREATE TABLE a (id int)".to_string(),
            "ALTER TABLE a ADD COLUMN note text DEFAULT 'a;b'".to_string(),
            "-- cannot express this change".to_string(),
        ];
        let text = statements_to_text(&statements);
        assert_eq!(split_statements(&text), statements);
    }

    #[test]
    fn a_comment_marker_before_a_statement_does_not_swallow_it() {
        // Regression: the marker and the DROP came back as one string that
        // started with `--`, so apply skipped the DROP as a comment.
        let statements = vec![
            "-- cannot express this change".to_string(),
            "DROP TABLE b".to_string(),
            "-- another note".to_string(),
            "CREATE TABLE a (id int)".to_string(),
        ];
        let split = split_statements(&statements_to_text(&statements));
        assert_eq!(split, statements);
        assert_eq!(executable_count(&split), 2);
    }

    #[test]
    fn a_comment_written_above_a_statement_is_split_off_it() {
        let split =
            split_statements("-- drop the old one\nDROP TABLE old;\nSELECT 1 -- keep; this\n");
        assert_eq!(
            split,
            vec![
                "-- drop the old one",
                "DROP TABLE old",
                "SELECT 1 -- keep; this"
            ]
        );
        assert_eq!(executable_count(&split), 2);
    }

    // --- statement splitting ---------------------------------------------

    #[test]
    fn split_ignores_semicolons_inside_string_literals() {
        let out = split_statements("INSERT INTO t VALUES ('a;b'); SELECT 1");
        assert_eq!(out, vec!["INSERT INTO t VALUES ('a;b')", "SELECT 1"]);
    }

    #[test]
    fn split_handles_doubled_and_backslash_quote_escapes() {
        let out = split_statements("SELECT 'it''s; fine'; SELECT 2");
        assert_eq!(out, vec!["SELECT 'it''s; fine'", "SELECT 2"]);

        let out = split_statements("SELECT 'a\\'; b'; SELECT 3");
        assert_eq!(out, vec!["SELECT 'a\\'; b'", "SELECT 3"]);
    }

    #[test]
    fn split_ignores_semicolons_inside_quoted_identifiers() {
        let out = split_statements("DROP TABLE \"weird;name\"; DROP TABLE `back;tick`");
        assert_eq!(
            out,
            vec!["DROP TABLE \"weird;name\"", "DROP TABLE `back;tick`"]
        );
    }

    #[test]
    fn split_ignores_semicolons_inside_comments() {
        let out = split_statements("SELECT 1 -- trailing; comment\n; SELECT 2");
        assert_eq!(out, vec!["SELECT 1 -- trailing; comment", "SELECT 2"]);

        let out = split_statements("SELECT /* a; b */ 1; SELECT 2");
        assert_eq!(out, vec!["SELECT /* a; b */ 1", "SELECT 2"]);
    }

    #[test]
    fn split_ignores_semicolons_inside_dollar_quoted_bodies() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN; RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("BEGIN; RETURN 1; END;"));
        assert_eq!(out[1], "SELECT 1");

        let tagged = "CREATE FUNCTION g() AS $body$ x; y $body$; SELECT 2";
        let out = split_statements(tagged);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("x; y"));
    }

    #[test]
    fn split_drops_blank_and_trailing_empty_statements() {
        assert!(split_statements("   \n\n  ").is_empty());
        assert_eq!(split_statements("SELECT 1;;;"), vec!["SELECT 1"]);
        assert_eq!(split_statements(";"), Vec::<String>::new());
    }

    #[test]
    fn lone_dollar_is_not_a_dollar_quote() {
        // `$1` is a bind placeholder, not the start of a quoted body; the
        // semicolon after it still terminates the statement.
        let out = split_statements("SELECT $1; SELECT 2");
        assert_eq!(out, vec!["SELECT $1", "SELECT 2"]);
    }

    // --- executable counting ---------------------------------------------

    #[test]
    fn comment_only_statements_do_not_count_as_executable() {
        let statements = vec![
            "-- unsupported".to_string(),
            "DROP TABLE t".to_string(),
            "  -- another".to_string(),
        ];
        assert_eq!(executable_count(&statements), 1);
        assert_eq!(executable_count(&[]), 0);
    }

    // --- diff projection --------------------------------------------------

    #[test]
    fn diff_rows_flatten_every_change() {
        let mut diff = SchemaDiff::default();
        diff.added_tables
            .push(table("public", "users", vec![column("id", "int")]));
        diff.removed_tables.push(table("public", "legacy", vec![]));

        let mut td = table_diff("public", "orders");
        td.added_columns.push(column("total", "numeric"));
        td.modified_columns.push(ColumnChange {
            name: "qty".into(),
            source: column("qty", "bigint"),
            target: column("qty", "int"),
        });
        td.removed_indexes.push(IndexInfo::new(
            "idx_old".into(),
            vec!["qty".into()],
            false,
            false,
        ));
        diff.modified_tables.push(td);

        let result = diff_rows(&diff);
        assert_eq!(result.columns.len(), 6);
        assert_eq!(result.rows.len(), 5);

        let first = &result.rows[0];
        assert_eq!(first[0], serde_json::json!("Added"));
        assert_eq!(first[1], serde_json::json!("table"));
        assert_eq!(first[3], serde_json::json!("users"));
        assert_eq!(first[5], serde_json::json!("1 column(s)"));

        // The modified column carries the direction of the change.
        let modified = result
            .rows
            .iter()
            .find(|r| r[4] == serde_json::json!("qty"))
            .expect("qty row");
        assert_eq!(modified[0], serde_json::json!("Modified"));
        assert_eq!(modified[5], serde_json::json!("int -> bigint"));
    }

    #[test]
    fn empty_diff_projects_to_no_rows() {
        let result = diff_rows(&SchemaDiff::default());
        assert!(result.rows.is_empty());
        assert_eq!(result.columns.len(), 6);
    }

    // --- outcome formatting ----------------------------------------------

    #[test]
    fn apply_summary_reports_success_partial_and_noop() {
        let ok = ApplyResult {
            executed: 4,
            succeeded: 4,
            failed_statement: None,
            error: None,
        };
        assert_eq!(apply_summary(&ok), "Applied 4 statement(s)");

        let partial = ApplyResult {
            executed: 4,
            succeeded: 2,
            failed_statement: Some("DROP TABLE t".into()),
            error: Some("permission denied".into()),
        };
        assert_eq!(
            apply_summary(&partial),
            "Failed after 2/4 statement(s): permission denied"
        );

        let nothing = ApplyResult::default();
        assert_eq!(
            apply_summary(&nothing),
            "Nothing to apply: every statement was a comment"
        );
    }

    #[test]
    fn truncate_collapses_whitespace_and_elides() {
        assert_eq!(truncate("a\n  b", 10), "a b");
        assert_eq!(truncate("abcdefghij", 5), "abcde…");
    }

    // --- wizard state -----------------------------------------------------

    #[test]
    fn reset_clears_derived_state_but_keeps_choices() {
        let mut m = Wizard::default();
        m.source_id = Some("1".into());
        m.target_id = Some("2".into());
        m.schema = "app".into();
        m.step = Step::Sql;
        m.set_diff(SchemaDiff::default());
        m.generated = vec!["DROP TABLE t".into()];
        m.sql = "DROP TABLE t;".into();
        m.acknowledged = true;
        m.confirming = true;
        m.outcome = Some(ApplyResult::default());

        m.reset();

        assert_eq!(m.step, Step::Select);
        assert!(m.diff.is_none());
        assert!(m.diff_rows.is_none());
        assert!(m.generated.is_empty());
        assert!(m.sql.is_empty());
        assert!(!m.acknowledged);
        assert!(!m.confirming);
        assert!(m.outcome.is_none());
        // Choices survive, so reopening after a failure is not a retype.
        assert_eq!(m.source_id.as_deref(), Some("1"));
        assert_eq!(m.target_id.as_deref(), Some("2"));
        assert_eq!(m.schema, "app");
    }

    #[test]
    fn defaults_match_the_web_shell() {
        let m = Wizard::default();
        assert_eq!(m.schema, "public");
        assert!(m.use_transaction);
        assert!(!m.acknowledged);
        assert_eq!(m.step, Step::Select);
    }

    #[test]
    fn set_diff_builds_the_projection_alongside_the_diff() {
        let mut m = Wizard::default();
        let mut diff = SchemaDiff::default();
        diff.removed_tables.push(table("public", "legacy", vec![]));
        m.set_diff(diff);
        assert!(m.diff.is_some());
        assert_eq!(m.diff_rows.as_ref().map(|r| r.rows.len()), Some(1));
    }

    #[test]
    fn steps_are_ordered_and_titled() {
        let idx: Vec<usize> = Step::ALL.iter().map(|s| s.index()).collect();
        assert_eq!(idx, vec![0, 1, 2]);
        assert_eq!(Step::Sql.title(), "SQL");
    }
}
