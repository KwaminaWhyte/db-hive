import { FC, useState, useEffect, useRef } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { SQLEditor } from './SQLEditor';
import { ResultsViewer } from './ResultsViewer';
import { QueryPlanVisualizer, parseExplainJson } from './QueryPlanVisualizer';
import type { QueryPlanResult } from '@/types/database';
import { HistoryPanel } from './HistoryPanel';
import { SnippetSidebar } from './SnippetSidebar';
import { TemplatesPanel } from './TemplatesPanel';
import { AiAssistant } from './AiAssistant';
import { QueryExecutionResult, ConnectionProfile } from '@/types/database';
import { createQueryHistory } from '@/types/history';
import { QueryCancelledError } from '@/utils/queryErrors';
import { invoke } from '@tauri-apps/api/core';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs';
import { GripHorizontal, GripVertical, Sparkles } from 'lucide-react';
import { QueryStatusBar } from './QueryStatusBar';
import { ConnectionLostError } from './ConnectionLostError';
import { useNavigate } from '@tanstack/react-router';
import { useSchemaContext } from '@/hooks/useSchemaContext';

interface QueryPanelProps {
  /** Active connection ID */
  connectionId: string | null;

  /** Active connection profile (for history metadata) */
  connectionProfile?: ConnectionProfile;

  /** Current database name */
  currentDatabase?: string;

  /** Callback to execute query - returns a Promise with results */
  onExecuteQuery: (sql: string) => Promise<QueryExecutionResult>;

  /** Pending query to load into the editor */
  pendingQuery?: string | null;

  /** Callback when pending query is loaded */
  onQueryLoaded?: () => void;

  /**
   * Stable identifier for this panel instance. When provided, the panel's
   * editor state (SQL text, results, query plan) is snapshotted to an
   * in-memory cache so it survives unmount/remount — required now that the
   * query route only mounts the active tab (PERF-02).
   */
  panelId?: string;

  /** Called (debounced) with the editor's SQL so the parent can persist it
   *  (e.g. to TabContext / localStorage). */
  onPersistSql?: (sql: string) => void;
}

/**
 * In-memory snapshots of panel state, keyed by `panelId`. Lets a QueryPanel
 * be unmounted (inactive route tab) and restored later without losing SQL
 * text, results, or the plan view. Never serialized — results can be large.
 * Entries are cleared via `clearQueryPanelState` when a tab closes.
 *
 * One snapshot per route-level tab: the QueryPanel manages a single editor
 * (UX-02 — the former inner "Query 1 / +" tab system was removed; the
 * route-level tab bar is the only tab model).
 */
interface QueryPanelSnapshot {
  sql: string;
  results: QueryExecutionResult | null;
  error: string | null;
  queryPlan: QueryPlanResult | null;
  showPlanView: boolean;
}

/** Pre-UX-02 snapshot shape: multiple inner editor tabs per panel. */
interface LegacyQueryPanelSnapshot {
  tabs: Array<{
    id: string;
    sql: string;
    results: QueryExecutionResult | null;
    error: string | null;
  }>;
  activeTabId: string;
  queryPlan: QueryPlanResult | null;
  showPlanView: boolean;
}

const panelStateCache = new Map<
  string,
  QueryPanelSnapshot | LegacyQueryPanelSnapshot
>();

function isLegacySnapshot(
  snapshot: QueryPanelSnapshot | LegacyQueryPanelSnapshot
): snapshot is LegacyQueryPanelSnapshot {
  return Array.isArray((snapshot as LegacyQueryPanelSnapshot).tabs);
}

/**
 * Read a snapshot, migrating the legacy multi-inner-tab shape by keeping the
 * active inner tab's SQL/results (other inner tabs are dropped — they were
 * never persisted anywhere).
 */
function readSnapshot(panelId?: string): QueryPanelSnapshot | undefined {
  if (!panelId) return undefined;
  const raw = panelStateCache.get(panelId);
  if (!raw) return undefined;
  if (isLegacySnapshot(raw)) {
    const active =
      raw.tabs.find((tab) => tab.id === raw.activeTabId) ?? raw.tabs[0];
    return {
      sql: active?.sql ?? '',
      results: active?.results ?? null,
      error: active?.error ?? null,
      queryPlan: raw.queryPlan ?? null,
      showPlanView: raw.showPlanView ?? false,
    };
  }
  return raw;
}

