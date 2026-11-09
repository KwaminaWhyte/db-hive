//! The AI assistant panel's session and what it shows.
//!
//! `ai_assistant` holds the pieces (transcript types, readiness rules, request
//! planning). This module is the state machine around them: which provider is
//! chosen, what its status and model list came back as, what schema context is
//! loaded, what is in flight, and the transcript. It has no toolkit types and
//! starts no work; the shell calls the methods when something lands or is
//! clicked, runs the [`Dispatch`] it is handed, and draws [`AiPanel::view`].
//!
//! Three rules shape it:
//!
//! * **Nothing is sent that the user cannot see.** The schema context that goes
//!   into a prompt is the exact text [`AiPanel::context_text`] returns, and the
//!   user can switch it off. Row data is never part of any request.
//! * **Nothing runs.** A statement in an answer is offered to the editor
//!   ([`apply_text`]); running it is a separate act in the editor, through the
//!   normal run path and its destructive-statement guard.
//! * **A late answer does not land on the wrong provider.** Results carry the
//!   provider they were asked of, and are dropped if the user has moved on.

use dbhive_core::ai::{AiModel, AiProviderType};
use dbhive_core::models::DbDriver;
use dbhive_core::service_ai::{self, AiResponse, AiSettings, AiTask, ProviderStatus};

use crate::ai_assistant::{
    empty_state, format_duration, model_label, plan_request, selected_model_text, status_tone,
    EmptyState, Kind, Readiness, RequestInputs, RequestPlan, StatusTone, Turn, Who,
};
use crate::history_panel::append_sql;

// ===========================================================================
// Small pure helpers
// ===========================================================================

/// What a request of `kind` puts on the wire, in words, so the user can see it
/// before pressing the button. Row data is never part of any of them.
pub fn request_disclosure(kind: Kind, include_schema: bool) -> &'static str {
    match (kind, include_schema) {
        (Kind::Generate, true) => "Your request and the schema context.",
        (Kind::Generate, false) => {
            "Your request only. Without the schema the model guesses table names."
        }
        (Kind::Explain, _) => "The query in the editor. No schema.",
        (Kind::Optimize, true) => "The query in the editor and the schema context.",
        (Kind::Optimize, false) => "The query in the editor.",
        (Kind::Fix, true) => {
            "The query, the error text as the database reported it, and the schema context."
        }
        (Kind::Fix, false) => "The query and the error text as the database reported it.",
        (Kind::Chat, true) => "Your message, the earlier turns and the schema context.",
        (Kind::Chat, false) => "Your message and the earlier turns.",
    }
}

/// How the schema context is summarised: the number of tables and its size,
/// counted from the exact text that would be sent.
pub fn context_summary(context: &str) -> String {
    let tables = context
        .lines()
        .filter(|l| !l.starts_with("  - ") && l.ends_with(':') && l.contains('.'))
        .count();
    let bytes = context.len();
    let size = if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    };
    let noun = if tables == 1 { "table" } else { "tables" };
    format!("{} {}, {}", tables, noun, size)
}

/// The database name that goes into the context header. A SQLite database is
/// named by its file path, which has no business in a prompt: send the file
/// name.
pub fn context_database_label(driver: &DbDriver, database: &str) -> String {
    match driver {
        DbDriver::Sqlite => std::path::Path::new(database)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(database)
            .to_string(),
        _ => database.to_string(),
    }
}

/// A reason the base URL cannot be used, or `None`.
pub fn base_url_problem(url: &str) -> Option<&'static str> {
    let u = url.trim();
    if u.is_empty() {
        return Some("Enter a base URL.");
    }
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Some("The base URL must start with http:// or https://.");
    }
    if u.contains(char::is_whitespace) {
        return Some("The base URL cannot contain spaces.");
    }
    None
}

/// A warning for a URL that is allowed but worth a second look: an API key
/// sent over plain http to a machine that is not this one.
pub fn base_url_warning(url: &str, needs_key: bool) -> Option<&'static str> {
    let u = url.trim().to_ascii_lowercase();
    if !needs_key || !u.starts_with("http://") {
        return None;
    }
    let host = u["http://".len()..].split(['/', ':']).next().unwrap_or("");
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        None
    } else {
        Some("This URL is not encrypted. Your API key would be sent in the clear.")
    }
}

/// The part of an answer that is not the statement offered for the editor.
///
/// The statement is shown in its own block with its own buttons, so the prose
/// is what is left once that block is taken out. A bare statement leaves
/// nothing.
pub fn answer_prose(content: &str, sql: Option<&str>) -> String {
    let Some(sql) = sql else {
        return content.trim().to_string();
    };
    let mut kept: Vec<&str> = Vec::new();
    let mut block: Option<Vec<&str>> = None;
    let mut removed = false;

    fn settle<'a>(block: Vec<&'a str>, kept: &mut Vec<&'a str>, removed: &mut bool, sql: &str) {
        let inner: Vec<&str> = block
            .iter()
            .copied()
            .skip(1)
            .filter(|l| !l.trim_start().starts_with("```"))
            .collect();
        if !*removed && inner.join("\n").trim() == sql {
            *removed = true;
        } else {
            kept.extend(block);
        }
    }

    for line in content.lines() {
        let is_fence = line.trim_start().starts_with("```");
        match (block.as_mut(), is_fence) {
            (None, true) => block = Some(vec![line]),
            (None, false) => kept.push(line),
            (Some(b), false) => b.push(line),
            (Some(b), true) => {
                b.push(line);
                let done = block.take().unwrap_or_default();
                settle(done, &mut kept, &mut removed, sql);
            }
        }
    }
    if let Some(b) = block.take() {
        settle(b, &mut kept, &mut removed, sql);
    }

    if !removed {
        // A bare statement: the whole answer is the statement.
        return if content.trim() == sql {
            String::new()
        } else {
            content.trim().to_string()
        };
    }
    // Collapse the blank lines the removed block left behind.
    let mut out: Vec<&str> = Vec::new();
    for line in kept {
        if line.trim().is_empty() && out.last().is_none_or(|l| l.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

/// An error as the user should read it: `DbError::AiError` renders as
/// "AI error: OpenAI API error: ...", and the first prefix says nothing the
/// second does not.
pub fn error_text(message: &str) -> String {
    message
        .trim()
        .strip_prefix("AI error: ")
        .unwrap_or(message.trim())
        .to_string()
}

/// A request as the transcript shows it: the statement clipped to two lines,
/// and for a fix the error that went with it, so what was sent can be read
/// without scrolling past the whole statement. The full text stays in the
/// transcript.
pub fn user_turn_body(content: &str) -> String {
    match content.split_once("\n\nError: ") {
        Some((sql, error)) => format!("{}\nError: {}", clip_lines(sql, 2), clip_lines(error, 2)),
        None => clip_lines(content, 2),
    }
}

/// Keep at most `max` lines of `text`, ending with an ellipsis when cut.
pub fn clip_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max {
        return text.trim_end().to_string();
    }
    let mut out = lines[..max].join("\n");
    out.push_str("\n…");
    out
}

/// Where a statement from an answer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    /// Overwrite the active query tab's text.
    Replace,
    /// Add it below what is there, after a blank line.
    Append,
    /// A new query tab.
    NewTab,
}

