//! Query history, saved snippets and query templates: the UI-independent part.
//!
//! Lifted from the egui `history` screen so the Slint shell reuses it, tests
//! and all. Parity target: the React `HistoryPanel`, `SnippetSidebar` and
//! `TemplatesPanel`.
//!
//! History and snippets live in `AppState` (`history.json`, `snippets.json`).
//! Templates had no backend at all in the web shell: `useQueryTemplates`
//! kept them in `localStorage`, which does not exist here, so they are
//! persisted to `templates.json` in the same application data directory
//! through `dbhive_core::storage::JsonStore`.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Local, Utc};
use dbhive_core::models::{QueryHistory, QuerySnippet};
use dbhive_core::storage::JsonStore;
use serde::{Deserialize, Serialize};

/// Row-count choices offered by the history limit selector.
///
/// Same four values the web shell's `<Select>` had; changing them changes
/// what an existing user sees, so they are a constant rather than a range.
pub const HISTORY_LIMITS: [usize; 4] = [25, 50, 100, 500];

/// Name of the JSON store templates are persisted to.
pub const TEMPLATE_STORE: &str = "templates.json";
pub const TEMPLATE_KEY: &str = "templates";

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
    pub fn new(
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
                || s.description
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
                || t.description
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
    b[11..13].iter().all(u8::is_ascii_digit)
        && b[13] == b':'
        && b[14..16].iter().all(u8::is_ascii_digit)
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
        .map(
            |(name, ty)| match existing.iter().find(|p| p.name == name) {
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
            },
        )
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
pub fn parse_tags(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// First line of a statement, for a one-line summary.
pub fn first_line(sql: &str, max: usize) -> String {
    let line = sql
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
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

pub fn load_templates() -> Result<Vec<QueryTemplate>, String> {
    let store = JsonStore::open(TEMPLATE_STORE).map_err(|e| e.to_string())?;
    match store.get(TEMPLATE_KEY) {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| format!("templates.json could not be read: {}", e)),
    }
}

pub fn save_templates(templates: &[QueryTemplate]) -> Result<(), String> {
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
pub fn export_path() -> Result<std::path::PathBuf, String> {
    let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y-%m-%d");
    Ok(dir.join(format!("query-templates-{}.json", stamp)))
}

pub fn export_templates_now(templates: &[QueryTemplate]) -> Result<std::path::PathBuf, String> {
    let path = export_path()?;
    let json = serde_json::to_string_pretty(templates).map_err(|e| e.to_string())?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Could not write {}: {}", path.display(), e))?;
    Ok(path)
}

pub fn import_templates_now(path: &str) -> Result<Vec<QueryTemplate>, String> {
    if path.is_empty() {
        return Err("Enter the path of a templates JSON file".to_string());
    }
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {}", path, e))?;
    serde_json::from_str::<Vec<QueryTemplate>>(&raw)
        .map_err(|e| format!("{} is not a template export: {}", path, e))
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
            .map(|i| {
                entry(
                    &i.to_string(),
                    "a",
                    "db1",
                    &format!("2025-01-01T10:0{}:00Z", i),
                    true,
                )
            })
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
            snippet(
                "Active users",
                "SELECT * FROM users",
                Some("live only"),
                &["users"],
            ),
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
        assert_eq!(
            format_parameter_for_sql("o'brien", ParamType::String),
            "'o''brien'"
        );
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
        assert_eq!(
            errors.get("a").map(String::as_str),
            Some("This field is required")
        );
        assert_eq!(
            errors.get("b").map(String::as_str),
            Some("Must be a valid integer")
        );
    }

    #[test]
    fn render_uses_defaults_for_blank_fields() {
        let mut t = template(
            "t",
            "SELECT * FROM u WHERE id = {{id:integer}} AND n = {{n}}",
            None,
        );
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
            assert_eq!(
                declared,
                found.iter().map(String::as_str).collect::<Vec<_>>()
            );
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
