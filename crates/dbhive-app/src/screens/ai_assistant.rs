//! AI assistant: natural language to SQL, explain, optimise, fix, and chat.
//!
//! Parity target: the React `AiAssistant.tsx` panel mounted inside
//! `QueryPanel` (see 2.14 in docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! Three things shape this file:
//!
//! * **Nothing it produces runs itself.** The panel has no connection to
//!   execute with and never asks for one. A model's answer becomes an
//!   "Insert into editor" button that returns [`AiAction::UseSql`]; running
//!   the statement is a separate, deliberate act by the user in the editor.
//!   This is the whole reason the screen returns an action instead of
//!   reaching into the workspace.
//! * **API keys never live here.** They go straight to the OS keyring
//!   through `dbhive_core::service_ai`, are read back only when a provider
//!   client is built, and are rendered as a password field that starts
//!   empty. The panel knows whether a key exists, not what it is.
//! * **All provider work is off-frame.** The provider clients in
//!   `dbhive_core::ai` are request/response with no streaming API, so every
//!   call is a `crate::task::Job` polled once per frame rather than a
//!   token channel. When a client grows a streaming method, the transcript
//!   is already the right shape to append into.
//!
//! The transcript is a superset of the web shell, which was single-shot: the
//! four actions and free-form chat append to one conversation, and the
//! conversation (trimmed) is what a follow-up question is sent with.

use dbhive_core::ai::{AiModel, AiProviderType, ChatMessage};
use dbhive_core::models::DbError;
use dbhive_core::service_ai::{self, AiResponse, AiSettings, AiTask, ProviderStatus};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};

/// What the caller should do once the panel has been drawn.
///
/// The panel owns neither the editor nor a connection, so a generated
/// statement is handed back rather than applied, and never executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiAction {
    None,
    /// Put this statement in the editor. The user still has to run it.
    UseSql(String),
}

/// Which of the five requests a turn came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Generate,
    Explain,
    Optimize,
    Fix,
    Chat,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Generate => "Generate SQL",
            Kind::Explain => "Explain",
            Kind::Optimize => "Optimize",
            Kind::Fix => "Fix Error",
            Kind::Chat => "Chat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    User,
    Assistant,
}

/// One entry in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub who: Who,
    /// Heading shown above the body, e.g. "You" or "Explain".
    pub heading: String,
    pub content: String,
    /// SQL pulled out of an assistant answer, if there was any.
    pub sql: Option<String>,
    pub duration_ms: Option<u64>,
    pub failed: bool,
}

impl Turn {
    fn user(heading: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            who: Who::User,
            heading: heading.into(),
            content: content.into(),
            sql: None,
            duration_ms: None,
            failed: false,
        }
    }

    fn assistant(heading: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        let sql = service_ai::extract_sql(&content);
        Self {
            who: Who::Assistant,
            heading: heading.into(),
            content,
            sql,
            duration_ms: None,
            failed: false,
        }
    }

    fn failure(heading: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            who: Who::Assistant,
            heading: heading.into(),
            content: message.into(),
            sql: None,
            duration_ms: None,
            failed: true,
        }
    }
}

#[derive(Default)]
pub struct AiAssistant {
    pub open: bool,

    /// Non-secret configuration. Keys are never held here.
    settings: AiSettings,
    settings_requested: bool,
    provider: AiProviderType,

    status: Option<ProviderStatus>,
    models: Vec<AiModel>,
    models_requested_for: Option<AiProviderType>,
    model: String,

    /// Typed key, cleared the moment it is handed to the keyring. Rendered
    /// as a password field and never echoed back from storage.
    api_key_input: String,
    base_url_input: String,
    show_settings: bool,
    show_context: bool,

    prompt: String,
    chat_input: String,
    turns: Vec<Turn>,
    pending: Option<Kind>,
    error: Option<String>,

    schema_context: String,
    context_key: Option<(String, String)>,
    context_error: Option<String>,

    settings_job: Job<Result<AiSettings, DbError>>,
    save_job: Job<Result<(), DbError>>,
    key_job: Job<Result<(), DbError>>,
    status_job: Job<Result<ProviderStatus, DbError>>,
    models_job: Job<Result<Vec<AiModel>, DbError>>,
    request_job: Job<(Kind, Result<AiResponse, DbError>)>,
    context_job: Job<Result<String, DbError>>,
}

impl AiAssistant {
    fn busy(&self) -> bool {
        self.pending.is_some() || self.request_job.is_running()
    }
}

// ===========================================================================
// Pure logic
// ===========================================================================

/// Format a request duration the way the web shell did: milliseconds below a
/// second, then one decimal of seconds.
pub fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

/// Turn the transcript into provider messages.
///
/// Failed turns are dropped: an error message is UI feedback, not something
/// the model said, and feeding it back invites the model to apologise for
/// an outage instead of answering.
pub fn history_messages(turns: &[Turn]) -> Vec<ChatMessage> {
    turns
        .iter()
        .filter(|t| !t.failed)
        .map(|t| match t.who {
            Who::User => ChatMessage::user(t.content.clone()),
            Who::Assistant => ChatMessage::assistant(t.content.clone()),
        })
        .collect()
}

