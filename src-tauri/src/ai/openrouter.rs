//! OpenRouter AI Provider
//!
//! Provides integration with OpenRouter's OpenAI-compatible API gateway.
//! OpenRouter fronts many models (Anthropic, Google, Meta, OpenAI, etc.)
//! via a single endpoint using the OpenAI `/chat/completions` format.
//!
//! Models are specified with fully-qualified identifiers, e.g.
//! `anthropic/claude-3.5-sonnet`, `openai/gpt-4o`, `google/gemini-2.0-flash-exp`.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::provider::{AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole, TokenUsage};

/// OpenRouter API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterConfig {
    /// API key for OpenRouter
    pub api_key: String,
    /// Base URL for OpenRouter API (default: https://openrouter.ai/api/v1)
    pub base_url: String,
    /// Default model to use (fully qualified, e.g. anthropic/claude-3.5-sonnet)
    pub default_model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            default_model: "anthropic/claude-3.5-sonnet".to_string(),
            timeout_secs: 120,
        }
    }
}

/// OpenAI-compatible chat request format
#[derive(Debug, Serialize)]
struct OpenRouterChatRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenRouterMessage {
    role: String,
    content: String,
}

impl From<&ChatMessage> for OpenRouterMessage {
    fn from(msg: &ChatMessage) -> Self {
        Self {
            role: match msg.role {
                ChatRole::System => "system".to_string(),
                ChatRole::User => "user".to_string(),
                ChatRole::Assistant => "assistant".to_string(),
            },
            content: msg.content.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenRouterChatResponse {
    #[allow(dead_code)]
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: String,
    choices: Vec<OpenRouterChoice>,
    usage: Option<OpenRouterUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
    #[allow(dead_code)]
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// OpenRouter models list response
#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModelInfo>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelInfo {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    context_length: Option<u32>,
}

/// OpenRouter error response (OpenAI-style)
#[derive(Debug, Deserialize)]
struct OpenRouterErrorResponse {
    error: OpenRouterError,
}

#[derive(Debug, Deserialize)]
struct OpenRouterError {
    message: String,
    #[allow(dead_code)]
    #[serde(default, rename = "type")]
    error_type: Option<String>,
}

/// Recommended headers for OpenRouter usage analytics.
const REFERER_HEADER: &str = "https://db-hive.app";
const TITLE_HEADER: &str = "DB Hive";

/// OpenRouter API provider
pub struct OpenRouterProvider {
    client: Client,
    config: OpenRouterConfig,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider
    pub fn new(api_key: String) -> Self {
        let mut config = OpenRouterConfig::default();
        config.api_key = api_key;
        Self::with_config(config)
    }

    /// Create a new OpenRouter provider with custom configuration
    pub fn with_config(config: OpenRouterConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &OpenRouterConfig {
        &self.config
    }
}

#[async_trait]
impl AiProvider for OpenRouterProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::OpenRouter
    }

    async fn is_available(&self) -> bool {
        if self.config.api_key.is_empty() {
            return false;
        }

        let url = format!("{}/models", self.config.base_url);

        let request = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("HTTP-Referer", REFERER_HEADER)
            .header("X-Title", TITLE_HEADER);

        match request.send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn list_models(&self) -> Result<Vec<AiModel>, String> {
        if self.config.api_key.is_empty() {
            return Err("OpenRouter API key not configured".to_string());
        }

        let url = format!("{}/models", self.config.base_url);

        let request = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("HTTP-Referer", REFERER_HEADER)
            .header("X-Title", TITLE_HEADER);

        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to connect to OpenRouter: {}", e))?;

        if !response.status().is_success() {
            let error: OpenRouterErrorResponse = response.json().await
                .unwrap_or(OpenRouterErrorResponse {
                    error: OpenRouterError {
                        message: "Unknown error".to_string(),
                        error_type: None,
                    },
                });
            return Err(format!("OpenRouter API error: {}", error.error.message));
        }

        let data: OpenRouterModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let models: Vec<AiModel> = data.data
            .into_iter()
            .map(|m| AiModel {
                name: m.name.clone().unwrap_or_else(|| m.id.clone()),
                description: m.description.or_else(|| Some("OpenRouter model".to_string())),
                context_window: m.context_length,
                id: m.id,
                provider: AiProviderType::OpenRouter,
            })
            .collect();

        Ok(models)
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        model: Option<&str>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<ChatCompletion, String> {
        if self.config.api_key.is_empty() {
            return Err("OpenRouter API key not configured".to_string());
        }

        let url = format!("{}/chat/completions", self.config.base_url);

        let or_messages: Vec<OpenRouterMessage> = messages.iter().map(|m| m.into()).collect();

        let request_body = OpenRouterChatRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            messages: or_messages,
            temperature,
            max_tokens,
        };

        let request = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", REFERER_HEADER)
            .header("X-Title", TITLE_HEADER);

        let response = request
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error: OpenRouterErrorResponse = response.json().await
                .unwrap_or(OpenRouterErrorResponse {
                    error: OpenRouterError {
                        message: "Unknown error".to_string(),
                        error_type: None,
                    },
                });
            return Err(format!("OpenRouter API error: {}", error.error.message));
        }

        let chat_response: OpenRouterChatResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let content = chat_response.choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(ChatCompletion {
            content,
            model: chat_response.model,
            provider: AiProviderType::OpenRouter,
            usage: chat_response.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = OpenRouterConfig::default();
        assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(cfg.default_model, "anthropic/claude-3.5-sonnet");
        assert!(cfg.api_key.is_empty());
    }

    #[test]
    fn test_provider_type() {
        let p = OpenRouterProvider::new("test-key".to_string());
        assert_eq!(p.provider_type(), AiProviderType::OpenRouter);
    }
}
