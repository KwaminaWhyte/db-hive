//! Query history, saved snippets and query templates.
//!
//! Parity target: the React `HistoryPanel`, `SnippetSidebar` and
//! `TemplatesPanel`, which the web shell showed as three separate panels.
//! They share one side panel here because they are all "a list of SQL you
//! can put back in the editor", and a native window has no room for three
//! sidebars.
//!
//! Clicking an entry does not run anything itself: the panel has no editor
//! and no connection. It returns [`HistoryAction::UseSql`] and the caller
//! decides what to do with the statement, which keeps the panel usable from
//! the workspace, a dialog, or anywhere else.
//!
//! History and snippets live in `AppState` (`history.json`, `snippets.json`).
//! Templates had no backend at all in the web shell: `useQueryTemplates`
//! kept them in `localStorage`, which does not exist here, so they are
//! persisted to `templates.json` in the same application data directory
//! through `dbhive_core::storage::JsonStore`.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Local, Utc};
use dbhive_core::models::{QueryHistory, QuerySnippet};
use dbhive_core::state::AppState;
use dbhive_core::storage::JsonStore;
use serde::{Deserialize, Serialize};

use crate::app::DbHiveApp;
use crate::task::Job;

/// Row-count choices offered by the history limit selector.
///
/// Same four values the web shell's `<Select>` had; changing them changes
/// what an existing user sees, so they are a constant rather than a range.
pub const HISTORY_LIMITS: [usize; 4] = [25, 50, 100, 500];

/// Name of the JSON store templates are persisted to.
const TEMPLATE_STORE: &str = "templates.json";
const TEMPLATE_KEY: &str = "templates";

/// What the caller should do once the panel has been drawn.
///
/// The panel cannot execute SQL itself (it holds neither the editor nor a
/// connection), so a click is handed back rather than acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAction {
    None,
    /// Put this statement in the editor.
    UseSql(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    History,
    Snippets,
    Templates,
}

// ===========================================================================
// Template model
// ===========================================================================

/// A template parameter's type.
///
/// Serialised as the lowercase strings the web shell wrote, so a
/// `query-templates-*.json` exported from the React build imports here
/// unchanged. Unknown values decode to `String` instead of failing: a
/// template file that mentions a type we do not know is still a template the
/// user wants to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ParamType {
    String,
    Integer,
    Decimal,
    Boolean,
    Date,
    Datetime,
    Text,
}

impl ParamType {
    pub const ALL: [ParamType; 7] = [
        ParamType::String,
        ParamType::Integer,
        ParamType::Decimal,
        ParamType::Boolean,
        ParamType::Date,
        ParamType::Datetime,
        ParamType::Text,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Integer => "integer",
            ParamType::Decimal => "decimal",
            ParamType::Boolean => "boolean",
            ParamType::Date => "date",
            ParamType::Datetime => "datetime",
            ParamType::Text => "text",
        }
    }

    pub fn parse(s: &str) -> ParamType {
        match s.to_ascii_lowercase().as_str() {
            "integer" => ParamType::Integer,
            "decimal" => ParamType::Decimal,
            "boolean" => ParamType::Boolean,
            "date" => ParamType::Date,
            "datetime" => ParamType::Datetime,
            "text" => ParamType::Text,
            _ => ParamType::String,
        }
    }
}

impl From<String> for ParamType {
    fn from(s: String) -> Self {
        ParamType::parse(&s)
    }
}

impl From<ParamType> for String {
    fn from(t: ParamType) -> Self {
        t.as_str().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateParameter {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_param_type", rename = "type")]
    pub param_type: ParamType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_param_type() -> ParamType {
    ParamType::String
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTemplate {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sql: String,
    #[serde(default)]
    pub parameters: Vec<TemplateParameter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl QueryTemplate {
    fn new(
        name: String,
        description: Option<String>,
        sql: String,
        parameters: Vec<TemplateParameter>,
        category: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description,
            sql,
            parameters,
            category,
            tags,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

// ===========================================================================
// Screen state
// ===========================================================================

pub struct History {
    pub open: bool,
    pub tab: Tab,

    // History tab
    pub limit: usize,
    pub entries: Vec<QueryHistory>,
    /// Connection, database and limit the loaded entries belong to. A change
    /// here reloads, which is what the React `useCallback` dependency list
    /// did.
    loaded_key: Option<(Option<String>, Option<String>, usize)>,
    pub confirm_clear: bool,

    // Snippets tab
    pub snippets: Vec<QuerySnippet>,
    snippets_loaded: bool,
    pub snippet_search: String,
    pub snippet_tag: Option<String>,
    pub snippet_form: Option<SnippetForm>,
    /// (id, name) of the snippet awaiting delete confirmation.
    pub snippet_delete: Option<(String, String)>,

    // Templates tab
    pub templates: Vec<QueryTemplate>,
    templates_requested: bool,
    pub template_search: String,
    pub template_category: Option<String>,
    pub template_form: Option<TemplateForm>,
    pub template_run: Option<TemplateRun>,
    pub template_delete: Option<(String, String)>,
    pub import_path: String,
    pub show_import: bool,

    pub error: Option<String>,
    load_templates_job: Job<Result<Vec<QueryTemplate>, String>>,
    save_job: Job<Result<(), String>>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::default(),
            limit: 50,
            entries: Vec::new(),
            loaded_key: None,
            confirm_clear: false,
            snippets: Vec::new(),
            snippets_loaded: false,
            snippet_search: String::new(),
            snippet_tag: None,
            snippet_form: None,
            snippet_delete: None,
            templates: Vec::new(),
            templates_requested: false,
            template_search: String::new(),
            template_category: None,
            template_form: None,
            template_run: None,
            template_delete: None,
            import_path: String::new(),
            show_import: false,
            error: None,
            load_templates_job: Job::default(),
            save_job: Job::default(),
        }
    }
}

impl History {
    /// Drop the loaded history so the next frame refetches it.
    ///
    /// The web shell passed a `refreshTrigger` prop that it bumped after
    /// every execution; call this from the workspace once a statement has
    /// been recorded, or the panel keeps showing the previous set until the
    /// user presses Refresh.
    pub fn invalidate_history(&mut self) {
        self.loaded_key = None;
    }

    /// Drop both history and snippets, e.g. after another window wrote to
    /// the shared stores.
    pub fn invalidate(&mut self) {
        self.loaded_key = None;
        self.snippets_loaded = false;
    }
}

/// Create/edit form for a snippet.
#[derive(Default)]
pub struct SnippetForm {
    /// `None` while creating.
    pub id: Option<String>,
    pub name: String,
    pub query: String,
    pub description: String,
    /// Comma separated, as the web shell's single tags input was.
    pub tags: String,
}

/// Create/edit form for a template.
pub struct TemplateForm {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: String,
    pub sql: String,
    pub parameters: Vec<TemplateParameter>,
    /// The SQL the parameter list was last derived from. When the two differ
    /// the list is rebuilt, which is the web shell's two-way live sync.
    synced_sql: String,
}

impl Default for TemplateForm {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            description: String::new(),
            category: String::new(),
            tags: String::new(),
            sql: String::new(),
            parameters: Vec::new(),
            synced_sql: String::new(),
        }
    }
}

/// Fill-in form shown before running a parameterised template.
pub struct TemplateRun {
    pub template: QueryTemplate,
    pub values: HashMap<String, String>,
    pub errors: HashMap<String, String>,
}

/// Work that has to happen outside the drawing borrow of `app.screens`.
enum Intent {
    RefreshHistory,
    ClearHistory,
    SaveSnippet(QuerySnippet),
    DeleteSnippet(String),
    SaveTemplates,
    ExportTemplates,
    ImportTemplates(String),
}

// ===========================================================================
// Pure logic
// ===========================================================================

/// Entries to show, mirroring what the web shell ended up with.
///
/// The backend command filtered by connection, sorted newest first and
/// truncated to `limit`; the panel then dropped anything from another
/// database. That order matters: the database filter runs *after* the
/// truncation, so a busy connection can show fewer than `limit` rows. It is
/// reproduced rather than "fixed" so the two shells agree.
pub fn filter_history(
    all: &[QueryHistory],
    connection_id: Option<&str>,
    database: Option<&str>,
    limit: usize,
) -> Vec<QueryHistory> {
    let mut rows: Vec<QueryHistory> = all
        .iter()
        .filter(|h| connection_id.is_none_or(|c| h.connection_id == c))
        .cloned()
        .collect();

    // ISO 8601 with a fixed offset sorts correctly as text, which is what
    // the Tauri command relied on too.
    rows.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));
    rows.truncate(limit);

    if let Some(db) = database {
        rows.retain(|h| h.database == db);
    }
    rows
}

/// "Just now" / "5m ago" / "3h ago" / "2d ago", falling back to an absolute
/// local timestamp after a week. `now` is a parameter so this is testable.
pub fn format_relative(iso: &str, now: DateTime<Utc>) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(iso) else {
        // An unparseable timestamp is still information; showing it raw
        // beats showing nothing.
        return iso.to_string();
    };
    let dt = parsed.with_timezone(&Utc);
    let diff = now.signed_duration_since(dt);
    let mins = diff.num_minutes();
    let hours = diff.num_hours();
    let days = diff.num_days();

