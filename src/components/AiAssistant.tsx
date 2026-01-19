/**
 * AI Assistant Component
 *
 * Provides AI-powered SQL assistance using multiple providers:
 * - Ollama (local LLM)
 * - OpenAI (GPT-4, GPT-3.5)
 * - Anthropic (Claude)
 * - Google (Gemini)
 */

import { useState, useEffect, useCallback, useMemo } from "react";
import {
  checkProviderStatus,
  listAiModels,
  generateSql,
  explainQuery,
  optimizeQuery,
  fixQuery,
  getAiConfig,
  setActiveProvider,
  setApiKey,
  type AiModel,
  type AiConfig,
  type AiProviderType,
  AI_PROVIDERS,
} from "../api/ai";

// Module-level pure function
const formatDuration = (ms: number): string => {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
};
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { Input } from "./ui/input";
import { Button } from "./ui/button";
import { Badge } from "./ui/badge";

interface AiAssistantProps {
  /** Current SQL in the editor */
  currentSql?: string;
  /** Schema context for SQL generation */
  schemaContext?: string;
  /** Last error message from query execution */
  lastError?: string;
  /** Callback when SQL is generated */
  onSqlGenerated?: (sql: string) => void;
}

type AiAction = "generate" | "explain" | "optimize" | "fix";

