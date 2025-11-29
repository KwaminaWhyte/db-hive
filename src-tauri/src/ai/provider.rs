//! AI Provider Trait
//!
//! Defines the common interface for all AI providers (Ollama, OpenAI, Claude, Gemini).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// AI Provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderType {
    #[default]
    Ollama,
    OpenAI,
    Anthropic,
    Google,
}

impl std::fmt::Display for AiProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiProviderType::Ollama => write!(f, "Ollama"),
            AiProviderType::OpenAI => write!(f, "OpenAI"),
            AiProviderType::Anthropic => write!(f, "Claude"),
            AiProviderType::Google => write!(f, "Gemini"),
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

/// A chat message (provider-agnostic)
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

/// Model information (provider-agnostic)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub provider: AiProviderType,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
}

/// Chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletion {
    pub content: String,
    pub model: String,
    pub provider: AiProviderType,
    pub usage: Option<TokenUsage>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Common trait for all AI providers
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Get the provider type
    fn provider_type(&self) -> AiProviderType;

    /// Check if the provider is available/configured
    async fn is_available(&self) -> bool;

    /// List available models
    async fn list_models(&self) -> Result<Vec<AiModel>, String>;

    /// Send a chat completion request
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        model: Option<&str>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<ChatCompletion, String>;

    /// Generate SQL from natural language
    async fn generate_sql(
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

        let response = self.chat(messages, model, Some(0.1), Some(2048)).await?;
        Ok(extract_sql(&response.content))
    }

    /// Explain a SQL query in plain English
    async fn explain_query(&self, sql: &str, model: Option<&str>) -> Result<String, String> {
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

        let response = self.chat(messages, model, Some(0.3), Some(2048)).await?;
        Ok(response.content)
    }

    /// Suggest optimizations for a SQL query
    async fn optimize_query(
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

        let response = self.chat(messages, model, Some(0.3), Some(2048)).await?;
        Ok(response.content)
    }

    /// Fix SQL syntax errors
    async fn fix_query(
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

        let response = self.chat(messages, model, Some(0.1), Some(2048)).await?;
        Ok(extract_sql(&response.content))
    }
}

/// Extract SQL from a response that might contain markdown
pub fn extract_sql(content: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sql() {
        // Test with SQL code block
        let input = "Here's the query:\n```sql\nSELECT * FROM users;\n```";
        assert_eq!(extract_sql(input), "SELECT * FROM users;");

        // Test with generic code block
        let input = "```\nSELECT * FROM users;\n```";
        assert_eq!(extract_sql(input), "SELECT * FROM users;");

        // Test without code blocks
        let input = "SELECT * FROM users;";
        assert_eq!(extract_sql(input), "SELECT * FROM users;");
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

    #[test]
    fn test_provider_type_display() {
        assert_eq!(AiProviderType::Ollama.to_string(), "Ollama");
        assert_eq!(AiProviderType::OpenAI.to_string(), "OpenAI");
        assert_eq!(AiProviderType::Anthropic.to_string(), "Claude");
        assert_eq!(AiProviderType::Google.to_string(), "Gemini");
    }
}