/// The editor text after applying `sql` to a query tab that holds `existing`.
/// `NewTab` starts from nothing, so the text is the statement alone.
pub fn apply_text(mode: Apply, existing: &str, sql: &str) -> String {
    match mode {
        Apply::Replace | Apply::NewTab => sql.to_string(),
        Apply::Append => append_sql(existing, sql),
    }
}

// ===========================================================================
// State
// ===========================================================================

/// How a list that is fetched in the background stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// A request the shell should run. Built from the panel's own state, so what
/// is sent is exactly what the panel showed.
#[derive(Debug, Clone)]
pub struct Dispatch {
    pub kind: Kind,
    pub provider: AiProviderType,
    pub settings: AiSettings,
    pub model: Option<String>,
    pub task: AiTask,
}

/// The editor's side of a request, read when a button is pressed.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub current_sql: &'a str,
    pub last_error: Option<&'a str>,
    pub prompt: &'a str,
    /// The chat box: a free-form question about the database.
    pub chat: &'a str,
}

#[derive(Debug, Clone)]
pub struct AiPanel {
    settings: AiSettings,
    settings_loaded: bool,
    provider: AiProviderType,
    status: Option<ProviderStatus>,
    models: Vec<AiModel>,
    models_load: Load,
    model: String,
    turns: Vec<Turn>,
    pending: Option<Kind>,
    notice: Option<String>,
    context: String,
    /// "2 tables, 208 bytes", counted once when the context lands.
    context_stats: String,
    context_load: Load,
    context_key: Option<(String, String)>,
    include_schema: bool,
    show_context: bool,
    show_settings: bool,
}

impl Default for AiPanel {
    fn default() -> Self {
        let settings = AiSettings::default();
        let provider = settings.active_provider;
        let model = settings.get(provider).default_model.clone();
        Self {
            settings,
            settings_loaded: false,
            provider,
            status: None,
            models: Vec::new(),
            models_load: Load::Idle,
            model,
            turns: Vec::new(),
            pending: None,
            notice: None,
            context: String::new(),
            context_stats: String::new(),
            context_load: Load::Idle,
            context_key: None,
            include_schema: true,
            show_context: false,
            show_settings: false,
        }
    }
}

impl AiPanel {
    // -- Settings and provider ------------------------------------------------

    pub fn settings_loaded(&self) -> bool {
        self.settings_loaded
    }

    /// The saved settings came back. An unreadable file leaves the defaults in
    /// place and says so; the panel still works.
    pub fn settings_result(&mut self, result: Result<AiSettings, String>) {
        self.settings_loaded = true;
        match result {
            Ok(settings) => {
                self.provider = settings.active_provider;
                self.model = settings.get(self.provider).default_model.clone();
                self.settings = settings;
            }
            Err(e) => self.notice = Some(e),
        }
    }

    pub fn settings(&self) -> &AiSettings {
        &self.settings
    }

    pub fn provider(&self) -> AiProviderType {
        self.provider
    }

    pub fn needs_key(&self) -> bool {
        service_ai::provider_info(self.provider).needs_api_key()
    }

    /// Switch provider. Returns whether anything changed; if so the shell
    /// persists the choice and checks the new provider's status. Whatever was
    /// in flight is abandoned: its answer would be for the old provider.
    pub fn change_provider(&mut self, provider: AiProviderType) -> bool {
        if provider == self.provider {
            return false;
        }
        self.provider = provider;
        self.settings.active_provider = provider;
        self.reset_provider_state();
        true
    }

    fn reset_provider_state(&mut self) {
        self.status = None;
        self.models.clear();
        self.models_load = Load::Idle;
        self.model = self.settings.get(self.provider).default_model.clone();
        self.pending = None;
        self.notice = None;
    }

    /// The key was saved or removed: the old status and models no longer hold.
    pub fn key_changed(&mut self) {
        self.status = None;
        self.models.clear();
        self.models_load = Load::Idle;
        self.model = self.settings.get(self.provider).default_model.clone();
        self.notice = None;
    }

    /// Validate and apply a new base URL for the current provider. Returns the
    /// settings to persist.
    pub fn set_base_url(&mut self, url: &str) -> Result<AiSettings, String> {
        if let Some(problem) = base_url_problem(url) {
            return Err(problem.to_string());
        }
        self.settings.get_mut(self.provider).base_url =
            url.trim().trim_end_matches('/').to_string();
        self.key_changed();
        Ok(self.settings.clone())
    }

    pub fn base_url(&self) -> &str {
        &self.settings.get(self.provider).base_url
    }

    // -- Status and models ------------------------------------------------------

    pub fn begin_status_check(&mut self) {
        self.status = None;
    }

    /// A status check landed. Ignored when it was for another provider.
    pub fn status_result(&mut self, provider: AiProviderType, status: ProviderStatus) {
        if provider != self.provider {
            return;
        }
        self.status = Some(status);
        if !(status.available && status.configured) {
            self.models.clear();
            self.models_load = Load::Idle;
        }
    }

    pub fn status(&self) -> Option<ProviderStatus> {
        self.status
    }

    fn usable(&self) -> bool {
        self.status.is_some_and(|s| s.available && s.configured)
    }

    /// The provider answers and has a key, and no model list has been asked
    /// for yet: the shell should fetch one.
    pub fn wants_models(&self) -> bool {
        self.usable() && self.models_load == Load::Idle
    }

    pub fn begin_models(&mut self) {
        self.models_load = Load::Loading;
    }

    pub fn models_result(
        &mut self,
        provider: AiProviderType,
        result: Result<Vec<AiModel>, String>,
    ) {
        if provider != self.provider {
            return;
        }
        match result {
            Ok(models) => {
                let default = self.settings.get(provider).default_model.clone();
                self.model =
                    service_ai::preferred_model(&models, &self.model, &default).unwrap_or_default();
                self.models = models;
                self.models_load = Load::Ready;
            }
            Err(e) => self.models_load = Load::Failed(e),
        }
    }

    /// Forget the list so the next status check asks for it again.
    pub fn refresh_models(&mut self) {
        self.models.clear();
        self.models_load = Load::Idle;
    }