export function AiAssistant({
  currentSql = "",
  schemaContext = "",
  lastError = "",
  onSqlGenerated,
}: AiAssistantProps) {
  const [, setConfig] = useState<AiConfig | null>(null);
  const [selectedProvider, setSelectedProvider] = useState<AiProviderType>("ollama");
  const [isAvailable, setIsAvailable] = useState<boolean | null>(null);
  const [isConfigured, setIsConfigured] = useState(false);
  const [models, setModels] = useState<AiModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [prompt, setPrompt] = useState("");
  const [result, setResult] = useState("");
  const [loading, setLoading] = useState(false);
  const [activeAction, setActiveAction] = useState<AiAction | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showContext, setShowContext] = useState(false);
  const [durationMs, setDurationMs] = useState<number | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [savingApiKey, setSavingApiKey] = useState(false);

  // Load config on mount
  useEffect(() => {
    loadConfig();
  }, []);

  // Check provider status when provider changes
  useEffect(() => {
    checkStatus();
  }, [selectedProvider]);

  // Load models when provider is available
  useEffect(() => {
    if (isAvailable && isConfigured) {
      loadModels();
    } else {
      setModels([]);
      setSelectedModel("");
    }
  }, [isAvailable, isConfigured, selectedProvider]);

  const loadConfig = useCallback(async () => {
    try {
      const cfg = await getAiConfig();
      setConfig(cfg);
      setSelectedProvider(cfg.activeProvider);
    } catch (err) {
      console.error("Failed to load config:", err);
    }
  }, []);

  const checkStatus = useCallback(async () => {
    try {
      const status = await checkProviderStatus(selectedProvider);
      setIsAvailable(status.available);
      setIsConfigured(status.configured);
      setError(null);
    } catch (err) {
      setIsAvailable(false);
      setIsConfigured(false);
      setError("Failed to check provider status");
    }
  }, [selectedProvider]);

  const loadModels = useCallback(async () => {
    try {
      const modelList = await listAiModels(selectedProvider);
      setModels(modelList);
      if (modelList.length > 0 && !selectedModel) {
        setSelectedModel(modelList[0].id);
      }
    } catch (err) {
      console.error("Failed to load models:", err);
      setModels([]);
    }
  }, [selectedProvider, selectedModel]);

  const handleProviderChange = async (provider: AiProviderType) => {
    setSelectedProvider(provider);
    setSelectedModel("");
    setApiKeyInput("");
    try {
      await setActiveProvider(provider);
    } catch (err) {
      console.error("Failed to set provider:", err);
    }
  };

  const handleSaveApiKey = async () => {
    if (!apiKeyInput.trim()) return;

    setSavingApiKey(true);
    try {
      await setApiKey(selectedProvider, apiKeyInput);
      setApiKeyInput("");
      // Refresh status
      await checkStatus();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingApiKey(false);
    }
  };

  const handleGenerate = async () => {
    if (!prompt.trim()) return;

    setLoading(true);
    setActiveAction("generate");
    setError(null);
    setResult("");

    try {
      const response = await generateSql(
        prompt,
        schemaContext,
        selectedModel || undefined,
        selectedProvider
      );
      setResult(response.content);
      setDurationMs(response.durationMs);
      if (onSqlGenerated) {
        onSqlGenerated(response.content);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setActiveAction(null);
    }
  };

  const handleExplain = async () => {
    if (!currentSql.trim()) {
      setError("No SQL to explain. Enter a query in the editor first.");
      return;
    }

    setLoading(true);
    setActiveAction("explain");
    setError(null);
    setResult("");

    try {
      const response = await explainQuery(currentSql, selectedModel || undefined, selectedProvider);
      setResult(response.content);
      setDurationMs(response.durationMs);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setActiveAction(null);
    }
  };

  const handleOptimize = async () => {
    if (!currentSql.trim()) {
      setError("No SQL to optimize. Enter a query in the editor first.");
      return;
    }

    setLoading(true);
    setActiveAction("optimize");
    setError(null);
    setResult("");

    try {
      const response = await optimizeQuery(
        currentSql,
        schemaContext,
        selectedModel || undefined,
        selectedProvider
      );
      setResult(response.content);
      setDurationMs(response.durationMs);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setActiveAction(null);
    }
  };

  const handleFix = async () => {
    if (!currentSql.trim()) {
      setError("No SQL to fix. Enter a query in the editor first.");
      return;
    }

    if (!lastError) {
      setError("No error to fix. Run the query first to see errors.");
      return;
    }

    setLoading(true);
    setActiveAction("fix");
    setError(null);
    setResult("");

    try {
      const response = await fixQuery(
        currentSql,
        lastError,
        schemaContext,
        selectedModel || undefined,
        selectedProvider
      );
      setResult(response.content);
      setDurationMs(response.durationMs);
      if (onSqlGenerated) {
        onSqlGenerated(response.content);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setActiveAction(null);
    }
  };

  const handleApplySql = useCallback(() => {
    if (result && onSqlGenerated) {
      // Extract SQL from result if it contains explanation
      const sqlMatch = result.match(/```sql\n([\s\S]*?)\n```/);
      if (sqlMatch) {
        onSqlGenerated(sqlMatch[1]);
      } else if (
        result.trim().toUpperCase().startsWith("SELECT") ||
        result.trim().toUpperCase().startsWith("INSERT") ||
        result.trim().toUpperCase().startsWith("UPDATE") ||
        result.trim().toUpperCase().startsWith("DELETE") ||
        result.trim().toUpperCase().startsWith("CREATE") ||
        result.trim().toUpperCase().startsWith("ALTER") ||
        result.trim().toUpperCase().startsWith("DROP") ||
        result.trim().toUpperCase().startsWith("WITH")
      ) {
        onSqlGenerated(result);
      }
    }
  }, [result, onSqlGenerated]);

  // Memoized filtered and deduplicated models
  const uniqueModels = useMemo(() => {
    return models
      .filter(m => m.id)
      .filter((model, idx, arr) => arr.findIndex(m => m.id === model.id) === idx);
  }, [models]);

  const needsApiKey = selectedProvider !== "ollama" && !isConfigured;
  const providerInfo = AI_PROVIDERS.find(p => p.value === selectedProvider);

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="px-3 py-2 border-b flex items-center justify-between">
        <div className="flex items-center gap-2">
          {isAvailable !== null && (
            <span
              className={`w-2 h-2 rounded-full ${
                isAvailable && isConfigured ? "bg-green-500" : needsApiKey ? "bg-yellow-500" : "bg-red-500"
              }`}
              title={
                isAvailable && isConfigured
                  ? `${providerInfo?.label} connected`
                  : needsApiKey
                  ? "API key required"
                  : `${providerInfo?.label} not available`
              }
            />
          )}
          <span className="text-sm text-muted-foreground">
            {providerInfo?.label || "AI Assistant"}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setShowSettings(!showSettings)}
            className={`p-1 hover:bg-muted rounded ${showSettings ? 'bg-muted' : ''}`}
            title="Settings"
          >
            <svg
              className="w-4 h-4 text-muted-foreground"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
              />
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
              />
            </svg>
          </button>
          <button
            onClick={() => setShowContext(!showContext)}
            className={`p-1 hover:bg-muted rounded ${showContext ? 'bg-muted' : ''}`}
            title="View schema context"
          >
            <svg
              className="w-4 h-4 text-muted-foreground"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"
              />
            </svg>
          </button>
          <button
            onClick={checkStatus}
            className="p-1 hover:bg-muted rounded"
            title="Refresh status"
          >
            <svg
              className="w-4 h-4 text-muted-foreground"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
              />
            </svg>
          </button>
        </div>
      </div>

      {/* Provider Selection */}
      <div className="px-3 py-2 border-b">
        <label className="block text-xs font-medium text-muted-foreground mb-1">
          AI Provider
        </label>
        <Select value={selectedProvider} onValueChange={(v) => handleProviderChange(v as AiProviderType)}>
          <SelectTrigger className="w-full">
            <SelectValue placeholder="Select provider" />
          </SelectTrigger>
          <SelectContent>
            {AI_PROVIDERS.map((provider) => (
              <SelectItem key={provider.value} value={provider.value}>
                <div className="flex items-center gap-2">
                  <span>{provider.label}</span>
                  <span className="text-xs text-muted-foreground">({provider.description})</span>
                </div>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Schema Context Panel */}
      {showContext && (
        <div className="p-3 border-b bg-muted/30 max-h-48 overflow-y-auto">
          <div className="flex items-center justify-between mb-2">
            <label className="text-xs font-medium text-muted-foreground">
              Schema Context ({schemaContext ? 'loaded' : 'empty'})
            </label>
          </div>
          {schemaContext ? (
            <pre className="text-xs text-foreground whitespace-pre-wrap font-mono bg-background p-2 rounded border">
              {schemaContext}
            </pre>
          ) : (
            <p className="text-xs text-muted-foreground italic">
              No schema context available. Make sure you're connected to a database.
            </p>
          )}
        </div>
      )}

      {/* Settings Panel */}
      {showSettings && (
        <div className="p-3 border-b bg-muted/30">
          <div className="space-y-3">
            {selectedProvider !== "ollama" && (
              <div>
                <label className="block text-xs font-medium text-muted-foreground mb-1">
                  API Key {isConfigured && <Badge variant="outline" className="ml-1 text-xs">Configured</Badge>}
                </label>
                <div className="flex gap-2">
                  <Input
                    type="password"
                    value={apiKeyInput}
                    onChange={(e) => setApiKeyInput(e.target.value)}
                    placeholder={isConfigured ? "Enter new key to update" : "Enter API key"}
                    className="flex-1"
                  />
                  <Button
                    size="sm"
                    onClick={handleSaveApiKey}
                    disabled={!apiKeyInput.trim() || savingApiKey}
                  >
                    {savingApiKey ? "..." : "Save"}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {selectedProvider === "openai" && "Get your key at platform.openai.com"}
                  {selectedProvider === "anthropic" && "Get your key at console.anthropic.com"}
                  {selectedProvider === "google" && "Get your key at ai.google.dev"}
                </p>
              </div>
            )}
            {selectedProvider === "ollama" && (
              <p className="text-xs text-muted-foreground">
                Ollama runs locally and doesn't require an API key. Make sure Ollama is running on your machine.
              </p>
            )}
          </div>
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {needsApiKey ? (
          <div className="text-center py-6">
            <svg
              className="w-12 h-12 mx-auto text-yellow-500 mb-3"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z"
              />
            </svg>
            <p className="text-sm text-muted-foreground mb-2">
              API key required for {providerInfo?.label}
            </p>
            <p className="text-xs text-muted-foreground mb-4">
              Click the settings icon above to add your API key
            </p>
          </div>
        ) : !isAvailable && selectedProvider === "ollama" ? (
          <div className="text-center py-6">
            <svg
              className="w-12 h-12 mx-auto text-muted-foreground mb-3"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <p className="text-sm text-muted-foreground mb-2">
              Ollama is not available
            </p>
            <p className="text-xs text-muted-foreground mb-4">
              Make sure Ollama is running on your machine
            </p>
            <a
              href="https://ollama.ai"
              target="_blank"
              rel="noopener noreferrer"
              className="text-sm text-primary hover:text-primary/80"
            >
              Get Ollama
            </a>
          </div>
        ) : (
          <>
            {/* Model Selection */}
            <div>
              <label className="block text-xs font-medium text-muted-foreground mb-1">
                Model
              </label>
              <Select value={selectedModel} onValueChange={setSelectedModel}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a model" />
                </SelectTrigger>
                <SelectContent>
                  {uniqueModels.map((model) => (
                    <SelectItem key={model.id} value={model.id}>
                      <div className="flex items-center gap-2">
                        <span>{model.name}</span>
                        {model.description && (
                          <span className="text-xs text-muted-foreground">
                            ({model.description})
                          </span>
                        )}
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Natural Language Input */}
            <div>
              <label className="block text-xs font-medium text-muted-foreground mb-1">
                Describe what you want
              </label>
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder="e.g., Show me all users who signed up last month"
                className="w-full px-2 py-1.5 text-sm border rounded bg-background text-foreground resize-none"
                rows={3}
              />
              <button
                onClick={handleGenerate}
                disabled={loading || !prompt.trim() || !selectedModel}
                className="mt-2 w-full px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
              >
                {loading && activeAction === "generate" ? (
                  <>
                    <svg
                      className="animate-spin h-4 w-4"
                      fill="none"
                      viewBox="0 0 24 24"
                    >
                      <circle
                        className="opacity-25"
                        cx="12"
                        cy="12"
                        r="10"
                        stroke="currentColor"
                        strokeWidth="4"
                      />
                      <path
                        className="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                      />
                    </svg>
                    Generating...
                  </>
                ) : (
                  <>
                    <svg
                      className="w-4 h-4"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M13 10V3L4 14h7v7l9-11h-7z"
                      />
                    </svg>
                    Generate SQL
                  </>
                )}
              </button>
            </div>

            {/* Quick Actions */}
            <div>
              <label className="block text-xs font-medium text-muted-foreground mb-2">
                Query Actions
              </label>
              <div className="grid grid-cols-3 gap-2">
                <button
                  onClick={handleExplain}
                  disabled={loading || !currentSql || !selectedModel}
                  className="px-2 py-1.5 text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 rounded hover:bg-blue-500/20 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Explain the current query"
                >
                  {loading && activeAction === "explain" ? "..." : "Explain"}
                </button>
                <button
                  onClick={handleOptimize}
                  disabled={loading || !currentSql || !selectedModel}
                  className="px-2 py-1.5 text-xs bg-green-500/10 text-green-600 dark:text-green-400 rounded hover:bg-green-500/20 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Optimize the current query"
                >
                  {loading && activeAction === "optimize" ? "..." : "Optimize"}
                </button>
                <button
                  onClick={handleFix}
                  disabled={loading || !currentSql || !lastError || !selectedModel}
                  className="px-2 py-1.5 text-xs bg-red-500/10 text-red-600 dark:text-red-400 rounded hover:bg-red-500/20 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Fix errors in the current query"
                >
                  {loading && activeAction === "fix" ? "..." : "Fix Error"}
                </button>
              </div>
            </div>

            {/* Error Display */}
            {error && (
              <div className="p-2 bg-destructive/10 border border-destructive/20 rounded text-sm text-destructive">
                {error}
              </div>
            )}

            {/* Result Display */}
            {result && (
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-muted-foreground">
                    Result
                  </label>
                  {durationMs && (
                    <span className="text-xs text-muted-foreground">
                      {formatDuration(durationMs)}
                    </span>
                  )}
                </div>
                <div className="p-2 bg-muted/30 border rounded text-foreground whitespace-pre-wrap max-h-64 overflow-y-auto font-mono text-xs">
                  {result}
                </div>
                {(result.includes("SELECT") ||
                  result.includes("INSERT") ||
                  result.includes("UPDATE") ||
                  result.includes("DELETE") ||
                  result.includes("WITH") ||
                  result.includes("```sql")) && (
                  <button
                    onClick={handleApplySql}
                    className="w-full px-3 py-1.5 text-sm bg-secondary text-secondary-foreground rounded hover:bg-secondary/80"
                  >
                    Apply to Editor
                  </button>
                )}
              </div>
            )}
          </>
        )}
      </div>

      {/* Footer with tips */}
      {isAvailable && isConfigured && (
        <div className="p-2 border-t bg-muted/30">
          <p className="text-xs text-muted-foreground text-center">
            Tip: Select tables in schema browser for better context
          </p>
        </div>
      )}
    </div>
  );
}
