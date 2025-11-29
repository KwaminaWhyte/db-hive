//! Ollama AI Provider
//!
//! Provides integration with Ollama for local LLM inference.
//! Default endpoint: http://localhost:11434

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::provider::{AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole, TokenUsage};

/// Ollama API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaConfig {
    /// Base URL for Ollama API (default: http://localhost:11434)
    pub base_url: String,
    /// Default model to use (e.g., "llama3.2", "codellama", "mistral")
    pub default_model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
            default_model: "llama3.2".to_string(),
            timeout_secs: 120,
        }
    }
}

/// Request body for Ollama chat API
#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaChatOptions>,
}

/// Ollama-specific chat message format
#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

impl From<&ChatMessage> for OllamaChatMessage {
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

/// Model options for chat requests
#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
}

/// Response from Ollama chat API
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    model: String,
    message: OllamaResponseMessage,
    #[allow(dead_code)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: String,
}

/// Model information from Ollama
#[derive(Debug, Deserialize)]
struct OllamaModelInfo {
    name: String,
    modified_at: String,
    size: u64,
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    family: Option<String>,
}

/// Response from list models API
#[derive(Debug, Deserialize)]
struct ListModelsResponse {
    models: Vec<OllamaModelInfo>,
}

/// Ollama API client
pub struct OllamaProvider {
    client: Client,
    config: OllamaConfig,
}

impl OllamaProvider {
    /// Create a new Ollama provider with default configuration
    pub fn new() -> Self {
        Self::with_config(OllamaConfig::default())
    }

    /// Create a new Ollama provider with custom configuration
    pub fn with_config(config: OllamaConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &OllamaConfig {
        &self.config
    }

    /// Format size in human-readable format
    fn format_size(size: u64) -> String {
        if size >= 1_000_000_000 {
            format!("{:.1} GB", size as f64 / 1_000_000_000.0)
        } else if size >= 1_000_000 {
            format!("{:.1} MB", size as f64 / 1_000_000.0)
        } else {
            format!("{} bytes", size)
        }
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::Ollama
    }

    async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.config.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn list_models(&self) -> Result<Vec<AiModel>, String> {
        let url = format!("{}/api/tags", self.config.base_url);

        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Ollama API error: {}", response.status()));
        }

        let data: ListModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let models = data.models.into_iter().map(|m| {
            let description = format!(
                "{} ({})",
                m.details.as_ref()
                    .and_then(|d| d.family.as_ref())
                    .map(|f| f.as_str())
                    .unwrap_or("Unknown"),
                Self::format_size(m.size)
            );

            AiModel {
                id: m.name.clone(),
                name: m.name,
                provider: AiProviderType::Ollama,
                description: Some(description),
                context_window: None,
            }
        }).collect();

        Ok(models)
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        model: Option<&str>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<ChatCompletion, String> {
        let url = format!("{}/api/chat", self.config.base_url);

        let ollama_messages: Vec<OllamaChatMessage> = messages.iter().map(|m| m.into()).collect();

        let request = OllamaChatRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            messages: ollama_messages,
            stream: false,
            options: Some(OllamaChatOptions {
                temperature,
                num_predict: max_tokens.map(|t| t as i32),
            }),
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("Ollama API error: {}", error_text));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(ChatCompletion {
            content: chat_response.message.content,
            model: chat_response.model,
            provider: AiProviderType::Ollama,
            usage: Some(TokenUsage {
                prompt_tokens: chat_response.prompt_eval_count,
                completion_tokens: chat_response.eval_count,
                total_tokens: chat_response.prompt_eval_count + chat_response.eval_count,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(OllamaProvider::format_size(500), "500 bytes");
        assert_eq!(OllamaProvider::format_size(1_500_000), "1.5 MB");
        assert_eq!(OllamaProvider::format_size(4_000_000_000), "4.0 GB");
    }

    #[test]
    fn test_chat_message_conversion() {
        let msg = ChatMessage::user("Hello");
        let ollama_msg: OllamaChatMessage = (&msg).into();
        assert_eq!(ollama_msg.role, "user");
        assert_eq!(ollama_msg.content, "Hello");
    }
}
