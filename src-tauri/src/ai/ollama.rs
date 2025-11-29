//! Ollama API Client
//!
//! Provides a client for interacting with the Ollama API for local LLM inference.
//! Default endpoint: http://localhost:11434

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Ollama API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Chat message role
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// Request body for Ollama chat API
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ChatOptions>,
}

/// Model options for chat requests
#[derive(Debug, Serialize)]
struct ChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
}

/// Response from Ollama chat API
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub model: String,
    pub message: ChatMessage,
    pub done: bool,
    #[serde(default)]
    pub total_duration: u64,
    #[serde(default)]
    pub eval_count: u32,
}

/// Model information from Ollama
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub modified_at: String,
    pub size: u64,
    #[serde(default)]
    pub digest: String,
}

/// Response from list models API
#[derive(Debug, Deserialize)]
struct ListModelsResponse {
    models: Vec<OllamaModel>,
}

/// Ollama API client
pub struct OllamaClient {
    client: Client,
    config: OllamaConfig,
}

impl OllamaClient {
    /// Create a new Ollama client with default configuration
    pub fn new() -> Self {
        Self::with_config(OllamaConfig::default())
    }

    /// Create a new Ollama client with custom configuration
    pub fn with_config(config: OllamaConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Check if Ollama is running and accessible
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.config.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<OllamaModel>, String> {
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

        Ok(data.models)
    }

    /// Send a chat message and get a response
    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        model: Option<&str>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/api/chat", self.config.base_url);

        let request = ChatRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            messages,
            stream: false,
            options: Some(ChatOptions {
                temperature,
                num_predict: Some(2048),
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

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(chat_response)
    }

    /// Generate SQL from natural language
    pub async fn generate_sql(
        &self,
        prompt: &str,
        schema_context: &str,
        model: Option<&str>,
    ) -> Result<String, String> {
        let system_prompt = format!(
            r#"You are a SQL expert assistant. Generate SQL queries based on natural language requests.

DATABASE SCHEMA:
{}

RULES:
1. Generate only valid SQL that matches the schema above
2. Use proper table and column names exactly as shown in the schema
3. Include appropriate JOINs when querying related tables
4. Add LIMIT clauses for SELECT queries unless counting
5. Use parameterized placeholders ($1, $2) for user-provided values when appropriate
6. Output ONLY the SQL query, no explanations or markdown

Generate the SQL query for the following request:"#,
            schema_context
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(prompt),
        ];

        let response = self.chat(messages, model, Some(0.1)).await?;

        // Clean up the response - extract SQL from potential markdown
        let sql = self.extract_sql(&response.message.content);
        Ok(sql)
    }

    /// Explain a SQL query in plain English
    pub async fn explain_query(
        &self,
        sql: &str,
        model: Option<&str>,
    ) -> Result<String, String> {
        let system_prompt = r#"You are a SQL expert. Explain SQL queries in clear, simple terms.

Provide:
1. A brief summary of what the query does
2. Step-by-step breakdown of each clause
3. Any potential performance considerations
4. Suggestions for improvement if applicable

Be concise but thorough."#;

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(format!("Explain this SQL query:\n\n```sql\n{}\n```", sql)),
        ];

        let response = self.chat(messages, model, Some(0.3)).await?;
        Ok(response.message.content)
    }

    /// Suggest optimizations for a SQL query
    pub async fn optimize_query(
        &self,
        sql: &str,
        schema_context: &str,
        model: Option<&str>,
    ) -> Result<String, String> {
        let system_prompt = format!(
            r#"You are a SQL performance optimization expert.

DATABASE SCHEMA:
{}

Analyze the provided SQL query and suggest optimizations. Consider:
1. Index usage and suggestions
2. Query structure improvements
3. JOIN optimization
4. Subquery vs JOIN alternatives
5. Potential N+1 query issues
6. Appropriate use of LIMIT/OFFSET

Provide the optimized query and explain the improvements."#,
            schema_context
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(format!("Optimize this SQL query:\n\n```sql\n{}\n```", sql)),
        ];

        let response = self.chat(messages, model, Some(0.3)).await?;
        Ok(response.message.content)
    }

    /// Fix SQL syntax errors
    pub async fn fix_query(
        &self,
        sql: &str,
        error_message: &str,
        schema_context: &str,
        model: Option<&str>,
    ) -> Result<String, String> {
        let system_prompt = format!(
            r#"You are a SQL debugging expert.

DATABASE SCHEMA:
{}

Fix the SQL query based on the error message. Output ONLY the corrected SQL query, no explanations."#,
            schema_context
        );

        let user_prompt = format!(
            "Fix this SQL query:\n\n```sql\n{}\n```\n\nError: {}",
            sql, error_message
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let response = self.chat(messages, model, Some(0.1)).await?;
        let fixed_sql = self.extract_sql(&response.message.content);
        Ok(fixed_sql)
    }

    /// Extract SQL from a response that might contain markdown
    fn extract_sql(&self, content: &str) -> String {
        // Check for SQL code blocks
        if let Some(start) = content.find("```sql") {
            if let Some(end) = content[start + 6..].find("```") {
                return content[start + 6..start + 6 + end].trim().to_string();
            }
        }

        // Check for generic code blocks
        if let Some(start) = content.find("```") {
            let after_start = start + 3;
            // Skip the language identifier if present
            let content_start = content[after_start..]
                .find('\n')
                .map(|i| after_start + i + 1)
                .unwrap_or(after_start);

            if let Some(end) = content[content_start..].find("```") {
                return content[content_start..content_start + end].trim().to_string();
            }
        }

        // Return as-is if no code blocks found
        content.trim().to_string()
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sql() {
        let client = OllamaClient::new();

        // Test with SQL code block
        let input = "Here's the query:\n```sql\nSELECT * FROM users;\n```";
        assert_eq!(client.extract_sql(input), "SELECT * FROM users;");

        // Test with generic code block
        let input = "```\nSELECT * FROM users;\n```";
        assert_eq!(client.extract_sql(input), "SELECT * FROM users;");

        // Test without code blocks
        let input = "SELECT * FROM users;";
        assert_eq!(client.extract_sql(input), "SELECT * FROM users;");
    }

    #[test]
    fn test_chat_message_constructors() {
        let system = ChatMessage::system("You are helpful");
        assert_eq!(system.role, ChatRole::System);

        let user = ChatMessage::user("Hello");
        assert_eq!(user.role, ChatRole::User);

        let assistant = ChatMessage::assistant("Hi there!");
        assert_eq!(assistant.role, ChatRole::Assistant);
    }
}
