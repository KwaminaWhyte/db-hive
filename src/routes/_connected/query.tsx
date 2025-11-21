import { createFileRoute } from "@tanstack/react-router";
import { QueryPanel } from "@/components/QueryPanel";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { invoke } from "@tauri-apps/api/core";
import { QueryExecutionResult } from "@/types/database";

/**
 * Query Panel Route
 *
 * SQL editor interface for executing queries against the connected database.
 *
 * URL: /_connected/query
 *
 * Features:
 * - Monaco editor for SQL
 * - Query history
 * - Query execution
 * - Results display
 */
export const Route = createFileRoute("/_connected/query")({
  component: QueryPanelRoute,
});

function QueryPanelRoute() {
  const { connectionId, connectionProfile, currentDatabase } = useConnectionContext();

  const handleExecuteQuery = async (sql: string): Promise<QueryExecutionResult> => {
    try {
      const result = await invoke<QueryExecutionResult>("execute_query", {
        connectionId,
        sql,
      });
      return result;
    } catch (error) {
      throw error;
    }
  };

  return (
    <div className="flex-1 h-full">
      <QueryPanel
        connectionId={connectionId}
        connectionProfile={connectionProfile}
        currentDatabase={currentDatabase}
        onExecuteQuery={handleExecuteQuery}
      />
    </div>
  );
}
