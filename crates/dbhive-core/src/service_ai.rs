//! Shell-agnostic AI assistant operations.
//!
//! The bodies of `src-tauri/src/commands/ai.rs` live here, taking a plain
//! `&Mutex<AppState>` instead of Tauri's `State`/`AppHandle`, so the Tauri
//! shell and the native egui shell run the same code.
//!
//! Two things differ from the Tauri original, deliberately:
//!
//! * **API keys are not part of the config.** `AiState` in the Tauri shell
//!   held every provider's key in memory inside `AiConfig`, and `AiConfig`
//!   was handed to the frontend wholesale by `get_ai_config`. Here the keys
//!   live in the OS keyring ([`CredentialManager`]) under `ai-<provider>`,
//!   and [`AiSettings`] is structurally incapable of holding one: it has no
//!   key field, so nothing can serialise a key into `ai.json` by accident.
//! * **The settings persist.** The Tauri `AiState` was in-memory only, so
//!   the chosen provider and model were lost on restart. They are written to
//!   `ai.json` in the application data directory, next to `profiles.json`.
//!
//! Nothing here executes SQL. `generate_sql` and `fix_query` return text;
//! putting it in an editor, and running it, are separate decisions made by
//! the caller.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::ai::{
    AiModel, AiProvider, AiProviderType, AnthropicConfig, AnthropicProvider, ChatMessage,
    GoogleAiConfig, GoogleAiProvider, OllamaConfig, OllamaProvider, OpenAiConfig, OpenAiProvider,
    OpenRouterConfig, OpenRouterProvider,
};
use crate::credentials::CredentialManager;
use crate::drivers::DatabaseDriver;
use crate::models::DbError;
use crate::state::{AppState, MetadataCache};
use crate::storage::JsonStore;

/// Name of the JSON store the non-secret AI settings are persisted to.
const SETTINGS_STORE: &str = "ai.json";
const SETTINGS_KEY: &str = "settings";

/// Schemas never worth spending context tokens on. Same list the web shell's
/// `useSchemaContext` filtered with.
pub const SYSTEM_SCHEMAS: [&str; 5] = [
    "information_schema",
    "pg_catalog",
    "mysql",
    "performance_schema",
    "sys",
];

/// How many non-system chat messages are kept when trimming a conversation.
///
/// Every message is resent on each turn, so an untrimmed conversation grows
/// the request (and the bill) without bound and eventually overruns the
/// model's context window.
pub const DEFAULT_MAX_HISTORY: usize = 20;

// ===========================================================================
// Providers
// ===========================================================================

/// Static description of a provider, for pickers and empty states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderInfo {
    pub provider: AiProviderType,
    pub label: &'static str,
    pub description: &'static str,
    /// Where the user gets a key. `None` means the provider needs no key.
    pub key_hint: Option<&'static str>,
}

impl ProviderInfo {
    pub fn needs_api_key(&self) -> bool {
        self.key_hint.is_some()
    }
}

/// Every provider, in the order the web shell listed them.
pub const PROVIDERS: [ProviderInfo; 5] = [
    ProviderInfo {
        provider: AiProviderType::Ollama,
        label: "Ollama",
        description: "Local LLM (free, private)",
        key_hint: None,
    },
    ProviderInfo {
        provider: AiProviderType::OpenAI,
        label: "OpenAI",
        description: "GPT-4, GPT-3.5",
        key_hint: Some("Get your key at platform.openai.com"),
    },
    ProviderInfo {
        provider: AiProviderType::Anthropic,
        label: "Claude",
        description: "Claude 3.5, Claude 3",
        key_hint: Some("Get your key at console.anthropic.com"),
    },
    ProviderInfo {
        provider: AiProviderType::Google,
        label: "Gemini",
        description: "Gemini Pro, Gemini Flash",
        key_hint: Some("Get your key at ai.google.dev"),
    },
    ProviderInfo {
        provider: AiProviderType::OpenRouter,
        label: "OpenRouter",
        description: "Multi-model gateway; use IDs like anthropic/claude-3.5-sonnet",
        key_hint: Some("Get your key at openrouter.ai/keys"),
    },
];

pub fn provider_info(provider: AiProviderType) -> &'static ProviderInfo {
    PROVIDERS
        .iter()
        .find(|p| p.provider == provider)
        // PROVIDERS covers every variant of the enum, so this is unreachable
        // unless a variant is added without updating the table.
        .unwrap_or(&PROVIDERS[0])
}

/// Stable, human-readable identifier used for keyring entries and settings
/// keys. Deliberately not `Display`, which renders Anthropic as "Claude".
pub fn provider_slug(provider: AiProviderType) -> &'static str {
    match provider {
        AiProviderType::Ollama => "ollama",
        AiProviderType::OpenAI => "openai",
        AiProviderType::Anthropic => "anthropic",
        AiProviderType::Google => "google",
        AiProviderType::OpenRouter => "openrouter",
    }
}

pub fn provider_from_slug(slug: &str) -> Option<AiProviderType> {
    PROVIDERS
        .iter()
        .find(|p| provider_slug(p.provider) == slug)
        .map(|p| p.provider)
}

// ===========================================================================
// Secrets
// ===========================================================================

/// Keyring entry name for a provider's API key.
///
/// Namespaced with an `ai-` prefix so it cannot collide with a connection
/// profile, whose entries are keyed by UUID.
pub fn key_id(provider: AiProviderType) -> String {
    format!("ai-{}", provider_slug(provider))
}

