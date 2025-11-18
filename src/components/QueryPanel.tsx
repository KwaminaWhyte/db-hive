import { FC, useState } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { SQLEditor } from './SQLEditor';
import { ResultsViewer } from './ResultsViewer';
import { QueryExecutionResult } from '@/types/database';
import { GripHorizontal } from 'lucide-react';

interface QueryPanelProps {
  /** Active connection ID */
  connectionId: string | null;

  /** Callback to execute query - returns a Promise with results */
  onExecuteQuery: (sql: string) => Promise<QueryExecutionResult>;
}

export const QueryPanel: FC<QueryPanelProps> = ({
  connectionId,
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

    try {
      const result = await onExecuteQuery(sqlToExecute);
      setResults(result);
    } catch (err: any) {
      // Handle error - could be a DbError from Tauri
      const errorMessage = err?.message || String(err);
      setError(errorMessage);
    } finally {
      setLoading(false);
    }
  };

  return (
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

      {/* Resizable Handle */}
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
  );
};
