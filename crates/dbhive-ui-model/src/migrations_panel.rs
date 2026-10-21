//! What the Slint migrations panel shows, as plain values.
//!
//! `migrations` holds the wizard's state, statement splitting and diff
//! projection that both shells share. This module adds the parts the Slint
//! panel needs on top: the connection choices, the schema a connection
//! implies, why a step cannot proceed, the diff as an expandable tree, how
//! dangerous the SQL is, what the apply confirmation says, and how the result
//! is reported.
//!
//! No toolkit types.

use std::collections::HashSet;

use dbhive_core::migrations::{ColumnChange, SchemaDiff, TableDiff};
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::service_migrations::{is_noop_statement, ApplyResult};

use crate::migrations::{
    apply_summary, change_count, executable_count, is_empty_diff, offline_selection,
    selection_problem, split_statements, truncate, Step, Wizard,
};

// ===========================================================================
// Choosing connections
// ===========================================================================

/// One entry of a connection picker.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileOption {
    pub id: String,
    /// "Production (PostgreSQL)".
    pub label: String,
    pub connected: bool,
}

/// Every saved connection, in the order given, marked with whether it is open.
///
/// `driver_label` is the shell's own name for a driver, so the picker says
/// the same thing as the connections screen.
pub fn profile_options(
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
    driver_label: fn(&DbDriver) -> &'static str,
) -> Vec<ProfileOption> {
    profiles
        .iter()
        .map(|p| ProfileOption {
            id: p.id.clone(),
            label: format!("{} ({})", p.name, driver_label(&p.driver)),
            connected: connected.contains(&p.id),
        })
        .collect()
}

/// Index of `id` in `options`, or -1, for a dropdown's selection.
pub fn option_index(options: &[ProfileOption], id: Option<&str>) -> i32 {
    id.and_then(|id| options.iter().position(|o| o.id == id))
        .map_or(-1, |i| i as i32)
}

/// The schema name to start with for a connection.
///
/// Postgres keeps tables in `public` unless told otherwise. MySQL has no
/// schemas apart from the database, so its database is the schema. SQLite's
/// only schema is `main`. Anything else starts blank and the user types one.
pub fn default_schema(driver: &DbDriver, database: Option<&str>) -> String {
    if driver.is_postgres_compatible() {
        return "public".to_string();
    }
    match driver {
        DbDriver::MySql => database.unwrap_or_default().to_string(),
        DbDriver::Sqlite | DbDriver::Turso => "main".to_string(),
        _ => String::new(),
    }
}

/// A note when the two connections speak different SQL.
pub fn mixed_driver_warning(
    source: Option<&ConnectionProfile>,
    target: Option<&ConnectionProfile>,
    driver_label: fn(&DbDriver) -> &'static str,
) -> Option<String> {
    let (s, t) = (source?, target?);
    if s.driver == t.driver {
        return None;
    }
    Some(format!(
        "{} and {} are different database types. The SQL uses the target's dialect and may not translate cleanly.",
        driver_label(&s.driver),
        driver_label(&t.driver)
    ))
}

fn profile_of<'a>(profiles: &'a [ConnectionProfile], id: Option<&str>) -> Option<&'a ConnectionProfile> {
    let id = id?;
    profiles.iter().find(|p| p.id == id)
}

/// Why the current step's forward button is disabled, or `None` if it can go.
pub fn blocked_reason(
    wizard: &Wizard,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
) -> Option<String> {
    match wizard.step {
        Step::Select => {
            if let Some(p) = selection_problem(
                wizard.source_id.as_deref(),
                wizard.target_id.as_deref(),
                &wizard.schema,
            ) {
                return Some(p.to_string());
            }
            let offline = offline_selection(
                profile_of(profiles, wizard.source_id.as_deref()),
                profile_of(profiles, wizard.target_id.as_deref()),
                connected,
            );
            (!offline.is_empty()).then(|| format!("Connect first: {}", offline.join(", ")))
        }
        Step::Diff => match &wizard.diff {
            None => Some("No diff yet".to_string()),
            Some(d) if is_empty_diff(d) => Some("Nothing to generate".to_string()),
            Some(_) => None,
        },
        Step::Sql => {
            if executable_count(&split_statements(&wizard.sql)) == 0 {
                Some("No executable statements".to_string())
            } else if !wizard.acknowledged {
                Some("Tick the box to enable Apply".to_string())
            } else {
                None
            }
        }
    }
}

