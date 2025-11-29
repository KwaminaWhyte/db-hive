//! Anthropic AI Provider
//!
//! Provides integration with Anthropic's Claude API.
//! Supports Claude 3.5 Sonnet, Claude 3 Opus, Claude 3 Haiku, etc.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::provider::{AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole, TokenUsage};

/// Anthropic API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicConfig {
    /// API key for Anthropic
    pub api_key: String,
    /// Base URL for Anthropic API
    pub base_url: String,
    /// Default model to use
    pub default_model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-sonnet-4-20250514".to_string(),
            timeout_secs: 120,
        }
    }
}

/// Anthropic message request format
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Anthropic message format
#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

/// Convert our messages to Anthropic format, extracting system message
fn convert_messages(messages: Vec<ChatMessage>) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system_message = None;
    let mut anthropic_messages = Vec::new();

    for msg in messages {
        match msg.role {
            ChatRole::System => {
                // Anthropic uses a separate system parameter
                system_message = Some(msg.content);
            }
            ChatRole::User => {
                anthropic_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: msg.content,
                });
            }
            ChatRole::Assistant => {
                anthropic_messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: msg.content,
                });
            }
        }
    }

    (system_message, anthropic_messages)
}

/// Anthropic response format
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Anthropic error response
#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: AnthropicError,
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    message: String,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    error_type: String,
}

/// Available Claude models with their context windows
const CLAUDE_MODELS: &[(&str, &str, u32)] = &[
    ("claude-sonnet-4-20250514", "Claude Sonnet 4", 200_000),
    ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet", 200_000),
    ("claude-3-5-haiku-20241022", "Claude 3.5 Haiku", 200_000),
    ("claude-3-opus-20240229", "Claude 3 Opus", 200_000),
    ("claude-3-sonnet-20240229", "Claude 3 Sonnet", 200_000),
    ("claude-3-haiku-20240307", "Claude 3 Haiku", 200_000),
];

/// Anthropic API provider
pub struct AnthropicProvider {
    client: Client,
    config: AnthropicConfig,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider
    pub fn new(api_key: String) -> Self {
        let mut config = AnthropicConfig::default();
        config.api_key = api_key;
        Self::with_config(config)
    }

    /// Create a new Anthropic provider with custom configuration
    pub fn with_config(config: AnthropicConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &AnthropicConfig {
        &self.config
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::Anthropic
    }

    async fn is_available(&self) -> bool {
        if self.config.api_key.is_empty() {
            return false;
        }

        // Try a simple request to check availability
        // We'll just verify the API key format is valid
        !self.config.api_key.is_empty()
    }

    async fn list_models(&self) -> Result<Vec<AiModel>, String> {
        // Anthropic doesn't have a models list endpoint, so we return hardcoded list
        let models = CLAUDE_MODELS
            .iter()
            .map(|(id, name, context)| AiModel {
                id: id.to_string(),
                name: name.to_string(),
                provider: AiProviderType::Anthropic,
                description: Some(format!("{}K context", context / 1000)),
                context_window: Some(*context),
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
            return Err("Anthropic API key not configured".to_string());
        }

        let url = format!("{}/v1/messages", self.config.base_url);

        let (system, anthropic_messages) = convert_messages(messages);

        // Ensure we have at least one message
        if anthropic_messages.is_empty() {
            return Err("At least one non-system message is required".to_string());
        }

        let request_body = AnthropicRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            messages: anthropic_messages,
            max_tokens: max_tokens.unwrap_or(4096),
            system,
            temperature,
        };

        let response = self.client.post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error: AnthropicErrorResponse = response.json().await
                .unwrap_or(AnthropicErrorResponse {
                    error: AnthropicError {
                        message: "Unknown error".to_string(),
                        error_type: "unknown".to_string(),
                    },
                });
            return Err(format!("Anthropic API error: {}", error.error.message));
        }

        let api_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Extract text content
        let content = api_response.content
            .iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.clone())
            .collect::<Vec<String>>()
            .join("\n");

        Ok(ChatCompletion {
            content,
            model: api_response.model,
            provider: AiProviderType::Anthropic,
            usage: Some(TokenUsage {
                prompt_tokens: api_response.usage.input_tokens,
                completion_tokens: api_response.usage.output_tokens,
                total_tokens: api_response.usage.input_tokens + api_response.usage.output_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let (system, msgs) = convert_messages(messages);

        assert_eq!(system, Some("You are helpful".to_string()));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn test_models_list() {
        assert!(!CLAUDE_MODELS.is_empty());
        assert!(CLAUDE_MODELS.iter().any(|(id, _, _)| id.contains("sonnet")));
    }
}
