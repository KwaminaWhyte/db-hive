import { FC, useState, useEffect, useCallback } from 'react';
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
import { invoke } from '@tauri-apps/api/core';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs';
import { Button } from './ui/button';
import { GripHorizontal, GripVertical, Plus, X, Sparkles } from 'lucide-react';
import { ConnectionLostError } from './ConnectionLostError';
import { useNavigate } from '@tanstack/react-router';

interface EditorTab {
  id: string;
  name: string;
  sql: string;
  loading: boolean;
  results: QueryExecutionResult | null;
  error: string | null;
}

interface QueryPanelProps {
  /** Active connection ID */
  connectionId: string | null;

  /** Active connection profile (for history metadata) */
  connectionProfile?: ConnectionProfile;

  /** Current database name */
  currentDatabase?: string;

  /** Callback to execute query - returns a Promise with results */
  onExecuteQuery: (sql: string) => Promise<QueryExecutionResult>;

  /** Pending query to load into active tab */
  pendingQuery?: string | null;

  /** Callback when pending query is loaded */
  onQueryLoaded?: () => void;
}

let tabIdCounter = 1;

export const QueryPanel: FC<QueryPanelProps> = ({
  connectionId,
  connectionProfile,
  currentDatabase,
  onExecuteQuery,
  pendingQuery,
  onQueryLoaded,
}) => {
  // Tab management
  const [tabs, setTabs] = useState<EditorTab[]>([
    {
      id: 'tab-1',
      name: 'Query 1',
      sql: '',
      loading: false,
      results: null,
      error: null,
    },
  ]);
  const [activeTabId, setActiveTabId] = useState('tab-1');
  const [historyRefreshKey, setHistoryRefreshKey] = useState(0);
  const [connectionLost, setConnectionLost] = useState(false);
  const [schemaContext, setSchemaContext] = useState('');
  const navigate = useNavigate();

  // Query Plan Visualizer state
  const [queryPlan, setQueryPlan] = useState<QueryPlanResult | null>(null);
  const [showPlanView, setShowPlanView] = useState(false);

  // Get the active tab
  const activeTab = tabs.find((t) => t.id === activeTabId) || tabs[0];

  // Handle pending query from parent
  useEffect(() => {
    if (pendingQuery) {
      updateTab(activeTabId, { sql: pendingQuery });
      onQueryLoaded?.();
    }
  }, [pendingQuery]);

  // Load schema context for AI assistant
  useEffect(() => {
    const loadSchemaContext = async () => {
      if (!connectionId) {
        setSchemaContext('');
        return;
      }

      try {
        // First get schemas
        const schemas = await invoke<Array<{ name: string }>>('get_schemas', {
          connectionId,
          database: currentDatabase || '',
        });

        let context = `Database: ${currentDatabase || 'unknown'}\n\nTables:\n`;
        let totalTables = 0;

        // Get tables from each schema (prioritize 'public' for PostgreSQL, skip system schemas)
        const schemaOrder = schemas
          .map(s => s.name)
          .filter(name => !['information_schema', 'pg_catalog', 'mysql', 'performance_schema', 'sys'].includes(name))
          .sort((a, b) => {
            if (a === 'public') return -1;
            if (b === 'public') return 1;
            return a.localeCompare(b);
          });

        for (const schemaName of schemaOrder) {
          try {
            const tables = await invoke<Array<{ name: string; schema: string; table_type: string }>>('get_tables', {
              connectionId,
              schema: schemaName,
            });

            for (const table of tables) {
              try {
                const tableSchema = await invoke<{ columns: Array<{ name: string; dataType: string; nullable: boolean; isPrimaryKey: boolean }> }>('get_table_schema', {
                  connectionId,
                  table: table.name,
                  schema: schemaName,
                });

                context += `\n${schemaName}.${table.name}:\n`;
                for (const col of tableSchema.columns) {
                  context += `  - ${col.name}: ${col.dataType}${col.isPrimaryKey ? ' (PK)' : ''}${col.nullable ? '' : ' NOT NULL'}\n`;
                }
                totalTables++;
              } catch (err) {
                console.warn(`Failed to get schema for ${schemaName}.${table.name}:`, err);
              }
            }
          } catch (err) {
            console.warn(`Failed to get tables for schema ${schemaName}:`, err);
          }
        }

        if (totalTables === 0) {
          setSchemaContext(`Database: ${currentDatabase || 'unknown'}\n\nNo tables found.`);
          return;
        }

        console.log('Schema context loaded:', context.substring(0, 200) + '...');
        setSchemaContext(context);
      } catch (err) {
        console.error('Failed to load schema context:', err);
        setSchemaContext('');
      }
    };

    loadSchemaContext();
  }, [connectionId, currentDatabase]);

  // Update tab state - memoized to prevent unnecessary re-renders
  const updateTab = useCallback((
    tabId: string,
    updates: Partial<Omit<EditorTab, 'id' | 'name'>>
  ) => {
    setTabs((prevTabs) =>
      prevTabs.map((tab) =>
        tab.id === tabId ? { ...tab, ...updates } : tab
      )
    );
  }, []);

  // Add new tab - memoized
  const handleAddTab = useCallback(() => {
    tabIdCounter++;
    const newTab: EditorTab = {
      id: `tab-${tabIdCounter}`,
      name: `Query ${tabIdCounter}`,
      sql: '',
      loading: false,
      results: null,
      error: null,
    };
    setTabs((prevTabs) => [...prevTabs, newTab]);
    setActiveTabId(newTab.id);
  }, []);

  // Close tab - memoized
  const handleCloseTab = useCallback((tabId: string) => {
    setTabs((prevTabs) => {
      if (prevTabs.length === 1) {
        // Don't close the last tab, just reset it
        return [
          {
            id: tabId,
            name: 'Query 1',
            sql: '',
            loading: false,
            results: null,
            error: null,
          },
        ];
      }

      const tabIndex = prevTabs.findIndex((t) => t.id === tabId);
      const newTabs = prevTabs.filter((t) => t.id !== tabId);

      // If closing active tab, switch to adjacent tab
      setActiveTabId((currentActiveId) => {
        if (tabId === currentActiveId) {
          const newActiveIndex = Math.min(tabIndex, newTabs.length - 1);
          return newTabs[newActiveIndex].id;
        }
        return currentActiveId;
      });

      return newTabs;
    });
  }, []);

  // Handle query execution
  const handleExecute = async (sqlToExecute: string) => {
    if (!connectionId) {
      updateTab(activeTabId, { error: 'No active connection' });
      return;
    }

    updateTab(activeTabId, {
      loading: true,
      error: null,
      results: null,
    });

    const startTime = Date.now();

    try {
      const result = await onExecuteQuery(sqlToExecute);
      updateTab(activeTabId, {
        results: result,
        loading: false,
      });

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

      updateTab(activeTabId, {
        error: errorMessage,
        loading: false,
      });

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
    updateTab(activeTabId, { sql: query });
  };

  // Handle executing query from history
  const handleExecuteFromHistory = (query: string) => {
    updateTab(activeTabId, { sql: query });
    handleExecute(query);
  };

  // Handle SQL change
  const handleSqlChange = (value: string) => {
    updateTab(activeTabId, { sql: value });
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
            // Clear all tab errors
            setTabs(tabs.map(tab => ({ ...tab, error: null })));
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
          {/* SQL Editor Panel with Tabs */}
          <Panel defaultSize={40} minSize={20} className="relative flex flex-col">
            {/* Tab Bar */}
            <div className="flex items-center gap-1 bg-muted/30 border-b px-2 py-1 overflow-x-auto">
              {tabs.map((tab) => (
                <div
                  key={tab.id}
                  className={`
                    group flex items-center gap-2 px-3 py-1.5 rounded-t-md text-sm cursor-pointer transition-colors
                    ${
                      tab.id === activeTabId
                        ? 'bg-background border-t border-x text-foreground'
                        : 'text-muted-foreground hover:text-foreground hover:bg-muted/50'
                    }
                  `}
                  onClick={() => setActiveTabId(tab.id)}
                >
                  <span className="select-none">{tab.name}</span>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleCloseTab(tab.id);
                    }}
                    className="opacity-0 group-hover:opacity-100 hover:bg-muted rounded p-0.5 transition-opacity"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              ))}
              <Button
                variant="ghost"
                size="sm"
                onClick={handleAddTab}
                className="h-7 px-2"
              >
                <Plus className="h-4 w-4" />
              </Button>
            </div>

            {/* Active Tab Editor */}
            <div className="flex-1 overflow-hidden">
              <SQLEditor
                connectionId={connectionId}
                database={currentDatabase || null}
                onExecuteQuery={handleExecute}
                value={activeTab.sql}
                onChange={(value) => handleSqlChange(value || '')}
                loading={activeTab.loading}
              />
            </div>
          </Panel>

          {/* Horizontal Resizable Handle */}
          <PanelResizeHandle className="group relative h-1 bg-border hover:bg-primary/50 transition-colors">
            <div className="absolute inset-0 flex items-center justify-center">
              <GripHorizontal className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>
          </PanelResizeHandle>

          {/* Results Viewer Panel */}
          <Panel defaultSize={60} minSize={30} className="relative">
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

            {showPlanView && queryPlan ? (
              <div className="h-full overflow-auto p-4">
                <QueryPlanVisualizer planResult={queryPlan} />
              </div>
            ) : (
              <ResultsViewer
                columns={activeTab.results?.columns || []}
                rows={activeTab.results?.rows || []}
                rowsAffected={activeTab.results?.rowsAffected || null}
                loading={activeTab.loading}
                error={activeTab.error}
                executionTime={activeTab.results?.executionTime}
              />
            )}
          </Panel>
        </PanelGroup>
      </Panel>

      {/* Vertical Resizable Handle */}
      <PanelResizeHandle className="group relative w-1 bg-border hover:bg-primary/50 transition-colors">
        <div className="absolute inset-0 flex items-center justify-center">
          <GripVertical className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
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
              currentSql={activeTab.sql}
              schemaContext={schemaContext}
              lastError={activeTab.error || undefined}
              onSqlGenerated={(sql) => updateTab(activeTabId, { sql })}
            />
          </TabsContent>
        </Tabs>
      </Panel>
    </PanelGroup>
  );
};
