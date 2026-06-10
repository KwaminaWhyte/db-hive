import { useState, useEffect, useCallback, useRef } from "react";
import { createFileRoute, Outlet, useNavigate } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
  type ImperativePanelHandle,
} from "react-resizable-panels";
import { SchemaExplorer } from "@/components/SchemaExplorer";
import { EnvironmentBadge } from "@/components/EnvironmentBadge";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useTabContext } from "@/contexts/TabContext";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from "@/components/ui/tooltip";
import {
  LogOut,
  Database,
  PanelLeftClose,
  PanelLeftOpen,
  GripVertical,
} from "lucide-react";

const isMacOS = navigator.userAgent.includes("Mac");

// Sidebar layout is scoped per OS window so multiple open windows
// (multi-window mode) don't clobber each other's panel sizes. The `main`
// window keeps the un-suffixed key for backward compatibility.
function sidebarLayoutId(): string {
  try {
    const label = getCurrentWindow().label;
    return label === "main"
      ? "db-hive-connected-layout"
      : `db-hive-connected-layout-${label}`;
  } catch {
    return "db-hive-connected-layout";
  }
}

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
  const [disconnectPromptOpen, setDisconnectPromptOpen] = useState(false);
  const sidebarPanelRef = useRef<ImperativePanelHandle>(null);
  const [layoutId] = useState(sidebarLayoutId);

  const toggleSidebar = useCallback(() => {
    const panel = sidebarPanelRef.current;
    if (!panel) return;
    if (panel.isCollapsed()) {
      panel.expand();
    } else {
      panel.collapse();
    }
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

  // Guard: If not connected, redirect to home page.
  // The redirect lives in an effect (not the render body) because connection
  // state lives in React context, which beforeLoad can't reach.
  const isConnected = Boolean(connectionId && connectionProfile);
  useEffect(() => {
    if (!isConnected) {
      navigate({ to: "/" });
    }
  }, [isConnected, navigate]);

  if (!connectionId || !connectionProfile) {
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

  const handleRedisKeySelect = (key: string) => {
    const tabId = `rediskey-${encodeURIComponent(key)}`;
    navigate({
      to: "/query",
      search: (prev: { tabs?: string; active?: number }) => {
        const currentTabIds = prev.tabs
          ? prev.tabs.split(",")
          : [`query-${Date.now()}`];
        const existingIndex = currentTabIds.indexOf(tabId);
        if (existingIndex >= 0) {
          return {
            tabs: prev.tabs || currentTabIds[0],
            active: existingIndex,
          };
        }
        const newTabIds = [...currentTabIds, tabId];
        return {
          tabs: newTabIds.join(","),
          active: newTabIds.length - 1,
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
                  aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
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
                {sidebarCollapsed ? "Show sidebar" : "Hide sidebar"} (
                {isMacOS ? "Cmd" : "Ctrl"}+B)
              </TooltipContent>
            </Tooltip>
            <Database className="size-3.5 text-muted-foreground" />
            <span className="text-xs font-medium">
              {connectionProfile.name}
            </span>
            {connectionProfile.environment && (
              <EnvironmentBadge
                environment={connectionProfile.environment}
                className="text-[0.6rem]"
              />
            )}
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
            onClick={() => setDisconnectPromptOpen(true)}
            className="h-6 px-2 text-xs"
            title="Disconnect"
          >
            <LogOut className="size-3 mr-1" />
            Disconnect
          </Button>
        </div>
      </div>

      {/* Main Content Area (with top padding for titlebar + connection bar) */}
      <div className="flex-1 pt-8 overflow-hidden">
        <PanelGroup
          direction="horizontal"
          autoSaveId={layoutId}
          className="h-full"
        >
          {/* Left Sidebar - Schema Explorer */}
          <Panel
            ref={sidebarPanelRef}
            collapsible
            collapsedSize={0}
            defaultSize={22}
            minSize={15}
            maxSize={40}
            onCollapse={() => setSidebarCollapsed(true)}
            onExpand={() => setSidebarCollapsed(false)}
          >
            <div className="h-full overflow-y-auto">
              <SchemaExplorer
                connectionId={connectionId}
                connectionProfile={connectionProfile}
                onDisconnect={handleDisconnect}
                onTableSelect={handleTableSelect}
                onDatabaseChange={setCurrentDatabase}
                onOpenERDiagram={handleOpenERDiagram}
                onExecuteQuery={handleExecuteQuery}
                onRedisKeySelect={handleRedisKeySelect}
              />
            </div>
          </Panel>

          {/* Sidebar Resize Handle */}
          <PanelResizeHandle className="group relative w-1 bg-border hover:bg-primary/50 transition-colors">
            <div className="absolute inset-0 flex items-center justify-center">
              <GripVertical className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 transition-opacity" />
            </div>
          </PanelResizeHandle>

          {/* Main Content - Child Routes */}
          <Panel defaultSize={78} minSize={50}>
            <div className="h-full overflow-hidden">
              <Outlet />
            </div>
          </Panel>
        </PanelGroup>
      </div>

      {/* Disconnect Confirmation */}
      <AlertDialog
        open={disconnectPromptOpen}
        onOpenChange={setDisconnectPromptOpen}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Disconnect from {connectionProfile.name}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              Any unsaved query results in this window will be lost.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDisconnect}>
              Disconnect
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
