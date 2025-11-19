/**
 * History Panel Component
 *
 * Displays query execution history with filtering and actions to
 * re-run queries or clear history. Shows metadata like execution time,
 * row count, and success/failure status.
 */

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { QueryHistory } from "../types/history";
import { Button } from "./ui/button";
import { ScrollArea } from "./ui/scroll-area";
import { Badge } from "./ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./ui/card";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "./ui/alert-dialog";

interface HistoryPanelProps {
  /** Current connection ID (to filter history) */
  connectionId?: string;
  /** Callback to execute a query from history */
  onExecuteQuery?: (query: string) => void;
  /** Trigger to refresh history (changes when new queries are executed) */
  refreshTrigger?: number;
}

export function HistoryPanel({
  connectionId,
  onExecuteQuery,
  refreshTrigger,
}: HistoryPanelProps) {
  const [history, setHistory] = useState<QueryHistory[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(50);

  // Load history on mount and when connectionId, limit, or refreshTrigger changes
  useEffect(() => {
    loadHistory();
  }, [connectionId, limit, refreshTrigger]);

  const loadHistory = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<QueryHistory[]>("get_query_history", {
        connectionId: connectionId || null,
        limit,
      });
      setHistory(result);
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    } finally {
      setLoading(false);
    }
  };

  const handleClearHistory = async () => {
    try {
      await invoke("clear_history", {
        connectionId: connectionId || null,
      });
      await loadHistory(); // Reload history after clearing
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  };

  const formatDate = (isoDate: string) => {
    const date = new Date(isoDate);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;

    return date.toLocaleDateString() + " " + date.toLocaleTimeString();
  };

  const formatExecutionTime = (ms?: number) => {
    if (!ms) return "N/A";
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
  };

  return (
    <div className="flex flex-col h-full">
      <CardHeader className="shrink-0">
        <div className="flex flex-col gap-2">
          <div>
            <CardTitle>Query History</CardTitle>
            <CardDescription>
              {connectionId
                ? "History for current connection"
                : "History for all connections"}
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <select
              className="px-3 py-1 border rounded-md text-sm"
              value={limit}
              onChange={(e) => setLimit(parseInt(e.target.value))}
            >
              <option value={25}>Last 25</option>
              <option value={50}>Last 50</option>
              <option value={100}>Last 100</option>
              <option value={500}>Last 500</option>
            </select>
            <Button variant="outline" size="sm" onClick={loadHistory}>
              Refresh
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="destructive" size="sm">
                  Clear
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Clear query history?</AlertDialogTitle>
                  <AlertDialogDescription>
                    {connectionId
                      ? "This will permanently delete the query history for the current connection."
                      : "This will permanently delete all query history."}
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={handleClearHistory}>
                    Clear History
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex-1 overflow-hidden p-0">
        {error && (
          <div className="px-4 py-3 bg-destructive/10 text-destructive text-sm">
            {error}
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center h-full">
            <div className="text-muted-foreground">Loading history...</div>
          </div>
        ) : history.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-center px-4">
            <div className="text-muted-foreground mb-2">No query history</div>
            <div className="text-sm text-muted-foreground">
              Execute queries to see them appear here
            </div>
          </div>
        ) : (
          <ScrollArea className="h-full">
            <div className="space-y-2 p-4">
              {history.map((entry) => (
                <Card key={entry.id} className="overflow-hidden gap-0">
                  <CardHeader>
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <Badge
                            variant={entry.success ? "default" : "destructive"}
                          >
                            {entry.success ? "Success" : "Failed"}
                          </Badge>
                          <span className="text-xs text-muted-foreground">
                            {formatDate(entry.executedAt)}
                          </span>
                        </div>
                        <div className="text-sm text-muted-foreground">
                          <span className="font-medium">
                            {entry.connectionName}
                          </span>
                          {" / "}
                          {entry.database}
                        </div>
                      </div>
                      {onExecuteQuery && entry.success && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => onExecuteQuery(entry.query)}
                        >
                          Re-run
                        </Button>
                      )}
                    </div>
                  </CardHeader>
                  <CardContent className="pt-0">
                    <div className="bg-muted rounded p-2 mb-2">
                      <pre className="text-xs overflow-x-auto whitespace-pre-wrap">
                        {entry.query}
                      </pre>
                    </div>
                    <div className="flex items-center gap-4 text-xs text-muted-foreground">
                      {entry.executionTimeMs !== undefined && (
                        <div>
                          <span className="font-medium">Time:</span>{" "}
                          {formatExecutionTime(entry.executionTimeMs)}
                        </div>
                      )}
                      {entry.rowCount !== undefined && entry.success && (
                        <div>
                          <span className="font-medium">Rows:</span>{" "}
                          {entry.rowCount.toLocaleString()}
                        </div>
                      )}
                    </div>
                    {entry.errorMessage && (
                      <div className="mt-2 text-xs text-destructive">
                        <span className="font-medium">Error:</span>{" "}
                        {entry.errorMessage}
                      </div>
                    )}
                  </CardContent>
                </Card>
              ))}
            </div>
          </ScrollArea>
        )}
      </CardContent>
    </div>
  );
}
