//! AI assistant view logic, lifted from the egui `ai_assistant` screen.
//! See lib.rs.
//!
//! The plain-data transcript (`Kind`, `Who`, `Turn`), the returned
//! `AiAction`, the enable/disable rules (`Readiness`), the guided empty
//! states, the status dot's meaning, label and duration formatting, and the
//! per-request validation that turns the panel's inputs into an `AiTask`.
//!
//! Prompt construction, SQL extraction and history trimming stay in
//! `dbhive_core::service_ai`; this module only calls them.

use dbhive_core::ai::{AiModel, AiProviderType, ChatMessage};
use dbhive_core::service_ai::{self, AiTask, ProviderStatus};

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
    pub fn user(heading: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            who: Who::User,
            heading: heading.into(),
            content: content.into(),
            sql: None,
            duration_ms: None,
            failed: false,
        }
    }

    pub fn assistant(heading: impl Into<String>, content: impl Into<String>) -> Self {
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

    pub fn failure(heading: impl Into<String>, message: impl Into<String>) -> Self {
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

/// What the header status dot means. The shell maps it to a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    /// Status unknown: still checking.
    Muted,
    /// Reachable and configured.
    Success,
    /// Missing its API key.
    Warning,
    /// Configured but not answering.
    Destructive,
}

pub fn status_tone(status: Option<ProviderStatus>) -> StatusTone {
    match status {
        None => StatusTone::Muted,
        Some(s) if s.available && s.configured => StatusTone::Success,
        Some(s) if !s.configured => StatusTone::Warning,
        Some(_) => StatusTone::Destructive,
    }
}

pub fn status_tooltip(provider: AiProviderType, status: Option<ProviderStatus>) -> String {
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

/// Text shown in the closed model picker.
pub fn selected_model_text(models: &[AiModel], selected_id: &str) -> String {
    models
        .iter()
        .find(|m| m.id == selected_id)
        .map(model_label)
        .unwrap_or_else(|| {
            if models.is_empty() {
                "No models available".to_string()
            } else {
                "Select a model".to_string()
            }
        })
}

/// Everything a request is built from, snapshotted from the panel and the
/// active editor tab.
#[derive(Debug, Clone, Copy)]
pub struct RequestInputs<'a> {
    pub prompt: &'a str,
    pub chat_input: &'a str,
    pub current_sql: &'a str,
    pub last_error: Option<&'a str>,
    pub schema_context: &'a str,
    pub turns: &'a [Turn],
    pub max_history_messages: usize,
}

/// What starting a request of some `Kind` comes to.
#[derive(Debug, Clone)]
pub enum RequestPlan {
    /// Nothing to send and nothing worth saying (an empty prompt box).
    Skip,
    /// Refused, with the message to show.
    Refuse(String),
    /// Send `task`, and append `turn` to the transcript as the user's side.
    Send { task: AiTask, turn: Turn },
}

/// Validate the inputs for `kind` and build the task and the user's turn.
///
/// Never touches a driver: this produces text only.
pub fn plan_request(kind: Kind, input: RequestInputs<'_>) -> RequestPlan {
    let schema_context = input.schema_context.to_string();
    let sql = input.current_sql.trim().to_string();

    match kind {
        Kind::Generate => {
            let prompt = input.prompt.trim().to_string();
            if prompt.is_empty() {
                return RequestPlan::Skip;
            }
            RequestPlan::Send {
                task: AiTask::Generate {
                    prompt: prompt.clone(),
                    schema_context,
                },
                turn: Turn::user("You", prompt),
            }
        }
        Kind::Explain => {
            if sql.is_empty() {
                return RequestPlan::Refuse(
                    "No SQL to explain. Enter a query in the editor first.".into(),
                );
            }
            RequestPlan::Send {
                task: AiTask::Explain { sql: sql.clone() },
                turn: Turn::user("Explain this", sql),
            }
        }
        Kind::Optimize => {
            if sql.is_empty() {
                return RequestPlan::Refuse(
                    "No SQL to optimize. Enter a query in the editor first.".into(),
                );
            }
            RequestPlan::Send {
                task: AiTask::Optimize {
                    sql: sql.clone(),
                    schema_context,
                },
                turn: Turn::user("Optimize this", sql),
            }
        }
        Kind::Fix => {
            if sql.is_empty() {
                return RequestPlan::Refuse(
                    "No SQL to fix. Enter a query in the editor first.".into(),
                );
            }
            let Some(error_message) = input.last_error.map(str::to_string) else {
                return RequestPlan::Refuse(
                    "No error to fix. Run the query first to see the error.".into(),
                );
            };
            RequestPlan::Send {
                task: AiTask::Fix {
                    sql: sql.clone(),
                    error_message: error_message.clone(),
                    schema_context,
                },
                turn: Turn::user("Fix this", format!("{}\n\nError: {}", sql, error_message)),
            }
        }
        Kind::Chat => {
            let question = input.chat_input.trim().to_string();
            if question.is_empty() {
                return RequestPlan::Skip;
            }
            let history = history_messages(input.turns);
            let messages = service_ai::build_chat_messages(
                &schema_context,
                &history,
                &question,
                input.max_history_messages,
            );
            RequestPlan::Send {
                task: AiTask::Chat { messages },
                turn: Turn::user("You", question),
            }
        }
    }
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
        assert_eq!(
            empty_state(AiProviderType::OpenAI, Some(status(true, true))),
            None
        );
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
        assert!(!msgs
            .iter()
            .any(|m| m.content.contains("Connection refused")));
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

    // --- status dot, picker text, request planning --------------------------

    #[test]
    fn status_tone_follows_the_provider_status() {
        assert_eq!(status_tone(None), StatusTone::Muted);
        assert_eq!(status_tone(Some(status(true, true))), StatusTone::Success);
        assert_eq!(status_tone(Some(status(true, false))), StatusTone::Warning);
        assert_eq!(status_tone(Some(status(false, false))), StatusTone::Warning);
        assert_eq!(
            status_tone(Some(status(false, true))),
            StatusTone::Destructive
        );
    }

    #[test]
    fn status_tooltip_names_the_provider() {
        let p = AiProviderType::OpenAI;
        assert_eq!(status_tooltip(p, None), "Checking provider status");
        assert_eq!(
            status_tooltip(p, Some(status(true, true))),
            "OpenAI connected"
        );
        assert_eq!(
            status_tooltip(p, Some(status(true, false))),
            "API key required"
        );
        assert_eq!(
            status_tooltip(p, Some(status(false, true))),
            "OpenAI not available"
        );
    }

    #[test]
    fn selected_model_text_distinguishes_empty_list_from_no_selection() {
        let models = vec![model("a", "Llama", Some("8B"))];
        assert_eq!(selected_model_text(&models, "a"), "Llama (8B)");
        assert_eq!(selected_model_text(&models, "zzz"), "Select a model");
        assert_eq!(selected_model_text(&[], "a"), "No models available");
    }

    fn inputs<'a>(turns: &'a [Turn]) -> RequestInputs<'a> {
        RequestInputs {
            prompt: "  list users  ",
            chat_input: "  which table?  ",
            current_sql: "  SELECT 1  ",
            last_error: Some("syntax error"),
            schema_context: "ctx",
            turns,
            max_history_messages: 4,
        }
    }

    #[test]
    fn empty_prompt_or_question_is_skipped_silently() {
        let base = inputs(&[]);
        let blank = RequestInputs {
            prompt: "   ",
            chat_input: "",
            ..base
        };
        assert!(matches!(
            plan_request(Kind::Generate, blank),
            RequestPlan::Skip
        ));
        assert!(matches!(plan_request(Kind::Chat, blank), RequestPlan::Skip));
    }

    #[test]
    fn statement_actions_refuse_without_sql_and_fix_without_an_error() {
        let base = inputs(&[]);
        let no_sql = RequestInputs {
            current_sql: "  ",
            ..base
        };
        for kind in [Kind::Explain, Kind::Optimize, Kind::Fix] {
            match plan_request(kind, no_sql) {
                RequestPlan::Refuse(m) => assert!(m.starts_with("No SQL to")),
                other => panic!("{:?} should refuse, got {:?}", kind, other),
            }
        }
        let no_error = RequestInputs {
            last_error: None,
            ..base
        };
        match plan_request(Kind::Fix, no_error) {
            RequestPlan::Refuse(m) => assert!(m.starts_with("No error to fix")),
            other => panic!("expected refusal, got {:?}", other),
        }
    }

    #[test]
    fn planned_turns_carry_trimmed_input() {
        let base = inputs(&[]);
        match plan_request(Kind::Generate, base) {
            RequestPlan::Send {
                task:
                    AiTask::Generate {
                        prompt,
                        schema_context,
                    },
                turn,
            } => {
                assert_eq!(prompt, "list users");
                assert_eq!(schema_context, "ctx");
                assert_eq!(turn, Turn::user("You", "list users"));
            }
            other => panic!("unexpected {:?}", other),
        }
        match plan_request(Kind::Fix, base) {
            RequestPlan::Send {
                task: AiTask::Fix {
                    sql, error_message, ..
                },
                turn,
            } => {
                assert_eq!(sql, "SELECT 1");
                assert_eq!(error_message, "syntax error");
                assert_eq!(turn.heading, "Fix this");
                assert_eq!(turn.content, "SELECT 1\n\nError: syntax error");
            }
            other => panic!("unexpected {:?}", other),
        }
        match plan_request(Kind::Explain, base) {
            RequestPlan::Send {
                task: AiTask::Explain { sql },
                turn,
            } => {
                assert_eq!(sql, "SELECT 1");
                assert_eq!(turn.heading, "Explain this");
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn a_chat_plan_includes_history_but_not_the_new_turn() {
        let turns = vec![Turn::user("You", "earlier"), Turn::failure("Chat", "boom")];
        match plan_request(Kind::Chat, inputs(&turns)) {
            RequestPlan::Send {
                task: AiTask::Chat { messages },
                turn,
            } => {
                let contents: Vec<&str> = messages.iter().map(|m| m.content.as_str()).collect();
                assert!(contents.contains(&"earlier"));
                assert!(!contents.contains(&"boom"));
                assert_eq!(*contents.last().unwrap(), "which table?");
                assert_eq!(turn, Turn::user("You", "which table?"));
            }
            other => panic!("unexpected {:?}", other),
        }
    }
}
