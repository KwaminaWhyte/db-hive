//! Stored procedures and functions: the UI-independent part.
//!
//! Lifted from the egui `procedures` screen so the Slint shell reuses it,
//! tests and all. Parity target: the React `StoredProceduresPanel.tsx`.
//!
//! The catalogue SQL, identifier quoting and CALL/SELECT/EXEC syntax live in
//! `dbhive_core::service_procedures`. What is here is what a screen needs on
//! top of that: one input box per argument (`parse_signature`), reading a box
//! into a value (`parse_arg`), search and grouping, and the plain-data screen
//! state with its open/select transitions.
//!
//! The state deliberately holds no task handles, editor state or grid state;
//! those belong to a shell. `ProceduresModel::select` reports whether the
//! selection changed so the shell can reset its own per-routine state.

use dbhive_core::drivers::QueryResult;
use dbhive_core::service_procedures::ProcedureInfo;

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Split `(a integer, b numeric(10,2))` into its arguments.
///
/// The split is on top-level commas only, so a parameterised type keeps its
/// own comma instead of being torn into two arguments. Nothing here parses
/// SQL properly; it only needs to produce one input box per argument.
pub fn parse_signature(signature: &str) -> Vec<String> {
    let inner = signature.trim();
    let inner = inner.strip_prefix('(').unwrap_or(inner);
    let inner = inner.strip_suffix(')').unwrap_or(inner);
    let inner = inner.trim();
    if inner.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    out.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Read one argument box.
///
/// A number, boolean, array or object typed as JSON arrives as that type;
/// anything else is a string, which is what makes `hello` work without
/// quoting it. An empty box is NULL, since there is no other way to say so.
pub fn parse_arg(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
}

/// Whether a routine matches the search box. Name and schema both count, so
/// typing a schema narrows to it.
pub fn matches(p: &ProcedureInfo, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    p.name.to_lowercase().contains(&q) || p.schema.to_lowercase().contains(&q)
}

/// The routines matching `query`, grouped by schema, schemas in name order
/// and routines in the order the catalogue returned them (already sorted).
pub fn grouped(procedures: &[ProcedureInfo], query: &str) -> Vec<(String, Vec<ProcedureInfo>)> {
    let mut out: Vec<(String, Vec<ProcedureInfo>)> = Vec::new();
    for p in procedures.iter().filter(|p| matches(p, query)) {
        match out.iter_mut().find(|(s, _)| *s == p.schema) {
            Some((_, items)) => items.push(p.clone()),
            None => out.push((p.schema.clone(), vec![p.clone()])),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A one-line summary for the list row.
pub fn summary(p: &ProcedureInfo) -> String {
    match &p.return_type {
        Some(r) => format!("{} ➡ {}", p.argument_signature, r),
        None => p.argument_signature.clone(),
    }
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// The plain-data state of the procedures screen.
#[derive(Default)]
pub struct ProceduresModel {
    pub open: bool,
    pub connection_id: Option<String>,
    /// The schema to list. Empty lists every schema the driver reports.
    pub schema: String,

    pub search: String,
    pub procedures: Vec<ProcedureInfo>,
    pub selected: Option<ProcedureInfo>,
    /// Raw text of each argument box, one per parsed argument.
    pub args: Vec<String>,

    pub definition: String,
    pub result: Option<QueryResult>,
    pub elapsed_ms: Option<u128>,
    pub last_error: Option<String>,

    /// Set when the list needs (re)loading. A shell starts the load after
    /// drawing, once every borrow of the screen has ended.
    pub needs_list: bool,
    /// Set when the connection's driver has no stored routines at all, so the
    /// empty list is explained rather than looking like a failure.
    pub unsupported: bool,
}

impl ProceduresModel {
    /// Point the screen at a connection and schema. Reopening the same target
    /// keeps the loaded list, the selection and the typed arguments.
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        self.open = true;
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            return;
        }
        self.connection_id = Some(c);
        self.schema = s;
        self.procedures.clear();
        self.selected = None;
        self.args.clear();
        self.definition.clear();
        self.result = None;
        self.elapsed_ms = None;
        self.last_error = None;
        self.unsupported = false;
        self.needs_list = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn refresh(&mut self) {
        self.needs_list = true;
        self.last_error = None;
    }

    /// Select a routine: reset the argument boxes to its signature and clear
    /// the previous one's definition and result, which belong to it and not
    /// to this one.
    ///
    /// Returns `true` when the selection changed, so a shell resets whatever
    /// per-routine state it keeps itself (the result grid's sort and filter).
    pub fn select(&mut self, p: ProcedureInfo) -> bool {
        if self.selected.as_ref() == Some(&p) {
            return false;
        }
        self.args = vec![String::new(); parse_signature(&p.argument_signature).len()];
        self.definition.clear();
        self.result = None;
        self.elapsed_ms = None;
        self.selected = Some(p);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::service_procedures::{self as svc, RoutineKind};

    fn proc(schema: &str, name: &str, kind: RoutineKind, sig: &str) -> ProcedureInfo {
        ProcedureInfo {
            schema: schema.into(),
            name: name.into(),
            kind,
            language: None,
            return_type: None,
            argument_signature: sig.into(),
        }
    }

    // ── signature parsing ────────────────────────────────────────────────

    #[test]
    fn a_routine_with_no_arguments_has_no_boxes() {
        assert!(parse_signature("()").is_empty());
        assert!(parse_signature("").is_empty());
        assert!(parse_signature("(   )").is_empty());
    }

    #[test]
    fn arguments_split_on_top_level_commas() {
        assert_eq!(
            parse_signature("(a integer, b text)"),
            vec!["a integer".to_string(), "b text".to_string()]
        );
    }

    #[test]
    fn a_parameterised_type_keeps_its_own_comma() {
        // numeric(10,2) is one argument, not two.
        assert_eq!(
            parse_signature("(amount numeric(10,2), note text)"),
            vec!["amount numeric(10,2)".to_string(), "note text".to_string()]
        );
    }

    #[test]
    fn an_unparenthesised_signature_still_parses() {
        assert_eq!(parse_signature("a integer"), vec!["a integer".to_string()]);
    }

    #[test]
    fn a_trailing_comma_does_not_add_an_empty_argument() {
        assert_eq!(
            parse_signature("(a integer, )"),
            vec!["a integer".to_string()]
        );
    }

    // ── argument parsing ─────────────────────────────────────────────────

    #[test]
    fn an_empty_box_is_null() {
        assert_eq!(parse_arg(""), serde_json::Value::Null);
        assert_eq!(parse_arg("   "), serde_json::Value::Null);
    }

    #[test]
    fn json_scalars_keep_their_type() {
        assert_eq!(parse_arg("42"), serde_json::json!(42));
        assert_eq!(parse_arg("1.5"), serde_json::json!(1.5));
        assert_eq!(parse_arg("true"), serde_json::json!(true));
        assert_eq!(parse_arg("\"hi\""), serde_json::json!("hi"));
        assert_eq!(parse_arg("[1,2]"), serde_json::json!([1, 2]));
        assert_eq!(parse_arg("{\"a\":1}"), serde_json::json!({"a": 1}));
    }

    #[test]
    fn unquoted_text_is_a_string_not_an_error() {
        assert_eq!(parse_arg("hello"), serde_json::json!("hello"));
        assert_eq!(parse_arg("2024-01-01"), serde_json::json!("2024-01-01"));
        // Including text that would end a SQL literal, which the core escapes
        // when it renders the argument.
        assert_eq!(parse_arg("o'brien"), serde_json::json!("o'brien"));
    }

    /// The whole point of `parse_arg`: what the boxes produce has to survive
    /// the core's rendering as a literal.
    #[test]
    fn parsed_arguments_render_as_safe_literals() {
        let escape = |s: &str| s.replace('\'', "''");
        assert_eq!(svc::render_arg(&parse_arg(""), &escape), "NULL");
        assert_eq!(svc::render_arg(&parse_arg("42"), &escape), "42");
        assert_eq!(
            svc::render_arg(&parse_arg("o'brien"), &escape),
            "'o''brien'"
        );
    }

    // ── filtering and grouping ───────────────────────────────────────────

    #[test]
    fn an_empty_search_matches_everything() {
        let p = proc("public", "add", RoutineKind::Function, "()");
        assert!(matches(&p, ""));
        assert!(matches(&p, "   "));
    }

    #[test]
    fn search_covers_name_and_schema_case_insensitively() {
        let p = proc("Billing", "chargeCard", RoutineKind::Procedure, "()");
        assert!(matches(&p, "charge"));
        assert!(matches(&p, "CHARGECARD"));
        assert!(matches(&p, "billing"));
        assert!(!matches(&p, "refund"));
    }

    #[test]
    fn routines_group_by_schema_in_name_order() {
        let list = vec![
            proc("public", "b", RoutineKind::Function, "()"),
            proc("audit", "a", RoutineKind::Procedure, "()"),
            proc("public", "a", RoutineKind::Function, "()"),
        ];
        let groups = grouped(&list, "");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "audit");
        assert_eq!(groups[1].0, "public");
        // Within a schema the catalogue's order is kept.
        assert_eq!(groups[1].1.len(), 2);
        assert_eq!(groups[1].1[0].name, "b");
    }

    #[test]
    fn grouping_applies_the_search() {
        let list = vec![
            proc("public", "add", RoutineKind::Function, "()"),
            proc("audit", "log", RoutineKind::Procedure, "()"),
        ];
        let groups = grouped(&list, "add");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1[0].name, "add");
    }

    #[test]
    fn the_summary_shows_the_return_type_when_there_is_one() {
        let mut p = proc("public", "add", RoutineKind::Function, "(a int)");
        assert_eq!(summary(&p), "(a int)");
        p.return_type = Some("integer".into());
        assert_eq!(summary(&p), "(a int) ➡ integer");
    }

    // ── selection ────────────────────────────────────────────────────────

    #[test]
    fn selecting_a_routine_sizes_the_argument_boxes() {
        let mut screen = ProceduresModel::default();
        screen.select(proc(
            "public",
            "add",
            RoutineKind::Function,
            "(a integer, b integer)",
        ));
        assert_eq!(screen.args.len(), 2);
    }

    #[test]
    fn selecting_another_routine_discards_the_previous_answers() {
        let mut screen = ProceduresModel::default();
        screen.select(proc("public", "add", RoutineKind::Function, "(a integer)"));
        screen.args[0] = "42".into();
        screen.definition = "CREATE FUNCTION add ...".into();

        screen.select(proc(
            "public",
            "sub",
            RoutineKind::Function,
            "(a int, b int)",
        ));
        assert_eq!(screen.args, vec![String::new(), String::new()]);
        assert!(
            screen.definition.is_empty(),
            "the previous routine's source stayed on screen"
        );
    }

    #[test]
    fn reselecting_the_same_routine_keeps_the_typed_arguments() {
        let mut screen = ProceduresModel::default();
        let p = proc("public", "add", RoutineKind::Function, "(a integer)");
        screen.select(p.clone());
        screen.args[0] = "42".into();
        screen.select(p);
        assert_eq!(screen.args[0], "42");
    }

    #[test]
    fn select_reports_whether_the_selection_changed() {
        let mut screen = ProceduresModel::default();
        let p = proc("public", "add", RoutineKind::Function, "(a integer)");
        assert!(screen.select(p.clone()), "a first selection is a change");
        assert!(!screen.select(p), "reselecting is not a change");
        assert!(screen.select(proc("public", "sub", RoutineKind::Function, "()")));
    }

    #[test]
    fn reopening_the_same_target_keeps_the_loaded_list() {
        let mut screen = ProceduresModel::default();
        screen.open_for("conn", "public");
        screen.procedures = vec![proc("public", "add", RoutineKind::Function, "()")];
        screen.open_for("conn", "public");
        assert_eq!(
            screen.procedures.len(),
            1,
            "a no-op reopen threw the list away"
        );

        // A different schema is a different list, so it does reset.
        screen.open_for("conn", "audit");
        assert!(screen.procedures.is_empty());
    }

    #[test]
    fn opening_a_new_target_requests_a_list_and_refresh_clears_the_error() {
        let mut screen = ProceduresModel::default();
        screen.open_for("conn", "public");
        assert!(screen.open && screen.needs_list);

        screen.needs_list = false;
        screen.last_error = Some("boom".into());
        screen.refresh();
        assert!(screen.needs_list);
        assert!(screen.last_error.is_none());

        screen.close();
        assert!(!screen.open);
    }
}
