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
//! - OpenRouter (multi-model gateway)

pub mod anthropic;
pub mod google;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod provider;

// Re-export common types
pub use provider::{
    extract_sql, AiModel, AiProvider, AiProviderType, ChatCompletion, ChatMessage, ChatRole,
    TokenUsage,
};

// Re-export providers
pub use anthropic::{AnthropicConfig, AnthropicProvider};
pub use google::{GoogleAiConfig, GoogleAiProvider};
pub use ollama::{OllamaConfig, OllamaProvider};
pub use openai::{OpenAiConfig, OpenAiProvider};
pub use openrouter::{OpenRouterConfig, OpenRouterProvider};