/// Everything the enable/disable rules depend on, in one testable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Readiness {
    pub configured: bool,
    pub available: bool,
    pub has_model: bool,
    pub busy: bool,
    pub has_prompt: bool,
    pub has_sql: bool,
    pub has_error: bool,
    pub has_question: bool,
}

impl Readiness {
    /// A request can be sent at all: provider reachable, key present, model
    /// chosen, nothing already in flight.
    pub fn can_request(&self) -> bool {
        self.configured && self.available && self.has_model && !self.busy
    }

    pub fn can_generate(&self) -> bool {
        self.can_request() && self.has_prompt
    }

    pub fn can_explain(&self) -> bool {
        self.can_request() && self.has_sql
    }

    pub fn can_optimize(&self) -> bool {
        self.can_request() && self.has_sql
    }

    /// Fixing needs both a statement and the error it produced.
    pub fn can_fix(&self) -> bool {
        self.can_request() && self.has_sql && self.has_error
    }

    pub fn can_chat(&self) -> bool {
        self.can_request() && self.has_question
    }
}

/// Which guided empty state to show instead of the controls, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyState {
    /// Status has not come back yet.
    Checking,
    /// Provider needs an API key and has none.
    NeedsApiKey,
    /// Ollama is configured by definition but is not answering.
    OllamaUnavailable,
    /// A remote provider has a key but did not answer.
    Unavailable,
}

pub fn empty_state(provider: AiProviderType, status: Option<ProviderStatus>) -> Option<EmptyState> {
    let Some(status) = status else {
        return Some(EmptyState::Checking);
    };
    if !status.configured {
        return Some(EmptyState::NeedsApiKey);
    }
    if !status.available {
        return Some(if provider == AiProviderType::Ollama {
            EmptyState::OllamaUnavailable
        } else {
            EmptyState::Unavailable
        });
    }
    None
}

/// Colour for the header status dot.
fn status_color(palette: &Palette, status: Option<ProviderStatus>) -> egui::Color32 {
    match status {
        None => palette.muted_foreground,
        Some(s) if s.available && s.configured => palette.success,
        Some(s) if !s.configured => palette.warning,
        Some(_) => palette.destructive,
    }
}

fn status_tooltip(provider: AiProviderType, status: Option<ProviderStatus>) -> String {
    let label = service_ai::provider_info(provider).label;
    match status {
        None => "Checking provider status".to_string(),
        Some(s) if s.available && s.configured => format!("{} connected", label),
        Some(s) if !s.configured => "API key required".to_string(),
        Some(_) => format!("{} not available", label),
    }
}

/// Label for a model in the picker: name plus its description, if any.
pub fn model_label(model: &AiModel) -> String {
    match model.description.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => format!("{} ({})", model.name, d),
        _ => model.name.clone(),
    }
}

// ===========================================================================
// Screen
// ===========================================================================

