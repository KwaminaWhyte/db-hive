import { FC, useState } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { SQLEditor } from './SQLEditor';
import { ResultsViewer } from './ResultsViewer';
import { HistoryPanel } from './HistoryPanel';
import { SnippetSidebar } from './SnippetSidebar';
import { QueryExecutionResult, ConnectionProfile } from '@/types/database';
import { createQueryHistory } from '@/types/history';
import { invoke } from '@tauri-apps/api/core';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs';
import { GripHorizontal, GripVertical } from 'lucide-react';

interface QueryPanelProps {
  /** Active connection ID */
  connectionId: string | null;

  /** Active connection profile (for history metadata) */
  connectionProfile?: ConnectionProfile;

  /** Callback to execute query - returns a Promise with results */
  onExecuteQuery: (sql: string) => Promise<QueryExecutionResult>;
}

export const QueryPanel: FC<QueryPanelProps> = ({
  connectionId,
  connectionProfile,
  onExecuteQuery,
}) => {
  const [sql, setSql] = useState('');
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<QueryExecutionResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Handle query execution
  const handleExecute = async (sqlToExecute: string) => {
    if (!connectionId) {
      setError('No active connection');
      return;
    }

    setLoading(true);
    setError(null);
    setResults(null);

    const startTime = Date.now();

    try {
      const result = await onExecuteQuery(sqlToExecute);
      setResults(result);

      const executionTime = Date.now() - startTime;

      // Save successful query to history
      if (connectionProfile) {
        const historyEntry = createQueryHistory(
          connectionId,
          connectionProfile.name,
          connectionProfile.database || 'unknown',
          sqlToExecute,
          true,
          executionTime,
          result.rowsAffected !== null ? result.rowsAffected : result.rows.length
        );

        // Save to backend (fire and forget - don't block UI)
        invoke('save_to_history', { history: historyEntry }).catch((err) => {
          console.error('Failed to save query to history:', err);
        });
      }
    } catch (err: any) {
      // Handle error - could be a DbError from Tauri
      const errorMessage = err?.message || String(err);
      setError(errorMessage);

      const executionTime = Date.now() - startTime;

      // Save failed query to history
      if (connectionProfile) {
        const historyEntry = createQueryHistory(
          connectionId,
          connectionProfile.name,
          connectionProfile.database || 'unknown',
          sqlToExecute,
          false,
          executionTime,
          undefined,
          errorMessage
        );

        // Save to backend (fire and forget)
        invoke('save_to_history', { history: historyEntry }).catch((err) => {
          console.error('Failed to save query to history:', err);
        });
      }
    } finally {
      setLoading(false);
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

  return (
    <PanelGroup direction="horizontal" className="h-full">
      {/* Main Query Editor Area */}
      <Panel defaultSize={70} minSize={40}>
        <PanelGroup direction="vertical" className="h-full">
          {/* SQL Editor Panel */}
          <Panel defaultSize={40} minSize={20} className="relative">
            <SQLEditor
              connectionId={connectionId}
              onExecuteQuery={handleExecute}
              value={sql}
              onChange={(value) => setSql(value || '')}
              loading={loading}
            />
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
              columns={results?.columns || []}
              rows={results?.rows || []}
              rowsAffected={results?.rowsAffected || null}
              loading={loading}
              error={error}
              executionTime={results?.executionTime}
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
              onExecuteQuery={handleExecuteFromHistory}
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
