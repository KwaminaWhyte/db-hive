/**
 * Visual Query Builder Route
 *
 * Route for the visual query builder interface that allows users to
 * construct SQL queries using a drag-and-drop interface.
 */

import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useConnectionContext } from '@/contexts/ConnectionContext';
import { useCallback, lazy, Suspense } from 'react';
import { toast } from 'sonner';
import { Spinner } from '@/components/ui/spinner';

// QueryBuilder embeds Monaco for its SQL preview pane.
const QueryBuilder = lazy(() =>
  import('@/components/QueryBuilder').then((m) => ({ default: m.QueryBuilder }))
);

export const Route = createFileRoute('/_connected/visual-query')({
  component: VisualQueryBuilderPage,
  validateSearch: (search: Record<string, unknown>) => {
    return search;
  },
});

function RouteChunkFallback({ label }: { label: string }) {
  return (
    <div
      role="status"
      aria-label={label}
      className="flex h-full w-full items-center justify-center"
    >
      <Spinner className="size-6 text-muted-foreground" />
    </div>
  );
}

function VisualQueryBuilderPage() {
  const navigate = useNavigate();
  const { connectionId, connectionProfile, currentDatabase } = useConnectionContext();

  const handleExecute = useCallback(
    async (sql: string) => {
      if (!connectionId || !currentDatabase) {
        toast.error('No active connection or database');
        return;
      }

      // Store the SQL in the TabContext format
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      const currentStorage = localStorage.getItem(storageKey);

      let tabStates: Record<string, any> = {};
      let tabIds: string[] = [];

      if (currentStorage) {
        try {
          const parsed = JSON.parse(currentStorage);
          // Handle TabContext format: { states: {...}, timestamp: ... }
          tabStates = parsed.states || {};
          tabIds = Object.keys(tabStates);
        } catch {
          tabStates = {};
          tabIds = [];
        }
      }

      // Check if there's already a query tab (reuse the first one)
      const existingQueryTabId = tabIds.find((id) => tabStates[id]?.type === 'query');

      if (existingQueryTabId) {
        // Update the existing query tab with the new SQL
        tabStates[existingQueryTabId] = {
          ...tabStates[existingQueryTabId],
          sql,
          label: 'Visual Query',
        };

        localStorage.setItem(storageKey, JSON.stringify({
          states: tabStates,
          timestamp: Date.now(),
        }));

        // Navigate to the existing tab
        const activeIndex = tabIds.indexOf(existingQueryTabId);
        navigate({
          to: '/query',
          search: {
            tabs: tabIds.join(','),
            active: activeIndex,
          },
        });
      } else {
        // No query tab exists, create a new one
        const visualBuilderTabId = 'visual-builder';
        tabStates[visualBuilderTabId] = {
          id: visualBuilderTabId,
          type: 'query',
          sql,
          label: 'Visual Query',
        };

        tabIds.push(visualBuilderTabId);
        localStorage.setItem(storageKey, JSON.stringify({
          states: tabStates,
          timestamp: Date.now(),
        }));

        // Navigate to the new tab
        navigate({
          to: '/query',
          search: {
            tabs: tabIds.join(','),
            active: tabIds.length - 1,
          },
        });
      }

      // Show success toast
      toast.success('Query loaded in SQL Editor', {
        description: 'Click Execute to run the query',
      });
    },
    [connectionId, currentDatabase, navigate]
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
      <Suspense fallback={<RouteChunkFallback label="Loading query builder" />}>
        <QueryBuilder
          connectionId={connectionId}
          driver={connectionProfile.driver}
          currentDatabase={currentDatabase}
          onExecute={handleExecute}
        />
      </Suspense>
    </div>
  );
}
