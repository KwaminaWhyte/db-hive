//! OpenAI AI Provider
//!
//! Provides integration with OpenAI's API for GPT models.
//! Supports GPT-4, GPT-4 Turbo, GPT-3.5 Turbo, etc.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::provider::{AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole, TokenUsage};

/// OpenAI API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiConfig {
    /// API key for OpenAI
    pub api_key: String,
    /// Base URL for OpenAI API (default: https://api.openai.com/v1)
    pub base_url: String,
    /// Default model to use
    pub default_model: String,
    /// Organization ID (optional)
    pub organization: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: "gpt-4o-mini".to_string(),
            organization: None,
            timeout_secs: 120,
        }
    }
}

/// OpenAI chat request format
#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// OpenAI message format
#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

impl From<&ChatMessage> for OpenAiMessage {
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

/// OpenAI chat completion response
#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    id: String,
    model: String,
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// OpenAI models list response
#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelInfo>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelInfo {
    id: String,
    #[allow(dead_code)]
    created: u64,
    #[allow(dead_code)]
    owned_by: String,
}

/// OpenAI error response
#[derive(Debug, Deserialize)]
struct OpenAiErrorResponse {
    error: OpenAiError,
}

#[derive(Debug, Deserialize)]
struct OpenAiError {
    message: String,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    error_type: Option<String>,
}

/// OpenAI API provider
pub struct OpenAiProvider {
    client: Client,
    config: OpenAiConfig,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider
    pub fn new(api_key: String) -> Self {
        let mut config = OpenAiConfig::default();
        config.api_key = api_key;
        Self::with_config(config)
    }

    /// Create a new OpenAI provider with custom configuration
    pub fn with_config(config: OpenAiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &OpenAiConfig {
        &self.config
    }

    /// Get model context window size
    fn get_context_window(model_id: &str) -> Option<u32> {
        match model_id {
            m if m.contains("gpt-4o") => Some(128_000),
            m if m.contains("gpt-4-turbo") => Some(128_000),
            m if m.contains("gpt-4-32k") => Some(32_768),
            m if m.contains("gpt-4") => Some(8_192),
            m if m.contains("gpt-3.5-turbo-16k") => Some(16_384),
            m if m.contains("gpt-3.5-turbo") => Some(4_096),
            m if m.contains("o1") => Some(128_000),
            _ => None,
        }
    }

    /// Get human-readable model name
    fn get_model_name(model_id: &str) -> String {
        match model_id {
            "gpt-4o" => "GPT-4o".to_string(),
            "gpt-4o-mini" => "GPT-4o Mini".to_string(),
            "gpt-4-turbo" => "GPT-4 Turbo".to_string(),
            "gpt-4-turbo-preview" => "GPT-4 Turbo Preview".to_string(),
            "gpt-4" => "GPT-4".to_string(),
            "gpt-4-32k" => "GPT-4 32K".to_string(),
            "gpt-3.5-turbo" => "GPT-3.5 Turbo".to_string(),
            "gpt-3.5-turbo-16k" => "GPT-3.5 Turbo 16K".to_string(),
            "o1" => "O1".to_string(),
            "o1-mini" => "O1 Mini".to_string(),
            "o1-preview" => "O1 Preview".to_string(),
            _ => model_id.to_string(),
        }
    }

    /// Check if model is a chat model
    fn is_chat_model(model_id: &str) -> bool {
        model_id.contains("gpt-4")
            || model_id.contains("gpt-3.5-turbo")
            || model_id.starts_with("o1")
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::OpenAI
    }

    async fn is_available(&self) -> bool {
        if self.config.api_key.is_empty() {
            return false;
        }

        // Check by listing models
        let url = format!("{}/models", self.config.base_url);

        let mut request = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key));

        if let Some(org) = &self.config.organization {
            request = request.header("OpenAI-Organization", org);
        }