    /// Pick the model at `index` of the list. The choice becomes the
    /// provider's saved default, so it is the one offered next time; the
    /// settings to persist come back, or `None` when nothing changed.
    pub fn choose_model(&mut self, index: usize) -> Option<AiSettings> {
        let id = self.models.get(index)?.id.clone();
        if id == self.model {
            return None;
        }
        self.model = id.clone();
        self.settings.get_mut(self.provider).default_model = id;
        Some(self.settings.clone())
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    // -- Schema context -----------------------------------------------------------

    /// The connection and database the context was (or is being) built for.
    pub fn context_wanted(&self, key: &(String, String)) -> bool {
        self.context_key.as_ref() != Some(key)
    }

    pub fn begin_context(&mut self, key: (String, String)) {
        self.context_key = Some(key);
        self.context.clear();
        self.context_stats.clear();
        self.context_load = Load::Loading;
    }

    /// Forget the context so the next open builds it again (a reload).
    pub fn invalidate_context(&mut self) {
        self.context_key = None;
    }

    /// A context build landed. Ignored when the panel moved to another
    /// connection or database meanwhile.
    pub fn context_result(&mut self, key: &(String, String), result: Result<String, String>) {
        if self.context_key.as_ref() != Some(key) {
            return;
        }
        match result {
            Ok(text) => {
                self.context_stats = context_summary(&text);
                self.context = text;
                self.context_load = Load::Ready;
            }
            Err(e) => {
                self.context.clear();
                self.context_stats.clear();
                self.context_load = Load::Failed(e);
            }
        }
    }

    /// The schema context as built, whether or not it will be sent.
    pub fn context_text(&self) -> &str {
        &self.context
    }

    /// What goes into a prompt: the context, or nothing when the user turned
    /// it off or it is not there.
    pub fn schema_to_send(&self) -> String {
        if self.include_schema && self.context_load == Load::Ready {
            self.context.clone()
        } else {
            String::new()
        }
    }

    pub fn set_include_schema(&mut self, on: bool) {
        self.include_schema = on;
    }

    pub fn include_schema(&self) -> bool {
        self.include_schema
    }

    pub fn toggle_context_viewer(&mut self) {
        self.show_context = !self.show_context;
    }

    pub fn toggle_settings(&mut self) {
        self.show_settings = !self.show_settings;
    }

    pub fn show_settings(&mut self, on: bool) {
        self.show_settings = on;
    }

    /// The schema is wanted and still being built: a request that carries it
    /// should wait rather than go out without it.
    fn waiting_for_schema(&self, kind: Kind) -> bool {
        self.include_schema
            && self.context_load == Load::Loading
            && matches!(
                kind,
                Kind::Generate | Kind::Optimize | Kind::Fix | Kind::Chat
            )
    }

    // -- Requests -----------------------------------------------------------------------

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn pending(&self) -> Option<Kind> {
        self.pending
    }

    pub fn readiness(&self, i: Inputs<'_>) -> Readiness {
        let status = self.status;
        Readiness {
            configured: status.is_some_and(|s| s.configured),
            available: status.is_some_and(|s| s.available),
            has_model: !self.model.is_empty() && self.models_load != Load::Loading,
            busy: self.pending.is_some(),
            has_prompt: !i.prompt.trim().is_empty(),
            has_sql: !i.current_sql.trim().is_empty(),
            has_error: i.last_error.is_some_and(|e| !e.trim().is_empty()),
            has_question: !i.chat.trim().is_empty(),
        }
    }

    pub fn can(&self, kind: Kind, i: Inputs<'_>) -> bool {
        let r = self.readiness(i);
        let ready = match kind {
            Kind::Generate => r.can_generate(),
            Kind::Explain => r.can_explain(),
            Kind::Optimize => r.can_optimize(),
            Kind::Fix => r.can_fix(),
            Kind::Chat => r.can_chat(),
        };
        ready && !self.waiting_for_schema(kind)
    }

    /// Start a request. Returns what to run, or `None` after saying why in
    /// the notice (or silently, for an empty prompt).
    pub fn start(&mut self, kind: Kind, i: Inputs<'_>) -> Option<Dispatch> {
        self.notice = None;
        if self.pending.is_some() {
            return None;
        }
        if self.waiting_for_schema(kind) {
            self.notice = Some("The schema is still loading. Try again in a moment.".into());
            return None;
        }
        let ready = self.readiness(i);
        if !ready.can_request() {
            self.notice = Some(self.not_ready_reason());
            return None;
        }

        let schema = self.schema_to_send();
        let plan = plan_request(
            kind,
            RequestInputs {
                prompt: i.prompt,
                chat_input: i.chat,
                current_sql: i.current_sql,
                last_error: i.last_error,
                schema_context: &schema,
                turns: &self.turns,
                max_history_messages: self.settings.max_history_messages,
            },
        );
        match plan {
            RequestPlan::Skip => None,
            RequestPlan::Refuse(message) => {
                self.notice = Some(message);
                None
            }
            RequestPlan::Send { task, turn } => {
                self.turns.push(turn);
                self.pending = Some(kind);
                Some(Dispatch {
                    kind,
                    provider: self.provider,
                    settings: self.settings.clone(),
                    model: Some(self.model.clone()),
                    task,
                })
            }
        }
    }

    fn not_ready_reason(&self) -> String {
        let label = service_ai::provider_info(self.provider).label;
        match self.status {
            None => "Still checking the provider. Try again in a moment.".into(),
            Some(s) if !s.configured => format!("Add an API key for {} first.", label),
            Some(s) if !s.available => format!("{} is not answering.", label),
            Some(_) if self.model.is_empty() => "Pick a model first.".into(),
            Some(_) => "The model list is still loading.".into(),
        }
    }

    /// An answer (or a failure) landed. Ignored when it was for another
    /// provider, or nothing is waiting for it any more.
    pub fn finish(
        &mut self,
        provider: AiProviderType,
        kind: Kind,
        result: Result<AiResponse, String>,
    ) {
        if provider != self.provider || self.pending != Some(kind) {
            return;
        }
        self.pending = None;
        match result {
            Ok(response) => {
                let mut turn = Turn::assistant(kind.label(), response.content);
                turn.duration_ms = Some(response.duration_ms);
                turn.model = Some(response.model);
                self.turns.push(turn);
            }
            Err(message) => self.turns.push(Turn::failure(kind.label(), message)),
        }
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// The statement of the answer at `index`, if it has one.
    pub fn turn_sql(&self, index: usize) -> Option<&str> {
        self.turns
            .get(index)
            .filter(|t| t.who == Who::Assistant && !t.failed)
            .and_then(|t| t.sql.as_deref())
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.notice = None;
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    pub fn set_notice(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    // -- View ----------------------------------------------------------------------------

    pub fn view(&self, i: Inputs<'_>) -> AiView {
        let info = service_ai::provider_info(self.provider);
        let status = self.status;

        let status_text = match status {
            None => "Checking",
            Some(s) if s.available && s.configured => "Connected",
            Some(s) if !s.configured => "API key required",
            Some(_) => "Not available",
        };

        let empty = empty_state(self.provider, status).map(|kind| {
            let label = info.label;
            let (title, message, action) = match kind {
                EmptyState::Checking => (
                    format!("Checking {}", label),
                    "Asking the provider whether it is reachable.".to_string(),
                    String::new(),
                ),
                EmptyState::NeedsApiKey => (
                    format!("Add your {} API key", label),
                    "The key is stored in your operating system's keyring, never in a settings file."
                        .to_string(),
                    "Add API key".to_string(),
                ),
                EmptyState::OllamaUnavailable => (
                    "Ollama is not running".to_string(),
                    format!(
                        "Nothing answered at {}. Start Ollama, or get it from ollama.com.",
                        self.base_url()
                    ),
                    "Check again".to_string(),
                ),
                EmptyState::Unavailable => (
                    format!("{} did not respond", label),
                    "The key is saved but the endpoint could not be reached. Check the base URL and your network."
                        .to_string(),
                    "Check again".to_string(),
                ),
            };
            EmptyView { kind, title, message, action }
        });

        let models: Vec<String> = self.models.iter().map(model_label).collect();
        let model_index = self
            .models
            .iter()
            .position(|m| m.id == self.model)
            .map_or(-1, |p| p as i32);
        let (models_loading, mut models_error) = match &self.models_load {
            Load::Loading => (true, String::new()),
            Load::Failed(e) => (false, e.clone()),
            _ => (false, String::new()),
        };
        if self.models_load == Load::Ready && self.models.is_empty() {
            models_error = if self.provider == AiProviderType::Ollama {
                "No models installed. Run `ollama pull llama3.2` in a terminal, then refresh."
                    .into()
            } else {
                "This provider listed no models for your key.".into()
            };
        }
        let model_text = if models_loading {
            "Loading models".to_string()
        } else if self.models_load == Load::Idle && !self.model.is_empty() {
            self.model.clone()
        } else {
            selected_model_text(&self.models, &self.model)
        };

        let (context_loading, context_error) = match &self.context_load {
            Load::Loading => (true, String::new()),
            Load::Failed(e) => (false, e.clone()),
            _ => (false, String::new()),
        };

        let disclosure = [
            Kind::Generate,
            Kind::Explain,
            Kind::Optimize,
            Kind::Fix,
            Kind::Chat,
        ]
        .into_iter()
        .map(|k| DisclosureLine {
            label: k.label().to_string(),
            text: request_disclosure(k, self.include_schema).to_string(),
        })
        .collect();

        let turns = self
            .turns
            .iter()
            .map(|t| {
                let user = t.who == Who::User;
                let sql = t.sql.clone().unwrap_or_default();
                let body = if user {
                    user_turn_body(&t.content)
                } else if t.failed {
                    t.content.clone()
                } else {
                    answer_prose(&t.content, t.sql.as_deref())
                };
                let mut meta = Vec::new();
                if let Some(m) = &t.model {
                    meta.push(m.clone());
                }
                if let Some(ms) = t.duration_ms {
                    meta.push(format_duration(ms));
                }
                TurnView {
                    user,
                    heading: t.heading.clone(),
                    body,
                    has_sql: !sql.is_empty(),
                    sql,
                    meta: meta.join(" · "),
                    failed: t.failed,
                }
            })
            .collect();

        AiView {
            providers: service_ai::PROVIDERS
                .iter()
                .map(|p| p.label.to_string())
                .collect(),
            provider_index: service_ai::PROVIDERS
                .iter()
                .position(|p| p.provider == self.provider)
                .unwrap_or(0),
            provider_description: info.description.to_string(),
            status_tone: status_tone(status),
            status_text: status_text.to_string(),
            needs_key: info.needs_api_key(),
            key_saved: status.is_some_and(|s| s.configured) && info.needs_api_key(),
            key_hint: info.key_hint.unwrap_or("").to_string(),
            base_url: self.base_url().to_string(),
            base_url_warning: base_url_warning(self.base_url(), info.needs_api_key())
                .unwrap_or("")
                .to_string(),
            show_settings: self.show_settings,
            models,
            model_index,
            model_text,
            models_loading,
            models_error,
            empty,
            busy: self.pending.is_some(),
            busy_label: match self.pending {
                Some(Kind::Generate) => "Generating SQL",
                Some(Kind::Explain) => "Explaining the query",
                Some(Kind::Optimize) => "Optimizing the query",
                Some(Kind::Fix) => "Fixing the query",
                Some(Kind::Chat) => "Thinking",
                None => "",
            }
            .to_string(),
            can_generate: self.can(Kind::Generate, i),
            can_explain: self.can(Kind::Explain, i),
            can_optimize: self.can(Kind::Optimize, i),
            can_fix: self.can(Kind::Fix, i),
            can_chat: self.can(Kind::Chat, i),
            has_sql: !i.current_sql.trim().is_empty(),
            sql_preview: clip_lines(i.current_sql.trim(), 2),
            has_error: i.last_error.is_some_and(|e| !e.trim().is_empty()),
            error_text: clip_lines(i.last_error.unwrap_or("").trim(), 2),
            include_schema: self.include_schema,
            show_context: self.show_context,
            // Only copied out while the viewer is open: the context of a big
            // database is large, and the view is rebuilt on every keystroke
            // in the editor.
            context_text: if self.show_context {
                self.context.clone()
            } else {
                String::new()
            },
            context_summary: self.context_stats.clone(),
            context_loading,
            context_error,
            disclosure,
            turns,
            notice: self.notice.clone().unwrap_or_default(),
        }
    }
}

// ===========================================================================
// View types
// ===========================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyView {
    pub kind: EmptyState,
    pub title: String,
    pub message: String,
    /// The one button the state offers; empty for none.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosureLine {
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnView {
    pub user: bool,
    pub heading: String,
    /// Prose for an answer, the request for a user turn, the message for a
    /// failure.
    pub body: String,
    pub has_sql: bool,
    pub sql: String,
    /// "gpt-4o-mini · 1.2s".
    pub meta: String,
    pub failed: bool,
}

/// Everything the panel draws, as plain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiView {
    pub providers: Vec<String>,
    pub provider_index: usize,
    pub provider_description: String,
    pub status_tone: StatusTone,
    pub status_text: String,
    pub needs_key: bool,
    pub key_saved: bool,
    pub key_hint: String,
    pub base_url: String,
    pub base_url_warning: String,
    pub show_settings: bool,
    pub models: Vec<String>,
    pub model_index: i32,
    pub model_text: String,
    pub models_loading: bool,
    pub models_error: String,
    pub empty: Option<EmptyView>,
    pub busy: bool,
    pub busy_label: String,
    pub can_generate: bool,
    pub can_explain: bool,
    pub can_optimize: bool,
    pub can_fix: bool,
    pub can_chat: bool,
    pub has_sql: bool,
    pub sql_preview: String,
    pub has_error: bool,
    pub error_text: String,
    pub include_schema: bool,
    pub show_context: bool,
    pub context_text: String,
    pub context_summary: String,
    pub context_loading: bool,
    pub context_error: String,
    pub disclosure: Vec<DisclosureLine>,
    pub turns: Vec<TurnView>,
    pub notice: String,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAI: AiProviderType = AiProviderType::OpenAI;

    fn model(id: &str) -> AiModel {
        AiModel {
            id: id.into(),
            name: id.into(),
            provider: OPENAI,
            description: None,
            context_window: None,
        }
    }

    fn ok_status(provider: AiProviderType) -> ProviderStatus {
        ProviderStatus {
            provider,
            available: true,
            configured: true,
        }
    }

    fn response(content: &str) -> AiResponse {
        AiResponse {
            content: content.into(),
            model: "gpt-test".into(),
            provider: OPENAI,
            duration_ms: 1500,
        }
    }

    fn inputs<'a>(sql: &'a str, error: Option<&'a str>, prompt: &'a str) -> Inputs<'a> {
        Inputs {
            current_sql: sql,
            last_error: error,
            prompt,
            chat: "",
        }
    }

    fn chat(question: &str) -> Inputs<'_> {
        Inputs {
            current_sql: "",
            last_error: None,
            prompt: "",
            chat: question,
        }
    }

