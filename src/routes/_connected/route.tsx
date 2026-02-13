import { useState, useEffect, useCallback } from "react";
import { createFileRoute, Outlet, useNavigate } from "@tanstack/react-router";
import { SchemaExplorer } from "@/components/SchemaExplorer";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useTabContext } from "@/contexts/TabContext";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from "@/components/ui/tooltip";
import { LogOut, Database, PanelLeftClose, PanelLeftOpen } from "lucide-react";

/**
 * Connected Layout Route
 *
 * This is a layout route that wraps all routes requiring an active database connection.
 * It includes a navigation guard that redirects to /connections if no connection exists.
 *
 * Layout includes:
 * - Top bar with connection info and controls
 * - Left sidebar with schema explorer
 * - Main content area (Outlet for child routes)
 */
export const Route = createFileRoute("/_connected")({
  beforeLoad: () => {
    // Note: We can't access context in beforeLoad in this simple setup
    // The guard is handled in the component instead
    // In a more advanced setup, you'd use router context
  },
  component: ConnectedLayout,
});

function ConnectedLayout() {
  const navigate = useNavigate();
  const {
    connectionId,
    connectionProfile,
    currentDatabase,
    setCurrentDatabase,
    disconnect,
  } = useConnectionContext();
  const { getTabState, updateTabState, createTabState } = useTabContext();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  const toggleSidebar = useCallback(() => {
    setSidebarCollapsed((prev) => !prev);
  }, []);

  // Keyboard shortcut: Cmd+B / Ctrl+B to toggle sidebar
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "b") {
        e.preventDefault();
        toggleSidebar();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [toggleSidebar]);

  // Guard: If not connected, redirect to home page
  if (!connectionId || !connectionProfile) {
    navigate({ to: "/" });
    return null;
  }

  const handleDisconnect = () => {
    disconnect();
    navigate({ to: "/" });
  };

  const handleTableSelect = (schema: string, tableName: string) => {
    // Create table tab ID
    const tableId = `table-${schema}.${tableName}`;

    // Navigate to query route with the table added to tabs
    navigate({
      to: "/query",
      search: (prev: { tabs?: string; active?: number }) => {
        const currentTabIds = prev.tabs
          ? prev.tabs.split(",")
          : [`query-${Date.now()}`];

        // Check if table is already open
        const existingIndex = currentTabIds.indexOf(tableId);

        if (existingIndex >= 0) {
          // Table already open, just switch to it
          return {
            tabs: prev.tabs || currentTabIds[0],
            active: existingIndex,
          };
        }

        // Add new table tab
        const newTabIds = [...currentTabIds, tableId];
        return {
          tabs: newTabIds.join(","),
          active: newTabIds.length - 1, // Switch to new tab
        };
      },
    });
  };

  const handleOpenERDiagram = (schema: string) => {
    navigate({
      to: "/er-diagram/$schema",
      params: { schema },
    });
  };

  /**
   * Handle SQL generation from SchemaExplorer (Generate SELECT/INSERT)
   * Inserts the SQL into the active query tab or creates a new one
   */
  const handleExecuteQuery = (sql: string) => {
    // Navigate to query route with the SQL inserted into the active tab or a new tab
    navigate({
      to: "/query",
      search: (prev: { tabs?: string; active?: number }) => {
        const currentTabIds = prev.tabs ? prev.tabs.split(",") : [`query-${Date.now()}`];
        const activeIndex = prev.active ?? 0;
        const activeTabId = currentTabIds[activeIndex];

        if (activeTabId) {
          const activeTab = getTabState(activeTabId);

          if (activeTab?.type === "query") {
            // Active tab is a query tab - append SQL to it
            const currentSql = activeTab.sql || "";
            const newSql = currentSql ? `${currentSql}\n\n${sql}` : sql;
            updateTabState(activeTabId, { sql: newSql });
            return {
              tabs: prev.tabs || currentTabIds.join(","),
              active: activeIndex,
            };
          } else {
            // Active tab is a table tab - find first query tab or create new one
            const queryTab = currentTabIds.find((id) => getTabState(id)?.type === "query");

            if (queryTab) {
              // Found a query tab - update it and switch to it
              const tabState = getTabState(queryTab);
              const currentSql = tabState?.sql || "";
              const newSql = currentSql ? `${currentSql}\n\n${sql}` : sql;
              updateTabState(queryTab, { sql: newSql });

              // Switch to the query tab
              const queryTabIndex = currentTabIds.indexOf(queryTab);
              return {
                tabs: prev.tabs || currentTabIds.join(","),
                active: queryTabIndex,
              };
            } else {
              // No query tabs exist - create a new one
              const newTabId = `query-${Date.now()}`;
              createTabState({
                id: newTabId,
                type: "query",
                label: "Query",
                sql,
              });

              const newTabIds = [...currentTabIds, newTabId];
              return {
                tabs: newTabIds.join(","),
                active: newTabIds.length - 1,
              };
            }
          }
        }

        // Fallback - create new query tab
        const newTabId = `query-${Date.now()}`;
        createTabState({
          id: newTabId,
          type: "query",
          label: "Query",
          sql,
        });

        return {
          tabs: newTabId,
          active: 0,
        };
      },
    });
  };

  return (
    <div className="flex-1 flex h-full relative">
      {/* Connection Info Bar - Slim bar below titlebar */}
      <div className="fixed top-10 left-0 right-0 h-8 border-b border-border bg-accent/30 z-40">
        <div className="flex items-center justify-between h-full px-4">
          {/* Left: Sidebar Toggle + Connection Info */}
          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={toggleSidebar}
                  className="h-6 w-6 rounded-sm"
                >
                  {sidebarCollapsed ? (
                    <PanelLeftOpen className="size-3.5" />
                  ) : (
                    <PanelLeftClose className="size-3.5" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="bottom">
                {sidebarCollapsed ? "Show sidebar" : "Hide sidebar"} (Cmd+B)
              </TooltipContent>
            </Tooltip>
            <Database className="size-3.5 text-muted-foreground" />
            <span className="text-xs font-medium">
              {connectionProfile.name}
            </span>
            {currentDatabase && (
              <>
                <span className="text-xs text-muted-foreground">/</span>
                <span className="text-xs text-muted-foreground">
                  {currentDatabase}
                </span>
              </>
            )}
          </div>

          {/* Right: Disconnect Button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleDisconnect}
            className="h-6 px-2 text-xs"
            title="Disconnect"
          >
            <LogOut className="size-3 mr-1" />
            Disconnect
          </Button>
        </div>
      </div>

      {/* Main Content Area (with top padding for titlebar + connection bar) */}
      <div className="flex-1 flex pt-8">
        {/* Left Sidebar - Schema Explorer */}
        <div
          className={`border-r overflow-hidden transition-[width] duration-200 ease-in-out ${
            sidebarCollapsed ? "w-0 border-r-0" : "w-80"
          }`}
        >
          <div className="w-80 h-full overflow-y-auto">
            <SchemaExplorer
              connectionId={connectionId}
              connectionProfile={connectionProfile}
              onDisconnect={handleDisconnect}
              onTableSelect={handleTableSelect}
              onDatabaseChange={setCurrentDatabase}
              onOpenERDiagram={handleOpenERDiagram}
              onExecuteQuery={handleExecuteQuery}
            />
          </div>
        </div>

        {/* Main Content - Child Routes */}
        <div className="flex-1 overflow-hidden">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
