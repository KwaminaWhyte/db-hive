/**
 * AI Assistant API
 *
 * Frontend API for interacting with AI-powered SQL assistant.
 * Supports multiple providers: Ollama, OpenAI, Claude, Gemini.
 */

import { invoke } from "@tauri-apps/api/core";

// Provider types
export type AiProviderType = "ollama" | "openai" | "anthropic" | "google";

export const AI_PROVIDERS: { value: AiProviderType; label: string; description: string }[] = [
  { value: "ollama", label: "Ollama", description: "Local LLM (free, private)" },
  { value: "openai", label: "OpenAI", description: "GPT-4, GPT-3.5" },
  { value: "anthropic", label: "Claude", description: "Claude 3.5, Claude 3" },
  { value: "google", label: "Gemini", description: "Gemini Pro, Gemini Flash" },
];

// Configuration types
export interface OllamaConfig {
  baseUrl: string;
  defaultModel: string;
  timeoutSecs: number;
}

export interface OpenAiConfig {
  apiKey: string;
  baseUrl: string;
  defaultModel: string;
  organization: string | null;
  timeoutSecs: number;
}

export interface AnthropicConfig {
  apiKey: string;
  baseUrl: string;
  defaultModel: string;
  timeoutSecs: number;
}

export interface GoogleAiConfig {
  apiKey: string;
  baseUrl: string;
  defaultModel: string;
  timeoutSecs: number;
}

export interface AiConfig {
  activeProvider: AiProviderType;
  ollama: OllamaConfig;
  openai: OpenAiConfig;
  anthropic: AnthropicConfig;
  google: GoogleAiConfig;
}

// Model information
export interface AiModel {
  id: string;
  name: string;
  provider: AiProviderType;
  description: string | null;
  contextWindow: number | null;
}

// Response types
export interface AiChatResponse {
  content: string;
  model: string;
  provider: AiProviderType;
  durationMs: number;
}

export interface ProviderStatus {
  provider: AiProviderType;
  available: boolean;
  configured: boolean;
}

// Chat types
export type ChatRole = "system" | "user" | "assistant";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

/**
 * Check provider availability status
 */
export async function checkProviderStatus(provider?: AiProviderType): Promise<ProviderStatus> {
  return invoke<ProviderStatus>("check_ai_provider_status", { provider });
}

/**
 * Check if Ollama is available (legacy, for backward compatibility)
 */
export async function checkOllamaStatus(): Promise<boolean> {
  return invoke<boolean>("check_ollama_status");
}

/**
 * Get current AI configuration
 */
export async function getAiConfig(): Promise<AiConfig> {
  return invoke<AiConfig>("get_ai_config");
}

/**
 * Update AI configuration
 */
export async function setAiConfig(config: AiConfig): Promise<void> {
  return invoke("set_ai_config", { config });
}

/**
 * Set the active AI provider
 */
export async function setActiveProvider(provider: AiProviderType): Promise<void> {
  return invoke("set_active_ai_provider", { provider });
}

/**
 * Set API key for a provider
 */
export async function setApiKey(provider: AiProviderType, apiKey: string): Promise<void> {
  return invoke("set_ai_api_key", { provider, apiKey });
}

/**
 * List available AI models for a provider
 */
export async function listAiModels(provider?: AiProviderType): Promise<AiModel[]> {
  return invoke<AiModel[]>("list_ai_models", { provider });
}

/**
 * Generate SQL from natural language
 */
export async function generateSql(
  prompt: string,
  schemaContext: string,
  model?: string,
  provider?: AiProviderType
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_generate_sql", {
    prompt,
    schemaContext,
    model,
    provider,
  });
}

/**
 * Explain a SQL query
 */
export async function explainQuery(
  sql: string,
  model?: string,
  provider?: AiProviderType
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_explain_query", { sql, model, provider });
}

/**
 * Optimize a SQL query
 */
export async function optimizeQuery(
  sql: string,
  schemaContext: string,
  model?: string,
  provider?: AiProviderType
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_optimize_query", {
    sql,
    schemaContext,
    model,
    provider,
  });
}

/**
 * Fix a SQL query based on error message
 */
export async function fixQuery(
  sql: string,
  errorMessage: string,
  schemaContext: string,
  model?: string,
  provider?: AiProviderType
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_fix_query", {
    sql,
    errorMessage,
    schemaContext,
    model,
    provider,
  });
}

/**
 * General chat with the AI
 */
export async function chat(
  messages: ChatMessage[],
  model?: string,
  provider?: AiProviderType
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>("ai_chat", { messages, model, provider });
}