/// One deferred effect. Immediate mode forbids mutating `app` while the
/// widgets that produced the click still borrow it, so clicks are recorded
/// and acted on after the draw.
enum Intent {
    ChangeProvider(AiProviderType),
    RefreshStatus,
    RefreshModels,
    SaveApiKey,
    SaveSettings,
    RefreshContext,
    Run(Kind),
    ClearConversation,
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) -> AiAction {
    let palette = app.mode.palette();
    let ctx = ui.ctx().clone();

    // --- what the workspace is currently holding --------------------------
    let (connection_id, database, current_sql, last_error) = match app.workspace.as_ref() {
        Some(ws) => {
            let tab = ws.tabs.get(ws.active_tab);
            (
                Some(ws.connection_id.clone()),
                ws.current_database.clone().unwrap_or_default(),
                tab.map(|t| t.sql.clone()).unwrap_or_default(),
                tab.and_then(|t| t.last_error.clone()),
            )
        }
        None => (None, String::new(), String::new(), None),
    };

    let mut intents: Vec<Intent> = Vec::new();

    // Anything that landed since the last frame, plus the follow-up work it
    // implies (a status check after settings load, a model list after a
    // successful status check).
    let (recheck_status, reload_models) = poll(app);
    if recheck_status {
        intents.push(Intent::RefreshStatus);
    }
    if reload_models {
        intents.push(Intent::RefreshModels);
    }

    // First load. Settings come off disk and the keyring, so even this goes
    // through a job rather than stalling the first frame.
    if !app.screens.ai_assistant.settings_requested {
        app.screens.ai_assistant.settings_requested = true;
        let handle = app.rt.handle().clone();
        app.screens
            .ai_assistant
            .settings_job
            .start(&handle, &ctx, async move { service_ai::load_settings() });
    }

    // Schema context is fetched once per connection + database. It is the
    // expensive part (one round trip per table), so it is not re-fetched on
    // every frame or every request.
    if let Some(conn) = connection_id.clone() {
        let key = (conn, database.clone());
        let s = &app.screens.ai_assistant;
        if s.context_key.as_ref() != Some(&key) && !s.context_job.is_running() {
            app.screens.ai_assistant.context_key = Some(key);
            intents.push(Intent::RefreshContext);
        }
    }

    let mut action = AiAction::None;

    {
        let s = &mut app.screens.ai_assistant;
        let status = s.status;
        let provider = s.provider;
        let info = service_ai::provider_info(provider);

        // --- header --------------------------------------------------------
        ui.horizontal(|ui| {
            ui.colored_label(status_color(palette, status), "\u{25CF}")
                .on_hover_text(status_tooltip(provider, status));
            ui.label(text::strong(palette, info.label));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Refresh")
                    .on_hover_text("Re-check provider status")
                    .clicked()
                {
                    intents.push(Intent::RefreshStatus);
                }
                if ui
                    .selectable_label(s.show_context, "Schema")
                    .on_hover_text("View the schema context sent to the model")
                    .clicked()
                {
                    s.show_context = !s.show_context;
                }
                if ui
                    .selectable_label(s.show_settings, "Settings")
                    .on_hover_text("Provider settings and API key")
                    .clicked()
                {
                    s.show_settings = !s.show_settings;
                }
            });
        });
        ui.add_space(4.0);

        // --- provider picker ------------------------------------------------
        ui.label(text::muted(palette, "AI Provider"));
        let mut chosen = provider;
        egui::ComboBox::from_id_salt("ai_provider")
            .selected_text(info.label)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for p in service_ai::PROVIDERS.iter() {
                    let label = format!("{} ({})", p.label, p.description);
                    if ui
                        .selectable_label(p.provider == provider, label)
                        .clicked()
                    {
                        chosen = p.provider;
                    }
                }
            });
        if chosen != provider {
            intents.push(Intent::ChangeProvider(chosen));
        }
        ui.add_space(4.0);

        // --- schema context viewer ------------------------------------------
        if s.show_context {
            let loaded = if s.schema_context.is_empty() {
                "empty"
            } else {
                "loaded"
            };
            ui.horizontal(|ui| {
                ui.label(text::muted(palette, format!("Schema Context ({})", loaded)));
                if ui.small_button("Reload").clicked() {
                    intents.push(Intent::RefreshContext);
                }
                if s.context_job.is_running() {
                    ui.add(egui::Spinner::new());
                }
            });
            if let Some(err) = s.context_error.clone() {
                ui.colored_label(palette.destructive, err);
            }
            if s.schema_context.is_empty() {
                ui.label(text::muted(
                    palette,
                    "No schema context. Connect to a database to give the model your tables.",
                ));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("ai_schema_context")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        let mut body = s.schema_context.clone();
                        ui.add(
                            egui::TextEdit::multiline(&mut body)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
            }
            ui.add_space(4.0);
        }

        // --- provider settings ----------------------------------------------
        if s.show_settings {
            if info.needs_api_key() {
                ui.horizontal(|ui| {
                    ui.label(text::muted(palette, "API Key"));
                    if status.map(|st| st.configured).unwrap_or(false) {
                        ui.label(text::muted(palette, "\u{2022} stored in the OS keyring"));
                    }
                });
                let configured = status.map(|st| st.configured).unwrap_or(false);
                ui.horizontal(|ui| {
                    let hint = if configured {
                        "Enter a new key to replace"
                    } else {
                        "Enter API key"
                    };
                    ui.add(
                        egui::TextEdit::singleline(&mut s.api_key_input)
                            .password(true)
                            .hint_text(hint)
                            .desired_width(ui.available_width() - 90.0),
                    );
                    let saving = s.key_job.is_running();
                    if ui
                        .add_enabled(
                            !saving && !s.api_key_input.trim().is_empty(),
                            egui::Button::new("Save"),
                        )
                        .clicked()
                    {
                        intents.push(Intent::SaveApiKey);
                    }
                    if saving {
                        ui.add(egui::Spinner::new());
                    }
                });
                if let Some(hint) = info.key_hint {
                    ui.label(text::muted(palette, hint));
                }
            } else {
                ui.label(text::muted(
                    palette,
                    "Ollama runs locally and needs no API key. Make sure it is running on this machine.",
                ));
            }

            ui.add_space(4.0);
            ui.label(text::muted(palette, "Base URL"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut s.base_url_input)
                        .desired_width(ui.available_width() - 90.0),
                );
                let dirty = s.base_url_input != s.settings.get(provider).base_url;
                if ui
                    .add_enabled(dirty && !s.save_job.is_running(), egui::Button::new("Save"))
                    .clicked()
                {
                    intents.push(Intent::SaveSettings);
                }
            });
            ui.add_space(4.0);
            ui.separator();
        }

        // --- guided empty states ---------------------------------------------
        match empty_state(provider, status) {
            Some(EmptyState::Checking) => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(text::muted(palette, "Checking provider status"));
                });
            }
            Some(EmptyState::NeedsApiKey) => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(text::strong(
                        palette,
                        format!("API key required for {}", info.label),
                    ));
                    ui.label(text::muted(
                        palette,
                        "The key is stored in your operating system's keyring, not in a settings file.",
                    ));
                    if ui.button("Add API key").clicked() {
                        s.show_settings = true;
                    }
                });
            }
            Some(EmptyState::OllamaUnavailable) => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(text::strong(palette, "Ollama is not available"));
                    ui.label(text::muted(
                        palette,
                        format!(
                            "Nothing answered at {}. Start Ollama, or get it from ollama.ai.",
                            s.settings.get(provider).base_url
                        ),
                    ));
                    if ui.button("Check again").clicked() {
                        intents.push(Intent::RefreshStatus);
                    }
                });
            }
            Some(EmptyState::Unavailable) => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(text::strong(
                        palette,
                        format!("{} did not respond", info.label),
                    ));
                    ui.label(text::muted(
                        palette,
                        "The key is stored but the endpoint could not be reached. Check the base URL and your network.",
                    ));
                    if ui.button("Check again").clicked() {
                        intents.push(Intent::RefreshStatus);
                    }
                });
            }
            None => {
                controls(s, ui, palette, &current_sql, &last_error, &mut intents);
            }
        }

        if let Some(err) = s.error.clone() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(palette.destructive, err);
                if ui.small_button("Dismiss").clicked() {
                    s.error = None;
                }
            });
        }

        ui.add_space(6.0);
        transcript(s, ui, palette, &mut action);
    }

    // --- act on the recorded clicks --------------------------------------
    for intent in intents {
        match intent {
            Intent::ChangeProvider(p) => change_provider(app, &ctx, p),
            Intent::RefreshStatus => refresh_status(app, &ctx),
            Intent::RefreshModels => refresh_models(app, &ctx),
            Intent::SaveApiKey => save_api_key(app, &ctx),
            Intent::SaveSettings => save_settings(app, &ctx),
            Intent::RefreshContext => refresh_context(app, &ctx, connection_id.clone(), &database),
            Intent::Run(kind) => run(app, &ctx, kind, &current_sql, last_error.as_deref()),
            Intent::ClearConversation => {
                app.screens.ai_assistant.turns.clear();
                app.screens.ai_assistant.error = None;
            }
        }
    }

    action
}