/// Store a provider's API key in the OS keyring.
///
/// An empty key deletes the entry rather than storing a blank secret, so
/// "clear my key" and "I have no key" are the same state. Ollama needs no
/// key and is a no-op.
pub fn set_api_key(provider: AiProviderType, api_key: &str) -> Result<(), DbError> {
    if !provider_info(provider).needs_api_key() {
        return Ok(());
    }
    let id = key_id(provider);
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        CredentialManager::delete_password(&id)
    } else {
        CredentialManager::save_password(&id, trimmed)
    }
}

/// Read a provider's API key from the OS keyring.
///
/// A keyring failure is reported as "no key" rather than an error: the
/// caller's next step is the same either way, and the status check will
/// surface it as "not configured".
pub fn get_api_key(provider: AiProviderType) -> Option<String> {
    if !provider_info(provider).needs_api_key() {
        return None;
    }
    CredentialManager::get_password(&key_id(provider))
        .ok()
        .flatten()
        .filter(|k| !k.is_empty())
}

pub fn has_api_key(provider: AiProviderType) -> bool {
    get_api_key(provider).is_some()
}

pub fn delete_api_key(provider: AiProviderType) -> Result<(), DbError> {
    CredentialManager::delete_password(&key_id(provider))
}

/// Where API keys are read from and written to.
///
/// Production uses [`KeyringKeyStore`]. The trait exists so a shell can be
/// driven in a test against [`MemoryKeyStore`]: the OS keyring refuses an
/// unsigned test binary, and a test must never write a key into the
/// developer's own keychain either.
pub trait ApiKeyStore: Send + Sync {
    /// The stored key, or `None` when there is none (or the store failed:
    /// the caller's next step is the same).
    fn get(&self, provider: AiProviderType) -> Option<String>;
    /// Store a key; an empty key deletes the entry. A provider that needs no
    /// key is a no-op.
    fn set(&self, provider: AiProviderType, api_key: &str) -> Result<(), DbError>;
    fn delete(&self, provider: AiProviderType) -> Result<(), DbError>;

    fn has(&self, provider: AiProviderType) -> bool {
        self.get(provider).is_some()
    }
}

/// The OS keyring, through the free functions above.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringKeyStore;

impl ApiKeyStore for KeyringKeyStore {
    fn get(&self, provider: AiProviderType) -> Option<String> {
        get_api_key(provider)
    }

    fn set(&self, provider: AiProviderType, api_key: &str) -> Result<(), DbError> {
        set_api_key(provider, api_key)
    }

    fn delete(&self, provider: AiProviderType) -> Result<(), DbError> {
        delete_api_key(provider)
    }
}

/// Keys held in memory only. For tests and previews; nothing is persisted.
#[derive(Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<String, String>>,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

// The derived Debug of a map of secrets would print them; this prints only
// how many there are.
impl std::fmt::Debug for MemoryKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.keys.lock().map(|k| k.len()).unwrap_or(0);
        write!(f, "MemoryKeyStore({} keys)", n)
    }
}

impl ApiKeyStore for MemoryKeyStore {
    fn get(&self, provider: AiProviderType) -> Option<String> {
        if !provider_info(provider).needs_api_key() {
            return None;
        }
        self.keys
            .lock()
            .ok()?
            .get(&key_id(provider))
            .cloned()
            .filter(|k| !k.is_empty())
    }

    fn set(&self, provider: AiProviderType, api_key: &str) -> Result<(), DbError> {
        if !provider_info(provider).needs_api_key() {
            return Ok(());
        }
        let trimmed = api_key.trim();
        let mut keys = self.keys.lock().unwrap();
        if trimmed.is_empty() {
            keys.remove(&key_id(provider));
        } else {
            keys.insert(key_id(provider), trimmed.to_string());
        }
        Ok(())
    }

    fn delete(&self, provider: AiProviderType) -> Result<(), DbError> {
        self.keys.lock().unwrap().remove(&key_id(provider));
        Ok(())
    }
}

// ===========================================================================
// Settings
// ===========================================================================

/// Per-provider settings that are safe to write to disk.
///
/// There is no `api_key` field on purpose; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub base_url: String,
    /// Model used when the caller does not name one.
    pub default_model: String,
    pub timeout_secs: u64,
}

/// The AI configuration, minus every secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub active_provider: AiProviderType,
    pub ollama: ProviderSettings,
    pub openai: ProviderSettings,
    pub anthropic: ProviderSettings,
    pub google: ProviderSettings,
    pub openrouter: ProviderSettings,
    /// Non-system messages kept when a conversation is resent.
    pub max_history_messages: usize,
}

impl Default for AiSettings {
    fn default() -> Self {
        let ollama = OllamaConfig::default();
        let openai = OpenAiConfig::default();
        let anthropic = AnthropicConfig::default();
        let google = GoogleAiConfig::default();
        let openrouter = OpenRouterConfig::default();
        Self {
            active_provider: AiProviderType::default(),
            ollama: ProviderSettings {
                base_url: ollama.base_url,
                default_model: ollama.default_model,
                timeout_secs: ollama.timeout_secs,
            },
            openai: ProviderSettings {
                base_url: openai.base_url,
                default_model: openai.default_model,
                timeout_secs: openai.timeout_secs,
            },
            anthropic: ProviderSettings {
                base_url: anthropic.base_url,
                default_model: anthropic.default_model,
                timeout_secs: anthropic.timeout_secs,
            },
            google: ProviderSettings {
                base_url: google.base_url,
                default_model: google.default_model,
                timeout_secs: google.timeout_secs,
            },
            openrouter: ProviderSettings {
                base_url: openrouter.base_url,
                default_model: openrouter.default_model,
                timeout_secs: openrouter.timeout_secs,
            },
            max_history_messages: DEFAULT_MAX_HISTORY,
        }
    }
}

