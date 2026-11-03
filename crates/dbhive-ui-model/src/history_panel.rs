//! The History / Snippets / Templates panel: view projections and form logic.
//!
//! `history.rs` holds the data model and the filters lifted from the egui
//! screen. This module is what a panel needs on top of them and that is not
//! drawing: turning stored records into display rows, building a record from a
//! form, the parameter-run form with live validation, and the small decisions
//! (can this history entry be re-run here, how does a snippet land in an
//! editor) that were tangled into the egui screen. Plain Rust types only, so a
//! shell maps them to its own widgets and tests need no window.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use dbhive_core::models::{QueryHistory, QuerySnippet};

use crate::history::{
    filter_history, filter_snippets, filter_templates, first_line, format_execution_time,
    format_relative, format_row_count, parse_tags, preview_sql, render_template, sync_parameters,
    validate_parameter_value, validate_template_values, ParamType, QueryTemplate,
    TemplateParameter,
};

/// The panel's three tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelTab {
    #[default]
    History,
    Snippets,
    Templates,
}

impl PanelTab {
    pub fn from_index(i: i32) -> PanelTab {
        match i {
            1 => PanelTab::Snippets,
            2 => PanelTab::Templates,
            _ => PanelTab::History,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            PanelTab::History => 0,
            PanelTab::Snippets => 1,
            PanelTab::Templates => 2,
        }
    }
}

/// Longest single line of SQL shown as a row title.
const TITLE_CHARS: usize = 110;

// ===========================================================================
// Display rows
// ===========================================================================

/// One row of any of the three lists, already formatted for display.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowView {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// An error message, shown under the row in the danger colour.
    pub note: String,
    /// `Some(true)` success, `Some(false)` failure, `None` no status dot.
    pub status: Option<bool>,
    pub tags: Vec<String>,
}

fn plural_rows(n: u64) -> String {
    let word = if n == 1 { "row" } else { "rows" };
    format!("{} {}", format_row_count(n), word)
}

/// Rows for the history list, newest first (the order `filter_history`
/// returns). `now` is a parameter so relative times are testable.
pub fn history_rows(entries: &[QueryHistory], now: DateTime<Utc>) -> Vec<RowView> {
    entries
        .iter()
        .map(|h| {
            let mut parts = vec![format_relative(&h.executed_at, now)];
            let place = match (h.connection_name.is_empty(), h.database.is_empty()) {
                (false, false) => format!("{} / {}", h.connection_name, h.database),
                (false, true) => h.connection_name.clone(),
                (true, false) => h.database.clone(),
                (true, true) => String::new(),
            };
            if !place.is_empty() {
                parts.push(place);
            }
            parts.push(format_execution_time(h.execution_time_ms));
            if let Some(n) = h.row_count {
                parts.push(plural_rows(n));
            }
            RowView {
                id: h.id.clone(),
                title: first_line(&h.query, TITLE_CHARS),
                subtitle: parts.join(" · "),
                note: if h.success {
                    String::new()
                } else {
                    h.error_message
                        .clone()
                        .unwrap_or_else(|| "Query failed".to_string())
                },
                status: Some(h.success),
                tags: Vec::new(),
            }
        })
        .collect()
}

/// Snippets matching the search and tag, sorted by name.
pub fn snippet_rows(snippets: &[QuerySnippet], search: &str, tag: Option<&str>) -> Vec<RowView> {
    let mut matched = filter_snippets(snippets, search, tag);
    matched.sort_by_key(|s| s.name.to_lowercase());
    matched
        .into_iter()
        .map(|s| RowView {
            id: s.id.clone(),
            title: s.name.clone(),
            subtitle: match s.description.as_deref().filter(|d| !d.trim().is_empty()) {
                Some(d) => d.trim().to_string(),
                None => first_line(&s.query, TITLE_CHARS),
            },
            note: String::new(),
            status: None,
            tags: s.tags.clone().unwrap_or_default(),
        })
        .collect()
}

/// "2 parameters", "no parameters".
fn parameter_count(n: usize) -> String {
    match n {
        0 => "No parameters".to_string(),
        1 => "1 parameter".to_string(),
        n => format!("{} parameters", n),
    }
}

