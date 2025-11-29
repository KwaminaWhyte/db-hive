//! AI Assistant Module
//!
//! Provides integration with Ollama for:
//! - Natural language to SQL conversion
//! - Query explanation
//! - Query optimization suggestions

pub mod ollama;

pub use ollama::{OllamaClient, OllamaConfig, ChatMessage, ChatRole};