/// Model picker, prompt box and the four query actions.
fn controls(
    s: &mut AiAssistant,
    ui: &mut egui::Ui,
    palette: &Palette,
    current_sql: &str,
    last_error: &Option<String>,
    intents: &mut Vec<Intent>,
) {
    let ready = Readiness {
        configured: s.status.map(|st| st.configured).unwrap_or(false),
        available: s.status.map(|st| st.available).unwrap_or(false),
        has_model: !s.model.is_empty(),
        busy: s.busy(),
        has_prompt: !s.prompt.trim().is_empty(),
        has_sql: !current_sql.trim().is_empty(),
        has_error: last_error.is_some(),
        has_question: !s.chat_input.trim().is_empty(),
    };

    // --- model -----------------------------------------------------------
    ui.horizontal(|ui| {
        ui.label(text::muted(palette, "Model"));
        if ui.small_button("Reload").on_hover_text("Fetch the model list again").clicked() {
            intents.push(Intent::RefreshModels);
        }
        if s.models_job.is_running() {
            ui.add(egui::Spinner::new());
        }
    });
    let selected = s
        .models
        .iter()
        .find(|m| m.id == s.model)
        .map(model_label)
        .unwrap_or_else(|| {
            if s.models.is_empty() {
                "No models available".to_string()
            } else {
                "Select a model".to_string()
            }
        });
    let mut picked: Option<String> = None;
    egui::ComboBox::from_id_salt("ai_model")
        .selected_text(selected)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for m in &s.models {
                if ui
                    .selectable_label(m.id == s.model, model_label(m))
                    .clicked()
                {
                    picked = Some(m.id.clone());
                }
            }
        });
    if let Some(id) = picked {
        s.model = id;
    }
    ui.add_space(6.0);

    // --- natural language to SQL -----------------------------------------
    ui.label(text::muted(palette, "Describe what you want"));
    ui.add(
        egui::TextEdit::multiline(&mut s.prompt)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("e.g. Show me all users who signed up last month"),
    );
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                ready.can_generate(),
                egui::Button::new("Generate SQL").fill(palette.primary),
            )
            .clicked()
        {
            intents.push(Intent::Run(Kind::Generate));
        }
        if s.pending == Some(Kind::Generate) {
            ui.add(egui::Spinner::new());
        }
    });
    ui.add_space(6.0);

    // --- actions on the editor's statement --------------------------------
    ui.label(text::muted(palette, "Query Actions"));
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(ready.can_explain(), egui::Button::new("Explain"))
            .on_hover_text("Explain the statement in the active editor tab")
            .on_disabled_hover_text("Type a statement in the editor first")
            .clicked()
        {
            intents.push(Intent::Run(Kind::Explain));
        }
        if ui
            .add_enabled(ready.can_optimize(), egui::Button::new("Optimize"))
            .on_hover_text("Suggest improvements for the current statement")
            .on_disabled_hover_text("Type a statement in the editor first")
            .clicked()
        {
            intents.push(Intent::Run(Kind::Optimize));
        }
        if ui
            .add_enabled(ready.can_fix(), egui::Button::new("Fix Error"))
            .on_hover_text("Rewrite the statement using the last error message")
            .on_disabled_hover_text("Run the statement first so there is an error to fix")
            .clicked()
        {
            intents.push(Intent::Run(Kind::Fix));
        }
        if let Some(kind) = s.pending {
            if kind != Kind::Generate {
                ui.add(egui::Spinner::new());
                ui.label(text::muted(palette, format!("{}…", kind.label())));
            }
        }
    });
    ui.add_space(6.0);

    // --- chat -------------------------------------------------------------
    ui.label(text::muted(palette, "Ask a question"));
    ui.add(
        egui::TextEdit::multiline(&mut s.chat_input)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .hint_text("e.g. Which table stores refunds?"),
    );
    ui.horizontal(|ui| {
        if ui
            .add_enabled(ready.can_chat(), egui::Button::new("Send"))
            .clicked()
        {
            intents.push(Intent::Run(Kind::Chat));
        }
        if s.pending == Some(Kind::Chat) {
            ui.add(egui::Spinner::new());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!s.turns.is_empty(), egui::Button::new("Clear"))
                .on_hover_text("Discard the conversation")
                .clicked()
            {
                intents.push(Intent::ClearConversation);
            }
        });
    });
}

