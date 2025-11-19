import { FC, useState, useEffect } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { SQLEditor } from './SQLEditor';
import { ResultsViewer } from './ResultsViewer';
import { HistoryPanel } from './HistoryPanel';
import { SnippetSidebar } from './SnippetSidebar';
import { QueryExecutionResult, ConnectionProfile } from '@/types/database';
import { createQueryHistory } from '@/types/history';
import { invoke } from '@tauri-apps/api/core';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs';
import { Button } from './ui/button';
import { GripHorizontal, GripVertical, Plus, X } from 'lucide-react';

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

  // Get the active tab
  const activeTab = tabs.find((t) => t.id === activeTabId) || tabs[0];

  // Handle pending query from parent
  useEffect(() => {
    if (pendingQuery) {
      updateTab(activeTabId, { sql: pendingQuery });
      onQueryLoaded?.();
    }
  }, [pendingQuery]);

  // Add new tab
  const handleAddTab = () => {
    tabIdCounter++;
    const newTab: EditorTab = {
      id: `tab-${tabIdCounter}`,
      name: `Query ${tabIdCounter}`,
      sql: '',
      loading: false,
      results: null,
      error: null,
    };
    setTabs([...tabs, newTab]);
    setActiveTabId(newTab.id);
  };

  // Close tab
  const handleCloseTab = (tabId: string) => {
    if (tabs.length === 1) {
      // Don't close the last tab, just reset it
      setTabs([
        {
          id: tabId,
          name: 'Query 1',
          sql: '',
          loading: false,
          results: null,
          error: null,
        },
      ]);
      return;
    }

    const tabIndex = tabs.findIndex((t) => t.id === tabId);
    const newTabs = tabs.filter((t) => t.id !== tabId);
    setTabs(newTabs);

    // If closing active tab, switch to adjacent tab
    if (tabId === activeTabId) {
      const newActiveIndex = Math.min(tabIndex, newTabs.length - 1);
      setActiveTabId(newTabs[newActiveIndex].id);
    }
  };

  // Update tab state
  const updateTab = (
    tabId: string,
    updates: Partial<Omit<EditorTab, 'id' | 'name'>>
  ) => {
    setTabs((prevTabs) =>
      prevTabs.map((tab) =>
        tab.id === tabId ? { ...tab, ...updates } : tab
      )
    );
  };

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
            <ResultsViewer
              columns={activeTab.results?.columns || []}
              rows={activeTab.results?.rows || []}
              rowsAffected={activeTab.results?.rowsAffected || null}
              loading={activeTab.loading}
              error={activeTab.error}
              executionTime={activeTab.results?.executionTime}
            />
          </Panel>
        </PanelGroup>
      </Panel>

      {/* Vertical Resizable Handle */}
      <PanelResizeHandle className="group relative w-1 bg-border hover:bg-primary/50 transition-colors">
        <div className="absolute inset-0 flex items-center justify-center">
          <GripVertical className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
        </div>
      </PanelResizeHandle>

      {/* Right Sidebar - History and Snippets */}
      <Panel defaultSize={30} minSize={20} maxSize={50}>
        <Tabs defaultValue="history" className="h-full flex flex-col">
          <div className="border-b px-4 shrink-0">
            <TabsList className="h-10">
              <TabsTrigger value="history">History</TabsTrigger>
              <TabsTrigger value="snippets">Snippets</TabsTrigger>
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
        </Tabs>
      </Panel>
    </PanelGroup>
  );
};
