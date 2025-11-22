/**
 * Visual Query Builder Route
 *
 * Route for the visual query builder interface that allows users to
 * construct SQL queries using a drag-and-drop interface.
 */

import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useConnectionContext } from '@/contexts/ConnectionContext';
import { QueryBuilder } from '@/components/QueryBuilder';
import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { QueryExecutionResult } from '@/types/database';

export const Route = createFileRoute('/_connected/visual-query')({
  component: VisualQueryBuilderPage,
  validateSearch: (search: Record<string, unknown>) => {
    return search;
  },
});

function VisualQueryBuilderPage() {
  const navigate = useNavigate();
  const { connectionId, connectionProfile, currentDatabase } = useConnectionContext();

  const handleExecute = useCallback(
    async (sql: string) => {
      if (!connectionId) {
        toast.error('No active connection');
        return;
      }

      try {
        const startTime = performance.now();

        const result = await invoke<QueryExecutionResult>('execute_query', {
          connectionId,
          sql,
        });

        const executionTime = Math.round(performance.now() - startTime);

        toast.success(`Query executed successfully in ${executionTime}ms`, {
          description: `${result.rows.length} rows returned`,
        });

        // Navigate to query tab with results
        // Store the results in localStorage or state management
        localStorage.setItem(
          'last-query-result',
          JSON.stringify({
            sql,
            result,
            executionTime,
            timestamp: Date.now(),
          })
        );

        // Navigate to query page to show results
        navigate({
          to: '/query',
          search: {
            tabs: 'query-visual-builder',
            active: 0,
          },
        });
      } catch (error: any) {
        console.error('Query execution failed:', error);
        toast.error('Query execution failed', {
          description: error.message || 'An unknown error occurred',
        });
      }
    },
    [connectionId, navigate]
  );

  if (!connectionId || !connectionProfile) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-center">
          <h2 className="text-xl font-semibold text-foreground">No Connection</h2>
          <p className="text-sm text-muted-foreground mt-2">
            Please connect to a database to use the visual query builder.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full">
      <QueryBuilder
        connectionId={connectionId}
        driver={connectionProfile.driver}
        currentDatabase={currentDatabase}
        onExecute={handleExecute}
      />
    </div>
  );
}
