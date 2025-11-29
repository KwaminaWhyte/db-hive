//! AI Assistant Commands
//!
//! Tauri commands for AI-powered SQL assistance using Ollama.

use crate::ai::{ChatMessage, OllamaClient, OllamaConfig};
use crate::models::DbError;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

/// AI Assistant state
pub struct AiState {
    pub client: Mutex<Option<OllamaClient>>,
    pub config: Mutex<OllamaConfig>,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            config: Mutex::new(OllamaConfig::default()),
        }
    }
}

/// Ollama model information for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub name: String,
    pub size: String,
    pub modified: String,
}

/// AI chat request
#[derive(Debug, Deserialize)]
pub struct AiChatRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub schema_context: Option<String>,
}

/// AI chat response
#[derive(Debug, Serialize)]
pub struct AiChatResponse {
    pub content: String,
    pub model: String,
    pub duration_ms: u64,
}

/// Check if Ollama is available
#[tauri::command]
pub async fn check_ollama_status(
    state: State<'_, AiState>,
) -> Result<bool, DbError> {
    let client = get_or_create_client(&state)?;
    Ok(client.is_available().await)
}

/// Get Ollama configuration
#[tauri::command]
pub async fn get_ai_config(
    state: State<'_, AiState>,
) -> Result<OllamaConfig, DbError> {
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
    Ok(config.clone())
}

/// Update Ollama configuration
#[tauri::command]
pub async fn set_ai_config(
    state: State<'_, AiState>,
    config: OllamaConfig,
) -> Result<(), DbError> {
    // Update config
    {
        let mut current_config = state.config.lock()
            .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
        *current_config = config.clone();
    }

    // Reset client to use new config
    {
        let mut client = state.client.lock()
            .map_err(|e| DbError::AiError(format!("Failed to access client: {}", e)))?;
        *client = Some(OllamaClient::with_config(config));
    }

    Ok(())
}

/// List available Ollama models
#[tauri::command]
pub async fn list_ai_models(
    state: State<'_, AiState>,
) -> Result<Vec<AiModel>, DbError> {
    let client = get_or_create_client(&state)?;

    let models = client.list_models().await
        .map_err(|e| DbError::AiError(e))?;

    let ai_models: Vec<AiModel> = models.into_iter().map(|m| {
        // Format size in human-readable format
        let size = if m.size >= 1_000_000_000 {
            format!("{:.1} GB", m.size as f64 / 1_000_000_000.0)
        } else if m.size >= 1_000_000 {
            format!("{:.1} MB", m.size as f64 / 1_000_000.0)
        } else {
            format!("{} bytes", m.size)
        };

        AiModel {
            name: m.name,
            size,
            modified: m.modified_at,
        }
    }).collect();

    Ok(ai_models)
}

/// Generate SQL from natural language
#[tauri::command]
pub async fn ai_generate_sql(
    state: State<'_, AiState>,
    prompt: String,
    schema_context: String,
    model: Option<String>,
) -> Result<AiChatResponse, DbError> {
    let client = get_or_create_client(&state)?;

    let start = std::time::Instant::now();
    let sql = client.generate_sql(&prompt, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: sql,
        model: model.unwrap_or_else(|| {
            state.config.lock().map(|c| c.default_model.clone()).unwrap_or_default()
        }),
        duration_ms,
    })
}

/// Explain a SQL query
#[tauri::command]
pub async fn ai_explain_query(
    state: State<'_, AiState>,
    sql: String,
    model: Option<String>,
) -> Result<AiChatResponse, DbError> {
    let client = get_or_create_client(&state)?;

    let start = std::time::Instant::now();
    let explanation = client.explain_query(&sql, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: explanation,
        model: model.unwrap_or_else(|| {
            state.config.lock().map(|c| c.default_model.clone()).unwrap_or_default()
        }),
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
) -> Result<AiChatResponse, DbError> {
    let client = get_or_create_client(&state)?;

    let start = std::time::Instant::now();
    let optimization = client.optimize_query(&sql, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: optimization,
        model: model.unwrap_or_else(|| {
            state.config.lock().map(|c| c.default_model.clone()).unwrap_or_default()
        }),
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
) -> Result<AiChatResponse, DbError> {
    let client = get_or_create_client(&state)?;

    let start = std::time::Instant::now();
    let fixed = client.fix_query(&sql, &error_message, &schema_context, model.as_deref()).await
        .map_err(|e| DbError::AiError(e))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: fixed,
        model: model.unwrap_or_else(|| {
            state.config.lock().map(|c| c.default_model.clone()).unwrap_or_default()
        }),
        duration_ms,
    })
}

/// General chat with the AI
#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AiState>,
    messages: Vec<ChatMessage>,
    model: Option<String>,
) -> Result<AiChatResponse, DbError> {
    let client = get_or_create_client(&state)?;

    let start = std::time::Instant::now();
    let response = client.chat(messages, model.as_deref(), Some(0.7)).await
        .map_err(|e| DbError::AiError(e))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(AiChatResponse {
        content: response.message.content,
        model: response.model,
        duration_ms,
    })
}

/// Helper to get or create the Ollama client
fn get_or_create_client(state: &State<'_, AiState>) -> Result<OllamaClient, DbError> {
    let mut client_guard = state.client.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access client: {}", e)))?;

    if client_guard.is_none() {
        let config = state.config.lock()
            .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
        *client_guard = Some(OllamaClient::with_config(config.clone()));
    }

    // Clone the client for use
    let config = state.config.lock()
        .map_err(|e| DbError::AiError(format!("Failed to access config: {}", e)))?;
    Ok(OllamaClient::with_config(config.clone()))
}