impl AiSettings {
    pub fn get(&self, provider: AiProviderType) -> &ProviderSettings {
        match provider {
            AiProviderType::Ollama => &self.ollama,
            AiProviderType::OpenAI => &self.openai,
            AiProviderType::Anthropic => &self.anthropic,
            AiProviderType::Google => &self.google,
            AiProviderType::OpenRouter => &self.openrouter,
        }
    }

    pub fn get_mut(&mut self, provider: AiProviderType) -> &mut ProviderSettings {
        match provider {
            AiProviderType::Ollama => &mut self.ollama,
            AiProviderType::OpenAI => &mut self.openai,
            AiProviderType::Anthropic => &mut self.anthropic,
            AiProviderType::Google => &mut self.google,
            AiProviderType::OpenRouter => &mut self.openrouter,
        }
    }

    /// The model a request should use: the caller's choice, else the
    /// provider's configured default.
    pub fn resolve_model(&self, provider: AiProviderType, model: Option<&str>) -> String {
        match model.map(str::trim) {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => self.get(provider).default_model.clone(),
        }
    }
}

/// Read the saved settings. A missing or unreadable entry yields defaults:
/// that is a first run, not a failure.
pub fn load_settings() -> Result<AiSettings, DbError> {
    let store = JsonStore::open(SETTINGS_STORE)?;
    match store.get(SETTINGS_KEY) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| {
            DbError::AiError(format!("Saved AI settings could not be read: {}", e))
        }),
        None => Ok(AiSettings::default()),
    }
}

pub fn save_settings(settings: &AiSettings) -> Result<(), DbError> {
    let mut store = JsonStore::open(SETTINGS_STORE)?;
    let value = serde_json::to_value(settings)
        .map_err(|e| DbError::AiError(format!("Failed to serialise AI settings: {}", e)))?;
    store.set(SETTINGS_KEY, value);
    store.save()
}

/// Persist the active provider, leaving everything else alone.
pub fn set_active_provider(provider: AiProviderType) -> Result<(), DbError> {
    let mut settings = load_settings()?;
    settings.active_provider = provider;
    save_settings(&settings)
}

// ===========================================================================
// Provider construction and status
// ===========================================================================

/// Build a provider client, pulling its API key from the keyring.
///
/// The key is read here and nowhere else, so it exists only for the life of
/// the client and never reaches settings, logs or the UI.
pub fn build_provider(settings: &AiSettings, provider: AiProviderType) -> Box<dyn AiProvider> {
    build_provider_with(&KeyringKeyStore, settings, provider)
}

/// [`build_provider`] reading the key from `store` instead of the keyring.
pub fn build_provider_with(
    store: &dyn ApiKeyStore,
    settings: &AiSettings,
    provider: AiProviderType,
) -> Box<dyn AiProvider> {
    let s = settings.get(provider);
    let api_key = store.get(provider).unwrap_or_default();
    match provider {
        AiProviderType::Ollama => Box::new(OllamaProvider::with_config(OllamaConfig {
            base_url: s.base_url.clone(),
            default_model: s.default_model.clone(),
            timeout_secs: s.timeout_secs,
        })),
        AiProviderType::OpenAI => Box::new(OpenAiProvider::with_config(OpenAiConfig {
            api_key,
            base_url: s.base_url.clone(),
            default_model: s.default_model.clone(),
            organization: None,
            timeout_secs: s.timeout_secs,
        })),
        AiProviderType::Anthropic => Box::new(AnthropicProvider::with_config(AnthropicConfig {
            api_key,
            base_url: s.base_url.clone(),
            default_model: s.default_model.clone(),
            timeout_secs: s.timeout_secs,
        })),
        AiProviderType::Google => Box::new(GoogleAiProvider::with_config(GoogleAiConfig {
            api_key,
            base_url: s.base_url.clone(),
            default_model: s.default_model.clone(),
            timeout_secs: s.timeout_secs,
        })),
        AiProviderType::OpenRouter => Box::new(OpenRouterProvider::with_config(OpenRouterConfig {
            api_key,
            base_url: s.base_url.clone(),
            default_model: s.default_model.clone(),
            timeout_secs: s.timeout_secs,
        })),
    }
}

/// Whether a provider can be reached and whether it has what it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: AiProviderType,
    /// The endpoint answered.
    pub available: bool,
    /// An API key is present (always true for Ollama, which needs none).
    pub configured: bool,
}

/// Port of `check_ai_provider_status`.
pub async fn provider_status(
    settings: &AiSettings,
    provider: AiProviderType,
) -> Result<ProviderStatus, DbError> {
    provider_status_with(&KeyringKeyStore, settings, provider).await
}

/// [`provider_status`] reading the key from `store`.
pub async fn provider_status_with(
    store: &dyn ApiKeyStore,
    settings: &AiSettings,
    provider: AiProviderType,
) -> Result<ProviderStatus, DbError> {
    let client = build_provider_with(store, settings, provider);
    let available = client.is_available().await;
    let configured = !provider_info(provider).needs_api_key() || store.has(provider);
    Ok(ProviderStatus {
        provider,
        available,
        configured,
    })
}

/// Port of `check_ollama_status`.
pub async fn ollama_available(settings: &AiSettings) -> bool {
    provider_status(settings, AiProviderType::Ollama)
        .await
        .map(|s| s.available)
        .unwrap_or(false)
}

/// Port of `list_ai_models`.
pub async fn list_models(
    settings: &AiSettings,
    provider: AiProviderType,
) -> Result<Vec<AiModel>, DbError> {
    list_models_with(&KeyringKeyStore, settings, provider).await
}