    /// A panel on OpenAI that is connected, has models and a loaded schema.
    fn ready_panel() -> AiPanel {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        p.status_result(OPENAI, ok_status(OPENAI));
        p.begin_models();
        p.models_result(OPENAI, Ok(vec![model("gpt-a"), model("gpt-b")]));
        let key = ("conn".to_string(), "shop".to_string());
        p.begin_context(key.clone());
        p.context_result(
            &key,
            Ok("Database: shop\n\nTables:\n\nmain.users:\n  - id: integer\n".into()),
        );
        p
    }

    // --- disclosure and helpers ------------------------------------------------

    #[test]
    fn disclosure_says_what_each_request_sends() {
        assert!(request_disclosure(Kind::Explain, true).contains("No schema"));
        assert!(request_disclosure(Kind::Fix, true).contains("error text"));
        assert!(request_disclosure(Kind::Fix, true).contains("schema context"));
        assert!(!request_disclosure(Kind::Fix, false).contains("schema context"));
        assert!(!request_disclosure(Kind::Optimize, false).contains("schema context"));
        for kind in [Kind::Generate, Kind::Explain, Kind::Optimize, Kind::Fix] {
            for schema in [true, false] {
                assert!(
                    !request_disclosure(kind, schema)
                        .to_lowercase()
                        .contains("rows"),
                    "no request sends row data, and none should hint that it might"
                );
            }
        }
    }

