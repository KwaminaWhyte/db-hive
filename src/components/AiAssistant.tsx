/**
 * AI Assistant Component
 *
 * Provides AI-powered SQL assistance using local Ollama models
 */

import { useState, useEffect, useCallback } from "react";
import {
  checkOllamaStatus,
  listAiModels,
  generateSql,
  explainQuery,
  optimizeQuery,
  fixQuery,
  getAiConfig,
  setAiConfig,
  type AiModel,
  type OllamaConfig,
} from "../api/ai";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";

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
  const [isAvailable, setIsAvailable] = useState<boolean | null>(null);
  const [models, setModels] = useState<AiModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [config, setConfig] = useState<OllamaConfig | null>(null);
  const [prompt, setPrompt] = useState("");
  const [result, setResult] = useState("");
  const [loading, setLoading] = useState(false);
  const [activeAction, setActiveAction] = useState<AiAction | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showContext, setShowContext] = useState(false);
  const [durationMs, setDurationMs] = useState<number | null>(null);

  // Check Ollama status on mount
  useEffect(() => {
    checkStatus();
  }, []);

  // Load models when available
  useEffect(() => {
    if (isAvailable) {
      loadModels();
      loadConfig();
    }
  }, [isAvailable]);

  const checkStatus = useCallback(async () => {
    try {
      const available = await checkOllamaStatus();
      setIsAvailable(available);
      setError(null);
    } catch (err) {
      setIsAvailable(false);
      setError("Failed to check Ollama status");
    }
  }, []);

  const loadModels = useCallback(async () => {
    try {
      const modelList = await listAiModels();
      setModels(modelList);
      if (modelList.length > 0 && !selectedModel) {
        setSelectedModel(modelList[0].name);
      }
    } catch (err) {
      console.error("Failed to load models:", err);
    }
  }, [selectedModel]);

  const loadConfig = useCallback(async () => {
    try {
      const cfg = await getAiConfig();
      setConfig(cfg);
      if (cfg.default_model && !selectedModel) {
        setSelectedModel(cfg.default_model);
      }
    } catch (err) {
      console.error("Failed to load config:", err);
    }
  }, [selectedModel]);

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
        selectedModel || undefined
      );
      setResult(response.content);
      setDurationMs(response.duration_ms);
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
      const response = await explainQuery(currentSql, selectedModel || undefined);
      setResult(response.content);
      setDurationMs(response.duration_ms);
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
        selectedModel || undefined
      );
      setResult(response.content);
      setDurationMs(response.duration_ms);
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
        selectedModel || undefined
      );
      setResult(response.content);
      setDurationMs(response.duration_ms);
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

  const handleApplySql = () => {
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
        result.trim().toUpperCase().startsWith("DROP")
      ) {
        onSqlGenerated(result);
      }
    }
  };

  const handleSaveConfig = async () => {
    if (!config) return;

    try {
      await setAiConfig({ ...config, default_model: selectedModel });
      setShowSettings(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="px-3 py-2 border-b flex items-center justify-between">
        <div className="flex items-center gap-2">
          {isAvailable !== null && (
            <span
              className={`w-2 h-2 rounded-full ${
                isAvailable ? "bg-green-500" : "bg-red-500"
              }`}
              title={isAvailable ? "Ollama connected" : "Ollama not available"}
            />
          )}
          <span className="text-sm text-muted-foreground">
            {isAvailable ? "Connected to Ollama" : "Ollama offline"}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setShowSettings(!showSettings)}
            className="p-1 hover:bg-muted rounded"
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
      {showSettings && config && (
        <div className="p-3 border-b bg-muted/30">
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-medium text-muted-foreground mb-1">
                Ollama URL
              </label>
              <input
                type="text"
                value={config.base_url}
                onChange={(e) =>
                  setConfig({ ...config, base_url: e.target.value })
                }
                className="w-full px-2 py-1 text-sm border rounded bg-background text-foreground"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-muted-foreground mb-1">
                Timeout (seconds)
              </label>
              <input
                type="number"
                value={config.timeout_secs}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    timeout_secs: parseInt(e.target.value) || 120,
                  })
                }
                className="w-full px-2 py-1 text-sm border rounded bg-background text-foreground"
              />
            </div>
            <button
              onClick={handleSaveConfig}
              className="w-full px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded hover:bg-primary/90"
            >
              Save Settings
            </button>
          </div>
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {!isAvailable ? (
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
                  {models
                    .filter(m => m.name)
                    .filter((model, idx, arr) => arr.findIndex(m => m.name === model.name) === idx)
                    .map((model) => (
                      <SelectItem key={model.name} value={model.name}>
                        {model.name} ({model.size})
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
                disabled={loading || !prompt.trim()}
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
                  disabled={loading || !currentSql}
                  className="px-2 py-1.5 text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 rounded hover:bg-blue-500/20 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Explain the current query"
                >
                  {loading && activeAction === "explain" ? "..." : "Explain"}
                </button>
                <button
                  onClick={handleOptimize}
                  disabled={loading || !currentSql}
                  className="px-2 py-1.5 text-xs bg-green-500/10 text-green-600 dark:text-green-400 rounded hover:bg-green-500/20 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Optimize the current query"
                >
                  {loading && activeAction === "optimize" ? "..." : "Optimize"}
                </button>
                <button
                  onClick={handleFix}
                  disabled={loading || !currentSql || !lastError}
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
      {isAvailable && (
        <div className="p-2 border-t bg-muted/30">
          <p className="text-xs text-muted-foreground text-center">
            Tip: Select tables in schema browser for better context
          </p>
        </div>
      )}
    </div>
  );
}
