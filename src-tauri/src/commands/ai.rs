//! AI Assistant Commands
//!
//! Tauri commands for AI-powered SQL assistance.
//! Supports multiple providers: Ollama, OpenAI, Anthropic, Google.

use crate::ai::{
    AiProvider, AiProviderType, AiModel as ProviderAiModel, ChatMessage, ChatCompletion,
    OllamaProvider, OllamaConfig,
    OpenAiProvider, OpenAiConfig,
    AnthropicProvider, AnthropicConfig,
    GoogleAiProvider, GoogleAiConfig,
};
use crate::models::DbError;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

/// AI configuration for all providers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    /// Active provider
    pub active_provider: AiProviderType,
    /// Ollama configuration
    pub ollama: OllamaConfig,
    /// OpenAI configuration
    pub openai: OpenAiConfig,
    /// Anthropic configuration
    pub anthropic: AnthropicConfig,
    /// Google AI configuration
    pub google: GoogleAiConfig,
}

/// AI Assistant state
pub struct AiState {
    pub config: Mutex<AiConfig>,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            config: Mutex::new(AiConfig::default()),
        }
    }
}

/// AI model information for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelInfo {
    pub id: String,
    pub name: String,
    pub provider: AiProviderType,
    pub description: Option<String>,
    pub context_window: Option<u32>,
}

impl From<ProviderAiModel> for AiModelInfo {
    fn from(m: ProviderAiModel) -> Self {
        Self {
            id: m.id,
            name: m.name,
            provider: m.provider,
            description: m.description,
            context_window: m.context_window,
        }
    }
}

/// AI chat response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatResponse {
    pub content: String,
    pub model: String,
    pub provider: AiProviderType,
    pub duration_ms: u64,
}

impl AiChatResponse {
    fn from_completion(completion: ChatCompletion, duration_ms: u64) -> Self {
        Self {
            content: completion.content,
            model: completion.model,
            provider: completion.provider,
            duration_ms,
        }
    }
}

/// Provider status response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: AiProviderType,
    pub available: bool,
    pub configured: bool,
}

/// Get a provider instance based on the config
fn get_provider(config: &AiConfig, provider_type: Option<AiProviderType>) -> Box<dyn AiProvider> {
    let provider_type = provider_type.unwrap_or(config.active_provider);

    match provider_type {
        AiProviderType::Ollama => Box::new(OllamaProvider::with_config(config.ollama.clone())),
        AiProviderType::OpenAI => Box::new(OpenAiProvider::with_config(config.openai.clone())),
        AiProviderType::Anthropic => Box::new(AnthropicProvider::with_config(config.anthropic.clone())),
        AiProviderType::Google => Box::new(GoogleAiProvider::with_config(config.google.clone())),
    }
}

/// Check provider availability status
#[tauri::command]
pub async fn check_ai_provider_status(
    state: State<'_, AiState>,
    provider: Option<AiProviderType>,
) -> Result<ProviderStatus, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let provider_type = provider.unwrap_or(config.active_provider);
    let ai_provider = get_provider(&config, Some(provider_type));

    let available = ai_provider.is_available().await;

    let configured = match provider_type {
        AiProviderType::Ollama => true, // Ollama doesn't require API key
        AiProviderType::OpenAI => !config.openai.api_key.is_empty(),
        AiProviderType::Anthropic => !config.anthropic.api_key.is_empty(),
        AiProviderType::Google => !config.google.api_key.is_empty(),
    };

    Ok(ProviderStatus {
        provider: provider_type,
        available,
        configured,
    })
}

/// Check if Ollama is available (legacy endpoint for compatibility)
#[tauri::command]
pub async fn check_ollama_status(
    state: State<'_, AiState>,
) -> Result<bool, DbError> {
    let status = check_ai_provider_status(state, Some(AiProviderType::Ollama)).await?;
    Ok(status.available)
}

/// Get AI configuration
#[tauri::command]
pub async fn get_ai_config(
    state: State<'_, AiState>,
) -> Result<AiConfig, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
    Ok(config.clone())
}

/// Update AI configuration
#[tauri::command]
pub async fn set_ai_config(
    state: State<'_, AiState>,
    config: AiConfig,
) -> Result<(), DbError> {
    let mut current_config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
    *current_config = config;
    Ok(())
}