    #[test]
    fn context_summary_counts_tables_and_size() {
        let ctx = "Database: shop\n\nTables:\n\npublic.users:\n  - id: integer\n\npublic.orders:\n  - id: integer\n";
        assert_eq!(
            context_summary(ctx),
            format!("2 tables, {} bytes", ctx.len())
        );
        let one = "Database: shop\n\nTables:\n\nmain.t:\n  - a: int\n";
        assert!(context_summary(one).starts_with("1 table,"));
        assert_eq!(context_summary(&"x".repeat(2048)), "0 tables, 2.0 KB");
    }

    #[test]
    fn a_sqlite_path_is_reduced_to_its_file_name_before_it_reaches_a_prompt() {
        assert_eq!(
            context_database_label(&DbDriver::Sqlite, "/Users/me/secret-project/app.sqlite"),
            "app.sqlite"
        );
        assert_eq!(context_database_label(&DbDriver::Postgres, "shop"), "shop");
        assert_eq!(
            context_database_label(&DbDriver::Sqlite, "plain.db"),
            "plain.db"
        );
    }

    #[test]
    fn base_urls_are_checked_and_cleartext_remote_keys_are_flagged() {
        assert!(base_url_problem("").is_some());
        assert!(base_url_problem("localhost:11434").is_some());
        assert!(base_url_problem("http://a b").is_some());
        assert!(base_url_problem("https://api.openai.com/v1").is_none());
        assert!(base_url_problem(" http://localhost:11434 ").is_none());

        assert!(base_url_warning("http://example.com/v1", true).is_some());
        assert!(base_url_warning("http://localhost:8080/v1", true).is_none());
        assert!(base_url_warning("http://127.0.0.1:8080/v1", true).is_none());
        assert!(base_url_warning("https://example.com/v1", true).is_none());
        assert!(base_url_warning("http://example.com", false).is_none());
    }

    #[test]
    fn prose_is_what_is_left_after_the_statement_block() {
        let content = "Here you go:\n```sql\nSELECT 1;\n```\nThat returns one.";
        let sql = service_ai::extract_sql(content);
        assert_eq!(
            answer_prose(content, sql.as_deref()),
            "Here you go:\nThat returns one."
        );
    }

    #[test]
    fn a_bare_statement_has_no_prose_and_a_prose_answer_is_all_prose() {
        assert_eq!(answer_prose("SELECT 1;", Some("SELECT 1;")), "");
        assert_eq!(answer_prose("  SELECT 1;\n", Some("SELECT 1;")), "");
        assert_eq!(
            answer_prose("  It scans the table.  ", None),
            "It scans the table."
        );
    }

    #[test]
    fn only_the_offered_block_is_removed_when_there_are_several() {
        let content =
            "Before:\n```sql\nSELECT a FROM t;\n```\nAfter:\n```sql\nSELECT b FROM t;\n```";
        let sql = service_ai::extract_sql(content);
        assert_eq!(sql.as_deref(), Some("SELECT a FROM t;"));
        assert_eq!(
            answer_prose(content, sql.as_deref()),
            "Before:\nAfter:\n```sql\nSELECT b FROM t;\n```"
        );
    }

    #[test]
    fn an_unterminated_block_is_removed_too() {
        let content = "Try:\n```sql\nSELECT 1\nFROM t";
        let sql = service_ai::extract_sql(content);
        assert_eq!(answer_prose(content, sql.as_deref()), "Try:");
    }

    #[test]
    fn the_ai_error_prefix_is_dropped_once() {
        assert_eq!(
            error_text("AI error: OpenAI API error: bad key"),
            "OpenAI API error: bad key"
        );
        assert_eq!(
            error_text("  Credential error: denied "),
            "Credential error: denied"
        );
    }

    #[test]
    fn a_fix_request_shows_its_error_after_a_clipped_statement() {
        let content = "SELECT a\nFROM t\nWHERE x\n\nError: syntax error near \"FROM\"";
        assert_eq!(
            user_turn_body(content),
            "SELECT a\nFROM t\n…\nError: syntax error near \"FROM\""
        );
        assert_eq!(user_turn_body("SELECT 1"), "SELECT 1");
        assert_eq!(user_turn_body("list users"), "list users");
    }

    #[test]
    fn clipping_keeps_the_first_lines_and_marks_the_cut() {
        assert_eq!(clip_lines("a\nb", 4), "a\nb");
        assert_eq!(clip_lines("a\nb\nc\nd\ne", 2), "a\nb\n…");
    }

    #[test]
    fn applying_replaces_appends_or_starts_fresh() {
        assert_eq!(
            apply_text(Apply::Replace, "SELECT 1", "SELECT 2"),
            "SELECT 2"
        );
        assert_eq!(
            apply_text(Apply::Append, "SELECT 1\n", "SELECT 2"),
            "SELECT 1\n\nSELECT 2"
        );
        assert_eq!(apply_text(Apply::Append, "  ", "SELECT 2"), "SELECT 2");
        assert_eq!(
            apply_text(Apply::NewTab, "SELECT 1", "SELECT 2"),
            "SELECT 2"
        );
    }

    // --- provider, status, models -----------------------------------------------

