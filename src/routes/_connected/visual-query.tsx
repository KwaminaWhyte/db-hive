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
import { toast } from 'sonner';

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
      if (!connectionId || !currentDatabase) {
        toast.error('No active connection or database');
        return;
      }

      // Store the SQL in a special key for the query page to pick up
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      const currentTabs = localStorage.getItem(storageKey);

      let tabs: any[];
      if (currentTabs) {
        try {
          tabs = JSON.parse(currentTabs);
        } catch {
          tabs = [];
        }
      } else {
        tabs = [];
      }

      // Add or update the visual builder tab
      const visualBuilderTabIndex = tabs.findIndex((t) => t.id === 'visual-builder');
      const visualBuilderTab = {
        id: 'visual-builder',
        type: 'query',
        sql,
        name: 'Visual Query',
      };

      if (visualBuilderTabIndex >= 0) {
        tabs[visualBuilderTabIndex] = visualBuilderTab;
      } else {
        tabs.push(visualBuilderTab);
      }

      localStorage.setItem(storageKey, JSON.stringify(tabs));

      // Navigate to query page
      navigate({
        to: '/query',
        search: {
          tabs: tabs.map((t) => t.id).join(','),
          active: tabs.length - 1,
        },
      });

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
      <QueryBuilder
        connectionId={connectionId}
        driver={connectionProfile.driver}
        currentDatabase={currentDatabase}
        onExecute={handleExecute}
      />
    </div>
  );
}