        match request.send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn list_models(&self) -> Result<Vec<AiModel>, String> {
        if self.config.api_key.is_empty() {
            return Err("OpenAI API key not configured".to_string());
        }

        let url = format!("{}/models", self.config.base_url);

        let mut request = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key));

        if let Some(org) = &self.config.organization {
            request = request.header("OpenAI-Organization", org);
        }

        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to connect to OpenAI: {}", e))?;

        if !response.status().is_success() {
            let error: OpenAiErrorResponse = response.json().await
                .unwrap_or(OpenAiErrorResponse {
                    error: OpenAiError {
                        message: "Unknown error".to_string(),
                        error_type: None,
                    },
                });
            return Err(format!("OpenAI API error: {}", error.error.message));
        }

        let data: OpenAiModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Filter to only chat models and sort by preference
        let mut models: Vec<AiModel> = data.data
            .into_iter()
            .filter(|m| Self::is_chat_model(&m.id))
            .map(|m| {
                let context_window = Self::get_context_window(&m.id);
                let description = context_window
                    .map(|c| format!("{}K context", c / 1000))
                    .or_else(|| Some("Chat model".to_string()));

                AiModel {
                    id: m.id.clone(),
                    name: Self::get_model_name(&m.id),
                    provider: AiProviderType::OpenAI,
                    description,
                    context_window,
                }
            })
            .collect();

        // Sort by preference (GPT-4o first, then other GPT-4 models, then GPT-3.5)
        models.sort_by(|a, b| {
            let priority_a = Self::model_priority(&a.id);
            let priority_b = Self::model_priority(&b.id);
            priority_a.cmp(&priority_b)
        });

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
            return Err("OpenAI API key not configured".to_string());
        }

        let url = format!("{}/chat/completions", self.config.base_url);

        let openai_messages: Vec<OpenAiMessage> = messages.iter().map(|m| m.into()).collect();

        let request_body = OpenAiChatRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            messages: openai_messages,
            temperature,
            max_tokens,
        };

        let mut request = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json");

        if let Some(org) = &self.config.organization {
            request = request.header("OpenAI-Organization", org);
        }

        let response = request
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error: OpenAiErrorResponse = response.json().await
                .unwrap_or(OpenAiErrorResponse {
                    error: OpenAiError {
                        message: "Unknown error".to_string(),
                        error_type: None,
                    },
                });
            return Err(format!("OpenAI API error: {}", error.error.message));
        }

        let chat_response: OpenAiChatResponse = response
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
            provider: AiProviderType::OpenAI,
            usage: chat_response.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }
}

impl OpenAiProvider {
    /// Model priority for sorting (lower is better)
    fn model_priority(model_id: &str) -> u8 {
        match model_id {
            "gpt-4o" => 0,
            "gpt-4o-mini" => 1,
            m if m.starts_with("o1") => 2,
            "gpt-4-turbo" => 3,
            "gpt-4-turbo-preview" => 4,
            m if m.contains("gpt-4") => 5,
            m if m.contains("gpt-3.5-turbo-16k") => 6,
            m if m.contains("gpt-3.5-turbo") => 7,
            _ => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_name() {
        assert_eq!(OpenAiProvider::get_model_name("gpt-4o"), "GPT-4o");
        assert_eq!(OpenAiProvider::get_model_name("gpt-4o-mini"), "GPT-4o Mini");
        assert_eq!(OpenAiProvider::get_model_name("gpt-3.5-turbo"), "GPT-3.5 Turbo");
    }

    #[test]
    fn test_is_chat_model() {
        assert!(OpenAiProvider::is_chat_model("gpt-4o"));
        assert!(OpenAiProvider::is_chat_model("gpt-3.5-turbo"));
        assert!(!OpenAiProvider::is_chat_model("text-embedding-ada-002"));
    }

    #[test]
    fn test_context_window() {
        assert_eq!(OpenAiProvider::get_context_window("gpt-4o"), Some(128_000));
        assert_eq!(OpenAiProvider::get_context_window("gpt-4"), Some(8_192));
        assert_eq!(OpenAiProvider::get_context_window("unknown"), None);
    }
}