/// Templates matching the search and category, sorted by name.
pub fn template_rows(
    templates: &[QueryTemplate],
    search: &str,
    category: Option<&str>,
) -> Vec<RowView> {
    let mut matched = filter_templates(templates, search, category);
    matched.sort_by_key(|t| t.name.to_lowercase());
    matched
        .into_iter()
        .map(|t| {
            let mut parts = vec![parameter_count(t.parameters.len())];
            if let Some(c) = t.category.as_deref().filter(|c| !c.is_empty()) {
                parts.push(c.to_string());
            }
            if let Some(d) = t.description.as_deref().filter(|d| !d.trim().is_empty()) {
                parts.push(d.trim().to_string());
            }
            RowView {
                id: t.id.clone(),
                title: t.name.clone(),
                subtitle: parts.join(" · "),
                note: String::new(),
                status: None,
                tags: t.tags.clone().unwrap_or_default(),
            }
        })
        .collect()
}

/// History entries for the panel: scoped to a connection and database, or all.
pub fn scoped_history(
    all: &[QueryHistory],
    scope: Option<(&str, Option<&str>)>,
    limit: usize,
) -> Vec<QueryHistory> {
    match scope {
        Some((conn, db)) => filter_history(all, Some(conn), db, limit),
        None => filter_history(all, None, None, limit),
    }
}

// ===========================================================================
// Acting on a history entry or a snippet
// ===========================================================================

/// What "re-run" can safely do with a history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RerunPlan {
    /// Open it in a new tab on the current connection and run it.
    RunHere,
    /// Open it in a new tab but do not run it, and say why.
    OpenOnly(String),
}

/// A statement is only run automatically when it succeeded before and ran on
/// the connection that is open now. Anything else (it failed, or it belongs to
/// another database) is opened for the user to look at first: running someone
/// else's `DELETE` against the wrong connection is the one mistake this panel
/// could cause.
pub fn rerun_plan(entry: &QueryHistory, current_connection_id: &str) -> RerunPlan {
    if entry.connection_id != current_connection_id {
        let name = if entry.connection_name.is_empty() {
            "another connection"
        } else {
            entry.connection_name.as_str()
        };
        return RerunPlan::OpenOnly(format!(
            "That query ran on {}. Opened it without running.",
            name
        ));
    }
    if !entry.success {
        return RerunPlan::OpenOnly(
            "That query failed last time. Opened it without running.".into(),
        );
    }
    RerunPlan::RunHere
}

/// Put `addition` into an editor that already holds `existing`: replace it
/// when the editor is blank, otherwise append after a blank line.
pub fn append_sql(existing: &str, addition: &str) -> String {
    if existing.trim().is_empty() {
        return addition.to_string();
    }
    format!("{}\n\n{}", existing.trim_end(), addition)
}

// ===========================================================================
// Snippet and template forms
// ===========================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnippetDraft {
    pub name: String,
    pub description: String,
    pub query: String,
    /// Comma separated, as typed.
    pub tags: String,
}