// ===========================================================================
// The diff as a tree
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
}

/// One line of the diff tree.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffTreeRow {
    /// Identifies a table row for expanding it; empty on other rows.
    pub key: String,
    /// 0 for a section heading, 1 for a table, 2 for a change inside one.
    pub depth: u8,
    pub kind: DiffKind,
    /// "+", "-" or "~".
    pub marker: &'static str,
    pub label: String,
    pub detail: String,
    pub expandable: bool,
    pub expanded: bool,
    pub heading: bool,
}

fn marker(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Added => "+",
        DiffKind::Removed => "-",
        DiffKind::Modified => "~",
    }
}

fn table_key(schema: &str, name: &str) -> String {
    format!("{}.{}", schema, name)
}

fn qualified(schema: &str, name: &str) -> String {
    if schema.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", schema, name)
    }
}

/// Above this many changes the tree starts collapsed, so a large diff opens
/// as a scannable list of tables rather than a wall of lines.
pub const AUTO_EXPAND_LIMIT: usize = 40;

/// The tables to show expanded when a diff first appears: every changed
/// table when the whole diff is small, none when it is large.
pub fn default_expanded(diff: &SchemaDiff) -> HashSet<String> {
    let total: usize = diff.modified_tables.iter().map(change_count).sum::<usize>()
        + diff.added_tables.iter().map(|t| t.columns.len()).sum::<usize>();
    if total > AUTO_EXPAND_LIMIT {
        return HashSet::new();
    }
    let mut keys = HashSet::new();
    for t in &diff.added_tables {
        keys.insert(table_key(&t.table.schema, &t.table.name));
    }
    for t in &diff.modified_tables {
        keys.insert(table_key(&t.schema, &t.name));
    }
    keys
}

/// How a modified column differs, old to new: "type int -> bigint, NOT NULL".
pub fn column_change_detail(c: &ColumnChange) -> String {
    let (old, new) = (&c.target, &c.source);
    let mut parts = Vec::new();
    if !old.data_type.eq_ignore_ascii_case(&new.data_type) {
        parts.push(format!("type {} -> {}", old.data_type, new.data_type));
    }
    if old.nullable != new.nullable {
        parts.push(if new.nullable { "now nullable" } else { "now NOT NULL" }.to_string());
    }
    if old.default_value != new.default_value {
        parts.push(match &new.default_value {
            Some(d) => format!("default {}", d),
            None => "default removed".to_string(),
        });
    }
    if parts.is_empty() {
        "definition changed".to_string()
    } else {
        parts.join(", ")
    }
}

fn change(kind: DiffKind, label: String, detail: String) -> DiffTreeRow {
    DiffTreeRow {
        key: String::new(),
        depth: 2,
        kind,
        marker: marker(kind),
        label,
        detail,
        expandable: false,
        expanded: false,
        heading: false,
    }
}

fn modified_rows(td: &TableDiff, out: &mut Vec<DiffTreeRow>) {
    use DiffKind::*;
    for c in &td.added_columns {
        out.push(change(Added, format!("column {}", c.name), c.data_type.clone()));
    }
    for c in &td.removed_columns {
        out.push(change(Removed, format!("column {}", c.name), "data in it is lost".to_string()));
    }
    for c in &td.modified_columns {
        out.push(change(Modified, format!("column {}", c.name), column_change_detail(c)));
    }
    for i in &td.added_indexes {
        out.push(change(Added, format!("index {}", i.name), i.columns.join(", ")));
    }
    for i in &td.removed_indexes {
        out.push(change(Removed, format!("index {}", i.name), String::new()));
    }
    for f in &td.added_fks {
        out.push(change(
            Added,
            format!("foreign key {}", f.name),
            format!("{} -> {}({})", f.columns.join(", "), f.referenced_table, f.referenced_columns.join(", ")),
        ));
    }
    for f in &td.removed_fks {
        out.push(change(Removed, format!("foreign key {}", f.name), String::new()));
    }
}