    if mins < 1 {
        "Just now".to_string()
    } else if mins < 60 {
        format!("{}m ago", mins)
    } else if hours < 24 {
        format!("{}h ago", hours)
    } else if days < 7 {
        format!("{}d ago", days)
    } else {
        dt.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }
}

/// `1234` -> `1.23s`, `12` -> `12ms`, missing or zero -> `N/A`.
///
/// Zero counting as "unknown" is deliberate: the web shell's `if (!ms)`
/// treated 0 as absent, and a driver that reports 0 means "not measured".
pub fn format_execution_time(ms: Option<u64>) -> String {
    match ms {
        None | Some(0) => "N/A".to_string(),
        Some(v) if v < 1000 => format!("{}ms", v),
        Some(v) => format!("{:.2}s", v as f64 / 1000.0),
    }
}

/// Thousands-separated row count, matching `Number.toLocaleString()`.
pub fn format_row_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Snippets matching the search box and the selected tag pill.
pub fn filter_snippets<'a>(
    snippets: &'a [QuerySnippet],
    search: &str,
    tag: Option<&str>,
) -> Vec<&'a QuerySnippet> {
    let needle = search.trim().to_lowercase();
    snippets
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.query.to_lowercase().contains(&needle)
                || s
                    .description
                    .as_deref()
                    .is_some_and(|d| d.to_lowercase().contains(&needle))
        })
        .filter(|s| match tag {
            None => true,
            Some(t) => s
                .tags
                .as_ref()
                .is_some_and(|tags| tags.iter().any(|x| x == t)),
        })
        .collect()
}

/// Every tag used by any snippet, sorted, for the filter pills.
pub fn snippet_tags(snippets: &[QuerySnippet]) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for s in snippets {
        if let Some(tags) = &s.tags {
            for t in tags {
                set.insert(t.clone());
            }
        }
    }
    set.into_iter().collect()
}

/// Templates matching the category pill and the search box.
///
/// The web shell's panel searched name, description and tags but not the SQL
/// body (its `searchTemplates` helper did, and was never called). Same here.
pub fn filter_templates<'a>(
    templates: &'a [QueryTemplate],
    search: &str,
    category: Option<&str>,
) -> Vec<&'a QueryTemplate> {
    let needle = search.trim().to_lowercase();
    templates
        .iter()
        .filter(|t| match category {
            None => true,
            Some(c) => t.category.as_deref() == Some(c),
        })
        .filter(|t| {
            needle.is_empty()
                || t.name.to_lowercase().contains(&needle)
                || t
                    .description
                    .as_deref()
                    .is_some_and(|d| d.to_lowercase().contains(&needle))
                || t.tags
                    .as_ref()
                    .is_some_and(|tags| tags.iter().any(|x| x.to_lowercase().contains(&needle)))
        })
        .collect()
}

/// Distinct categories, sorted, for the filter pills.
pub fn template_categories(templates: &[QueryTemplate]) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for t in templates {
        if let Some(c) = &t.category {
            if !c.is_empty() {
                set.insert(c.clone());
            }
        }
    }
    set.into_iter().collect()
}

/// One `{{name}}` or `{{name:type}}` occurrence in a template body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub declared_type: Option<String>,
}