/// The conversation, newest at the bottom.
fn transcript(
    s: &mut AiAssistant,
    ui: &mut egui::Ui,
    palette: &Palette,
    action: &mut AiAction,
) {
    ui.separator();
    if s.turns.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(text::muted(
                palette,
                "Answers appear here. Generated SQL is never run for you.",
            ));
        });
        return;
    }

    let mut copy: Option<String> = None;
    let mut insert: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt("ai_transcript")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (i, turn) in s.turns.iter().enumerate() {
                ui.push_id(i, |ui| {
                    let fill = match turn.who {
                        Who::User => palette.secondary,
                        Who::Assistant => palette.card,
                    };
                    egui::Frame::none()
                        .fill(fill)
                        .stroke(egui::Stroke::new(1.0_f32, palette.border))
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if turn.failed {
                                    ui.colored_label(palette.destructive, turn.heading.clone());
                                } else {
                                    ui.label(text::strong(palette, turn.heading.clone()));
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(ms) = turn.duration_ms {
                                            ui.label(text::muted(palette, format_duration(ms)));
                                        }
                                    },
                                );
                            });

                            let body = egui::RichText::new(turn.content.clone())
                                .monospace()
                                .color(if turn.failed {
                                    palette.destructive
                                } else {
                                    palette.foreground
                                });
                            ui.add(egui::Label::new(body).wrap());

                            if turn.who == Who::Assistant && !turn.failed {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if let Some(sql) = turn.sql.clone() {
                                        if ui
                                            .button("Insert into editor")
                                            .on_hover_text(
                                                "Puts the statement in the editor. It is not run.",
                                            )
                                            .clicked()
                                        {
                                            insert = Some(sql.clone());
                                        }
                                        if ui.small_button("Copy SQL").clicked() {
                                            copy = Some(sql);
                                        }
                                    }
                                    if ui.small_button("Copy answer").clicked() {
                                        copy = Some(turn.content.clone());
                                    }
                                });
                            }
                        });
                });
                ui.add_space(6.0);
            }
        });

    if let Some(t) = copy {
        ui.output_mut(|o| o.copied_text = t);
    }
    if let Some(sql) = insert {
        // Handed back for the caller to place in the editor. Nothing here
        // executes it, and nothing downstream is asked to.
        *action = AiAction::UseSql(sql);
    }
}

// ===========================================================================
// Background work
// ===========================================================================

/// Collect everything that landed since the last frame.
///
/// Returns `(recheck status, reload models)`: the follow-up work a landed
/// result implies. It is returned rather than started here because starting
/// a job needs the frame's `egui::Context`, which the caller has.
fn poll(app: &mut DbHiveApp) -> (bool, bool) {
    let mut toast_error: Option<String> = None;
    let mut refresh_status_now = false;
    let mut refresh_models_now = false;

    {
        let s = &mut app.screens.ai_assistant;

        if let Some(result) = s.settings_job.take_result() {
            match result {
                Ok(settings) => {
                    s.provider = settings.active_provider;
                    s.base_url_input = settings.get(settings.active_provider).base_url.clone();
                    s.settings = settings;
                    refresh_status_now = true;
                }
                Err(e) => {
                    // Defaults still work; say so rather than failing silently.
                    s.error = Some(format!("Using default AI settings: {}", e));
                    s.base_url_input = s.settings.get(s.provider).base_url.clone();
                    refresh_status_now = true;
                }
            }
        }

        if let Some(Err(e)) = s.save_job.take_result() {
            toast_error = Some(format!("Could not save AI settings: {}", e));
        }

        if let Some(result) = s.key_job.take_result() {
            match result {
                Ok(()) => refresh_status_now = true,
                Err(e) => toast_error = Some(format!("Could not store the API key: {}", e)),
            }
        }

        if let Some(result) = s.status_job.take_result() {
            match result {
                Ok(status) => {
                    s.status = Some(status);
                    if status.available && status.configured && s.models_requested_for != Some(s.provider)
                    {
                        refresh_models_now = true;
                    }
                }
                Err(e) => {
                    s.status = Some(ProviderStatus {
                        provider: s.provider,
                        available: false,
                        configured: false,
                    });
                    s.error = Some(e.to_string());
                }
            }
        }

        if let Some(result) = s.models_job.take_result() {
            match result {
                Ok(models) => {
                    let default_model = s.settings.get(s.provider).default_model.clone();
                    s.model = service_ai::preferred_model(&models, &s.model, &default_model)
                        .unwrap_or_default();
                    s.models = models;
                }
                Err(e) => {
                    s.models.clear();
                    s.model.clear();
                    s.error = Some(format!("Could not list models: {}", e));
                }
            }
        }

        if let Some(result) = s.context_job.take_result() {
            match result {
                Ok(context) => {
                    s.schema_context = context;
                    s.context_error = None;
                }
                Err(e) => {
                    // Not fatal: the model still answers, with less to go on.
                    s.schema_context.clear();
                    s.context_error = Some(format!("Schema context unavailable: {}", e));
                }
            }
        }

        if let Some((kind, result)) = s.request_job.take_result() {
            s.pending = None;
            match result {
                Ok(response) => {
                    let mut turn = Turn::assistant(kind.label(), response.content);
                    turn.duration_ms = Some(response.duration_ms);
                    s.turns.push(turn);
                }
                Err(e) => {
                    let message = e.to_string();
                    s.turns.push(Turn::failure(kind.label(), message.clone()));
                    s.error = Some(message);
                }
            }
        }
    }

    if let Some(message) = toast_error {
        app.toasts.error(message);
    }
    (refresh_status_now, refresh_models_now)
}