/// Flatten a diff into the rows the panel draws, honouring `expanded`.
pub fn diff_tree(diff: &SchemaDiff, expanded: &HashSet<String>) -> Vec<DiffTreeRow> {
    let mut out = Vec::new();
    let heading = |kind, label: String, out: &mut Vec<DiffTreeRow>| {
        out.push(DiffTreeRow {
            key: String::new(),
            depth: 0,
            kind,
            marker: marker(kind),
            label,
            detail: String::new(),
            expandable: false,
            expanded: false,
            heading: true,
        })
    };

    if !diff.added_tables.is_empty() {
        heading(DiffKind::Added, format!("Added tables ({})", diff.added_tables.len()), &mut out);
        for t in &diff.added_tables {
            let key = table_key(&t.table.schema, &t.table.name);
            let open = expanded.contains(&key);
            out.push(DiffTreeRow {
                key,
                depth: 1,
                kind: DiffKind::Added,
                marker: "+",
                label: qualified(&t.table.schema, &t.table.name),
                detail: format!(
                    "{} {}",
                    t.columns.len(),
                    if t.columns.len() == 1 { "column" } else { "columns" }
                ),
                expandable: !t.columns.is_empty(),
                expanded: open,
                heading: false,
            });
            if open {
                for c in &t.columns {
                    out.push(change(DiffKind::Added, format!("column {}", c.name), c.data_type.clone()));
                }
            }
        }
    }

    if !diff.removed_tables.is_empty() {
        heading(DiffKind::Removed, format!("Removed tables ({})", diff.removed_tables.len()), &mut out);
        for t in &diff.removed_tables {
            out.push(DiffTreeRow {
                key: String::new(),
                depth: 1,
                kind: DiffKind::Removed,
                marker: "-",
                label: qualified(&t.table.schema, &t.table.name),
                detail: "dropped, with all its data".to_string(),
                expandable: false,
                expanded: false,
                heading: false,
            });
        }
    }

    if !diff.modified_tables.is_empty() {
        heading(DiffKind::Modified, format!("Modified tables ({})", diff.modified_tables.len()), &mut out);
        for td in &diff.modified_tables {
            let key = table_key(&td.schema, &td.name);
            let open = expanded.contains(&key);
            let n = change_count(td);
            out.push(DiffTreeRow {
                key,
                depth: 1,
                kind: DiffKind::Modified,
                marker: "~",
                label: qualified(&td.schema, &td.name),
                detail: format!("{} {}", n, if n == 1 { "change" } else { "changes" }),
                expandable: n > 0,
                expanded: open,
                heading: false,
            });
            if open {
                modified_rows(td, &mut out);
            }
        }
    }
    out
}

/// "2 added, 1 removed, 3 modified", or a statement that nothing differs.
pub fn diff_summary(diff: &SchemaDiff) -> String {
    if is_empty_diff(diff) {
        return "The schemas are identical".to_string();
    }
    let mut parts = Vec::new();
    for (n, word) in [
        (diff.added_tables.len(), "added"),
        (diff.removed_tables.len(), "removed"),
        (diff.modified_tables.len(), "modified"),
    ] {
        if n > 0 {
            parts.push(format!("{} {}", n, word));
        }
    }
    format!("{} {}", parts.join(", "), "table(s)")
}

// ===========================================================================
// The SQL
// ===========================================================================