    #[test]
    fn settings_choose_the_provider_and_its_default_model() {
        let mut p = AiPanel::default();
        assert!(!p.settings_loaded());
        let mut s = AiSettings::default();
        s.active_provider = AiProviderType::Anthropic;
        s.get_mut(AiProviderType::Anthropic).default_model = "claude-x".into();
        p.settings_result(Ok(s));
        assert!(p.settings_loaded());
        assert_eq!(p.provider(), AiProviderType::Anthropic);
        assert_eq!(p.model(), "claude-x");
    }

    #[test]
    fn unreadable_settings_keep_the_defaults_and_say_so() {
        let mut p = AiPanel::default();
        p.settings_result(Err("Saved AI settings could not be read: bad json".into()));
        assert!(p.settings_loaded());
        assert_eq!(p.provider(), AiProviderType::Ollama);
        assert!(p
            .view(inputs("", None, ""))
            .notice
            .contains("could not be read"));
    }

    #[test]
    fn changing_provider_resets_status_models_and_the_request_in_flight() {
        let mut p = ready_panel();
        p.start(Kind::Generate, inputs("", None, "list users"))
            .unwrap();
        assert!(p.busy());

        assert!(!p.change_provider(OPENAI), "same provider is not a change");
        assert!(p.change_provider(AiProviderType::Google));
        assert!(p.status().is_none());
        assert!(!p.busy());
        assert!(p.view(inputs("", None, "")).models.is_empty());
        assert_eq!(
            p.model(),
            AiSettings::default()
                .get(AiProviderType::Google)
                .default_model
        );
    }

