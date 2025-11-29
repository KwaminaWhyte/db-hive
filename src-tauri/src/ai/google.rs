//! Google AI Provider
//!
//! Provides integration with Google's Gemini API.
//! Supports Gemini Pro, Gemini Ultra, etc.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::provider::{AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole, TokenUsage};

/// Google AI API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAiConfig {
    /// API key for Google AI
    pub api_key: String,
    /// Base URL for Google AI API
    pub base_url: String,
    /// Default model to use
    pub default_model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for GoogleAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            default_model: "gemini-1.5-flash".to_string(),
            timeout_secs: 120,
        }
    }
}

/// Gemini content request format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

/// Convert our messages to Gemini format, extracting system instruction
fn convert_messages(messages: Vec<ChatMessage>) -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let mut system_instruction = None;
    let mut gemini_contents = Vec::new();

    for msg in messages {
        match msg.role {
            ChatRole::System => {
                // Gemini uses a separate system_instruction field
                system_instruction = Some(GeminiContent {
                    role: None,
                    parts: vec![GeminiPart { text: msg.content }],
                });
            }
            ChatRole::User => {
                gemini_contents.push(GeminiContent {
                    role: Some("user".to_string()),
                    parts: vec![GeminiPart { text: msg.content }],
                });
            }
            ChatRole::Assistant => {
                gemini_contents.push(GeminiContent {
                    role: Some("model".to_string()),
                    parts: vec![GeminiPart { text: msg.content }],
                });
            }
        }
    }

    (system_instruction, gemini_contents)
}

/// Gemini response format
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    #[allow(dead_code)]
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}

/// Gemini error response
#[derive(Debug, Deserialize)]
struct GeminiErrorResponse {
    error: GeminiError,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    message: String,
    #[allow(dead_code)]
    status: String,
}

/// Gemini models list response
#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModelInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModelInfo {
    name: String,
    display_name: String,
    description: Option<String>,
    #[serde(default)]
    input_token_limit: Option<u32>,
    #[serde(default)]
    output_token_limit: Option<u32>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

/// Available Gemini models with their context windows (fallback if API unavailable)
const GEMINI_MODELS: &[(&str, &str, u32)] = &[
    ("gemini-2.0-flash", "Gemini 2.0 Flash", 1_048_576),
    ("gemini-1.5-pro", "Gemini 1.5 Pro", 2_097_152),
    ("gemini-1.5-flash", "Gemini 1.5 Flash", 1_048_576),
    ("gemini-1.5-flash-8b", "Gemini 1.5 Flash 8B", 1_048_576),
    ("gemini-1.0-pro", "Gemini 1.0 Pro", 32_760),
];

/// Google AI API provider
pub struct GoogleAiProvider {
    client: Client,
    config: GoogleAiConfig,
}

impl GoogleAiProvider {
    /// Create a new Google AI provider
    pub fn new(api_key: String) -> Self {
        let mut config = GoogleAiConfig::default();
        config.api_key = api_key;
        Self::with_config(config)
    }

    /// Create a new Google AI provider with custom configuration
    pub fn with_config(config: GoogleAiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &GoogleAiConfig {
        &self.config
    }

    /// Check if model supports generateContent method
    fn supports_generate_content(methods: &[String]) -> bool {
        methods.iter().any(|m| m == "generateContent")
    }
}

#[async_trait]
impl AiProvider for GoogleAiProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::Google
    }

    async fn is_available(&self) -> bool {
        if self.config.api_key.is_empty() {
            return false;
        }

        // Check by listing models
        let url = format!("{}/models?key={}", self.config.base_url, self.config.api_key);

        match self.client.get(&url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn list_models(&self) -> Result<Vec<AiModel>, String> {
        if self.config.api_key.is_empty() {
            // Return hardcoded list if no API key
            return Ok(GEMINI_MODELS
                .iter()
                .map(|(id, name, context)| AiModel {
                    id: id.to_string(),
                    name: name.to_string(),
                    provider: AiProviderType::Google,
                    description: Some(format!("{}K context", context / 1000)),
                    context_window: Some(*context),
                })
                .collect());
        }

        let url = format!("{}/models?key={}", self.config.base_url, self.config.api_key);

        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to Google AI: {}", e))?;

        if !response.status().is_success() {
            // Fall back to hardcoded list on error
            return Ok(GEMINI_MODELS
                .iter()
                .map(|(id, name, context)| AiModel {
                    id: id.to_string(),
                    name: name.to_string(),
                    provider: AiProviderType::Google,
                    description: Some(format!("{}K context", context / 1000)),
                    context_window: Some(*context),
                })
                .collect());
        }

        let data: GeminiModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Filter to models that support generateContent
        let models: Vec<AiModel> = data.models
            .into_iter()
            .filter(|m| Self::supports_generate_content(&m.supported_generation_methods))
            .filter(|m| m.name.contains("gemini"))
            .map(|m| {
                // Model name is "models/gemini-xxx", extract just the model id
                let id = m.name.strip_prefix("models/").unwrap_or(&m.name).to_string();
                let context_window = m.input_token_limit;

                AiModel {
                    id,
                    name: m.display_name,
                    provider: AiProviderType::Google,
                    description: m.description.or_else(|| {
                        context_window.map(|c| format!("{}K context", c / 1000))
                    }),
                    context_window,
                }
            })
            .collect();

        if models.is_empty() {
            // Fall back to hardcoded list if no suitable models found
            return Ok(GEMINI_MODELS
                .iter()
                .map(|(id, name, context)| AiModel {
                    id: id.to_string(),
                    name: name.to_string(),
                    provider: AiProviderType::Google,
                    description: Some(format!("{}K context", context / 1000)),
                    context_window: Some(*context),
                })
                .collect());
        }

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
            return Err("Google AI API key not configured".to_string());
        }

        let model_id = model.unwrap_or(&self.config.default_model);
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.config.base_url, model_id, self.config.api_key
        );

        let (system_instruction, contents) = convert_messages(messages);

        // Ensure we have at least one message
        if contents.is_empty() {
            return Err("At least one non-system message is required".to_string());
        }

        let request_body = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                temperature,
                max_output_tokens: max_tokens,
            }),
        };

        let response = self.client.post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error: GeminiErrorResponse = response.json().await
                .unwrap_or(GeminiErrorResponse {
                    error: GeminiError {
                        message: "Unknown error".to_string(),
                        status: "UNKNOWN".to_string(),
                    },
                });
            return Err(format!("Google AI API error: {}", error.error.message));
        }

        let api_response: GeminiResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Extract text content from first candidate
        let content = api_response.candidates
            .first()
            .map(|c| {
                c.content.parts
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        Ok(ChatCompletion {
            content,
            model: model_id.to_string(),
            provider: AiProviderType::Google,
            usage: api_response.usage_metadata.map(|u| TokenUsage {
                prompt_tokens: u.prompt_token_count,
                completion_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
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

        assert!(system.is_some());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role.as_ref().unwrap(), "user");
        assert_eq!(msgs[1].role.as_ref().unwrap(), "model");
    }

    #[test]
    fn test_models_list() {
        assert!(!GEMINI_MODELS.is_empty());
        assert!(GEMINI_MODELS.iter().any(|(id, _, _)| id.contains("gemini")));
    }
}