/// Statement text with leading comment lines dropped and spacing flattened,
/// upper-cased, for classification.
fn significant(stmt: &str) -> String {
    stmt.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Whether a statement removes data or structure that cannot be restored by
/// running the migration again: a drop, a truncate, a delete.
pub fn is_destructive_statement(stmt: &str) -> bool {
    if is_noop_statement(stmt) {
        return false;
    }
    let s = significant(stmt);
    s.starts_with("DROP ")
        || s.starts_with("TRUNCATE ")
        || s.starts_with("DELETE ")
        || (s.starts_with("ALTER TABLE ") && (s.contains(" DROP COLUMN ") || s.contains(" DROP CONSTRAINT ")))
}

/// How many of `statements` are destructive.
pub fn destructive_count(statements: &[String]) -> usize {
    statements.iter().filter(|s| is_destructive_statement(s)).count()
}

/// "3 statements" or "1 statement".
pub fn statement_count_label(n: usize) -> String {
    format!("{} {}", n, if n == 1 { "statement" } else { "statements" })
}

/// The line under the SQL editor.
pub fn sql_summary(
    statements: &[String],
    target_name: &str,
    edited: bool,
) -> String {
    let n = executable_count(statements);
    let d = destructive_count(statements);
    let mut s = format!("{} will run against {}", statement_count_label(n), target_name);
    if d > 0 {
        s.push_str(&format!(" ({} destructive)", d));
    }
    if edited {
        s.push_str(". Edited from the generated SQL");
    }
    s
}

/// Title and message of the confirmation raised by Apply.
pub fn apply_confirmation(
    target_name: &str,
    statements: &[String],
    use_transaction: bool,
) -> (String, String) {
    let n = executable_count(statements);
    let d = destructive_count(statements);
    let mut lines = vec![format!("{} will run against {}.", statement_count_label(n), target_name)];
    if d > 0 {
        lines.push(format!(
            "{} of them drop or delete data, which cannot be recovered.",
            d
        ));
    }
    lines.push(
        if use_transaction {
            "They run in one transaction: the first failure rolls everything back."
        } else {
            "No transaction: a failure part way leaves the earlier statements applied."
        }
        .to_string(),
    );
    ("Apply this migration?".to_string(), lines.join("\n"))
}

// ===========================================================================
// Outcome
// ===========================================================================

/// How an apply is reported.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeView {
    pub ok: bool,
    pub headline: String,
    pub detail: String,
    /// The statement that failed, one line; empty on success.
    pub failed_statement: String,
}

