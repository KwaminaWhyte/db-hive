/**
 * AI Assistant API
 *
 * Frontend API for interacting with Ollama-powered AI assistant
 */

import { invoke } from "@tauri-apps/api/core";

// Types
export interface OllamaConfig {
  base_url: string;
  default_model: string;
  timeout_secs: number;
}

export interface AiModel {
  name: string;
  size: string;
  modified: string;
}

export interface AiChatResponse {
  content: string;
  model: string;
  duration_ms: number;
}

export type ChatRole = "system" | "user" | "assistant";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

/**
 * Check if Ollama is available and running
 */
export async function checkOllamaStatus(): Promise<boolean> {
  return invoke<boolean>("check_ollama_status");
}

/**
 * Get current AI configuration
 */
export async function getAiConfig(): Promise<OllamaConfig> {
  return invoke<OllamaConfig>("get_ai_config");
}

/**
 * Update AI configuration
 */
export async function setAiConfig(config: OllamaConfig): Promise<void> {
  return invoke("set_ai_config", { config });
}

/**
 * List available Ollama models
 */
export async function listAiModels(): Promise<AiModel[]> {
  return invoke<AiModel[]>("list_ai_models");
}

/**
 * Generate SQL from natural language
 */
export async function generateSql(
  prompt: string,
  schemaContext: string,
  model?: string
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_generate_sql", {
    prompt,
    schemaContext,
    model,
  });
}

/**
 * Explain a SQL query
 */
export async function explainQuery(
  sql: string,
  model?: string
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_explain_query", { sql, model });
}

/**
 * Optimize a SQL query
 */
export async function optimizeQuery(
  sql: string,
  schemaContext: string,
  model?: string
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_optimize_query", {
    sql,
    schemaContext,
    model,
  });
}

/**
 * Fix a SQL query based on error message
 */
export async function fixQuery(
  sql: string,
  errorMessage: string,
  schemaContext: string,
  model?: string
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_fix_query", {
    sql,
    errorMessage,
    schemaContext,
    model,
  });
}

/**
 * General chat with the AI
 */
export async function chat(
  messages: ChatMessage[],
  model?: string
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_chat", { messages, model });
}