/// [`list_models`] reading the key from `store`.
pub async fn list_models_with(
    store: &dyn ApiKeyStore,
    settings: &AiSettings,
    provider: AiProviderType,
) -> Result<Vec<AiModel>, DbError> {
    let client = build_provider_with(store, settings, provider);
    let models = client.list_models().await.map_err(DbError::AiError)?;
    Ok(dedupe_models(models))
}

/// Drop models with no id and keep the first of each duplicate id.
///
/// Ollama happily reports the same model under several tags, and a picker
/// with two identical entries is a bug the user can see.
pub fn dedupe_models(models: Vec<AiModel>) -> Vec<AiModel> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for m in models {
        if m.id.trim().is_empty() || seen.iter().any(|s| s == &m.id) {
            continue;
        }
        seen.push(m.id.clone());
        out.push(m);
    }
    out
}

/// Pick the model to preselect: keep the current one if the list still has
/// it, else the provider default if offered, else the first entry.
pub fn preferred_model(models: &[AiModel], current: &str, default_model: &str) -> Option<String> {
    if models.iter().any(|m| m.id == current) && !current.is_empty() {
        return Some(current.to_string());
    }
    if models.iter().any(|m| m.id == default_model) {
        return Some(default_model.to_string());
    }
    models.first().map(|m| m.id.clone())
}

// ===========================================================================
// Requests
// ===========================================================================

/// A single AI request. One enum rather than five near-identical functions
/// so the UI can carry "what is in flight" as data.
#[derive(Debug, Clone)]
pub enum AiTask {
    /// Natural language to SQL.
    Generate {
        prompt: String,
        schema_context: String,
    },
    Explain {
        sql: String,
    },
    Optimize {
        sql: String,
        schema_context: String,
    },
    Fix {
        sql: String,
        error_message: String,
        schema_context: String,
    },
    /// Free-form conversation. The caller supplies the whole history,
    /// already trimmed.
    Chat {
        messages: Vec<ChatMessage>,
    },
}

/// Port of `AiChatResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiResponse {
    pub content: String,
    pub model: String,
    pub provider: AiProviderType,
    pub duration_ms: u64,
}

/// Run one request against a provider.
///
/// Returns text. It never runs the SQL it produces, and has no connection to
/// run it with.
pub async fn run_task(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    task: AiTask,
) -> Result<AiResponse, DbError> {
    run_task_with(&KeyringKeyStore, settings, provider, model, task).await
}

/// [`run_task`] reading the key from `store`.
pub async fn run_task_with(
    store: &dyn ApiKeyStore,
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    task: AiTask,
) -> Result<AiResponse, DbError> {
    let client = build_provider_with(store, &settings, provider);
    let resolved = settings.resolve_model(provider, model.as_deref());
    let m = Some(resolved.as_str());

    let start = std::time::Instant::now();
    let content = match &task {
        AiTask::Generate {
            prompt,
            schema_context,
        } => client.generate_sql(prompt, schema_context, m).await,
        AiTask::Explain { sql } => client.explain_query(sql, m).await,
        AiTask::Optimize {
            sql,
            schema_context,
        } => client.optimize_query(sql, schema_context, m).await,
        AiTask::Fix {
            sql,
            error_message,
            schema_context,
        } => client.fix_query(sql, error_message, schema_context, m).await,
        AiTask::Chat { messages } => client
            .chat(messages.clone(), m, Some(0.7), None)
            .await
            .map(|c| c.content),
    }
    .map_err(DbError::AiError)?;

    Ok(AiResponse {
        content,
        model: resolved,
        provider,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// Port of `ai_generate_sql`.
pub async fn generate_sql(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    prompt: String,
    schema_context: String,
) -> Result<AiResponse, DbError> {
    run_task(
        settings,
        provider,
        model,
        AiTask::Generate {
            prompt,
            schema_context,
        },
    )
    .await
}

/// Port of `ai_explain_query`.
pub async fn explain_query(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    sql: String,
) -> Result<AiResponse, DbError> {
    run_task(settings, provider, model, AiTask::Explain { sql }).await
}

/// Port of `ai_optimize_query`.
pub async fn optimize_query(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    sql: String,
    schema_context: String,
) -> Result<AiResponse, DbError> {
    run_task(
        settings,
        provider,
        model,
        AiTask::Optimize {
            sql,
            schema_context,
        },
    )
    .await
}

/// Port of `ai_fix_query`.
pub async fn fix_query(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    sql: String,
    error_message: String,
    schema_context: String,
) -> Result<AiResponse, DbError> {
    run_task(
        settings,
        provider,
        model,
        AiTask::Fix {
            sql,
            error_message,
            schema_context,
        },
    )
    .await
}

/// Port of `ai_chat`.
pub async fn chat(
    settings: AiSettings,
    provider: AiProviderType,
    model: Option<String>,
    messages: Vec<ChatMessage>,
) -> Result<AiResponse, DbError> {
    run_task(settings, provider, model, AiTask::Chat { messages }).await
}

// ===========================================================================
// Prompt construction (pure)
// ===========================================================================

/// System prompt for the conversational surface.
///
/// The single-shot actions get their prompts from the `AiProvider` trait's
/// defaults; chat has none of its own, so it is built here. The schema is
/// included when there is one, and left out entirely when there is not,
/// rather than sending an empty "DATABASE SCHEMA:" heading that invites the
/// model to invent tables.
pub fn chat_system_prompt(schema_context: &str) -> String {
    let base = "You are a database assistant embedded in a SQL client. \
Answer questions about SQL and the user's database. \
When you produce SQL, put it in a ```sql fenced code block. \
Never claim to have run a query: you cannot execute anything, the user runs SQL themselves.";

    let schema = schema_context.trim();
    if schema.is_empty() {
        base.to_string()
    } else {
        format!("{}\n\nDATABASE SCHEMA:\n{}", base, schema)
    }
}

/// Build the message list for a chat turn: system prompt, trimmed history,
/// then the new question.
pub fn build_chat_messages(
    schema_context: &str,
    history: &[ChatMessage],
    question: &str,
    max_history: usize,
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::system(chat_system_prompt(schema_context))];
    messages.extend(trim_history(history, max_history));
    messages.push(ChatMessage::user(question.to_string()));
    messages
}

/// Keep the conversation bounded.
///
/// System messages are always kept (they carry the schema, and dropping them
/// mid-conversation makes the model forget the database), plus the most
/// recent `max` user/assistant messages.
pub fn trim_history(messages: &[ChatMessage], max: usize) -> Vec<ChatMessage> {
    use crate::ai::ChatRole;
    let mut kept: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| m.role == ChatRole::System)
        .cloned()
        .collect();

    let recent: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| m.role != ChatRole::System)
        .cloned()
        .collect();
    let start = recent.len().saturating_sub(max);
    kept.extend(recent[start..].iter().cloned());
    kept
}