/// Find every placeholder, in order.
///
/// Hand-written rather than a regex because the shell crate has no `regex`
/// dependency, and this has to accept exactly what the web shell's
/// `/\{\{([a-zA-Z_][a-zA-Z0-9_]*)(?::([a-zA-Z]+))?\}\}/g` accepted:
/// an identifier-shaped name and, optionally, an all-alphabetic type. A
/// `{{...}}` that does not fit is left alone, not treated as a parameter.
pub fn scan_placeholders(sql: &str) -> Vec<Placeholder> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i + 1 < bytes.len() {
        if bytes[i] != b'{' || bytes[i + 1] != b'{' {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + 2;
        let name_start = j;

        if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
            j += 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            let name = sql[name_start..j].to_string();

            let mut declared_type = None;
            if j < bytes.len() && bytes[j] == b':' {
                let type_start = j + 1;
                let mut k = type_start;
                while k < bytes.len() && bytes[k].is_ascii_alphabetic() {
                    k += 1;
                }
                if k == type_start {
                    // `{{name:}}` is not a placeholder.
                    i += 1;
                    continue;
                }
                declared_type = Some(sql[type_start..k].to_string());
                j = k;
            }

            if j + 1 < bytes.len() && bytes[j] == b'}' && bytes[j + 1] == b'}' {
                out.push(Placeholder {
                    start,
                    end: j + 2,
                    name,
                    declared_type,
                });
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Distinct parameter names and their declared types, first occurrence wins.
pub fn extract_parameter_definitions(sql: &str) -> Vec<(String, ParamType)> {
    let mut seen: Vec<(String, ParamType)> = Vec::new();
    for p in scan_placeholders(sql) {
        if seen.iter().any(|(n, _)| n == &p.name) {
            continue;
        }
        let ty = p
            .declared_type
            .as_deref()
            .map(ParamType::parse)
            .unwrap_or(ParamType::String);
        seen.push((p.name, ty));
    }
    seen
}

/// Replace placeholders using `render`, leaving anything it declines alone.
pub fn substitute(sql: &str, render: impl Fn(&Placeholder) -> Option<String>) -> String {
    let places = scan_placeholders(sql);
    let mut out = String::with_capacity(sql.len());
    let mut cursor = 0usize;
    for p in &places {
        match render(p) {
            Some(text) => {
                out.push_str(&sql[cursor..p.start]);
                out.push_str(&text);
            }
            None => out.push_str(&sql[cursor..p.end]),
        }
        cursor = p.end;
    }
    out.push_str(&sql[cursor..]);
    out
}

/// Quote and escape a value for inlining into SQL.
///
/// Templates are inlined text, not bound parameters, so quoting is the only
/// thing standing between a value and an injection. Single quotes are
/// doubled, as the web shell did.
pub fn format_parameter_for_sql(value: &str, ty: ParamType) -> String {
    match ty {
        ParamType::Integer | ParamType::Decimal => value.to_string(),
        ParamType::Boolean => {
            let lower = value.to_lowercase();
            if lower == "true" || lower == "1" || lower == "yes" {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        _ => format!("'{}'", value.replace('\'', "''")),
    }
}

/// Type check one value. `Ok(())` for an empty value: emptiness is the
/// `required` flag's business, not the type's.
pub fn validate_parameter_value(value: &str, ty: ParamType) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    let ok = match ty {
        ParamType::Integer => is_integer(value),
        ParamType::Decimal => is_decimal(value),
        ParamType::Boolean => matches!(
            value.to_lowercase().as_str(),
            "true" | "false" | "1" | "0" | "yes" | "no"
        ),
        ParamType::Date => is_date(value),
        ParamType::Datetime => is_datetime(value),
        _ => true,
    };
    if ok {
        return Ok(());
    }
    Err(match ty {
        ParamType::Integer => "Must be a valid integer".to_string(),
        ParamType::Decimal => "Must be a valid decimal number".to_string(),
        ParamType::Boolean => "Must be true/false, yes/no, or 1/0".to_string(),
        ParamType::Date => "Must be in YYYY-MM-DD format".to_string(),
        ParamType::Datetime => "Must be in YYYY-MM-DD HH:MM:SS format".to_string(),
        _ => "Invalid value".to_string(),
    })
}

fn is_integer(v: &str) -> bool {
    let body = v.strip_prefix('-').unwrap_or(v);
    !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit())
}

/// `-?\d+\.?\d*`: at least one leading digit, an optional point, then
/// optional digits. `1.` is valid, `.5` is not, matching the web shell.
fn is_decimal(v: &str) -> bool {
    let body = v.strip_prefix('-').unwrap_or(v);
    let mut chars = body.chars().peekable();
    let mut leading = 0;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        leading += 1;
    }
    if leading == 0 {
        return false;
    }
    if chars.peek() == Some(&'.') {
        chars.next();
    }
    chars.all(|c| c.is_ascii_digit())
}

fn is_date(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 10
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// `\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2})?` anchored at the start only,
/// so a trailing timezone or fraction is accepted.
fn is_datetime(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() < 16 {
        return false;
    }
    if !is_date(&v[0..10]) {
        return false;
    }
    if b[10] != b'T' && b[10] != b' ' {
        return false;
    }
    b[11..13].iter().all(u8::is_ascii_digit) && b[13] == b':' && b[14..16].iter().all(u8::is_ascii_digit)
}

/// Live preview: filled parameters are formatted, unfilled ones keep their
/// placeholder so the user can see what is still missing.
pub fn preview_sql(template: &QueryTemplate, values: &HashMap<String, String>) -> String {
    substitute(&template.sql, |p| {
        let value = values.get(&p.name).map(String::as_str).unwrap_or("");
        if value.is_empty() {
            return Some(format!("{{{{{}}}}}", p.name));
        }
        let ty = template
            .parameters
            .iter()
            .find(|d| d.name == p.name)
            .map(|d| d.param_type)
            .unwrap_or_else(|| {
                p.declared_type
                    .as_deref()
                    .map(ParamType::parse)
                    .unwrap_or(ParamType::String)
            });
        Some(format_parameter_for_sql(value, ty))
    })
}

/// The statement to run: every declared parameter substituted, falling back
/// to its default when the user left the field blank.
pub fn render_template(template: &QueryTemplate, values: &HashMap<String, String>) -> String {
    substitute(&template.sql, |p| {
        let declared = template.parameters.iter().find(|d| d.name == p.name)?;
        let value = values
            .get(&p.name)
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| declared.default_value.clone())
            .unwrap_or_default();
        Some(format_parameter_for_sql(&value, declared.param_type))
    })
}

/// Required-and-empty plus per-type errors, keyed by parameter name.
pub fn validate_template_values(
    template: &QueryTemplate,
    values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut errors = HashMap::new();
    for p in &template.parameters {
        let value = values.get(&p.name).map(String::as_str).unwrap_or("");
        if p.required && value.is_empty() {
            errors.insert(p.name.clone(), "This field is required".to_string());
            continue;
        }
        if let Err(e) = validate_parameter_value(value, p.param_type) {
            errors.insert(p.name.clone(), e);
        }
    }
    errors
}

/// Rebuild a parameter list from the SQL, keeping the settings the user
/// already configured for parameters that survived the edit.
///
/// This is the web shell's two-way live sync: type comes from the SQL (it is
/// written there), everything else comes from the existing definition.
pub fn sync_parameters(sql: &str, existing: &[TemplateParameter]) -> Vec<TemplateParameter> {
    extract_parameter_definitions(sql)
        .into_iter()
        .map(|(name, ty)| match existing.iter().find(|p| p.name == name) {
            Some(prev) => TemplateParameter {
                param_type: ty,
                ..prev.clone()
            },
            None => TemplateParameter {
                label: humanize(&name),
                name,
                param_type: ty,
                default_value: None,
                required: true,
                description: None,
            },
        })
        .collect()
}

/// `user_id` -> `User Id`, as the web shell's label default did.
fn humanize(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Merge imported templates into `existing`, skipping names already present.
///
/// Returns the templates that were actually added, with fresh ids so an
/// import can never collide with or overwrite something the user has.
pub fn merge_imported(
    existing: &[QueryTemplate],
    incoming: Vec<QueryTemplate>,
) -> Vec<QueryTemplate> {
    let mut names: BTreeSet<String> = existing.iter().map(|t| t.name.clone()).collect();
    let mut added = Vec::new();
    let now = Utc::now().to_rfc3339();
    for mut t in incoming {
        if names.contains(&t.name) {
            continue;
        }
        names.insert(t.name.clone());
        t.id = uuid::Uuid::new_v4().to_string();
        if t.created_at.is_empty() {
            t.created_at = now.clone();
        }
        t.updated_at = now.clone();
        added.push(t);
    }
    added
}

/// Split a comma separated tag input into cleaned tags.
fn parse_tags(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// First line of a statement, for a one-line summary.
fn first_line(sql: &str, max: usize) -> String {
    let line = sql.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > max {
        let cut: String = line.chars().take(max).collect();
        format!("{}…", cut)
    } else {
        line.to_string()
    }
}

// ===========================================================================
// Persistence
// ===========================================================================

fn load_templates() -> Result<Vec<QueryTemplate>, String> {
    let store = JsonStore::open(TEMPLATE_STORE).map_err(|e| e.to_string())?;
    match store.get(TEMPLATE_KEY) {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| format!("templates.json could not be read: {}", e)),
    }
}

fn save_templates(templates: &[QueryTemplate]) -> Result<(), String> {
    let mut store = JsonStore::open(TEMPLATE_STORE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(templates).map_err(|e| e.to_string())?;
    store.set(TEMPLATE_KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Where an export lands.
///
/// The web shell handed a Blob to the browser's download machinery; there is
/// no such thing here and the shell has no file-dialog dependency, so the
/// file goes next to the other stores and the path is reported in a toast.
fn export_path() -> Result<std::path::PathBuf, String> {
    let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y-%m-%d");
    Ok(dir.join(format!("query-templates-{}.json", stamp)))
}

// ===========================================================================
// Screen
// ===========================================================================

/// Draw the panel. The returned action is the statement the user picked, if
/// any; the caller owns the editor and decides what to do with it.
pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) -> HistoryAction {
    let palette = app.mode.palette();
    let ctx = ui.ctx().clone();
    let (connection_id, database) = active_scope(app);

    // --- poll background work -------------------------------------------
    if let Some(result) = app.screens.history.load_templates_job.take_result() {
        match result {
            Ok(list) => app.screens.history.templates = list,
            Err(e) => {
                app.screens.history.error = Some(e.clone());
                app.toasts.error(format!("Could not load templates: {}", e));
            }
        }
    }
    if let Some(Err(e)) = app.screens.history.save_job.take_result() {
        app.screens.history.error = Some(e.clone());
        app.toasts.error(e);
    }

    // --- first load / reload on scope change ------------------------------
    if !app.screens.history.templates_requested {
        app.screens.history.templates_requested = true;
        app.screens
            .history
            .load_templates_job
            .start(app.rt.handle(), &ctx, async move { load_templates() });
    }
    if !app.screens.history.snippets_loaded {
        reload_snippets(app);
    }
    let key = (connection_id.clone(), database.clone(), app.screens.history.limit);
    if app.screens.history.loaded_key.as_ref() != Some(&key) {
        reload_history(app, &connection_id, &database);
    }

    // --- draw --------------------------------------------------------------
    let mut action = HistoryAction::None;
    let mut intents: Vec<Intent> = Vec::new();
    {
        let h = &mut app.screens.history;

        ui.horizontal(|ui| {
            ui.selectable_value(&mut h.tab, Tab::History, "History");
            ui.selectable_value(&mut h.tab, Tab::Snippets, "Snippets");
            ui.selectable_value(&mut h.tab, Tab::Templates, "Templates");
        });
        ui.add_space(6.0);

        if let Some(err) = h.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(palette.destructive, err);
                if ui.small_button("Dismiss").clicked() {
                    h.error = None;
                }
            });
            ui.add_space(4.0);
        }

        match h.tab {
            Tab::History => history_tab(
                h,
                ui,
                palette,
                connection_id.as_deref(),
                database.as_deref(),
                &mut action,
                &mut intents,
            ),
            Tab::Snippets => snippets_tab(h, ui, palette, &mut action, &mut intents),
            Tab::Templates => templates_tab(h, ui, palette, &mut action, &mut intents),
        }

        // Dialogs are drawn last so they sit above the list.
        clear_history_dialog(h, &ctx, palette, connection_id.as_deref(), database.as_deref(), &mut intents);
        snippet_form_dialog(h, &ctx, palette, &mut intents);
        confirm_delete_dialog(h, &ctx, palette, &mut intents);
        template_form_dialog(h, &ctx, palette, &mut intents);
        template_run_dialog(h, &ctx, palette, &mut action, &mut intents);
        import_dialog(h, &ctx, palette, &mut intents);
    }

    // --- act ---------------------------------------------------------------
    for intent in intents {
        apply(app, &ctx, &connection_id, &database, intent);
    }

    action
}

/// Connection and database the panel scopes itself to, from the open
/// workspace. Nothing open means unscoped, and the panel says so.
fn active_scope(app: &DbHiveApp) -> (Option<String>, Option<String>) {
    match app.workspace.as_ref() {
        Some(w) => (Some(w.connection_id.clone()), w.current_database.clone()),
        None => (None, None),
    }
}

fn reload_history(app: &mut DbHiveApp, connection_id: &Option<String>, database: &Option<String>) {
    // Copy out of the lock first; filtering happens with the lock released.
    let all: Vec<QueryHistory> = {
        let guard = app.state.lock().unwrap();
        guard.get_all_history()
    };
    let h = &mut app.screens.history;
    h.entries = filter_history(&all, connection_id.as_deref(), database.as_deref(), h.limit);
    h.loaded_key = Some((connection_id.clone(), database.clone(), h.limit));
}

fn reload_snippets(app: &mut DbHiveApp) {
    let mut snippets: Vec<QuerySnippet> = {
        let guard = app.state.lock().unwrap();
        guard.get_all_snippets()
    };
    snippets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let h = &mut app.screens.history;
    h.snippets = snippets;
    h.snippets_loaded = true;
}

fn apply(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    connection_id: &Option<String>,
    database: &Option<String>,
    intent: Intent,
) {
    match intent {
        Intent::RefreshHistory => reload_history(app, connection_id, database),

        Intent::ClearHistory => {
            // Clear inside the lock, snapshot, then write to disk off the
            // frame: a synchronous file write in `update` is a stall.
            let snapshot = {
                let mut guard = app.state.lock().unwrap();
                match connection_id {
                    Some(id) => guard.clear_history_by_connection(id),
                    None => guard.clear_all_history(),
                };
                guard.query_history.clone()
            };
            app.screens.history.save_job.start(app.rt.handle(), ctx, async move {
                AppState::save_history_to_store(&snapshot).map_err(|e| e.to_string())
            });
            reload_history(app, connection_id, database);
            app.toasts.success("Query history cleared");
        }

        Intent::SaveSnippet(snippet) => {
            {
                let mut guard = app.state.lock().unwrap();
                guard.add_snippet(snippet);
            }
            persist_snippets(app, ctx);
            reload_snippets(app);
            app.toasts.success("Snippet saved");
        }

        Intent::DeleteSnippet(id) => {
            let removed = {
                let mut guard = app.state.lock().unwrap();
                guard.remove_snippet(&id).is_some()
            };
            if removed {
                persist_snippets(app, ctx);
                reload_snippets(app);
                app.toasts.success("Snippet deleted");
            } else {
                app.toasts.error("That snippet no longer exists");
            }
        }

        Intent::SaveTemplates => {
            let snapshot = app.screens.history.templates.clone();
            app.screens
                .history
                .save_job
                .start(app.rt.handle(), ctx, async move { save_templates(&snapshot) });
        }

        Intent::ExportTemplates => {
            let snapshot = app.screens.history.templates.clone();
            match export_templates_now(&snapshot) {
                Ok(path) => app.toasts.success(format!("Exported to {}", path.display())),
                Err(e) => {
                    app.screens.history.error = Some(e.clone());
                    app.toasts.error(e);
                }
            }
        }

        Intent::ImportTemplates(path) => match import_templates_now(path.trim()) {
            Ok(incoming) => {
                let added = merge_imported(&app.screens.history.templates, incoming);
                let count = added.len();
                app.screens.history.templates.extend(added);
                if count > 0 {
                    apply(app, ctx, connection_id, database, Intent::SaveTemplates);
                }
                app.toasts
                    .success(format!("Imported {} template(s)", count));
                app.screens.history.show_import = false;
            }
            Err(e) => {
                app.screens.history.error = Some(e.clone());
                app.toasts.error(e);
            }
        },
    }
}

/// Persist snippets through core, off the frame.
///
/// The closure has no await points, so holding the `std::sync::Mutex` guard
/// inside it is safe; it just must not happen on the UI thread.
fn persist_snippets(app: &mut DbHiveApp, ctx: &egui::Context) {
    let state = app.state.clone();
    app.screens.history.save_job.start(app.rt.handle(), ctx, async move {
        let guard = state.lock().unwrap();
        guard.save_snippets_to_store().map_err(|e| e.to_string())
    });
}

fn export_templates_now(templates: &[QueryTemplate]) -> Result<std::path::PathBuf, String> {
    let path = export_path()?;
    let json = serde_json::to_string_pretty(templates).map_err(|e| e.to_string())?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Could not write {}: {}", path.display(), e))?;
    Ok(path)
}

fn import_templates_now(path: &str) -> Result<Vec<QueryTemplate>, String> {
    if path.is_empty() {
        return Err("Enter the path of a templates JSON file".to_string());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {}", path, e))?;
    serde_json::from_str::<Vec<QueryTemplate>>(&raw)
        .map_err(|e| format!("{} is not a template export: {}", path, e))
}

// ===========================================================================
// History tab
// ===========================================================================

#[allow(clippy::too_many_arguments)]
fn history_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    connection_id: Option<&str>,
    database: Option<&str>,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.label(crate::theme::text::strong(palette, "Query History"));
    ui.label(crate::theme::text::muted(
        palette,
        match (connection_id, database) {
            (Some(_), Some(db)) => format!("History for {}", db),
            (Some(_), None) => "History for current connection".to_string(),
            _ => "No active connection".to_string(),
        },
    ));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("history_limit")
            .selected_text(format!("Last {}", h.limit))
            .width(110.0)
            .show_ui(ui, |ui| {
                for limit in HISTORY_LIMITS {
                    ui.selectable_value(&mut h.limit, limit, format!("Last {}", limit));
                }
            });
        if ui.button("Refresh").clicked() {
            intents.push(Intent::RefreshHistory);
        }
        if ui
            .add_enabled(!h.entries.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            h.confirm_clear = true;
        }
    });
    ui.add_space(6.0);

    if h.entries.is_empty() {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::heading(palette, "No Query History"));
            ui.label(crate::theme::text::muted(
                palette,
                "Your executed queries will appear here. Run a query to get started.",
            ));
            ui.add_space(8.0);
            if ui.button("Run Your First Query").clicked() {
                *action = HistoryAction::UseSql("SELECT 1;".to_string());
            }
        });
        return;
    }

    let now = Utc::now();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &h.entries {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if entry.success {
                                ui.colored_label(palette.success, "Success");
                            } else {
                                ui.colored_label(palette.destructive, "Failed");
                            }
                            ui.label(crate::theme::text::muted(
                                palette,
                                format_relative(&entry.executed_at, now),
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Only successful statements are offered
                                    // for re-run, as the web shell did.
                                    if entry.success && ui.small_button("Re-run").clicked() {
                                        *action = HistoryAction::UseSql(entry.query.clone());
                                    }
                                },
                            );
                        });

                        ui.label(crate::theme::text::muted(
                            palette,
                            format!("{} / {}", entry.connection_name, entry.database),
                        ));
                        ui.add_space(4.0);

                        egui::Frame::none()
                            .fill(palette.muted)
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(entry.query.trim())
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.foreground),
                                    )
                                    .wrap(),
                                );
                            });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(crate::theme::text::muted(
                                palette,
                                format!("Time: {}", format_execution_time(entry.execution_time_ms)),
                            ));
                            if entry.success {
                                if let Some(rows) = entry.row_count {
                                    ui.label(crate::theme::text::muted(
                                        palette,
                                        format!("Rows: {}", format_row_count(rows)),
                                    ));
                                }
                            }
                        });

                        if let Some(err) = &entry.error_message {
                            ui.colored_label(
                                palette.destructive,
                                egui::RichText::new(format!("Error: {}", err)).size(11.0),
                            );
                        }
                    });
                ui.add_space(6.0);
            }
        });
}