fn change_provider(app: &mut DbHiveApp, ctx: &egui::Context, provider: AiProviderType) {
    {
        let s = &mut app.screens.ai_assistant;
        s.provider = provider;
        s.status = None;
        s.models.clear();
        s.models_requested_for = None;
        s.model.clear();
        s.api_key_input.clear();
        s.base_url_input = s.settings.get(provider).base_url.clone();
        s.settings.active_provider = provider;
        s.error = None;
    }
    let settings = app.screens.ai_assistant.settings.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .save_job
        .start(&handle, ctx, async move { service_ai::save_settings(&settings) });
    refresh_status(app, ctx);
}

fn refresh_status(app: &mut DbHiveApp, ctx: &egui::Context) {
    let s = &app.screens.ai_assistant;
    if s.status_job.is_running() {
        return;
    }
    let settings = s.settings.clone();
    let provider = s.provider;
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .status_job
        .start(&handle, ctx, async move {
            service_ai::provider_status(&settings, provider).await
        });
}

fn refresh_models(app: &mut DbHiveApp, ctx: &egui::Context) {
    let s = &app.screens.ai_assistant;
    if s.models_job.is_running() {
        return;
    }
    let settings = s.settings.clone();
    let provider = s.provider;
    let handle = app.rt.handle().clone();
    app.screens.ai_assistant.models_requested_for = Some(provider);
    app.screens
        .ai_assistant
        .models_job
        .start(&handle, ctx, async move {
            service_ai::list_models(&settings, provider).await
        });
}

/// Hand the typed key to the OS keyring and forget it immediately.
fn save_api_key(app: &mut DbHiveApp, ctx: &egui::Context) {
    let s = &mut app.screens.ai_assistant;
    let provider = s.provider;
    let key = std::mem::take(&mut s.api_key_input);
    if key.trim().is_empty() {
        return;
    }
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .key_job
        .start(&handle, ctx, async move {
            // `key` is moved in and dropped with the future. It is never
            // stored in the screen, the settings, or a log line.
            service_ai::set_api_key(provider, &key)
        });
}

fn save_settings(app: &mut DbHiveApp, ctx: &egui::Context) {
    let s = &mut app.screens.ai_assistant;
    let provider = s.provider;
    let base_url = s.base_url_input.trim().to_string();
    if base_url.is_empty() {
        s.error = Some("Base URL cannot be empty".to_string());
        return;
    }
    s.settings.get_mut(provider).base_url = base_url;
    let settings = s.settings.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .save_job
        .start(&handle, ctx, async move { service_ai::save_settings(&settings) });
    refresh_status(app, ctx);
}

fn refresh_context(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    connection_id: Option<String>,
    database: &str,
) {
    let Some(connection_id) = connection_id else {
        app.screens.ai_assistant.schema_context.clear();
        return;
    };
    let state = app.state.clone();
    let database = database.to_string();
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .context_job
        .start(&handle, ctx, async move {
            service_ai::schema_context(&state, &connection_id, &database, false).await
        });
}