// ===========================================================================
// SQL extraction (pure)
// ===========================================================================

/// Statement keywords that make a bare response worth offering as SQL.
const SQL_LEADS: [&str; 9] = [
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "WITH", "EXPLAIN",
];

/// Pull runnable SQL out of a model response.
///
/// Prefers the first ```sql fenced block, then any fenced block, then the
/// whole response if it reads like a statement. Returns `None` when the
/// response is prose, so the UI can hide "Insert into editor" instead of
/// offering to paste an explanation into the editor.
///
/// The result is never executed here or by the caller's own doing: it is
/// text handed to the user, who runs it explicitly if they want to.
pub fn extract_sql(content: &str) -> Option<String> {
    for block in fenced_blocks(content) {
        if block.language.eq_ignore_ascii_case("sql") && !block.body.trim().is_empty() {
            return Some(block.body.trim().to_string());
        }
    }
    for block in fenced_blocks(content) {
        let body = block.body.trim();
        if !body.is_empty() && looks_like_sql(body) {
            return Some(body.to_string());
        }
    }
    let trimmed = content.trim();
    if !trimmed.is_empty() && looks_like_sql(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

/// Whether a chunk of text opens with a SQL statement keyword.
pub fn looks_like_sql(text: &str) -> bool {
    let head = text
        .trim_start()
        .lines()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("--"))
        .unwrap_or("")
        .trim_start()
        .to_uppercase();
    SQL_LEADS.iter().any(|k| head.starts_with(k))
}

/// One ```-fenced block: its info string and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedBlock {
    pub language: String,
    pub body: String,
}

/// Split out every fenced code block, in source order.
///
/// Written by hand rather than with a regex because a response often
/// contains several blocks and an unterminated trailing fence, and the
/// behaviour in both cases has to be predictable.
pub fn fenced_blocks(content: &str) -> Vec<FencedBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            match current.take() {
                Some((language, body)) => blocks.push(FencedBlock {
                    language,
                    body: body.join("\n"),
                }),
                None => {
                    let language = trimmed.trim_start_matches('`').trim().to_string();
                    current = Some((language, Vec::new()));
                }
            }
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            body.push(line);
        }
    }
    // An unterminated block is still usable; the model simply ran out of
    // tokens. Keep it rather than silently dropping the answer.
    if let Some((language, body)) = current {
        if !body.is_empty() {
            blocks.push(FencedBlock {
                language,
                body: body.join("\n"),
            });
        }
    }
    blocks
}

// ===========================================================================
// Schema context
// ===========================================================================

/// One table's columns, as the AI context needs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaTable {
    pub schema: String,
    pub table: String,
    pub columns: Vec<SchemaColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaColumn {
    pub name: String,
    pub data_type: String,
}

/// Render the schema context string sent to the model.
///
/// Byte-for-byte the shape `useSchemaContext.buildSchemaContext` produced,
/// so a prompt tuned against the web shell behaves the same here:
///
/// ```text
/// Database: shop
///
/// Tables:
///
/// public.users:
///   - id: integer
/// ```
pub fn build_schema_context(database: &str, tables: &[SchemaTable]) -> String {
    let db = if database.trim().is_empty() {
        "unknown"
    } else {
        database.trim()
    };

    let mut visible: Vec<&SchemaTable> = tables
        .iter()
        .filter(|t| !SYSTEM_SCHEMAS.contains(&t.schema.as_str()))
        .collect();

    // "public" first (Postgres' default), then alphabetical by schema and
    // table, so the ordering does not wander between runs and bust any
    // upstream prompt cache.
    visible.sort_by(|a, b| {
        if a.schema != b.schema {
            if a.schema == "public" {
                return std::cmp::Ordering::Less;
            }
            if b.schema == "public" {
                return std::cmp::Ordering::Greater;
            }
            return a.schema.cmp(&b.schema);
        }
        a.table.cmp(&b.table)
    });

    if visible.is_empty() {
        return format!("Database: {}\n\nNo tables found.", db);
    }

    let mut out = format!("Database: {}\n\nTables:\n", db);
    for t in visible {
        out.push_str(&format!("\n{}.{}:\n", t.schema, t.table));
        for c in &t.columns {
            out.push_str(&format!("  - {}: {}\n", c.name, c.data_type));
        }
    }
    out
}