pub fn outcome_view(result: &ApplyResult, use_transaction: bool) -> OutcomeView {
    let headline = apply_summary(result);
    if result.is_success() {
        let detail = if result.executed == 0 {
            String::new()
        } else if use_transaction {
            "Committed in one transaction.".to_string()
        } else {
            "Ran statement by statement, without a transaction.".to_string()
        };
        return OutcomeView {
            ok: true,
            headline,
            detail,
            failed_statement: String::new(),
        };
    }

    let failed = result.failed_statement.clone().unwrap_or_default();
    let detail = match (use_transaction, failed.as_str()) {
        (true, "BEGIN") => "The transaction could not start. Nothing was run.".to_string(),
        (true, "COMMIT") => {
            "The commit failed. Check the target before running this again.".to_string()
        }
        (true, _) => "Rolled back. The target should be unchanged.".to_string(),
        (false, _) if result.succeeded == 0 => {
            "The first statement failed. Nothing was changed.".to_string()
        }
        (false, _) => format!(
            "{} earlier {} stay applied. The target is partly migrated.",
            result.succeeded,
            if result.succeeded == 1 { "statement" } else { "statements" }
        ),
    };
    OutcomeView {
        ok: false,
        headline,
        detail,
        failed_statement: truncate(&failed, 400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::migrations::compute_diff;
    use dbhive_core::migrations::diff::TableWithFks;
    use dbhive_core::models::metadata::{ColumnInfo, ForeignKeyInfo, IndexInfo, TableInfo, TableSchema};

    fn label(d: &DbDriver) -> &'static str {
        match d {
            DbDriver::Postgres => "PostgreSQL",
            DbDriver::Sqlite => "SQLite",
            _ => "Other",
        }
    }

    fn profile(id: &str, name: &str, driver: DbDriver) -> ConnectionProfile {
        ConnectionProfile::new(id.into(), name.into(), driver, String::new(), 0, String::new())
    }

    // -- connections ------------------------------------------------------

    #[test]
    fn options_name_the_driver_and_mark_what_is_open() {
        let profiles = vec![
            profile("a", "Prod", DbDriver::Postgres),
            profile("b", "Local", DbDriver::Sqlite),
        ];
        let connected: HashSet<String> = ["b".to_string()].into();
        let opts = profile_options(&profiles, &connected, label);
        assert_eq!(opts[0].label, "Prod (PostgreSQL)");
        assert!(!opts[0].connected);
        assert!(opts[1].connected);
        assert_eq!(option_index(&opts, Some("b")), 1);
        assert_eq!(option_index(&opts, Some("zzz")), -1);
        assert_eq!(option_index(&opts, None), -1);
    }

    #[test]
    fn each_dialect_starts_with_the_schema_it_would_use() {
        assert_eq!(default_schema(&DbDriver::Postgres, Some("app")), "public");
        assert_eq!(default_schema(&DbDriver::Supabase, None), "public");
        assert_eq!(default_schema(&DbDriver::MySql, Some("shop")), "shop");
        assert_eq!(default_schema(&DbDriver::MySql, None), "");
        assert_eq!(default_schema(&DbDriver::Sqlite, Some("/tmp/x.db")), "main");
        assert_eq!(default_schema(&DbDriver::Redis, None), "");
    }

    #[test]
    fn mixed_dialects_are_flagged_and_same_dialects_are_not() {
        let pg = profile("a", "A", DbDriver::Postgres);
        let lite = profile("b", "B", DbDriver::Sqlite);
        assert!(mixed_driver_warning(Some(&pg), Some(&lite), label)
            .unwrap()
            .contains("PostgreSQL and SQLite"));
        assert!(mixed_driver_warning(Some(&pg), Some(&pg), label).is_none());
        assert!(mixed_driver_warning(None, Some(&pg), label).is_none());
    }

    // -- blocked_reason ---------------------------------------------------

    #[test]
    fn each_step_says_why_it_cannot_continue() {
        let profiles = vec![
            profile("a", "Source", DbDriver::Sqlite),
            profile("b", "Target", DbDriver::Sqlite),
        ];
        let mut w = Wizard::default();
        let none = HashSet::new();
        assert!(blocked_reason(&w, &profiles, &none).unwrap().contains("both"));

        w.source_id = Some("a".into());
        w.target_id = Some("b".into());
        assert!(blocked_reason(&w, &profiles, &none).unwrap().starts_with("Connect first: Source, Target"));

        let open: HashSet<String> = ["a".to_string(), "b".to_string()].into();
        assert_eq!(blocked_reason(&w, &profiles, &open), None);

        w.step = Step::Diff;
        assert_eq!(blocked_reason(&w, &profiles, &open).unwrap(), "No diff yet");
        w.set_diff(SchemaDiff::default());
        assert_eq!(blocked_reason(&w, &profiles, &open).unwrap(), "Nothing to generate");

        w.step = Step::Sql;
        w.sql = "-- only a comment".into();
        assert_eq!(blocked_reason(&w, &profiles, &open).unwrap(), "No executable statements");
        w.sql = "DROP TABLE t;".into();
        assert!(blocked_reason(&w, &profiles, &open).unwrap().contains("Tick the box"));
        w.acknowledged = true;
        assert_eq!(blocked_reason(&w, &profiles, &open), None);
    }

    // -- the diff tree ----------------------------------------------------

    fn col(name: &str, ty: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo::new(name.into(), ty.into(), nullable)
    }

    fn table(name: &str, cols: Vec<ColumnInfo>, idx: Vec<IndexInfo>) -> TableWithFks {
        TableWithFks {
            schema: TableSchema::new(TableInfo::new(name.into(), "main".into(), "TABLE".into()), cols, idx),
            foreign_keys: Vec::<ForeignKeyInfo>::new(),
        }
    }

    /// Source has users(id, email bigint), orders(id); target has users(id,
    /// email int NOT NULL, legacy) and an old table.
    fn sample_diff() -> SchemaDiff {
        let src = vec![
            table("users", vec![col("id", "INTEGER", false), col("email", "BIGINT", true)], vec![]),
            table("orders", vec![col("id", "INTEGER", false)], vec![]),
        ];
        let tgt = vec![
            table(
                "users",
                vec![col("id", "INTEGER", false), col("email", "INT", false), col("legacy", "TEXT", true)],
                vec![],
            ),
            table("old_stuff", vec![col("id", "INTEGER", false)], vec![]),
        ];
        compute_diff(&src, &tgt)
    }

    #[test]
    fn the_tree_groups_added_removed_and_modified_tables() {
        let diff = sample_diff();
        let rows = diff_tree(&diff, &HashSet::new());
        let headings: Vec<&str> = rows.iter().filter(|r| r.heading).map(|r| r.label.as_str()).collect();
        assert_eq!(headings, vec!["Added tables (1)", "Removed tables (1)", "Modified tables (1)"]);

        let orders = rows.iter().find(|r| r.label == "main.orders").unwrap();
        assert_eq!((orders.marker, orders.depth, orders.detail.as_str()), ("+", 1, "1 column"));
        let old = rows.iter().find(|r| r.label == "main.old_stuff").unwrap();
        assert_eq!(old.marker, "-");
        assert!(old.detail.contains("dropped"));
        assert!(!old.expandable);
        let users = rows.iter().find(|r| r.label == "main.users").unwrap();
        assert_eq!(users.detail, "2 changes");
        assert!(users.expandable && !users.expanded);
        // Collapsed: no change rows yet.
        assert!(rows.iter().all(|r| r.depth < 2));
    }

    #[test]
    fn expanding_a_table_lists_its_column_changes_old_to_new() {
        let diff = sample_diff();
        let open: HashSet<String> = ["main.users".to_string()].into();
        let rows = diff_tree(&diff, &open);
        let changes: Vec<(&str, &str, &str)> = rows
            .iter()
            .filter(|r| r.depth == 2)
            .map(|r| (r.marker, r.label.as_str(), r.detail.as_str()))
            .collect();
        assert!(changes.contains(&("~", "column email", "type INT -> BIGINT, now nullable")), "{changes:?}");
        assert!(changes.contains(&("-", "column legacy", "data in it is lost")), "{changes:?}");
        assert!(rows.iter().find(|r| r.label == "main.users").unwrap().expanded);
    }

    #[test]
    fn a_small_diff_opens_expanded_and_a_large_one_does_not() {
        let small = default_expanded(&sample_diff());
        assert!(small.contains("main.users") && small.contains("main.orders"));

        let many: Vec<TableWithFks> = (0..60)
            .map(|i| table(&format!("t{i}"), vec![col("id", "INTEGER", false)], vec![]))
            .collect();
        let big = compute_diff(&many, &[]);
        assert!(default_expanded(&big).is_empty());
    }

    #[test]
    fn column_changes_say_only_what_changed() {
        let change = |old: ColumnInfo, new: ColumnInfo| ColumnChange {
            name: new.name.clone(),
            source: new,
            target: old,
        };
        let mut a = col("c", "INT", true);
        let mut b = col("c", "INT", false);
        assert_eq!(column_change_detail(&change(a.clone(), b.clone())), "now NOT NULL");
        b.default_value = Some("0".into());
        assert_eq!(column_change_detail(&change(a.clone(), b.clone())), "now NOT NULL, default 0");
        a.default_value = Some("1".into());
        b.default_value = None;
        assert!(column_change_detail(&change(a, b)).ends_with("default removed"));
        assert_eq!(
            column_change_detail(&change(col("c", "int", true), col("c", "INT", true))),
            "definition changed"
        );
    }

    #[test]
    fn the_summary_counts_each_kind_or_says_identical() {
        assert_eq!(diff_summary(&SchemaDiff::default()), "The schemas are identical");
        assert_eq!(diff_summary(&sample_diff()), "1 added, 1 removed, 1 modified table(s)");
    }

    // -- the SQL ----------------------------------------------------------

    #[test]
    fn drops_truncates_and_deletes_are_destructive_but_creates_are_not() {
        for s in [
            "DROP TABLE t",
            "  drop   table t;",
            "TRUNCATE TABLE t",
            "DELETE FROM t",
            "ALTER TABLE t DROP COLUMN c",
            "alter table \"t\" drop constraint fk",
        ] {
            assert!(is_destructive_statement(s), "{s}");
        }
        for s in [
            "CREATE TABLE t (id int)",
            "ALTER TABLE t ADD COLUMN c int",
            "CREATE INDEX i ON t (c)",
            "-- DROP TABLE t (not executed)",
            "",
        ] {
            assert!(!is_destructive_statement(s), "{s:?}");
        }
        let all = vec![
            "CREATE TABLE a (id int)".to_string(),
            "DROP TABLE b".to_string(),
            "ALTER TABLE a DROP COLUMN x".to_string(),
        ];
        assert_eq!(destructive_count(&all), 2);
    }

    #[test]
    fn the_confirmation_counts_statements_and_calls_out_destruction() {
        let stmts = vec!["CREATE TABLE a (id int)".to_string(), "DROP TABLE b".to_string()];
        let (title, msg) = apply_confirmation("Staging", &stmts, true);
        assert_eq!(title, "Apply this migration?");
        assert!(msg.contains("2 statements will run against Staging."));
        assert!(msg.contains("1 of them drop or delete data"));
        assert!(msg.contains("rolls everything back"));

        let (_, safe) = apply_confirmation("Staging", &stmts[..1], false);
        assert!(!safe.contains("drop or delete"));
        assert!(safe.contains("1 statement will run"));
        assert!(safe.contains("No transaction"));
    }

    #[test]
    fn comment_only_statements_are_not_counted_as_work() {
        let stmts = vec!["-- cannot alter".to_string(), "CREATE TABLE a (id int)".to_string()];
        let s = sql_summary(&stmts, "T", true);
        assert!(s.starts_with("1 statement will run against T"), "{s}");
        assert!(s.ends_with("Edited from the generated SQL"));
        assert!(!s.contains("destructive"));
        assert!(sql_summary(&["DROP TABLE x".into()], "T", false).contains("(1 destructive)"));
    }

    // -- outcome ----------------------------------------------------------

    fn result(executed: u32, succeeded: u32, failed: Option<&str>) -> ApplyResult {
        ApplyResult {
            executed,
            succeeded,
            failed_statement: failed.map(str::to_string),
            error: failed.map(|_| "boom".to_string()),
        }
    }

    #[test]
    fn a_clean_run_says_how_it_ran() {
        let v = outcome_view(&result(3, 3, None), true);
        assert!(v.ok);
        assert_eq!(v.headline, "Applied 3 statement(s)");
        assert_eq!(v.detail, "Committed in one transaction.");
        assert!(outcome_view(&result(3, 3, None), false).detail.contains("without a transaction"));
        let nothing = outcome_view(&result(0, 0, None), true);
        assert!(nothing.ok && nothing.detail.is_empty());
    }

    #[test]
    fn a_failure_in_a_transaction_reports_rollback() {
        let v = outcome_view(&result(2, 1, Some("DROP TABLE x")), true);
        assert!(!v.ok);
        assert_eq!(v.headline, "Failed after 1/2 statement(s): boom");
        assert_eq!(v.detail, "Rolled back. The target should be unchanged.");
        assert_eq!(v.failed_statement, "DROP TABLE x");
    }

    #[test]
    fn a_failed_commit_or_begin_is_not_called_a_rollback() {
        let commit = outcome_view(&result(3, 3, Some("COMMIT")), true);
        assert!(commit.detail.contains("commit failed"));
        assert!(!commit.detail.contains("Rolled back"));
        let begin = outcome_view(&result(0, 0, Some("BEGIN")), true);
        assert!(begin.detail.contains("Nothing was run"));
    }

    #[test]
    fn a_failure_without_a_transaction_says_how_much_stayed_applied() {
        let partial = outcome_view(&result(5, 3, Some("ALTER TABLE t ADD c int")), false);
        assert_eq!(partial.detail, "3 earlier statements stay applied. The target is partly migrated.");
        let one = outcome_view(&result(2, 1, Some("x")), false);
        assert!(one.detail.starts_with("1 earlier statement stay"));
        let first = outcome_view(&result(1, 0, Some("x")), false);
        assert_eq!(first.detail, "The first statement failed. Nothing was changed.");
    }

    #[test]
    fn a_long_failed_statement_is_shortened_to_one_line() {
        let long = format!("CREATE TABLE t (\n  {}\n)", "c int, ".repeat(200));
        let v = outcome_view(&result(1, 0, Some(&long)), false);
        assert!(!v.failed_statement.contains('\n'));
        assert!(v.failed_statement.chars().count() <= 401);
    }
}