fn clear_history_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    connection_id: Option<&str>,
    database: Option<&str>,
    intents: &mut Vec<Intent>,
) {
    if !h.confirm_clear {
        return;
    }
    let mut open = true;
    egui::Window::new("Clear query history?")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                match (connection_id, database) {
                    (Some(_), Some(db)) => format!(
                        "This will permanently delete the query history for the {} database.",
                        db
                    ),
                    (Some(_), None) => {
                        "This will permanently delete the query history for the current connection."
                            .to_string()
                    }
                    _ => "This will permanently delete all query history.".to_string(),
                },
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    h.confirm_clear = false;
                }
                if ui
                    .button(egui::RichText::new("Clear History").color(palette.destructive))
                    .clicked()
                {
                    intents.push(Intent::ClearHistory);
                    h.confirm_clear = false;
                }
            });
        });
    if !open {
        h.confirm_clear = false;
    }
}

// ===========================================================================
// Snippets tab
// ===========================================================================

fn snippets_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Snippets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New").clicked() {
                h.snippet_form = Some(SnippetForm::default());
            }
        });
    });
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut h.snippet_search)
            .hint_text("Search snippets...")
            .desired_width(f32::INFINITY),
    );

    let tags = snippet_tags(&h.snippets);
    if !tags.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(h.snippet_tag.is_none(), "All")
                .clicked()
            {
                h.snippet_tag = None;
            }
            for tag in &tags {
                let selected = h.snippet_tag.as_deref() == Some(tag.as_str());
                if ui.selectable_label(selected, tag).clicked() {
                    // A second click on the active pill clears it, which the
                    // web shell's single-select tag filter also did.
                    h.snippet_tag = if selected { None } else { Some(tag.clone()) };
                }
            }
        });
    }
    ui.add_space(6.0);

    let visible: Vec<QuerySnippet> = filter_snippets(
        &h.snippets,
        &h.snippet_search,
        h.snippet_tag.as_deref(),
    )
    .into_iter()
    .cloned()
    .collect();

    if visible.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            if h.snippets.is_empty() {
                ui.label(crate::theme::text::heading(palette, "No snippets yet"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "Save a statement you use often and it will show up here.",
                ));
            } else {
                ui.label(crate::theme::text::heading(palette, "No matches"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "No snippet matches that search.",
                ));
            }
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for snippet in &visible {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Clicking the body inserts. The web shell used
                            // double-click for this and single-click to edit,
                            // which was a persistent complaint; the actions
                            // are explicit buttons here instead.
                            let title = ui
                                .add(
                                    egui::Label::new(crate::theme::text::strong(
                                        palette,
                                        snippet.name.clone(),
                                    ))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Click to use this snippet");
                            if title.clicked() {
                                *action = HistoryAction::UseSql(snippet.query.clone());
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Delete").clicked() {
                                        h.snippet_delete =
                                            Some((snippet.id.clone(), snippet.name.clone()));
                                    }
                                    if ui.small_button("Edit").clicked() {
                                        h.snippet_form = Some(SnippetForm {
                                            id: Some(snippet.id.clone()),
                                            name: snippet.name.clone(),
                                            query: snippet.query.clone(),
                                            description: snippet
                                                .description
                                                .clone()
                                                .unwrap_or_default(),
                                            tags: snippet
                                                .tags
                                                .clone()
                                                .unwrap_or_default()
                                                .join(", "),
                                        });
                                    }
                                    if ui.small_button("Use").clicked() {
                                        *action = HistoryAction::UseSql(snippet.query.clone());
                                    }
                                },
                            );
                        });

                        if let Some(desc) = snippet.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }
                        ui.label(
                            egui::RichText::new(first_line(&snippet.query, 90))
                                .monospace()
                                .size(11.0)
                                .color(palette.muted_foreground),
                        );
                        if let Some(tags) = &snippet.tags {
                            if !tags.is_empty() {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    tags.join(" · "),
                                ));
                            }
                        }
                    });
                ui.add_space(6.0);
            }
        });

    // Nothing above this point may mutate the snippet list; it is borrowed
    // by the loop, so saves go through intents.
    let _ = intents;
}