/// Start one request. Never touches a driver: this produces text only.
fn run(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    kind: Kind,
    current_sql: &str,
    last_error: Option<&str>,
) {
    let s = &mut app.screens.ai_assistant;
    if s.busy() {
        return;
    }
    let schema_context = s.schema_context.clone();
    let sql = current_sql.trim().to_string();

    let (task, prompt_turn) = match kind {
        Kind::Generate => {
            let prompt = s.prompt.trim().to_string();
            if prompt.is_empty() {
                return;
            }
            (
                AiTask::Generate {
                    prompt: prompt.clone(),
                    schema_context,
                },
                Turn::user("You", prompt),
            )
        }
        Kind::Explain => {
            if sql.is_empty() {
                s.error = Some("No SQL to explain. Enter a query in the editor first.".into());
                return;
            }
            (
                AiTask::Explain { sql: sql.clone() },
                Turn::user("Explain this", sql),
            )
        }
        Kind::Optimize => {
            if sql.is_empty() {
                s.error = Some("No SQL to optimize. Enter a query in the editor first.".into());
                return;
            }
            (
                AiTask::Optimize {
                    sql: sql.clone(),
                    schema_context,
                },
                Turn::user("Optimize this", sql),
            )
        }
        Kind::Fix => {
            if sql.is_empty() {
                s.error = Some("No SQL to fix. Enter a query in the editor first.".into());
                return;
            }
            let Some(error_message) = last_error.map(str::to_string) else {
                s.error = Some("No error to fix. Run the query first to see the error.".into());
                return;
            };
            (
                AiTask::Fix {
                    sql: sql.clone(),
                    error_message: error_message.clone(),
                    schema_context,
                },
                Turn::user("Fix this", format!("{}\n\nError: {}", sql, error_message)),
            )
        }
        Kind::Chat => {
            let question = s.chat_input.trim().to_string();
            if question.is_empty() {
                return;
            }
            let history = history_messages(&s.turns);
            let messages = service_ai::build_chat_messages(
                &schema_context,
                &history,
                &question,
                s.settings.max_history_messages,
            );
            (AiTask::Chat { messages }, Turn::user("You", question))
        }
    };

    s.turns.push(prompt_turn);
    s.error = None;
    s.pending = Some(kind);
    match kind {
        Kind::Generate => s.prompt.clear(),
        Kind::Chat => s.chat_input.clear(),
        _ => {}
    }

    let settings = s.settings.clone();
    let provider = s.provider;
    let model = if s.model.is_empty() {
        None
    } else {
        Some(s.model.clone())
    };
    let handle = app.rt.handle().clone();
    app.screens
        .ai_assistant
        .request_job
        .start(&handle, ctx, async move {
            (
                kind,
                service_ai::run_task(settings, provider, model, task).await,
            )
        });
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::ai::ChatRole;

    fn model(id: &str, name: &str, description: Option<&str>) -> AiModel {
        AiModel {
            id: id.to_string(),
            name: name.to_string(),
            provider: AiProviderType::Ollama,
            description: description.map(str::to_string),
            context_window: None,
        }
    }

    fn status(available: bool, configured: bool) -> ProviderStatus {
        ProviderStatus {
            provider: AiProviderType::OpenAI,
            available,
            configured,
        }
    }

    // --- formatting -------------------------------------------------------

    #[test]
    fn duration_switches_to_seconds_at_one_second() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(999), "999ms");
        assert_eq!(format_duration(1000), "1.0s");
        assert_eq!(format_duration(2500), "2.5s");
    }

    #[test]
    fn model_label_appends_a_description_when_there_is_one() {
        assert_eq!(model_label(&model("a", "Llama", Some("8B"))), "Llama (8B)");
        assert_eq!(model_label(&model("a", "Llama", None)), "Llama");
        assert_eq!(model_label(&model("a", "Llama", Some("  "))), "Llama");
    }

    // --- provider and model lists -----------------------------------------

    #[test]
    fn the_picker_offers_the_same_five_providers_as_the_web_shell() {
        let labels: Vec<&str> = service_ai::PROVIDERS.iter().map(|p| p.label).collect();
        assert_eq!(
            labels,
            vec!["Ollama", "OpenAI", "Claude", "Gemini", "OpenRouter"]
        );
    }

    #[test]
    fn model_list_is_deduplicated_before_it_reaches_the_picker() {
        let models = service_ai::dedupe_models(vec![
            model("llama3.2", "llama3.2", None),
            model("llama3.2", "llama3.2", None),
            model("", "broken", None),
            model("mistral", "mistral", None),
        ]);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["llama3.2", "mistral"]);
    }

    #[test]
    fn selected_model_survives_a_reload_that_still_offers_it() {
        let models = vec![model("a", "a", None), model("b", "b", None)];
        assert_eq!(
            service_ai::preferred_model(&models, "b", "a"),
            Some("b".to_string())
        );
        // Gone from the list: fall back to the configured default.
        assert_eq!(
            service_ai::preferred_model(&models, "vanished", "a"),
            Some("a".to_string())
        );
        assert_eq!(service_ai::preferred_model(&[], "a", "a"), None);
    }

    // --- readiness rules --------------------------------------------------

    fn ready() -> Readiness {
        Readiness {
            configured: true,
            available: true,
            has_model: true,
            busy: false,
            has_prompt: true,
            has_sql: true,
            has_error: true,
            has_question: true,
        }
    }

    #[test]
    fn nothing_can_be_sent_without_a_key_a_provider_and_a_model() {
        for broken in [
            Readiness {
                configured: false,
                ..ready()
            },
            Readiness {
                available: false,
                ..ready()
            },
            Readiness {
                has_model: false,
                ..ready()
            },
            Readiness {
                busy: true,
                ..ready()
            },
        ] {
            assert!(!broken.can_request());
            assert!(!broken.can_generate());
            assert!(!broken.can_explain());
            assert!(!broken.can_fix());
            assert!(!broken.can_chat());
        }
    }

    #[test]
    fn each_action_requires_its_own_input() {
        let r = ready();
        assert!(r.can_generate() && r.can_explain() && r.can_optimize() && r.can_fix());

        let no_prompt = Readiness {
            has_prompt: false,
            ..ready()
        };
        assert!(!no_prompt.can_generate());
        assert!(no_prompt.can_explain());

        let no_sql = Readiness {
            has_sql: false,
            ..ready()
        };
        assert!(!no_sql.can_explain());
        assert!(!no_sql.can_optimize());
        assert!(!no_sql.can_fix());
        assert!(no_sql.can_generate());

        // Fixing needs the error as well as the statement.
        let no_error = Readiness {
            has_error: false,
            ..ready()
        };
        assert!(!no_error.can_fix());
        assert!(no_error.can_explain());

        let no_question = Readiness {
            has_question: false,
            ..ready()
        };
        assert!(!no_question.can_chat());
    }

    // --- empty states -----------------------------------------------------

    #[test]
    fn empty_states_follow_the_provider_status() {
        assert_eq!(
            empty_state(AiProviderType::OpenAI, None),
            Some(EmptyState::Checking)
        );
        assert_eq!(
            empty_state(AiProviderType::OpenAI, Some(status(true, false))),
            Some(EmptyState::NeedsApiKey)
        );
        assert_eq!(
            empty_state(AiProviderType::Ollama, Some(status(false, true))),
            Some(EmptyState::OllamaUnavailable)
        );
        assert_eq!(
            empty_state(AiProviderType::OpenAI, Some(status(false, true))),
            Some(EmptyState::Unavailable)
        );
        assert_eq!(empty_state(AiProviderType::OpenAI, Some(status(true, true))), None);
    }

    // --- transcript to messages -------------------------------------------

    #[test]
    fn history_keeps_roles_and_drops_failures() {
        let turns = vec![
            Turn::user("You", "count users"),
            Turn::assistant("Chat", "```sql\nSELECT count(*) FROM users;\n```"),
            Turn::failure("Chat", "Connection refused"),
        ];
        let msgs = history_messages(&turns);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, ChatRole::User);
        assert_eq!(msgs[1].role, ChatRole::Assistant);
        assert!(!msgs.iter().any(|m| m.content.contains("Connection refused")));
    }

    #[test]
    fn a_chat_turn_sends_system_prompt_trimmed_history_and_the_question() {
        let turns: Vec<Turn> = (0..10)
            .map(|i| Turn::user("You", format!("q{}", i)))
            .collect();
        let history = history_messages(&turns);
        let msgs =
            service_ai::build_chat_messages("Database: shop\n\nTables:\n", &history, "and now?", 4);

        assert_eq!(msgs[0].role, ChatRole::System);
        assert!(msgs[0].content.contains("Database: shop"));
        // system + 4 kept + the new question
        assert_eq!(msgs.len(), 6);
        assert_eq!(msgs[1].content, "q6");
        assert_eq!(msgs.last().unwrap().content, "and now?");
        assert_eq!(msgs.last().unwrap().role, ChatRole::User);
    }

    // --- SQL extraction, as the transcript uses it -------------------------

    #[test]
    fn an_answer_with_a_sql_fence_offers_that_statement() {
        let turn = Turn::assistant(
            "Generate SQL",
            "Here you go:\n```sql\nSELECT * FROM users LIMIT 10;\n```\nThat lists ten users.",
        );
        assert_eq!(turn.sql.as_deref(), Some("SELECT * FROM users LIMIT 10;"));
    }

    #[test]
    fn the_first_of_several_sql_fences_wins() {
        let turn = Turn::assistant(
            "Optimize",
            "Before:\n```sql\nSELECT a FROM t;\n```\nAfter:\n```sql\nSELECT b FROM t;\n```",
        );
        assert_eq!(turn.sql.as_deref(), Some("SELECT a FROM t;"));
    }

    #[test]
    fn a_bare_statement_is_still_insertable() {
        let turn = Turn::assistant("Fix Error", "SELECT * FROM users;");
        assert_eq!(turn.sql.as_deref(), Some("SELECT * FROM users;"));
    }

    #[test]
    fn a_prose_answer_offers_nothing_to_insert() {
        let turn = Turn::assistant(
            "Explain",
            "This query reads every row of the users table and counts them.",
        );
        assert!(turn.sql.is_none());
    }

    #[test]
    fn a_failed_turn_never_offers_sql() {
        let turn = Turn::failure("Generate SQL", "SELECT never ran: provider unreachable");
        assert!(turn.sql.is_none());
        assert!(turn.failed);
    }

    // --- schema context ----------------------------------------------------

    #[test]
    fn schema_context_is_built_in_the_shape_the_prompts_expect() {
        use dbhive_core::service_ai::{SchemaColumn, SchemaTable};
        let tables = vec![SchemaTable {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![SchemaColumn {
                name: "id".into(),
                data_type: "integer".into(),
            }],
        }];
        let context = service_ai::build_schema_context("shop", &tables);
        assert_eq!(
            context,
            "Database: shop\n\nTables:\n\npublic.users:\n  - id: integer\n"
        );
        // And it is what the chat system prompt carries.
        let prompt = service_ai::chat_system_prompt(&context);
        assert!(prompt.contains("public.users"));
        assert!(prompt.contains("DATABASE SCHEMA"));
    }

    #[test]
    fn an_empty_schema_context_does_not_pad_the_system_prompt() {
        assert!(!service_ai::chat_system_prompt("").contains("DATABASE SCHEMA"));
    }
}