impl SnippetDraft {
    pub fn from_snippet(s: &QuerySnippet) -> Self {
        Self {
            name: s.name.clone(),
            description: s.description.clone().unwrap_or_default(),
            query: s.query.clone(),
            tags: s.tags.clone().unwrap_or_default().join(", "),
        }
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Validate the form and build the record. With `existing`, the snippet keeps
/// its id and creation time and is updated in place.
pub fn build_snippet(
    draft: &SnippetDraft,
    existing: Option<&QuerySnippet>,
) -> Result<QuerySnippet, String> {
    if draft.name.trim().is_empty() {
        return Err("Give the snippet a name.".to_string());
    }
    if draft.query.trim().is_empty() {
        return Err("The snippet has no SQL.".to_string());
    }
    let tags = parse_tags(&draft.tags);
    let tags = (!tags.is_empty()).then_some(tags);
    match existing {
        Some(prev) => {
            let mut s = prev.clone();
            s.name = draft.name.trim().to_string();
            s.query = draft.query.trim().to_string();
            // `QuerySnippet::update` leaves a field alone when given None, so
            // clearing a description or tags has to be set directly.
            s.description = non_empty(&draft.description);
            s.tags = tags;
            s.updated_at = Utc::now().to_rfc3339();
            Ok(s)
        }
        None => Ok(QuerySnippet::new(
            draft.name.trim().to_string(),
            draft.query.trim().to_string(),
            non_empty(&draft.description),
            tags,
        )),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateDraft {
    pub name: String,
    pub description: String,
    pub category: String,
    /// Comma separated, as typed.
    pub tags: String,
    pub sql: String,
}

impl TemplateDraft {
    pub fn from_template(t: &QueryTemplate) -> Self {
        Self {
            name: t.name.clone(),
            description: t.description.clone().unwrap_or_default(),
            category: t.category.clone().unwrap_or_default(),
            tags: t.tags.clone().unwrap_or_default().join(", "),
            sql: t.sql.clone(),
        }
    }
}

/// Validate the form and build the record. `params` is the parameter list the
/// form was editing (derived from the SQL by [`sync_parameters`]); with
/// `existing`, the template keeps its id and creation time.
pub fn build_template(
    draft: &TemplateDraft,
    params: &[TemplateParameter],
    existing: Option<&QueryTemplate>,
) -> Result<QueryTemplate, String> {
    if draft.name.trim().is_empty() {
        return Err("Give the template a name.".to_string());
    }
    if draft.sql.trim().is_empty() {
        return Err("The template has no SQL.".to_string());
    }
    let tags = parse_tags(&draft.tags);
    let mut t = QueryTemplate::new(
        draft.name.trim().to_string(),
        non_empty(&draft.description),
        draft.sql.trim().to_string(),
        // Re-derive so a parameter list that lags the SQL cannot be saved.
        sync_parameters(draft.sql.trim(), params),
        non_empty(&draft.category),
        (!tags.is_empty()).then_some(tags),
    );
    if let Some(prev) = existing {
        t.id = prev.id.clone();
        t.created_at = prev.created_at.clone();
    }
    Ok(t)
}

/// Set a parameter's default; an empty string clears it.
pub fn set_param_default(params: &mut [TemplateParameter], index: usize, value: &str) {
    if let Some(p) = params.get_mut(index) {
        p.default_value = (!value.is_empty()).then(|| value.to_string());
    }
}

pub fn toggle_param_required(params: &mut [TemplateParameter], index: usize) {
    if let Some(p) = params.get_mut(index) {
        p.required = !p.required;
    }
}

/// A copy of `source` with a name no other template has ("X copy", then
/// "X copy 2"), a fresh id and fresh timestamps.
pub fn duplicate_template(source: &QueryTemplate, all: &[QueryTemplate]) -> QueryTemplate {
    let taken = |n: &str| all.iter().any(|t| t.name == n);
    let mut name = format!("{} copy", source.name);
    let mut n = 2;
    while taken(&name) {
        name = format!("{} copy {}", source.name, n);
        n += 1;
    }
    let mut t = QueryTemplate::new(
        name,
        source.description.clone(),
        source.sql.clone(),
        source.parameters.clone(),
        source.category.clone(),
        source.tags.clone(),
    );
    t.updated_at = t.created_at.clone();
    t
}

// ===========================================================================
// Running a template
// ===========================================================================

/// One parameter in the run form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunField {
    pub name: String,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub param_type: ParamType,
    pub initial: String,
}

/// The parameter form for one template: values, errors and the live preview.
#[derive(Debug, Clone)]
pub struct TemplateRun {
    template: QueryTemplate,
    values: HashMap<String, String>,
    errors: HashMap<String, String>,
}

impl TemplateRun {
    /// Start with every field at its default and no errors shown: a fresh
    /// form should not shout about empty required fields before anyone typed.
    pub fn new(template: &QueryTemplate) -> Self {
        let values = template
            .parameters
            .iter()
            .map(|p| (p.name.clone(), p.default_value.clone().unwrap_or_default()))
            .collect();
        Self {
            template: template.clone(),
            values,
            errors: HashMap::new(),
        }
    }

    pub fn template(&self) -> &QueryTemplate {
        &self.template
    }

    pub fn fields(&self) -> Vec<RunField> {
        self.template
            .parameters
            .iter()
            .map(|p| RunField {
                name: p.name.clone(),
                label: if p.label.is_empty() {
                    p.name.clone()
                } else {
                    p.label.clone()
                },
                description: p.description.clone().unwrap_or_default(),
                required: p.required,
                param_type: p.param_type,
                initial: self.values.get(&p.name).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// The user changed one field. Type errors show as they type; "required"
    /// only appears on submit.
    pub fn set_value(&mut self, index: usize, value: &str) {
        let Some(p) = self.template.parameters.get(index) else {
            return;
        };
        let (name, ty) = (p.name.clone(), p.param_type);
        self.values.insert(name.clone(), value.to_string());
        match validate_parameter_value(value, ty) {
            Ok(()) => {
                self.errors.remove(&name);
            }
            Err(e) => {
                self.errors.insert(name, e);
            }
        }
    }

    /// One message per parameter, in parameter order, empty when fine.
    pub fn errors(&self) -> Vec<String> {
        self.template
            .parameters
            .iter()
            .map(|p| self.errors.get(&p.name).cloned().unwrap_or_default())
            .collect()
    }

    pub fn preview(&self) -> String {
        preview_sql(&self.template, &self.values)
    }

    /// The statement to run, or `Err` with the errors now showing.
    pub fn submit(&mut self) -> Result<String, ()> {
        let errors = validate_template_values(&self.template, &self.values);
        if errors.is_empty() {
            self.errors.clear();
            Ok(render_template(&self.template, &self.values))
        } else {
            self.errors = errors;
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 3, 1, 12, 0, 0).unwrap()
    }

    fn entry(conn: &str, ok: bool) -> QueryHistory {
        let h = QueryHistory::new(
            conn.to_string(),
            "Prod".to_string(),
            "app".to_string(),
            "SELECT id,\n  name FROM users".to_string(),
            "2025-03-01T11:55:00Z".to_string(),
        );
        if ok {
            h.with_success(12, Some(1204))
        } else {
            h.with_error("relation \"nope\" does not exist".to_string(), Some(3))
        }
    }

    fn template(sql: &str) -> QueryTemplate {
        QueryTemplate::new(
            "T".into(),
            None,
            sql.into(),
            sync_parameters(sql, &[]),
            None,
            None,
        )
    }

    #[test]
    fn history_row_has_first_line_time_place_duration_and_rows() {
        let rows = history_rows(&[entry("c1", true)], now());
        assert_eq!(rows[0].title, "SELECT id,");
        assert_eq!(rows[0].subtitle, "5m ago · Prod / app · 12ms · 1,204 rows");
        assert_eq!(rows[0].status, Some(true));
        assert_eq!(rows[0].note, "");
    }

    #[test]
    fn failed_history_row_carries_the_error_and_no_row_count() {
        let rows = history_rows(&[entry("c1", false)], now());
        assert_eq!(rows[0].status, Some(false));
        assert_eq!(rows[0].note, "relation \"nope\" does not exist");
        assert_eq!(rows[0].subtitle, "5m ago · Prod / app · 3ms");
    }

    #[test]
    fn one_row_is_singular_and_a_zero_duration_is_not_measured() {
        let mut e = entry("c1", true);
        e.row_count = Some(1);
        e.execution_time_ms = Some(0);
        assert_eq!(
            history_rows(&[e], now())[0].subtitle,
            "5m ago · Prod / app · N/A · 1 row"
        );
    }

    #[test]
    fn scoped_history_filters_connection_and_database() {
        let mut other_db = entry("c1", true);
        other_db.database = "other".into();
        let all = vec![entry("c1", true), other_db, entry("c2", true)];
        assert_eq!(scoped_history(&all, Some(("c1", Some("app"))), 50).len(), 1);
        assert_eq!(scoped_history(&all, Some(("c1", None)), 50).len(), 2);
        assert_eq!(scoped_history(&all, None, 50).len(), 3);
    }

    #[test]
    fn rerun_runs_only_successful_queries_on_the_open_connection() {
        assert_eq!(rerun_plan(&entry("c1", true), "c1"), RerunPlan::RunHere);
        assert!(
            matches!(rerun_plan(&entry("c1", false), "c1"), RerunPlan::OpenOnly(m) if m.contains("failed"))
        );
        assert!(
            matches!(rerun_plan(&entry("c2", true), "c1"), RerunPlan::OpenOnly(m) if m.contains("Prod"))
        );
    }

    #[test]
    fn appending_replaces_a_blank_editor_and_otherwise_adds_after_a_gap() {
        assert_eq!(append_sql("", "SELECT 1"), "SELECT 1");
        assert_eq!(append_sql("  \n", "SELECT 1"), "SELECT 1");
        assert_eq!(
            append_sql("SELECT 2;\n", "SELECT 1"),
            "SELECT 2;\n\nSELECT 1"
        );
    }

    #[test]
    fn snippet_form_requires_name_and_sql() {
        let mut d = SnippetDraft::default();
        assert!(build_snippet(&d, None).is_err());
        d.name = "Active users".into();
        assert!(build_snippet(&d, None).unwrap_err().contains("no SQL"));
        d.query = " SELECT 1 ".into();
        d.tags = "a, b ,, ".into();
        d.description = "  ".into();
        let s = build_snippet(&d, None).unwrap();
        assert_eq!(s.query, "SELECT 1");
        assert_eq!(s.tags, Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(s.description, None);
    }

    #[test]
    fn editing_a_snippet_keeps_identity_and_can_clear_optional_fields() {
        let first = QuerySnippet::new(
            "n".into(),
            "SELECT 1".into(),
            Some("d".into()),
            Some(vec!["x".into()]),
        );
        let d = SnippetDraft {
            name: "n2".into(),
            query: "SELECT 2".into(),
            ..Default::default()
        };
        let edited = build_snippet(&d, Some(&first)).unwrap();
        assert_eq!(edited.id, first.id);
        assert_eq!(edited.created_at, first.created_at);
        assert_eq!(edited.name, "n2");
        assert_eq!(edited.description, None);
        assert_eq!(edited.tags, None);
    }

    #[test]
    fn snippet_draft_round_trips() {
        let s = QuerySnippet::new(
            "n".into(),
            "SELECT 1".into(),
            Some("d".into()),
            Some(vec!["x".into(), "y".into()]),
        );
        let d = SnippetDraft::from_snippet(&s);
        assert_eq!(d.tags, "x, y");
        let back = build_snippet(&d, Some(&s)).unwrap();
        assert_eq!(back.tags, s.tags);
        assert_eq!(back.description, s.description);
    }

    #[test]
    fn snippet_rows_sort_by_name_and_fall_back_to_the_sql_when_undescribed() {
        let a = QuerySnippet::new("beta".into(), "SELECT 2\nFROM t".into(), None, None);
        let b = QuerySnippet::new(
            "Alpha".into(),
            "SELECT 1".into(),
            Some("first".into()),
            Some(vec!["t".into()]),
        );
        let rows = snippet_rows(&[a, b], "", None);
        assert_eq!(rows[0].title, "Alpha");
        assert_eq!(rows[0].subtitle, "first");
        assert_eq!(rows[0].tags, vec!["t".to_string()]);
        assert_eq!(rows[1].subtitle, "SELECT 2");
    }

    #[test]
    fn snippet_rows_honour_search_and_tag() {
        let a = QuerySnippet::new("a".into(), "SELECT 1".into(), None, Some(vec!["x".into()]));
        let b = QuerySnippet::new("b".into(), "SELECT 2".into(), None, None);
        assert_eq!(
            snippet_rows(&[a.clone(), b.clone()], "", Some("x")).len(),
            1
        );
        assert_eq!(snippet_rows(&[a, b], "select 2", None).len(), 1);
    }

    #[test]
    fn template_form_derives_parameters_from_the_sql() {
        let d = TemplateDraft {
            name: "By status".into(),
            sql: "SELECT * FROM t WHERE status = {{status}} AND n > {{min_n:integer}}".into(),
            ..Default::default()
        };
        let t = build_template(&d, &[], None).unwrap();
        assert_eq!(t.parameters.len(), 2);
        assert_eq!(t.parameters[1].param_type, ParamType::Integer);
        assert_eq!(t.parameters[1].label, "Min N");
    }

    #[test]
    fn template_form_keeps_configured_settings_and_identity_on_edit() {
        let mut first = template("SELECT {{a}}");
        first.parameters[0].default_value = Some("7".into());
        first.parameters[0].required = false;
        let d = TemplateDraft {
            name: "T2".into(),
            sql: "SELECT {{a}}, {{b}}".into(),
            ..Default::default()
        };
        let edited = build_template(&d, &first.parameters, Some(&first)).unwrap();
        assert_eq!(edited.id, first.id);
        assert_eq!(edited.created_at, first.created_at);
        assert_eq!(edited.parameters[0].default_value.as_deref(), Some("7"));
        assert!(!edited.parameters[0].required);
        assert!(edited.parameters[1].required);
    }

    #[test]
    fn template_form_requires_name_and_sql() {
        assert!(build_template(&TemplateDraft::default(), &[], None).is_err());
        let d = TemplateDraft {
            name: "x".into(),
            ..Default::default()
        };
        assert!(build_template(&d, &[], None)
            .unwrap_err()
            .contains("no SQL"));
    }

    #[test]
    fn parameter_edits_apply_by_index_and_ignore_a_stale_one() {
        let mut t = template("SELECT {{a}}");
        set_param_default(&mut t.parameters, 0, "5");
        assert_eq!(t.parameters[0].default_value.as_deref(), Some("5"));
        set_param_default(&mut t.parameters, 0, "");
        assert_eq!(t.parameters[0].default_value, None);
        toggle_param_required(&mut t.parameters, 0);
        assert!(!t.parameters[0].required);
        set_param_default(&mut t.parameters, 9, "x");
        toggle_param_required(&mut t.parameters, 9);
    }

    #[test]
    fn duplicating_picks_an_unused_name_and_a_new_id() {
        let t = template("SELECT {{a}}");
        let mut all = vec![t.clone()];
        let c1 = duplicate_template(&t, &all);
        assert_eq!(c1.name, "T copy");
        assert_ne!(c1.id, t.id);
        assert_eq!(c1.sql, t.sql);
        all.push(c1);
        assert_eq!(duplicate_template(&t, &all).name, "T copy 2");
    }

    #[test]
    fn template_rows_summarise_parameters_and_category() {
        let mut t = template("SELECT {{a}}, {{b}}");
        t.category = Some("Reports".into());
        let rows = template_rows(&[t], "", None);
        assert_eq!(rows[0].subtitle, "2 parameters · Reports");
        assert_eq!(
            template_rows(&[template("SELECT 1")], "", None)[0].subtitle,
            "No parameters"
        );
    }

    #[test]
    fn run_form_starts_clean_then_validates_types_live() {
        let t = template("SELECT * FROM t WHERE n = {{n:integer}}");
        let mut run = TemplateRun::new(&t);
        assert_eq!(run.errors(), vec![String::new()]);
        run.set_value(0, "abc");
        assert_eq!(run.errors(), vec!["Must be a valid integer".to_string()]);
        run.set_value(0, "42");
        assert_eq!(run.errors(), vec![String::new()]);
        assert_eq!(run.preview(), "SELECT * FROM t WHERE n = 42");
    }

    #[test]
    fn run_form_preview_keeps_unfilled_placeholders() {
        let t = template("SELECT {{a}}, {{b}}");
        let mut run = TemplateRun::new(&t);
        run.set_value(0, "x");
        assert_eq!(run.preview(), "SELECT 'x', {{b}}");
    }

    #[test]
    fn run_form_submit_reports_required_and_type_errors_and_blocks() {
        let t = template("SELECT {{a}}, {{n:integer}}");
        let mut run = TemplateRun::new(&t);
        run.set_value(1, "oops");
        assert!(run.submit().is_err());
        assert_eq!(run.errors()[0], "This field is required");
        assert_eq!(run.errors()[1], "Must be a valid integer");
    }

    #[test]
    fn run_form_submit_renders_with_quoting() {
        let t = template("SELECT {{name}}");
        let mut run = TemplateRun::new(&t);
        run.set_value(0, "O'Brien");
        assert_eq!(run.submit().unwrap(), "SELECT 'O''Brien'");
    }

    #[test]
    fn run_form_injection_attempt_stays_inside_the_quotes() {
        let t = template("SELECT * FROM u WHERE name = {{name}}");
        let mut run = TemplateRun::new(&t);
        run.set_value(0, "x'; DROP TABLE u; --");
        assert_eq!(
            run.submit().unwrap(),
            "SELECT * FROM u WHERE name = 'x''; DROP TABLE u; --'"
        );
    }

    #[test]
    fn run_form_prefills_defaults() {
        let mut t = template("SELECT {{a}}");
        t.parameters[0].default_value = Some("dflt".into());
        let mut run = TemplateRun::new(&t);
        assert_eq!(run.fields()[0].initial, "dflt");
        assert_eq!(run.submit().unwrap(), "SELECT 'dflt'");
    }

    #[test]
    fn panel_tab_index_round_trips() {
        for t in [PanelTab::History, PanelTab::Snippets, PanelTab::Templates] {
            assert_eq!(PanelTab::from_index(t.index()), t);
        }
        assert_eq!(PanelTab::from_index(99), PanelTab::History);
    }
}
