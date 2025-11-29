//! AI Assistant Module
//!
//! Provides integration with multiple AI providers for:
//! - Natural language to SQL conversion
//! - Query explanation
//! - Query optimization suggestions
//!
//! Supported providers:
//! - Ollama (local LLM)
//! - OpenAI (GPT-4, GPT-3.5)
//! - Anthropic (Claude)
//! - Google (Gemini)

pub mod provider;
pub mod ollama;
pub mod openai;
pub mod anthropic;
pub mod google;

// Re-export common types
pub use provider::{
    AiProvider, AiProviderType, AiModel, ChatMessage, ChatRole,
    ChatCompletion, TokenUsage, extract_sql,
};

// Re-export providers
pub use ollama::{OllamaProvider, OllamaConfig};
pub use openai::{OpenAiProvider, OpenAiConfig};
pub use anthropic::{AnthropicProvider, AnthropicConfig};
pub use google::{GoogleAiProvider, GoogleAiConfig};