fn snippet_form_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    let Some(form) = h.snippet_form.as_mut() else {
        return;
    };
    let editing = form.id.is_some();
    let mut open = true;
    let mut close = false;

    egui::Window::new(if editing { "Edit snippet" } else { "New snippet" })
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(palette, "Name"));
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .hint_text("e.g. Active users")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "Description"));
            ui.add(
                egui::TextEdit::singleline(&mut form.description)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "Tags (comma separated)"));
            ui.add(egui::TextEdit::singleline(&mut form.tags).desired_width(f32::INFINITY));
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "SQL"));
            ui.add(
                egui::TextEdit::multiline(&mut form.query)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let valid = !form.name.trim().is_empty() && !form.query.trim().is_empty();
                if ui
                    .add_enabled(valid, egui::Button::new("Save"))
                    .clicked()
                {
                    let tags = parse_tags(&form.tags);
                    let mut snippet = QuerySnippet::new(
                        form.name.trim().to_string(),
                        form.query.clone(),
                        Some(form.description.trim().to_string())
                            .filter(|d| !d.is_empty()),
                        if tags.is_empty() { None } else { Some(tags) },
                    );
                    if let Some(id) = &form.id {
                        // Keep the id so this updates rather than duplicates.
                        snippet.id = id.clone();
                    }
                    intents.push(Intent::SaveSnippet(snippet));
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });

    if close || !open {
        h.snippet_form = None;
    }
}

fn confirm_delete_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    if let Some((id, name)) = h.snippet_delete.clone() {
        let mut open = true;
        let mut close = false;
        egui::Window::new("Delete snippet")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("Delete \"{}\"? This cannot be undone.", name),
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(palette.destructive))
                        .clicked()
                    {
                        intents.push(Intent::DeleteSnippet(id.clone()));
                        close = true;
                    }
                });
            });
        if close || !open {
            h.snippet_delete = None;
        }
    }

    if let Some((id, name)) = h.template_delete.clone() {
        let mut open = true;
        let mut close = false;
        egui::Window::new("Delete template")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("Delete \"{}\"? This cannot be undone.", name),
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(palette.destructive))
                        .clicked()
                    {
                        h.templates.retain(|t| t.id != id);
                        intents.push(Intent::SaveTemplates);
                        close = true;
                    }
                });
            });
        if close || !open {
            h.template_delete = None;
        }
    }
}

// ===========================================================================
// Templates tab
// ===========================================================================

fn templates_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Templates"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New").clicked() {
                h.template_form = Some(TemplateForm::default());
            }
            if ui
                .add_enabled(!h.templates.is_empty(), egui::Button::new("Export"))
                .on_hover_text("Write every template to a JSON file")
                .clicked()
            {
                intents.push(Intent::ExportTemplates);
            }
            if ui.button("Import").clicked() {
                h.show_import = true;
            }
        });
    });
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut h.template_search)
            .hint_text("Search templates...")
            .desired_width(f32::INFINITY),
    );

    let categories = template_categories(&h.templates);
    if !categories.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(h.template_category.is_none(), "All")
                .clicked()
            {
                h.template_category = None;
            }
            for cat in &categories {
                let selected = h.template_category.as_deref() == Some(cat.as_str());
                if ui.selectable_label(selected, cat).clicked() {
                    h.template_category = if selected { None } else { Some(cat.clone()) };
                }
            }
        });
    }
    ui.add_space(6.0);

    if h.load_templates_job.is_running() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading templates"));
        });
        return;
    }

    let visible: Vec<QueryTemplate> = filter_templates(
        &h.templates,
        &h.template_search,
        h.template_category.as_deref(),
    )
    .into_iter()
    .cloned()
    .collect();

    if visible.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            if h.templates.is_empty() {
                ui.label(crate::theme::text::heading(palette, "No templates yet"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "Create a template to save reusable queries with parameters.",
                ));
            } else {
                ui.label(crate::theme::text::heading(palette, "No templates found"));
            }
        });
        return;
    }

    let mut duplicate: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for template in &visible {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let title = ui
                                .add(
                                    egui::Label::new(crate::theme::text::strong(
                                        palette,
                                        template.name.clone(),
                                    ))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Click to use this template");
                            if title.clicked() {
                                run_template(h, template, action);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Delete").clicked() {
                                        h.template_delete =
                                            Some((template.id.clone(), template.name.clone()));
                                    }
                                    if ui.small_button("Duplicate").clicked() {
                                        duplicate = Some(template.id.clone());
                                    }
                                    if ui.small_button("Edit").clicked() {
                                        h.template_form = Some(form_from(template));
                                    }
                                    if ui.small_button("Use").clicked() {
                                        run_template(h, template, action);
                                    }
                                },
                            );
                        });

                        if let Some(desc) = template.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if let Some(cat) = template.category.as_deref() {
                                if !cat.is_empty() {
                                    ui.label(crate::theme::text::muted(palette, cat));
                                }
                            }
                            let n = template.parameters.len();
                            if n > 0 {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    format!("{} param{}", n, if n == 1 { "" } else { "s" }),
                                ));
                            }
                        });
                        ui.label(
                            egui::RichText::new(first_line(&template.sql, 90))
                                .monospace()
                                .size(11.0)
                                .color(palette.muted_foreground),
                        );
                    });
                ui.add_space(6.0);
            }
        });

    if let Some(id) = duplicate {
        if let Some(original) = h.templates.iter().find(|t| t.id == id).cloned() {
            let copy = QueryTemplate::new(
                format!("{} (Copy)", original.name),
                original.description.clone(),
                original.sql.clone(),
                original.parameters.clone(),
                original.category.clone(),
                original.tags.clone(),
            );
            h.templates.push(copy);
            intents.push(Intent::SaveTemplates);
        }
    }
}

/// A zero-parameter template runs straight away; anything else opens the
/// fill-in form first.
fn run_template(h: &mut History, template: &QueryTemplate, action: &mut HistoryAction) {
    if template.parameters.is_empty() {
        *action = HistoryAction::UseSql(template.sql.clone());
        return;
    }
    let values: HashMap<String, String> = template
        .parameters
        .iter()
        .map(|p| (p.name.clone(), p.default_value.clone().unwrap_or_default()))
        .collect();
    h.template_run = Some(TemplateRun {
        errors: validate_template_values(template, &values),
        template: template.clone(),
        values,
    });
    // The freshly opened form should not shout about empty required fields
    // before the user has typed anything.
    if let Some(run) = h.template_run.as_mut() {
        run.errors.retain(|_, e| e != "This field is required");
    }
}

fn form_from(template: &QueryTemplate) -> TemplateForm {
    TemplateForm {
        id: Some(template.id.clone()),
        name: template.name.clone(),
        description: template.description.clone().unwrap_or_default(),
        category: template.category.clone().unwrap_or_default(),
        tags: template.tags.clone().unwrap_or_default().join(", "),
        sql: template.sql.clone(),
        parameters: template.parameters.clone(),
        synced_sql: template.sql.clone(),
    }
}