/// Set the active AI provider
#[tauri::command]
pub async fn set_active_ai_provider(
    state: State<'_, AiState>,
    provider: AiProviderType,
) -> Result<(), DbError> {
    let mut config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
    config.active_provider = provider;
    Ok(())
}

/// Set API key for a provider
#[tauri::command]
pub async fn set_ai_api_key(
    state: State<'_, AiState>,
    provider: AiProviderType,
    api_key: String,
) -> Result<(), DbError> {
    let mut config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;

    match provider {
        AiProviderType::Ollama => {} // Ollama doesn't use API keys
        AiProviderType::OpenAI => config.openai.api_key = api_key,
        AiProviderType::Anthropic => config.anthropic.api_key = api_key,
        AiProviderType::Google => config.google.api_key = api_key,
    }

    Ok(())
}

/// List available AI models for a provider
#[tauri::command]
pub async fn list_ai_models(
    state: State<'_, AiState>,
    provider: Option<AiProviderType>,
) -> Result<Vec<AiModelInfo>, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let models = ai_provider.list_models().await
        .map_err(|e| DbError::AiError(e))?;

    Ok(models.into_iter().map(AiModelInfo::from).collect())
}

/// Generate SQL from natural language
#[tauri::command]
pub async fn ai_generate_sql(
    state: State<'_, AiState>,
    prompt: String,
    schema_context: String,
    model: Option<String>,
    provider: Option<AiProviderType>,
) -> Result<AiChatResponse, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let start = std::time::Instant::now();
    let sql = ai_provider.generate_sql(&prompt, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: sql,
        model: model.unwrap_or_else(|| get_default_model(&config, provider)),
        provider: provider.unwrap_or(config.active_provider),
        duration_ms,
    })
}

/// Explain a SQL query
#[tauri::command]
pub async fn ai_explain_query(
    state: State<'_, AiState>,
    sql: String,
    model: Option<String>,
    provider: Option<AiProviderType>,
) -> Result<AiChatResponse, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let start = std::time::Instant::now();
    let explanation = ai_provider.explain_query(&sql, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: explanation,
        model: model.unwrap_or_else(|| get_default_model(&config, provider)),
        provider: provider.unwrap_or(config.active_provider),
        duration_ms,
    })
}

/// Optimize a SQL query
#[tauri::command]
pub async fn ai_optimize_query(
    state: State<'_, AiState>,
    sql: String,
    schema_context: String,
    model: Option<String>,
    provider: Option<AiProviderType>,
) -> Result<AiChatResponse, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let start = std::time::Instant::now();
    let optimization = ai_provider.optimize_query(&sql, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: optimization,
        model: model.unwrap_or_else(|| get_default_model(&config, provider)),
        provider: provider.unwrap_or(config.active_provider),
        duration_ms,
    })
}

/// Fix a SQL query based on an error message
#[tauri::command]
pub async fn ai_fix_query(
    state: State<'_, AiState>,
    sql: String,
    error_message: String,
    schema_context: String,
    model: Option<String>,
    provider: Option<AiProviderType>,
) -> Result<AiChatResponse, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let start = std::time::Instant::now();
    let fixed = ai_provider.fix_query(&sql, &error_message, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: fixed,
        model: model.unwrap_or_else(|| get_default_model(&config, provider)),
        provider: provider.unwrap_or(config.active_provider),
        duration_ms,
    })
}

/// General chat with the AI
#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AiState>,
    messages: Vec<ChatMessage>,
    model: Option<String>,
    provider: Option<AiProviderType>,
) -> Result<AiChatResponse, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?
        .clone();

    let ai_provider = get_provider(&config, provider);

    let start = std::time::Instant::now();
    let completion = ai_provider.chat(messages, model.as_deref(), Some(0.7), None).await
        .map_err(|e| DbError::AiError(e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse::from_completion(completion, duration_ms))
}

/// Get the default model for a provider
fn get_default_model(config: &AiConfig, provider: Option<AiProviderType>) -> String {
    match provider.unwrap_or(config.active_provider) {
        AiProviderType::Ollama => config.ollama.default_model.clone(),
        AiProviderType::OpenAI => config.openai.default_model.clone(),
        AiProviderType::Anthropic => config.anthropic.default_model.clone(),
        AiProviderType::Google => config.google.default_model.clone(),
    }
}
