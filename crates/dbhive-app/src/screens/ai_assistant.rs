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

//!
//! The pure logic (transcript types, readiness rules, empty states, request
//! planning, formatting) lives in `dbhive_ui_model::ai_assistant` and is
//! re-exported here; this file keeps the drawing and the job glue.

use dbhive_core::ai::{AiModel, AiProviderType};
use dbhive_core::models::DbError;
use dbhive_core::service_ai::{self, AiResponse, AiSettings, ProviderStatus};

pub use dbhive_ui_model::ai_assistant::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};

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

/// Colour for the header status dot.
fn status_color(palette: &Palette, status: Option<ProviderStatus>) -> egui::Color32 {
    match status_tone(status) {
        StatusTone::Muted => palette.muted_foreground,
        StatusTone::Success => palette.success,
        StatusTone::Warning => palette.warning,
        StatusTone::Destructive => palette.destructive,
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
                    if ui.selectable_label(p.provider == provider, label).clicked() {
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
        if ui
            .small_button("Reload")
            .on_hover_text("Fetch the model list again")
            .clicked()
        {
            intents.push(Intent::RefreshModels);
        }
        if s.models_job.is_running() {
            ui.add(egui::Spinner::new());
        }
    });
    let selected = selected_model_text(&s.models, &s.model);
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
fn transcript(s: &mut AiAssistant, ui: &mut egui::Ui, palette: &Palette, action: &mut AiAction) {
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

                            let body = egui::RichText::new(turn.content.clone()).monospace().color(
                                if turn.failed {
                                    palette.destructive
                                } else {
                                    palette.foreground
                                },
                            );
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
                    if status.available
                        && status.configured
                        && s.models_requested_for != Some(s.provider)
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
        .start(
            &handle,
            ctx,
            async move { service_ai::save_settings(&settings) },
        );
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
        .start(
            &handle,
            ctx,
            async move { service_ai::save_settings(&settings) },
        );
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

    let plan = plan_request(
        kind,
        RequestInputs {
            prompt: &s.prompt,
            chat_input: &s.chat_input,
            current_sql,
            last_error,
            schema_context: &s.schema_context,
            turns: &s.turns,
            max_history_messages: s.settings.max_history_messages,
        },
    );
    let (task, prompt_turn) = match plan {
        RequestPlan::Skip => return,
        RequestPlan::Refuse(message) => {
            s.error = Some(message);
            return;
        }
        RequestPlan::Send { task, turn } => (task, turn),
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

// Tests for this screen's logic live with it in
// `dbhive_ui_model::ai_assistant`.