fn template_form_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    let Some(form) = h.template_form.as_mut() else {
        return;
    };
    let editing = form.id.is_some();
    let mut open = true;
    let mut close = false;
    let mut saved: Option<QueryTemplate> = None;

    egui::Window::new(if editing { "Edit template" } else { "New template" })
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .default_height(520.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(crate::theme::text::muted(palette, "Name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut form.name)
                            .hint_text("e.g. Find Users by Status")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);

                    ui.label(crate::theme::text::muted(palette, "Description"));
                    ui.add(
                        egui::TextEdit::singleline(&mut form.description)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(crate::theme::text::muted(palette, "Category"));
                            ui.add(
                                egui::TextEdit::singleline(&mut form.category)
                                    .desired_width(180.0),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.label(crate::theme::text::muted(
                                palette,
                                "Tags (comma separated)",
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut form.tags).desired_width(240.0),
                            );
                        });
                    });
                    ui.add_space(6.0);

                    ui.label(crate::theme::text::muted(
                        palette,
                        "SQL: use {{name}} or {{name:type}} for parameters",
                    ));
                    ui.add(
                        egui::TextEdit::multiline(&mut form.sql)
                            .code_editor()
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );

                    // Two-way live sync: the parameter list follows the SQL,
                    // keeping whatever the user configured for parameters
                    // that survived the edit.
                    if form.sql != form.synced_sql {
                        form.parameters = sync_parameters(&form.sql, &form.parameters);
                        form.synced_sql = form.sql.clone();
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::theme::text::strong(palette, "Parameters"));
                        if ui.small_button("Add").clicked() {
                            let n = form.parameters.len() + 1;
                            form.parameters.push(TemplateParameter {
                                name: format!("param_{}", n),
                                label: format!("Parameter {}", n),
                                param_type: ParamType::String,
                                default_value: None,
                                required: false,
                                description: None,
                            });
                        }
                    });

                    if form.parameters.is_empty() {
                        ui.label(crate::theme::text::muted(
                            palette,
                            "No parameters detected in the SQL.",
                        ));
                    }

                    let mut remove: Option<usize> = None;
                    for (idx, param) in form.parameters.iter_mut().enumerate() {
                        egui::Frame::none()
                            .fill(palette.muted)
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut param.name)
                                            .hint_text("name")
                                            .desired_width(140.0),
                                    );
                                    egui::ComboBox::from_id_salt(("param_type", idx))
                                        .selected_text(param.param_type.as_str())
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            for ty in ParamType::ALL {
                                                ui.selectable_value(
                                                    &mut param.param_type,
                                                    ty,
                                                    ty.as_str(),
                                                );
                                            }
                                        });
                                    ui.checkbox(&mut param.required, "Required");
                                    if ui.small_button("Remove").clicked() {
                                        remove = Some(idx);
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut param.label)
                                            .hint_text("label")
                                            .desired_width(160.0),
                                    );
                                    let mut default =
                                        param.default_value.clone().unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut default)
                                                .hint_text("default value")
                                                .desired_width(160.0),
                                        )
                                        .changed()
                                    {
                                        param.default_value =
                                            if default.is_empty() { None } else { Some(default) };
                                    }
                                });
                                let mut desc = param.description.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut desc)
                                            .hint_text("description")
                                            .desired_width(f32::INFINITY),
                                    )
                                    .changed()
                                {
                                    param.description =
                                        if desc.is_empty() { None } else { Some(desc) };
                                }
                            });
                        ui.add_space(4.0);
                    }
                    if let Some(idx) = remove {
                        form.parameters.remove(idx);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let valid =
                            !form.name.trim().is_empty() && !form.sql.trim().is_empty();
                        if ui.add_enabled(valid, egui::Button::new("Save")).clicked() {
                            let tags = parse_tags(&form.tags);
                            let mut template = QueryTemplate::new(
                                form.name.trim().to_string(),
                                Some(form.description.trim().to_string())
                                    .filter(|d| !d.is_empty()),
                                form.sql.trim().to_string(),
                                form.parameters.clone(),
                                Some(form.category.trim().to_string())
                                    .filter(|c| !c.is_empty()),
                                if tags.is_empty() { None } else { Some(tags) },
                            );
                            if let Some(id) = &form.id {
                                template.id = id.clone();
                            }
                            saved = Some(template);
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
        });

    if let Some(template) = saved {
        match h.templates.iter_mut().find(|t| t.id == template.id) {
            Some(existing) => {
                // Preserve the original creation time on an edit.
                let created = existing.created_at.clone();
                *existing = QueryTemplate {
                    created_at: created,
                    ..template
                };
            }
            None => h.templates.push(template),
        }
        intents.push(Intent::SaveTemplates);
    }
    if close || !open {
        h.template_form = None;
    }
}

fn template_run_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    _intents: &mut [Intent],
) {
    let Some(run) = h.template_run.as_mut() else {
        return;
    };
    let mut open = true;
    let mut close = false;

    egui::Window::new(format!("Run: {}", run.template.name))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for param in &run.template.parameters {
                        let label = if param.label.is_empty() {
                            param.name.clone()
                        } else {
                            param.label.clone()
                        };
                        ui.horizontal(|ui| {
                            ui.label(crate::theme::text::muted(palette, label));
                            if param.required {
                                ui.colored_label(palette.destructive, "*");
                            }
                        });
                        if let Some(desc) = param.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }

                        let value = run.values.entry(param.name.clone()).or_default();
                        let changed = match param.param_type {
                            ParamType::Text => ui
                                .add(
                                    egui::TextEdit::multiline(value)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            ParamType::Boolean => {
                                let mut picked = false;
                                egui::ComboBox::from_id_salt(("run_bool", &param.name))
                                    .selected_text(if value.is_empty() {
                                        "Select...".to_string()
                                    } else {
                                        value.clone()
                                    })
                                    .width(160.0)
                                    .show_ui(ui, |ui| {
                                        for option in ["true", "false"] {
                                            if ui
                                                .selectable_label(value == option, option)
                                                .clicked()
                                            {
                                                *value = option.to_string();
                                                picked = true;
                                            }
                                        }
                                    });
                                picked
                            }
                            ParamType::Date => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .hint_text("YYYY-MM-DD")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            ParamType::Datetime => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .hint_text("YYYY-MM-DD HH:MM:SS")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            _ => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                        };

                        if changed {
                            // Live per-field validation, exactly the web
                            // shell's behaviour: type errors appear as you
                            // type, "required" only on submit.
                            let current = run.values.get(&param.name).cloned().unwrap_or_default();
                            match validate_parameter_value(&current, param.param_type) {
                                Ok(()) => {
                                    run.errors.remove(&param.name);
                                }
                                Err(e) => {
                                    run.errors.insert(param.name.clone(), e);
                                }
                            }
                        }

                        if let Some(err) = run.errors.get(&param.name) {
                            ui.colored_label(
                                palette.destructive,
                                egui::RichText::new(err).size(11.0),
                            );
                        }
                        ui.add_space(6.0);
                    }

                    ui.separator();
                    ui.label(crate::theme::text::muted(palette, "Preview"));
                    egui::Frame::none()
                        .fill(palette.muted)
                        .rounding(egui::Rounding::same(4.0))
                        .inner_margin(egui::Margin::same(6.0))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(preview_sql(&run.template, &run.values))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.foreground),
                                )
                                .wrap(),
                            );
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Use SQL").clicked() {
                            let errors = validate_template_values(&run.template, &run.values);
                            if errors.is_empty() {
                                *action = HistoryAction::UseSql(render_template(
                                    &run.template,
                                    &run.values,
                                ));
                                close = true;
                            } else {
                                run.errors = errors;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
        });

    if close || !open {
        h.template_run = None;
    }
}