/// Flatten a metadata cache into the context input.
pub fn tables_from_cache(cache: &MetadataCache) -> Vec<SchemaTable> {
    let mut out = Vec::new();
    for (schema, tables) in &cache.tables {
        for table in tables {
            let key = format!("{}.{}", schema, table.name);
            let columns = cache
                .columns
                .get(&key)
                .map(|cols| {
                    cols.iter()
                        .map(|c| SchemaColumn {
                            name: c.name.clone(),
                            data_type: c.data_type.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(SchemaTable {
                schema: schema.clone(),
                table: table.name.clone(),
                columns,
            });
        }
    }
    out
}

/// Build the schema context for a live connection.
///
/// Uses `AppState`'s metadata cache when it is fresh, which is the same
/// cache SQL autocomplete fills, so opening the assistant after typing in
/// the editor usually costs nothing. Otherwise the metadata is fetched with
/// bounded concurrency: the sequential version issued one round trip per
/// table and took minutes on a large database.
pub async fn schema_context(
    state: &Mutex<AppState>,
    connection_id: &str,
    database: &str,
    force_refresh: bool,
) -> Result<String, DbError> {
    {
        let guard = state.lock().unwrap();
        if let Some(cache) = guard.metadata_cache.get(connection_id) {
            if !force_refresh && !cache.is_stale() && !cache.tables.is_empty() {
                let tables = tables_from_cache(cache);
                return Ok(build_schema_context(database, &tables));
            }
        }
    }

    let connection: Arc<dyn DatabaseDriver> = {
        let guard = state.lock().unwrap();
        guard
            .get_connection(connection_id)
            .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
            .clone()
    };

    let databases = connection.get_databases().await.unwrap_or_default();
    let schema_infos = connection.get_schemas(database).await.unwrap_or_default();

    // Drivers that have no notion of a schema (MySQL, SQLite) report none;
    // for them the database name is what `get_tables` expects. Passing
    // "public" there listed MySQL's internal tables instead of the user's.
    //
    // System schemas are dropped before anything is fetched, not after:
    // pg_catalog alone is a few hundred wasted round trips for tables the
    // context then throws away.
    let mut schema_names: Vec<String> = schema_infos
        .iter()
        .map(|s| s.name.clone())
        .filter(|n| !SYSTEM_SCHEMAS.contains(&n.as_str()))
        .collect();
    if schema_names.is_empty() {
        schema_names.push(database.to_string());
    }

    // Cap on in-flight requests. The sequential version issued one round
    // trip per table and took minutes on a large database; flooding the
    // connection instead is no better.
    const MAX_INFLIGHT: usize = 16;
    use tokio::task::JoinSet;

    let mut table_set: JoinSet<(String, Vec<crate::models::TableInfo>)> = JoinSet::new();
    for schema in schema_names {
        let conn = connection.clone();
        table_set.spawn(async move {
            // A schema that cannot be listed contributes nothing rather than
            // failing the whole context.
            let tables = conn.get_tables(&schema).await.unwrap_or_default();
            (schema, tables)
        });
    }

    let mut tables_by_schema: HashMap<String, Vec<crate::models::TableInfo>> = HashMap::new();
    let mut pending: Vec<(String, String)> = Vec::new();
    while let Some(joined) = table_set.join_next().await {
        let (schema, tables) =
            joined.map_err(|e| DbError::InternalError(format!("Metadata task failed: {}", e)))?;
        for t in &tables {
            pending.push((schema.clone(), t.name.clone()));
        }
        tables_by_schema.insert(schema, tables);
    }

    let mut columns: HashMap<String, Vec<crate::models::ColumnInfo>> = HashMap::new();
    let mut column_set: JoinSet<(String, Vec<crate::models::ColumnInfo>)> = JoinSet::new();
    let mut queue = pending.into_iter();

    let spawn_columns = |set: &mut JoinSet<(String, Vec<crate::models::ColumnInfo>)>,
                         conn: Arc<dyn DatabaseDriver>,
                         schema: String,
                         table: String| {
        set.spawn(async move {
            let key = format!("{}.{}", schema, table);
            // A table whose schema cannot be read still belongs in the
            // context by name; only its columns are missing.
            let cols = match conn.get_table_schema(&schema, &table).await {
                Ok(ts) => ts.columns,
                Err(_) => Vec::new(),
            };
            (key, cols)
        });
    };

    for _ in 0..MAX_INFLIGHT {
        match queue.next() {
            Some((s, t)) => spawn_columns(&mut column_set, connection.clone(), s, t),
            None => break,
        }
    }
    while let Some(joined) = column_set.join_next().await {
        let (key, cols) =
            joined.map_err(|e| DbError::InternalError(format!("Metadata task failed: {}", e)))?;
        columns.insert(key, cols);
        if let Some((s, t)) = queue.next() {
            spawn_columns(&mut column_set, connection.clone(), s, t);
        }
    }

    // Write through to the shared cache, so SQL autocomplete gets the round
    // trips this just paid for, and so a second visit to the panel is free.
    let tables = {
        let mut guard = state.lock().unwrap();
        let cache = guard
            .metadata_cache
            .entry(connection_id.to_string())
            .or_insert_with(MetadataCache::new);
        cache.databases = databases;
        cache.schemas.insert(database.to_string(), schema_infos);
        cache.tables = tables_by_schema;
        cache.columns = columns;
        cache.touch();
        tables_from_cache(cache)
    };

    Ok(build_schema_context(database, &tables))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ChatRole;

    fn model(id: &str) -> AiModel {
        AiModel {
            id: id.to_string(),
            name: id.to_string(),
            provider: AiProviderType::Ollama,
            description: None,
            context_window: None,
        }
    }

    fn table(schema: &str, name: &str, cols: &[(&str, &str)]) -> SchemaTable {
        SchemaTable {
            schema: schema.to_string(),
            table: name.to_string(),
            columns: cols
                .iter()
                .map(|(n, t)| SchemaColumn {
                    name: n.to_string(),
                    data_type: t.to_string(),
                })
                .collect(),
        }
    }

    // --- providers ------------------------------------------------------

    #[test]
    fn every_provider_has_info_and_a_distinct_slug() {
        let all = [
            AiProviderType::Ollama,
            AiProviderType::OpenAI,
            AiProviderType::Anthropic,
            AiProviderType::Google,
            AiProviderType::OpenRouter,
        ];
        let mut slugs: Vec<&str> = all.iter().map(|p| provider_slug(*p)).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), all.len());
        for p in all {
            assert_eq!(provider_info(p).provider, p);
            assert_eq!(provider_from_slug(provider_slug(p)), Some(p));
        }
    }

    #[test]
    fn only_ollama_needs_no_api_key() {
        assert!(!provider_info(AiProviderType::Ollama).needs_api_key());
        for p in [
            AiProviderType::OpenAI,
            AiProviderType::Anthropic,
            AiProviderType::Google,
            AiProviderType::OpenRouter,
        ] {
            assert!(provider_info(p).needs_api_key());
        }
    }

    #[test]
    fn key_ids_are_namespaced_away_from_connection_profiles() {
        assert_eq!(key_id(AiProviderType::OpenAI), "ai-openai");
        assert!(key_id(AiProviderType::Google).starts_with("ai-"));
    }

    // --- key stores -----------------------------------------------------

    #[test]
    fn the_memory_store_keeps_trimmed_keys_and_an_empty_key_clears_one() {
        let store = MemoryKeyStore::new();
        let p = AiProviderType::OpenAI;
        assert!(!store.has(p));
        store.set(p, "  test-key-not-real  ").unwrap();
        assert_eq!(store.get(p).as_deref(), Some("test-key-not-real"));
        // One provider's key is not another's.
        assert!(!store.has(AiProviderType::Anthropic));
        store.set(p, "   ").unwrap();
        assert!(!store.has(p));
        store.set(p, "again").unwrap();
        store.delete(p).unwrap();
        assert!(!store.has(p));
    }

    #[test]
    fn ollama_takes_no_key_in_any_store() {
        let store = MemoryKeyStore::new();
        store.set(AiProviderType::Ollama, "ignored").unwrap();
        assert!(!store.has(AiProviderType::Ollama));
    }

    #[test]
    fn a_store_never_prints_its_keys() {
        let store = MemoryKeyStore::new();
        store.set(AiProviderType::Google, "sentinel-secret-value").unwrap();
        assert!(!format!("{:?}", store).contains("sentinel"));
    }

    #[test]
    fn status_reads_the_key_from_the_store_it_is_given() {
        // Port 1 refuses connections at once, so nothing leaves the machine.
        let mut settings = AiSettings::default();
        settings.openai.base_url = "http://127.0.0.1:1/v1".into();
        let store = MemoryKeyStore::new();
        let p = AiProviderType::OpenAI;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();

        let none = rt.block_on(provider_status_with(&store, &settings, p)).unwrap();
        assert!(!none.configured && !none.available);

        store.set(p, "test-key-not-real").unwrap();
        let keyed = rt.block_on(provider_status_with(&store, &settings, p)).unwrap();
        assert!(keyed.configured, "a key in the store counts as configured");
        assert!(!keyed.available, "but nothing answers on that port");
    }

    // --- settings -------------------------------------------------------

    #[test]
    fn settings_cannot_carry_a_secret_to_disk() {
        let json = serde_json::to_string(&AiSettings::default()).unwrap();
        assert!(!json.to_lowercase().contains("apikey"));
        assert!(!json.to_lowercase().contains("api_key"));
    }

    #[test]
    fn settings_round_trip_and_tolerate_missing_fields() {
        let mut s = AiSettings::default();
        s.active_provider = AiProviderType::Anthropic;
        s.get_mut(AiProviderType::Anthropic).default_model = "claude-x".into();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<AiSettings>(&json).unwrap(), s);

        let partial: AiSettings = serde_json::from_str(r#"{"activeProvider":"google"}"#).unwrap();
        assert_eq!(partial.active_provider, AiProviderType::Google);
        assert_eq!(partial.ollama, AiSettings::default().ollama);
    }

    #[test]
    fn resolve_model_falls_back_to_the_provider_default() {
        let s = AiSettings::default();
        let default = s.get(AiProviderType::Ollama).default_model.clone();
        assert_eq!(s.resolve_model(AiProviderType::Ollama, None), default);
        assert_eq!(s.resolve_model(AiProviderType::Ollama, Some("  ")), default);
        assert_eq!(
            s.resolve_model(AiProviderType::Ollama, Some("codellama")),
            "codellama"
        );
    }

    // --- model lists ----------------------------------------------------

    #[test]
    fn dedupe_models_drops_repeats_and_blanks() {
        let out = dedupe_models(vec![model("a"), model("b"), model("a"), model("")]);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn preferred_model_prefers_current_then_default_then_first() {
        let list = [model("a"), model("b")];
        assert_eq!(preferred_model(&list, "b", "a"), Some("b".into()));
        assert_eq!(preferred_model(&list, "gone", "a"), Some("a".into()));
        assert_eq!(preferred_model(&list, "", "missing"), Some("a".into()));
        assert_eq!(preferred_model(&[], "a", "b"), None);
    }

    // --- prompts and history --------------------------------------------

    #[test]
    fn chat_system_prompt_omits_an_empty_schema() {
        let none = chat_system_prompt("   ");
        assert!(!none.contains("DATABASE SCHEMA"));
        let some = chat_system_prompt("Database: shop");
        assert!(some.contains("DATABASE SCHEMA:\nDatabase: shop"));
    }

    #[test]
    fn build_chat_messages_puts_system_first_and_question_last() {
        let history = vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")];
        let msgs = build_chat_messages("Database: shop", &history, "count users", 10);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, ChatRole::System);
        assert!(msgs[0].content.contains("Database: shop"));
        assert_eq!(msgs[3].role, ChatRole::User);
        assert_eq!(msgs[3].content, "count users");
    }

    #[test]
    fn trim_history_keeps_system_messages_and_the_newest_turns() {
        let history = vec![
            ChatMessage::system("schema"),
            ChatMessage::user("one"),
            ChatMessage::assistant("two"),
            ChatMessage::user("three"),
            ChatMessage::assistant("four"),
        ];
        let kept = trim_history(&history, 2);
        let got: Vec<&str> = kept.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(got, vec!["schema", "three", "four"]);

        // Zero keeps the system prompt only, never an empty request.
        let kept = trim_history(&history, 0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].role, ChatRole::System);

        // A short history is returned intact.
        assert_eq!(trim_history(&history, 100).len(), history.len());
        assert!(trim_history(&[], 5).is_empty());
    }

    // --- SQL extraction -------------------------------------------------

    #[test]
    fn extract_sql_prefers_the_sql_fence() {
        let input = "Sure!\n```sql\nSELECT 1;\n```\nHope that helps.";
        assert_eq!(extract_sql(input), Some("SELECT 1;".to_string()));
    }

    #[test]
    fn extract_sql_reads_the_first_sql_block_of_several() {
        let input = "First:\n```sql\nSELECT a FROM t;\n```\nThen:\n```sql\nSELECT b FROM t;\n```";
        assert_eq!(extract_sql(input), Some("SELECT a FROM t;".to_string()));
    }

    #[test]
    fn extract_sql_skips_a_non_sql_fence_for_a_later_sql_one() {
        let input = "```bash\nollama serve\n```\n```sql\nSELECT 1;\n```";
        assert_eq!(extract_sql(input), Some("SELECT 1;".to_string()));
    }

    #[test]
    fn extract_sql_accepts_an_unlabelled_fence_that_reads_as_sql() {
        let input = "Try:\n```\nUPDATE t SET a = 1;\n```";
        assert_eq!(extract_sql(input), Some("UPDATE t SET a = 1;".to_string()));
    }

    #[test]
    fn extract_sql_accepts_a_bare_statement() {
        assert_eq!(
            extract_sql("  SELECT * FROM users;  "),
            Some("SELECT * FROM users;".to_string())
        );
        assert_eq!(
            extract_sql("-- count them\nSELECT count(*) FROM users;"),
            Some("-- count them\nSELECT count(*) FROM users;".to_string())
        );
    }

    #[test]
    fn extract_sql_returns_none_for_prose() {
        assert_eq!(extract_sql("This query scans the users table."), None);
        assert_eq!(extract_sql(""), None);
        assert_eq!(extract_sql("```\nnot sql at all\n```"), None);
    }

    #[test]
    fn extract_sql_keeps_an_unterminated_block() {
        let input = "```sql\nSELECT 1\nFROM t";
        assert_eq!(extract_sql(input), Some("SELECT 1\nFROM t".to_string()));
    }

    #[test]
    fn fenced_blocks_records_language_and_order() {
        let blocks = fenced_blocks("```sql\na\n```\ntext\n```py\nb\n```");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language, "sql");
        assert_eq!(blocks[0].body, "a");
        assert_eq!(blocks[1].language, "py");
        assert_eq!(blocks[1].body, "b");
    }

    // --- schema context -------------------------------------------------

    #[test]
    fn schema_context_matches_the_web_shell_shape() {
        let tables = vec![table("public", "users", &[("id", "integer"), ("email", "text")])];
        assert_eq!(
            build_schema_context("shop", &tables),
            "Database: shop\n\nTables:\n\npublic.users:\n  - id: integer\n  - email: text\n"
        );
    }

    #[test]
    fn schema_context_skips_system_schemas() {
        let tables = vec![
            table("pg_catalog", "pg_class", &[("oid", "oid")]),
            table("information_schema", "tables", &[]),
            table("app", "orders", &[("id", "int")]),
        ];
        let out = build_schema_context("shop", &tables);
        assert!(!out.contains("pg_class"));
        assert!(!out.contains("information_schema"));
        assert!(out.contains("app.orders"));
    }

    #[test]
    fn schema_context_puts_public_first_then_sorts() {
        let tables = vec![
            table("zeta", "a", &[]),
            table("public", "z", &[]),
            table("public", "a", &[]),
            table("alpha", "a", &[]),
        ];
        let out = build_schema_context("shop", &tables);
        let order: Vec<&str> = out
            .lines()
            .filter(|l| l.ends_with(':') && l.contains('.'))
            .collect();
        assert_eq!(
            order,
            vec!["public.a:", "public.z:", "alpha.a:", "zeta.a:"]
        );
    }

    #[test]
    fn schema_context_handles_nothing_to_say() {
        assert_eq!(
            build_schema_context("shop", &[]),
            "Database: shop\n\nNo tables found."
        );
        assert_eq!(
            build_schema_context("", &[]),
            "Database: unknown\n\nNo tables found."
        );
    }
}