/** Drop the cached snapshot for a closed panel (frees result memory). */
export function clearQueryPanelState(panelId: string): void {
  panelStateCache.delete(panelId);
}

/** True if the panel's editor has non-empty SQL. */
export function queryPanelHasUnsavedSql(panelId: string): boolean {
  const snapshot = panelStateCache.get(panelId);
  if (!snapshot) return false;
  if (isLegacySnapshot(snapshot)) {
    return snapshot.tabs.some((tab) => tab.sql.trim().length > 0);
  }
  return snapshot.sql.trim().length > 0;
}

export const QueryPanel: FC<QueryPanelProps> = ({
  connectionId,
  connectionProfile,
  currentDatabase,
  onExecuteQuery,
  pendingQuery,
  onQueryLoaded,
  panelId,
  onPersistSql,
}) => {
  // Editor state — restored from the in-memory snapshot cache when this
  // panel was previously mounted (route tab switched away and back).
  const [initialSnapshot] = useState(() => readSnapshot(panelId));
  const [sql, setSql] = useState(initialSnapshot?.sql ?? '');
  // An in-flight query at unmount time never resolves into a remount, so
  // loading always starts false (never snapshotted).
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<QueryExecutionResult | null>(
    initialSnapshot?.results ?? null
  );
  const [error, setError] = useState<string | null>(
    initialSnapshot?.error ?? null
  );
  const [historyRefreshKey, setHistoryRefreshKey] = useState(0);
  const [connectionLost, setConnectionLost] = useState(false);
  const navigate = useNavigate();

  // Query Plan Visualizer state
  const [queryPlan, setQueryPlan] = useState<QueryPlanResult | null>(
    initialSnapshot?.queryPlan ?? null
  );
  const [showPlanView, setShowPlanView] = useState(
    initialSnapshot?.showPlanView ?? false
  );

  // Schema context for the AI assistant — single cached
  // get_autocomplete_metadata call shared across all panels on this
  // connection (PERF-01 / PERF-02).
  const schemaContext = useSchemaContext(connectionId, currentDatabase);

  // Snapshot panel state so it survives unmount (inactive route tab).
  useEffect(() => {
    if (!panelId) return;
    panelStateCache.set(panelId, {
      sql,
      results,
      error,
      queryPlan,
      showPlanView,
    });
  }, [panelId, sql, results, error, queryPlan, showPlanView]);

  // Persist the editor's SQL to the parent (debounced) so it survives app
  // restarts via TabContext/localStorage. Refs keep the effect off the
  // parent's render identity (avoids re-arm/update loops).
  const onPersistSqlRef = useRef(onPersistSql);
  onPersistSqlRef.current = onPersistSql;
  const sqlRef = useRef(sql);
  sqlRef.current = sql;

  useEffect(() => {
    const timer = setTimeout(() => {
      onPersistSqlRef.current?.(sqlRef.current);
    }, 500);
    return () => clearTimeout(timer);
  }, [sql]);

  // Flush the latest SQL on unmount so a quick tab switch loses nothing.
  useEffect(() => {
    return () => {
      onPersistSqlRef.current?.(sqlRef.current);
    };
  }, []);

  // Handle pending query from parent (skip echoes of our own persisted SQL)
  useEffect(() => {
    if (pendingQuery && pendingQuery !== sqlRef.current) {
      setSql(pendingQuery);
      onQueryLoaded?.();
    }
  }, [pendingQuery]);

  // Handle query execution
  const handleExecute = async (sqlToExecute: string) => {
    if (!connectionId) {
      setError('No active connection');
      return;
    }

    // Snapshot current state so a user-cancelled execution (e.g. dismissing
    // the destructive-query guard) can restore it untouched.
    const previousResults = results;
    const previousError = error;

    setLoading(true);
    setError(null);
    setResults(null);

    const startTime = Date.now();

    try {
      const result = await onExecuteQuery(sqlToExecute);
      setResults(result);
      setLoading(false);

      // Check if this is an EXPLAIN query and try to parse the plan
      const trimmedSql = sqlToExecute.trim().toUpperCase();
      if (trimmedSql.startsWith('EXPLAIN')) {
        try {
          // Check if result has JSON data (PostgreSQL EXPLAIN FORMAT JSON)
          if (result.rows.length > 0 && result.rows[0].length > 0) {
            const firstCell = result.rows[0][0];
            // PostgreSQL EXPLAIN (FORMAT JSON) returns JSON in first column
            if (typeof firstCell === 'object' || typeof firstCell === 'string') {
              const jsonData = typeof firstCell === 'string' ? JSON.parse(firstCell) : firstCell;
              const planResult = parseExplainJson(jsonData);
              setQueryPlan(planResult);
              setShowPlanView(true); // Automatically show plan view
            }
          }
        } catch (parseError) {
          console.error('Failed to parse EXPLAIN output:', parseError);
          // Not a valid EXPLAIN JSON, just show regular results
          setQueryPlan(null);
          setShowPlanView(false);
        }
      } else {
        // Not an EXPLAIN query, clear plan
        setQueryPlan(null);
        setShowPlanView(false);
      }

      const executionTime = Date.now() - startTime;

      // Save successful query to history
      if (connectionProfile) {
        const historyEntry = createQueryHistory(
          connectionId,
          connectionProfile.name,
          currentDatabase || connectionProfile.database || 'unknown',
          sqlToExecute,
          true,
          executionTime,
          result.rowsAffected !== null ? result.rowsAffected : result.rows.length
        );

        // Save to backend and trigger refresh
        invoke('save_to_history', { history: historyEntry })
          .then(() => {
            setHistoryRefreshKey(prev => prev + 1);
          })
          .catch((err) => {
            console.error('Failed to save query to history:', err);
          });
      }
    } catch (err: any) {
      // User cancelled the destructive-query guard — silent no-op: restore
      // the panel as it was, no error banner, no history entry (UX-04).
      if (err instanceof QueryCancelledError) {
        setLoading(false);
        setError(previousError);
        setResults(previousResults);
        return;
      }

      // Handle error - could be a DbError from Tauri
      const errorMessage = err?.message || String(err);

      // Check if this is a connection error
      const isConnectionError = err?.kind === "connection" ||
                               errorMessage.toLowerCase().includes("connection lost") ||
                               errorMessage.toLowerCase().includes("server has gone away") ||
                               errorMessage.toLowerCase().includes("connection refused") ||
                               errorMessage.toLowerCase().includes("connection closed");

      if (isConnectionError) {
        setConnectionLost(true);
      }

      setError(errorMessage);
      setLoading(false);

      const executionTime = Date.now() - startTime;

      // Save failed query to history
      if (connectionProfile) {
        const historyEntry = createQueryHistory(
          connectionId,
          connectionProfile.name,
          currentDatabase || connectionProfile.database || 'unknown',
          sqlToExecute,
          false,
          executionTime,
          undefined,
          errorMessage
        );

        // Save to backend and trigger refresh
        invoke('save_to_history', { history: historyEntry })
          .then(() => {
            setHistoryRefreshKey(prev => prev + 1);
          })
          .catch((err) => {
            console.error('Failed to save query to history:', err);
          });
      }
    }
  };

  // Handle inserting snippet into editor
  const handleInsertSnippet = (query: string) => {
    setSql(query);
  };

  // Handle executing query from history
  const handleExecuteFromHistory = (query: string) => {
    setSql(query);
    handleExecute(query);
  };


  // If connection is lost, show the ConnectionLostError component
  if (connectionLost) {
    return (
      <div className="h-full flex items-center justify-center p-8">
        <ConnectionLostError
          databaseName={connectionProfile?.name}
          message="The database connection was lost during query execution."
          onReconnect={() => {
            setConnectionLost(false);
            setError(null);
          }}
          onGoToDashboard={() => {
            navigate({ to: '/connections' });
          }}
        />
      </div>
    );
  }

  return (
    <PanelGroup direction="horizontal" className="h-full">
      {/* Main Query Editor Area */}
      <Panel defaultSize={70} minSize={40}>
        <PanelGroup direction="vertical" className="h-full">
          {/* SQL Editor Panel */}
          <Panel defaultSize={40} minSize={20} className="relative flex flex-col">
            <div className="flex-1 overflow-hidden flex flex-col">
              <SQLEditor
                connectionId={connectionId}
                database={currentDatabase || null}
                onExecuteQuery={handleExecute}
                value={sql}
                onChange={(value) => setSql(value || '')}
                loading={loading}
              />
            </div>
          </Panel>

          {/* Horizontal Resizable Handle */}
          <PanelResizeHandle className="group relative h-1 bg-border hover:bg-primary/50 transition-colors">
            <div className="absolute inset-0 flex items-center justify-center">
              <GripHorizontal className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 transition-opacity" />
            </div>
          </PanelResizeHandle>

          {/* Results Viewer Panel */}
          <Panel defaultSize={60} minSize={30} className="relative flex flex-col">
            {/* View Toggle for EXPLAIN queries */}
            {queryPlan && (
              <div className="absolute top-2 right-2 z-10 flex items-center gap-2">
                <button
                  onClick={() => setShowPlanView(false)}
                  className={`px-3 py-1 text-sm rounded ${
                    !showPlanView
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground hover:bg-muted/80'
                  }`}
                >
                  Results
                </button>
                <button
                  onClick={() => setShowPlanView(true)}
                  className={`px-3 py-1 text-sm rounded ${
                    showPlanView
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground hover:bg-muted/80'
                  }`}
                >
                  Query Plan
                </button>
              </div>
            )}

            <div className="flex-1 overflow-hidden relative">
              {showPlanView && queryPlan ? (
                <div className="h-full overflow-auto p-4">
                  <QueryPlanVisualizer planResult={queryPlan} />
                </div>
              ) : (
                <ResultsViewer
                  columns={results?.columns || []}
                  rows={results?.rows || []}
                  rowsAffected={results?.rowsAffected || null}
                  loading={loading}
                  error={error}
                  executionTime={results?.executionTime}
                />
              )}
            </div>

            <QueryStatusBar
              connectionName={connectionProfile?.name}
              databaseName={currentDatabase}
              rowCount={results?.rows.length ?? null}
              rowsAffected={results?.rowsAffected ?? null}
              executionTime={results?.executionTime}
              queryType={(results as any)?.queryType}
              loading={loading}
            />
          </Panel>
        </PanelGroup>
      </Panel>

      {/* Vertical Resizable Handle */}
      <PanelResizeHandle className="group relative w-1 bg-border hover:bg-primary/50 transition-colors">
        <div className="absolute inset-0 flex items-center justify-center">
          <GripVertical className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 transition-opacity" />
        </div>
      </PanelResizeHandle>

      {/* Right Sidebar - History, Snippets, and Templates */}
      <Panel defaultSize={30} minSize={20} maxSize={50}>
        <Tabs defaultValue="history" className="h-full flex flex-col">
          <div className="border-b px-4 shrink-0">
            <TabsList className="h-10">
              <TabsTrigger value="history">History</TabsTrigger>
              <TabsTrigger value="snippets">Snippets</TabsTrigger>
              <TabsTrigger value="templates">Templates</TabsTrigger>
              <TabsTrigger value="ai" className="gap-1">
                <Sparkles className="h-3.5 w-3.5" />
                AI
              </TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="history" className="flex-1 m-0 overflow-hidden">
            <HistoryPanel
              connectionId={connectionId || undefined}
              currentDatabase={currentDatabase}
              onExecuteQuery={handleExecuteFromHistory}
              refreshTrigger={historyRefreshKey}
            />
          </TabsContent>

          <TabsContent value="snippets" className="flex-1 m-0 overflow-hidden">
            <SnippetSidebar onInsertSnippet={handleInsertSnippet} />
          </TabsContent>

          <TabsContent value="templates" className="flex-1 m-0 overflow-hidden">
            <TemplatesPanel onExecuteQuery={handleInsertSnippet} />
          </TabsContent>

          <TabsContent value="ai" className="flex-1 m-0 overflow-hidden">
            <AiAssistant
              currentSql={sql}
              schemaContext={schemaContext}
              lastError={error || undefined}
              onSqlGenerated={(generated) => setSql(generated)}
            />
          </TabsContent>
        </Tabs>
      </Panel>
    </PanelGroup>
  );
};