fn import_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    if !h.show_import {
        return;
    }
    let mut open = true;
    egui::Window::new("Import templates")
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Path to a templates JSON file. Templates whose name already exists are skipped.",
            ));
            ui.add(
                egui::TextEdit::singleline(&mut h.import_path)
                    .hint_text("/path/to/query-templates.json")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !h.import_path.trim().is_empty(),
                        egui::Button::new("Import"),
                    )
                    .clicked()
                {
                    intents.push(Intent::ImportTemplates(h.import_path.clone()));
                }
                if ui.button("Cancel").clicked() {
                    h.show_import = false;
                }
            });
        });
    if !open {
        h.show_import = false;
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, conn: &str, db: &str, at: &str, success: bool) -> QueryHistory {
        let mut h = QueryHistory::new(
            conn.to_string(),
            format!("{} name", conn),
            db.to_string(),
            format!("SELECT {};", id),
            at.to_string(),
        );
        h.id = id.to_string();
        h.success = success;
        h
    }

    fn snippet(name: &str, query: &str, desc: Option<&str>, tags: &[&str]) -> QuerySnippet {
        QuerySnippet::new(
            name.to_string(),
            query.to_string(),
            desc.map(str::to_string),
            if tags.is_empty() {
                None
            } else {
                Some(tags.iter().map(|t| t.to_string()).collect())
            },
        )
    }

    fn template(name: &str, sql: &str, category: Option<&str>) -> QueryTemplate {
        QueryTemplate::new(
            name.to_string(),
            None,
            sql.to_string(),
            sync_parameters(sql, &[]),
            category.map(str::to_string),
            None,
        )
    }

    // --- history filtering -------------------------------------------------

    #[test]
    fn history_filters_by_connection_and_sorts_newest_first() {
        let all = vec![
            entry("1", "a", "db1", "2025-01-01T10:00:00Z", true),
            entry("2", "b", "db1", "2025-01-01T11:00:00Z", true),
            entry("3", "a", "db1", "2025-01-01T12:00:00Z", true),
        ];
        let got = filter_history(&all, Some("a"), None, 50);
        assert_eq!(
            got.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
            vec!["3", "1"]
        );
    }

    #[test]
    fn history_without_connection_returns_everything() {
        let all = vec![
            entry("1", "a", "db1", "2025-01-01T10:00:00Z", true),
            entry("2", "b", "db2", "2025-01-01T11:00:00Z", true),
        ];
        assert_eq!(filter_history(&all, None, None, 50).len(), 2);
    }

    #[test]
    fn history_limit_truncates_to_the_newest_entries() {
        let all: Vec<QueryHistory> = (0..10)
            .map(|i| entry(&i.to_string(), "a", "db1", &format!("2025-01-01T10:0{}:00Z", i), true))
            .collect();
        let got = filter_history(&all, Some("a"), None, 3);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].id, "9");
        assert_eq!(got[2].id, "7");
    }

    #[test]
    fn history_database_filter_runs_after_the_limit() {
        // Two db2 rows, then three newer db1 rows. With limit 3 the web
        // shell kept only the three newest (all db1) and then filtered for
        // db2, leaving nothing. That quirk is deliberate parity.
        let all = vec![
            entry("1", "a", "db2", "2025-01-01T10:00:00Z", true),
            entry("2", "a", "db2", "2025-01-01T10:01:00Z", true),
            entry("3", "a", "db1", "2025-01-01T10:02:00Z", true),
            entry("4", "a", "db1", "2025-01-01T10:03:00Z", true),
            entry("5", "a", "db1", "2025-01-01T10:04:00Z", true),
        ];
        assert!(filter_history(&all, Some("a"), Some("db2"), 3).is_empty());
        assert_eq!(filter_history(&all, Some("a"), Some("db2"), 50).len(), 2);
    }

    #[test]
    fn history_limits_are_the_four_the_web_shell_offered() {
        assert_eq!(HISTORY_LIMITS, [25, 50, 100, 500]);
        assert!(HISTORY_LIMITS.contains(&History::default().limit));
    }

    // --- formatting --------------------------------------------------------

    #[test]
    fn relative_time_buckets() {
        let now: DateTime<Utc> = "2025-01-10T12:00:00Z".parse().unwrap();
        assert_eq!(format_relative("2025-01-10T11:59:30Z", now), "Just now");
        assert_eq!(format_relative("2025-01-10T11:55:00Z", now), "5m ago");
        assert_eq!(format_relative("2025-01-10T09:00:00Z", now), "3h ago");
        assert_eq!(format_relative("2025-01-08T12:00:00Z", now), "2d ago");
        // Older than a week falls back to an absolute stamp, not "Nd ago".
        let old = format_relative("2024-12-01T12:00:00Z", now);
        assert!(old.starts_with("2024-12-01") || old.starts_with("2024-11-30"));
    }

    #[test]
    fn relative_time_keeps_an_unparseable_stamp() {
        let now: DateTime<Utc> = "2025-01-10T12:00:00Z".parse().unwrap();
        assert_eq!(format_relative("not a date", now), "not a date");
    }

    #[test]
    fn execution_time_formatting() {
        assert_eq!(format_execution_time(None), "N/A");
        assert_eq!(format_execution_time(Some(0)), "N/A");
        assert_eq!(format_execution_time(Some(12)), "12ms");
        assert_eq!(format_execution_time(Some(999)), "999ms");
        assert_eq!(format_execution_time(Some(1000)), "1.00s");
        assert_eq!(format_execution_time(Some(1234)), "1.23s");
    }

    #[test]
    fn row_count_formatting() {
        assert_eq!(format_row_count(0), "0");
        assert_eq!(format_row_count(999), "999");
        assert_eq!(format_row_count(1000), "1,000");
        assert_eq!(format_row_count(1234567), "1,234,567");
    }

    #[test]
    fn first_line_truncates_and_skips_blank_leading_lines() {
        assert_eq!(first_line("\n\nSELECT 1;", 40), "SELECT 1;");
        assert_eq!(first_line("SELECT 1;\nSELECT 2;", 40), "SELECT 1;");
        assert_eq!(first_line("abcdef", 3), "abc…");
    }

    // --- snippets ----------------------------------------------------------

    #[test]
    fn snippets_search_covers_name_query_and_description() {
        let list = vec![
            snippet("Active users", "SELECT * FROM users", Some("live only"), &["users"]),
            snippet("Orders", "SELECT * FROM orders", None, &["sales"]),
        ];
        assert_eq!(filter_snippets(&list, "active", None).len(), 1);
        assert_eq!(filter_snippets(&list, "orders", None).len(), 1);
        assert_eq!(filter_snippets(&list, "live", None).len(), 1);
        assert_eq!(filter_snippets(&list, "select", None).len(), 2);
        assert_eq!(filter_snippets(&list, "nothing", None).len(), 0);
    }

    #[test]
    fn snippets_tag_filter_is_exact_and_combines_with_search() {
        let list = vec![
            snippet("Active users", "SELECT 1", None, &["users", "live"]),
            snippet("All users", "SELECT 2", None, &["users"]),
            snippet("Orders", "SELECT 3", None, &["sales"]),
        ];
        assert_eq!(filter_snippets(&list, "", Some("users")).len(), 2);
        assert_eq!(filter_snippets(&list, "active", Some("users")).len(), 1);
        assert_eq!(filter_snippets(&list, "", Some("user")).len(), 0);
    }

    #[test]
    fn snippet_tags_are_deduplicated_and_sorted() {
        let list = vec![
            snippet("a", "SELECT 1", None, &["zeta", "alpha"]),
            snippet("b", "SELECT 2", None, &["alpha"]),
        ];
        assert_eq!(snippet_tags(&list), vec!["alpha", "zeta"]);
    }

    #[test]
    fn tags_input_is_trimmed_and_empties_dropped() {
        assert_eq!(parse_tags(" a , b ,, c "), vec!["a", "b", "c"]);
        assert!(parse_tags("  ,  ").is_empty());
    }

    // --- placeholders ------------------------------------------------------

    #[test]
    fn scans_typed_and_untyped_placeholders() {
        let found = scan_placeholders("SELECT * FROM t WHERE a = {{a}} AND b = {{b:integer}}");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "a");
        assert_eq!(found[0].declared_type, None);
        assert_eq!(found[1].name, "b");
        assert_eq!(found[1].declared_type.as_deref(), Some("integer"));
    }

    #[test]
    fn ignores_malformed_placeholders() {
        // A leading digit, an empty type and an unclosed brace are all
        // rejected by the web shell's regex, so they are rejected here.
        assert!(scan_placeholders("{{1bad}}").is_empty());
        assert!(scan_placeholders("{{bad:}}").is_empty());
        assert!(scan_placeholders("{{bad}").is_empty());
        assert!(scan_placeholders("{ {spaced} }").is_empty());
        assert!(scan_placeholders("{{}}").is_empty());
    }

    #[test]
    fn extraction_deduplicates_and_keeps_the_first_type() {
        let defs = extract_parameter_definitions("{{id:integer}} {{name}} {{id}}");
        assert_eq!(
            defs,
            vec![
                ("id".to_string(), ParamType::Integer),
                ("name".to_string(), ParamType::String),
            ]
        );
    }

    #[test]
    fn unknown_declared_type_falls_back_to_string() {
        let defs = extract_parameter_definitions("{{x:banana}}");
        assert_eq!(defs, vec![("x".to_string(), ParamType::String)]);
    }

    #[test]
    fn substitute_leaves_declined_placeholders_intact() {
        let out = substitute("a {{one}} b {{two}} c", |p| {
            (p.name == "one").then(|| "1".to_string())
        });
        assert_eq!(out, "a 1 b {{two}} c");
    }

    // --- template values ---------------------------------------------------

    #[test]
    fn sql_formatting_quotes_and_escapes() {
        assert_eq!(format_parameter_for_sql("o'brien", ParamType::String), "'o''brien'");
        assert_eq!(format_parameter_for_sql("42", ParamType::Integer), "42");
        assert_eq!(format_parameter_for_sql("1.5", ParamType::Decimal), "1.5");
        assert_eq!(format_parameter_for_sql("yes", ParamType::Boolean), "true");
        assert_eq!(format_parameter_for_sql("no", ParamType::Boolean), "false");
        assert_eq!(
            format_parameter_for_sql("2025-01-01", ParamType::Date),
            "'2025-01-01'"
        );
    }

    #[test]
    fn injection_attempt_is_quoted_not_executed() {
        let out = format_parameter_for_sql("'; DROP TABLE users; --", ParamType::String);
        assert_eq!(out, "'''; DROP TABLE users; --'");
    }

    #[test]
    fn value_validation_matches_the_web_shell() {
        assert!(validate_parameter_value("", ParamType::Integer).is_ok());
        assert!(validate_parameter_value("-12", ParamType::Integer).is_ok());
        assert!(validate_parameter_value("1.2", ParamType::Integer).is_err());
        assert!(validate_parameter_value("-1.", ParamType::Decimal).is_ok());
        assert!(validate_parameter_value(".5", ParamType::Decimal).is_err());
        assert!(validate_parameter_value("YES", ParamType::Boolean).is_ok());
        assert!(validate_parameter_value("maybe", ParamType::Boolean).is_err());
        assert!(validate_parameter_value("2025-01-01", ParamType::Date).is_ok());
        assert!(validate_parameter_value("2025-1-1", ParamType::Date).is_err());
        assert!(validate_parameter_value("2025-01-01 10:30", ParamType::Datetime).is_ok());
        assert!(validate_parameter_value("2025-01-01T10:30:00Z", ParamType::Datetime).is_ok());
        assert!(validate_parameter_value("2025-01-01", ParamType::Datetime).is_err());
        assert!(validate_parameter_value("anything", ParamType::Text).is_ok());
    }

    #[test]
    fn required_empty_and_bad_type_both_reported() {
        let t = template("t", "SELECT {{a}} , {{b:integer}}", None);
        let mut values = HashMap::new();
        values.insert("b".to_string(), "oops".to_string());
        let errors = validate_template_values(&t, &values);
        assert_eq!(errors.get("a").map(String::as_str), Some("This field is required"));
        assert_eq!(
            errors.get("b").map(String::as_str),
            Some("Must be a valid integer")
        );
    }

    #[test]
    fn render_uses_defaults_for_blank_fields() {
        let mut t = template("t", "SELECT * FROM u WHERE id = {{id:integer}} AND n = {{n}}", None);
        t.parameters[0].default_value = Some("7".to_string());
        let mut values = HashMap::new();
        values.insert("n".to_string(), "bob".to_string());
        assert_eq!(
            render_template(&t, &values),
            "SELECT * FROM u WHERE id = 7 AND n = 'bob'"
        );
    }

    #[test]
    fn render_substitutes_every_occurrence() {
        let t = template("t", "{{x:integer}} + {{x:integer}}", None);
        let mut values = HashMap::new();
        values.insert("x".to_string(), "2".to_string());
        assert_eq!(render_template(&t, &values), "2 + 2");
    }

    #[test]
    fn preview_keeps_unfilled_placeholders_visible() {
        let t = template("t", "SELECT {{a}} , {{b:integer}}", None);
        let mut values = HashMap::new();
        values.insert("a".to_string(), "x".to_string());
        assert_eq!(preview_sql(&t, &values), "SELECT 'x' , {{b}}");
    }

    // --- parameter sync ----------------------------------------------------

    #[test]
    fn sync_creates_parameters_with_humanised_labels() {
        let params = sync_parameters("SELECT {{user_id:integer}}", &[]);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "user_id");
        assert_eq!(params[0].label, "User Id");
        assert_eq!(params[0].param_type, ParamType::Integer);
        assert!(params[0].required);
    }

    #[test]
    fn sync_preserves_settings_and_drops_removed_parameters() {
        let existing = vec![TemplateParameter {
            name: "a".to_string(),
            label: "Custom".to_string(),
            param_type: ParamType::String,
            default_value: Some("d".to_string()),
            required: false,
            description: Some("why".to_string()),
        }];
        let params = sync_parameters("SELECT {{a:integer}}, {{b}}", &existing);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].label, "Custom");
        assert_eq!(params[0].default_value.as_deref(), Some("d"));
        assert!(!params[0].required);
        // The type follows the SQL, since that is where it is written.
        assert_eq!(params[0].param_type, ParamType::Integer);
        assert_eq!(params[1].name, "b");

        let params = sync_parameters("SELECT 1", &params);
        assert!(params.is_empty());
    }

    // --- templates list ----------------------------------------------------

    #[test]
    fn templates_filter_by_category_then_search() {
        let list = vec![
            template("Users by status", "SELECT 1", Some("Basics")),
            template("Table sizes", "SELECT 2", Some("Admin")),
            template("Uncategorised", "SELECT 3", None),
        ];
        assert_eq!(filter_templates(&list, "", None).len(), 3);
        assert_eq!(filter_templates(&list, "", Some("Basics")).len(), 1);
        assert_eq!(filter_templates(&list, "sizes", None).len(), 1);
        assert_eq!(filter_templates(&list, "sizes", Some("Basics")).len(), 0);
    }

    #[test]
    fn template_search_matches_tags_and_description() {
        let mut t = template("Anything", "SELECT 1", None);
        t.description = Some("counts the rows".to_string());
        t.tags = Some(vec!["maintenance".to_string()]);
        let list = vec![t];
        assert_eq!(filter_templates(&list, "counts", None).len(), 1);
        assert_eq!(filter_templates(&list, "MAINTEN", None).len(), 1);
        // The SQL body is deliberately not searched.
        assert_eq!(filter_templates(&list, "select", None).len(), 0);
    }

    #[test]
    fn categories_are_deduplicated_sorted_and_skip_blanks() {
        let mut blank = template("blank", "SELECT 1", Some(""));
        blank.category = Some(String::new());
        let list = vec![
            template("a", "SELECT 1", Some("Zeta")),
            template("b", "SELECT 2", Some("Alpha")),
            template("c", "SELECT 3", Some("Alpha")),
            blank,
        ];
        assert_eq!(template_categories(&list), vec!["Alpha", "Zeta"]);
    }

    #[test]
    fn template_list_stays_internally_consistent() {
        // Every template's declared parameters must match what its SQL
        // actually contains, or the run dialog asks for values it will never
        // substitute.
        let list = vec![
            template("no params", "SELECT 1;", Some("Basics")),
            template(
                "one param",
                "SELECT * FROM users WHERE id = {{id:integer}};",
                Some("Basics"),
            ),
        ];
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for t in &list {
            assert!(ids.insert(t.id.clone()), "duplicate template id");
            assert!(names.insert(t.name.clone()), "duplicate template name");
            assert!(!t.sql.trim().is_empty());
            let declared: Vec<&str> = t.parameters.iter().map(|p| p.name.as_str()).collect();
            let found: Vec<String> = extract_parameter_definitions(&t.sql)
                .into_iter()
                .map(|(n, _)| n)
                .collect();
            assert_eq!(declared, found.iter().map(String::as_str).collect::<Vec<_>>());
        }
    }

    // --- import ------------------------------------------------------------

    #[test]
    fn import_skips_existing_names_and_reassigns_ids() {
        let existing = vec![template("Keep", "SELECT 1", None)];
        let incoming = vec![
            template("Keep", "SELECT 2", None),
            template("Fresh", "SELECT 3", None),
        ];
        let incoming_ids: Vec<String> = incoming.iter().map(|t| t.id.clone()).collect();
        let added = merge_imported(&existing, incoming);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "Fresh");
        assert!(!incoming_ids.contains(&added[0].id));
        assert!(!added[0].updated_at.is_empty());
    }

    #[test]
    fn import_deduplicates_within_the_incoming_batch() {
        let incoming = vec![
            template("Same", "SELECT 1", None),
            template("Same", "SELECT 2", None),
        ];
        assert_eq!(merge_imported(&[], incoming).len(), 1);
    }

    // --- serialisation -----------------------------------------------------

    #[test]
    fn templates_round_trip_through_the_web_shells_json_shape() {
        let raw = r#"[{
            "id": "abc",
            "name": "Find user",
            "description": "by id",
            "sql": "SELECT * FROM users WHERE id = {{user_id:integer}}",
            "parameters": [{
                "name": "user_id",
                "label": "User Id",
                "type": "integer",
                "defaultValue": "1",
                "required": true
            }],
            "category": "Basics",
            "tags": ["users"],
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-02T00:00:00Z"
        }]"#;
        let parsed: Vec<QueryTemplate> = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].parameters[0].param_type, ParamType::Integer);
        assert_eq!(parsed[0].parameters[0].default_value.as_deref(), Some("1"));

        let out = serde_json::to_string(&parsed).unwrap();
        assert!(out.contains("\"defaultValue\":\"1\""));
        assert!(out.contains("\"type\":\"integer\""));
        let again: Vec<QueryTemplate> = serde_json::from_str(&out).unwrap();
        assert_eq!(again, parsed);
    }

    #[test]
    fn an_unknown_parameter_type_decodes_rather_than_failing() {
        let raw = r#"[{"name":"x","label":"X","type":"banana","required":false}]"#;
        let parsed: Vec<TemplateParameter> = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed[0].param_type, ParamType::String);
    }
}