    #[test]
    fn a_status_for_another_provider_is_ignored() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        p.status_result(AiProviderType::Google, ok_status(AiProviderType::Google));
        assert!(p.status().is_none());
        p.status_result(OPENAI, ok_status(OPENAI));
        assert!(p.status().is_some());
    }

    #[test]
    fn models_are_wanted_once_the_provider_is_usable() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        assert!(!p.wants_models());
        p.status_result(
            OPENAI,
            ProviderStatus {
                provider: OPENAI,
                available: true,
                configured: false,
            },
        );
        assert!(!p.wants_models());
        p.status_result(OPENAI, ok_status(OPENAI));
        assert!(p.wants_models());
        p.begin_models();
        assert!(!p.wants_models(), "already asked");
    }

    #[test]
    fn a_model_list_keeps_the_choice_it_still_offers_and_falls_back_otherwise() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        p.status_result(OPENAI, ok_status(OPENAI));
        p.models_result(OPENAI, Ok(vec![model("gpt-a"), model("gpt-b")]));
        assert_eq!(p.model(), "gpt-a", "not the default, so the first");
        let saved = p.choose_model(1).expect("a changed choice is to be saved");
        assert_eq!(saved.get(OPENAI).default_model, "gpt-b");
        assert_eq!(p.model(), "gpt-b");
        assert!(p.choose_model(1).is_none(), "the same choice saves nothing");
        assert!(
            p.choose_model(9).is_none(),
            "an index past the list is ignored"
        );
        p.refresh_models();
        p.models_result(OPENAI, Ok(vec![model("gpt-b"), model("gpt-c")]));
        assert_eq!(p.model(), "gpt-b");
        p.models_result(OPENAI, Ok(vec![model("gpt-c")]));
        assert_eq!(p.model(), "gpt-c");
        let v = p.view(inputs("", None, ""));
        assert_eq!(v.model_index, 0);
        assert_eq!(v.models, vec!["gpt-c"]);
    }

    #[test]
    fn an_empty_model_list_blocks_requests_and_says_what_to_do() {
        let mut p = AiPanel::default();
        p.status_result(AiProviderType::Ollama, ok_status(AiProviderType::Ollama));
        p.models_result(AiProviderType::Ollama, Ok(vec![]));
        assert_eq!(p.model(), "");
        let v = p.view(inputs("SELECT 1", None, "x"));
        assert!(v.models_error.contains("ollama pull"));
        assert!(!v.can_generate && !v.can_explain);
        assert_eq!(v.model_text, "No models available");
    }

    #[test]
    fn a_failed_model_list_is_reported_and_can_be_retried() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        p.status_result(OPENAI, ok_status(OPENAI));
        p.begin_models();
        assert!(p.view(inputs("", None, "")).models_loading);
        p.models_result(OPENAI, Err("OpenAI API error: bad key".into()));
        assert_eq!(
            p.view(inputs("", None, "")).models_error,
            "OpenAI API error: bad key"
        );
        p.refresh_models();
        assert!(p.wants_models());
    }

    #[test]
    fn a_new_base_url_is_validated_and_resets_what_depended_on_the_old_one() {
        let mut p = ready_panel();
        assert!(p.set_base_url("not a url").is_err());
        assert!(p.status().is_some(), "a refused URL changes nothing");
        let saved = p.set_base_url(" http://localhost:9999/v1/ ").unwrap();
        assert_eq!(saved.get(OPENAI).base_url, "http://localhost:9999/v1");
        assert_eq!(p.base_url(), "http://localhost:9999/v1");
        assert!(p.status().is_none());
    }

    // --- empty states and status in the view --------------------------------------------

    #[test]
    fn the_view_guides_the_user_through_each_way_the_provider_can_be_unusable() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        let v = p.view(inputs("", None, ""));
        assert_eq!(v.empty.as_ref().unwrap().kind, EmptyState::Checking);
        assert_eq!(v.status_text, "Checking");
        assert_eq!(v.status_tone, StatusTone::Muted);

        p.status_result(
            OPENAI,
            ProviderStatus {
                provider: OPENAI,
                available: false,
                configured: false,
            },
        );
        let v = p.view(inputs("", None, ""));
        let e = v.empty.unwrap();
        assert_eq!(e.kind, EmptyState::NeedsApiKey);
        assert_eq!(e.action, "Add API key");
        assert_eq!(v.status_text, "API key required");
        assert!(!v.key_saved);
        assert!(v.needs_key);

        p.status_result(
            OPENAI,
            ProviderStatus {
                provider: OPENAI,
                available: false,
                configured: true,
            },
        );
        let v = p.view(inputs("", None, ""));
        assert_eq!(v.empty.as_ref().unwrap().kind, EmptyState::Unavailable);
        assert_eq!(v.status_tone, StatusTone::Destructive);
        assert!(v.key_saved);

        p.status_result(OPENAI, ok_status(OPENAI));
        let v = p.view(inputs("", None, ""));
        assert!(v.empty.is_none());
        assert_eq!(v.status_text, "Connected");
        assert_eq!(v.status_tone, StatusTone::Success);
    }

    #[test]
    fn ollama_not_running_names_the_address_it_tried() {
        let mut p = AiPanel::default();
        p.status_result(
            AiProviderType::Ollama,
            ProviderStatus {
                provider: AiProviderType::Ollama,
                available: false,
                configured: true,
            },
        );
        let e = p.view(inputs("", None, "")).empty.unwrap();
        assert_eq!(e.kind, EmptyState::OllamaUnavailable);
        assert!(e.message.contains("http://localhost:11434"));
        assert!(!p.view(inputs("", None, "")).needs_key);
    }

    // --- requests -------------------------------------------------------------------------

    #[test]
    fn generate_sends_the_prompt_and_the_schema_and_records_the_user_turn() {
        let mut p = ready_panel();
        let d = p
            .start(Kind::Generate, inputs("", None, "  list users  "))
            .unwrap();
        assert_eq!(d.kind, Kind::Generate);
        assert_eq!(d.provider, OPENAI);
        assert_eq!(d.model.as_deref(), Some("gpt-a"));
        match d.task {
            AiTask::Generate {
                prompt,
                schema_context,
            } => {
                assert_eq!(prompt, "list users");
                assert!(schema_context.contains("main.users"));
            }
            other => panic!("unexpected {:?}", other),
        }
        assert!(p.busy());
        assert_eq!(p.turns().len(), 1);
        assert_eq!(p.turns()[0].content, "list users");
    }

    #[test]
    fn turning_the_schema_off_sends_none_and_the_view_says_so() {
        let mut p = ready_panel();
        p.set_include_schema(false);
        match p
            .start(Kind::Generate, inputs("", None, "list users"))
            .unwrap()
            .task
        {
            AiTask::Generate { schema_context, .. } => assert_eq!(schema_context, ""),
            other => panic!("unexpected {:?}", other),
        }
        let v = p.view(inputs("", None, ""));
        assert!(!v.include_schema);
        assert!(v
            .disclosure
            .iter()
            .all(|l| !l.text.contains("schema context")));
    }

    #[test]
    fn explain_never_sends_the_schema() {
        let mut p = ready_panel();
        match p
            .start(Kind::Explain, inputs("SELECT 1", None, ""))
            .unwrap()
            .task
        {
            AiTask::Explain { sql } => assert_eq!(sql, "SELECT 1"),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn fix_needs_the_statement_and_its_error() {
        let mut p = ready_panel();
        assert!(!p.view(inputs("SELECT * FROM nope", None, "")).can_fix);
        assert!(p
            .start(Kind::Fix, inputs("SELECT * FROM nope", None, ""))
            .is_none());
        assert!(!p.busy());
        assert!(p
            .view(inputs("", None, ""))
            .notice
            .starts_with("No error to fix"));

        let d = p
            .start(
                Kind::Fix,
                inputs("SELECT * FROM nope", Some("no such table: nope"), ""),
            )
            .unwrap();
        match d.task {
            AiTask::Fix {
                sql,
                error_message,
                schema_context,
            } => {
                assert_eq!(sql, "SELECT * FROM nope");
                assert_eq!(error_message, "no such table: nope");
                assert!(schema_context.contains("main.users"));
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn nothing_starts_while_something_is_in_flight() {
        let mut p = ready_panel();
        assert!(p
            .start(Kind::Explain, inputs("SELECT 1", None, ""))
            .is_some());
        assert!(p
            .start(Kind::Explain, inputs("SELECT 1", None, ""))
            .is_none());
        assert!(!p.view(inputs("SELECT 1", None, "x")).can_explain);
        assert_eq!(p.turns().len(), 1, "no second user turn");
    }

    #[test]
    fn a_request_that_carries_the_schema_waits_for_it() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        p.status_result(OPENAI, ok_status(OPENAI));
        p.models_result(OPENAI, Ok(vec![model("gpt-a")]));
        p.begin_context(("c".into(), "d".into()));
        let v = p.view(inputs("SELECT 1", None, "x"));
        assert!(v.context_loading);
        assert!(!v.can_generate, "would go out without the schema");
        assert!(v.can_explain, "explain does not use it");
        assert!(p.start(Kind::Generate, inputs("", None, "x")).is_none());
        assert!(p
            .view(inputs("", None, ""))
            .notice
            .contains("still loading"));
        // With the schema switched off there is nothing to wait for.
        p.set_include_schema(false);
        assert!(p.start(Kind::Generate, inputs("", None, "x")).is_some());
    }

    #[test]
    fn a_failed_schema_build_still_allows_requests_without_it() {
        let mut p = ready_panel();
        let key = ("c2".to_string(), "d2".to_string());
        p.begin_context(key.clone());
        p.context_result(&key, Err("permission denied".into()));
        let v = p.view(inputs("", None, "x"));
        assert_eq!(v.context_error, "permission denied");
        assert!(v.can_generate);
        match p.start(Kind::Generate, inputs("", None, "x")).unwrap().task {
            AiTask::Generate { schema_context, .. } => assert_eq!(schema_context, ""),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn requests_are_refused_with_a_reason_when_the_provider_is_not_ready() {
        let mut p = AiPanel::default();
        p.change_provider(OPENAI);
        assert!(p
            .start(Kind::Explain, inputs("SELECT 1", None, ""))
            .is_none());
        assert!(p
            .view(inputs("", None, ""))
            .notice
            .contains("Still checking"));

        p.status_result(
            OPENAI,
            ProviderStatus {
                provider: OPENAI,
                available: false,
                configured: false,
            },
        );
        p.start(Kind::Explain, inputs("SELECT 1", None, ""));
        assert_eq!(
            p.view(inputs("", None, "")).notice,
            "Add an API key for OpenAI first."
        );

        p.status_result(
            OPENAI,
            ProviderStatus {
                provider: OPENAI,
                available: false,
                configured: true,
            },
        );
        p.start(Kind::Explain, inputs("SELECT 1", None, ""));
        assert_eq!(
            p.view(inputs("", None, "")).notice,
            "OpenAI is not answering."
        );
    }

    #[test]
    fn an_empty_prompt_is_ignored_without_a_message() {
        let mut p = ready_panel();
        assert!(p.start(Kind::Generate, inputs("", None, "   ")).is_none());
        assert_eq!(p.view(inputs("", None, "")).notice, "");
        assert!(!p.busy());
    }

    #[test]
    fn an_answer_lands_as_a_turn_with_its_statement_model_and_duration() {
        let mut p = ready_panel();
        p.start(Kind::Generate, inputs("", None, "list users"))
            .unwrap();
        p.finish(
            OPENAI,
            Kind::Generate,
            Ok(response("SELECT id FROM users;")),
        );
        assert!(!p.busy());
        assert_eq!(p.turn_sql(1), Some("SELECT id FROM users;"));
        assert_eq!(p.turn_sql(0), None, "the user's own turn offers nothing");

        let v = p.view(inputs("", None, ""));
        assert_eq!(v.turns.len(), 2);
        assert!(v.turns[0].user);
        let t = &v.turns[1];
        assert_eq!(t.heading, "Generate SQL");
        assert_eq!(t.body, "", "a bare statement has no prose");
        assert!(t.has_sql);
        assert_eq!(t.sql, "SELECT id FROM users;");
        assert_eq!(t.meta, "gpt-test · 1.5s");
    }

    #[test]
    fn a_failure_lands_as_a_failed_turn_that_offers_no_sql() {
        let mut p = ready_panel();
        p.start(Kind::Explain, inputs("SELECT 1", None, ""))
            .unwrap();
        p.finish(
            OPENAI,
            Kind::Explain,
            Err("OpenAI API error: rate limited".into()),
        );
        assert!(!p.busy());
        let v = p.view(inputs("", None, ""));
        let t = v.turns.last().unwrap();
        assert!(t.failed);
        assert_eq!(t.body, "OpenAI API error: rate limited");
        assert!(!t.has_sql);
        assert_eq!(p.turn_sql(1), None);
    }

    #[test]
    fn a_late_answer_for_another_provider_or_after_a_reset_is_dropped() {
        let mut p = ready_panel();
        p.start(Kind::Explain, inputs("SELECT 1", None, ""))
            .unwrap();
        p.finish(AiProviderType::Google, Kind::Explain, Ok(response("x")));
        assert!(p.busy(), "the answer was for another provider");
        p.change_provider(AiProviderType::Google);
        p.finish(OPENAI, Kind::Explain, Ok(response("late")));
        assert_eq!(
            p.turns().len(),
            1,
            "only the user's turn; the late answer is gone"
        );
    }

    #[test]
    fn clearing_empties_the_conversation_but_not_a_request_in_flight() {
        let mut p = ready_panel();
        p.start(Kind::Explain, inputs("SELECT 1", None, ""))
            .unwrap();
        p.clear();
        assert!(p.turns().is_empty());
        assert!(p.busy());
    }

    #[test]
    fn a_long_user_turn_is_clipped_in_the_view_but_kept_whole_in_the_transcript() {
        let mut p = ready_panel();
        let sql = (0..10)
            .map(|i| format!("SELECT {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        p.start(Kind::Explain, inputs(&sql, None, "")).unwrap();
        let v = p.view(inputs("", None, ""));
        assert!(v.turns[0].body.ends_with('…'));
        assert_eq!(p.turns()[0].content, sql);
    }

    #[test]
    fn the_editor_preview_and_error_are_shown_clipped() {
        let p = ready_panel();
        let sql = (0..9)
            .map(|i| format!("SELECT {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let v = p.view(inputs(&sql, Some("syntax error near \"FORM\""), ""));
        assert!(v.has_sql && v.has_error);
        assert!(v.sql_preview.ends_with('…'));
        assert_eq!(v.error_text, "syntax error near \"FORM\"");
        let v = p.view(inputs("   ", Some("  "), ""));
        assert!(!v.has_sql && !v.has_error);
    }

    // --- schema context -------------------------------------------------------------------

    #[test]
    fn the_context_is_rebuilt_per_connection_and_database_and_a_stale_build_is_dropped() {
        let mut p = AiPanel::default();
        let a = ("c1".to_string(), "db".to_string());
        let b = ("c1".to_string(), "other".to_string());
        assert!(p.context_wanted(&a));
        p.begin_context(a.clone());
        assert!(!p.context_wanted(&a));
        assert!(p.context_wanted(&b));
        p.begin_context(b.clone());
        p.context_result(&a, Ok("stale".into()));
        assert_eq!(
            p.context_text(),
            "",
            "the answer for the old database is ignored"
        );
        p.context_result(&b, Ok("fresh".into()));
        assert_eq!(p.context_text(), "fresh");
        p.invalidate_context();
        assert!(p.context_wanted(&b));
    }

    #[test]
    fn the_viewer_shows_exactly_the_text_that_would_be_sent() {
        let mut p = ready_panel();
        // Closed: the text is not copied into the view, the summary still is.
        let closed = p.view(inputs("", None, ""));
        assert_eq!(closed.context_text, "");
        assert!(closed.context_summary.starts_with("1 table,"));
        p.toggle_context_viewer();
        let v = p.view(inputs("", None, ""));
        assert!(v.show_context);
        assert_eq!(v.context_text, p.schema_to_send());
        assert!(v.context_summary.starts_with("1 table,"));
        p.set_include_schema(false);
        assert_eq!(p.schema_to_send(), "");
        // The text stays visible so the user can see what they are holding back.
        assert_eq!(p.view(inputs("", None, "")).context_text, p.context_text());
    }

    // --- free-form chat --------------------------------------------------------

    #[test]
    fn chat_needs_a_question_and_a_usable_provider() {
        let p = ready_panel();
        assert!(!p.view(chat("   ")).can_chat);
        assert!(p.view(chat("which table holds users?")).can_chat);
        assert!(!AiPanel::default().view(chat("hello")).can_chat);
        let mut empty = ready_panel();
        assert!(empty.start(Kind::Chat, chat("  ")).is_none());
        assert!(empty.turns().is_empty(), "a blank question records nothing");
    }

    #[test]
    fn chat_sends_the_schema_the_earlier_turns_and_the_question() {
        let mut p = ready_panel();
        p.start(Kind::Generate, inputs("", None, "list users"))
            .unwrap();
        p.finish(
            OPENAI,
            Kind::Generate,
            Ok(response("SELECT id FROM users;")),
        );

        let d = p
            .start(Kind::Chat, chat("  and how many are there?  "))
            .unwrap();
        let AiTask::Chat { messages } = d.task else {
            panic!("expected a chat task");
        };
        use dbhive_core::ai::ChatRole;
        assert_eq!(messages[0].role, ChatRole::System);
        assert!(
            messages[0].content.contains("main.users"),
            "the schema goes with chat"
        );
        assert_eq!(messages[1].content, "list users");
        assert_eq!(messages[2].content, "SELECT id FROM users;");
        let last = messages.last().unwrap();
        assert_eq!(last.role, ChatRole::User);
        assert_eq!(last.content, "and how many are there?");
        assert_eq!(p.turns().last().unwrap().content, "and how many are there?");
        assert_eq!(p.view(chat("")).busy_label, "Thinking");
    }

    #[test]
    fn chat_without_the_schema_says_so_and_sends_none() {
        let mut p = ready_panel();
        assert!(request_disclosure(Kind::Chat, true).contains("schema context"));
        p.set_include_schema(false);
        let d = p.start(Kind::Chat, chat("hi")).unwrap();
        let AiTask::Chat { messages } = d.task else {
            panic!("expected a chat task");
        };
        assert!(!messages[0].content.contains("DATABASE SCHEMA"));
        let v = p.view(chat(""));
        assert!(v.disclosure.iter().any(|l| l.label == "Chat"));
    }

    #[test]
    fn a_chat_answer_offers_its_statement_and_runs_nothing() {
        let mut p = ready_panel();
        p.start(Kind::Chat, chat("count users")).unwrap();
        p.finish(
            OPENAI,
            Kind::Chat,
            Ok(response(
                "Like this:\n```sql\nSELECT count(*) FROM users;\n```",
            )),
        );
        assert_eq!(p.turn_sql(1), Some("SELECT count(*) FROM users;"));
        let v = p.view(chat(""));
        assert_eq!(v.turns[1].heading, "Chat");
        assert_eq!(v.turns[1].body, "Like this:");
        assert!(v.turns[1].has_sql);
    }

    #[test]
    fn chat_waits_for_a_schema_that_is_still_loading() {
        let mut p = ready_panel();
        p.begin_context(("conn".to_string(), "other".to_string()));
        assert!(!p.view(chat("hi")).can_chat);
        assert!(p.start(Kind::Chat, chat("hi")).is_none());
    }
}
