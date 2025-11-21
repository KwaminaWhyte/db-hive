import { createFileRoute, Outlet, useNavigate } from "@tanstack/react-router";
import { SchemaExplorer } from "@/components/SchemaExplorer";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { Button } from "@/components/ui/button";
import { LogOut, Database } from "lucide-react";

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

  // Guard: If not connected, redirect to connections page
  if (!connectionId || !connectionProfile) {
    navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } });
    return null;
  }

  const handleDisconnect = () => {
    disconnect();
    navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } });
  };

  const handleTableSelect = (schema: string, tableName: string) => {
    // Create table tab ID
    const tableId = `table-${schema}.${tableName}`;

    // Navigate to query route with the table added to tabs
    navigate({
      to: "/query",
      search: (prev) => {
        const currentTabIds = prev.tabs ? prev.tabs.split(",") : [`query-${Date.now()}`];

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

  return (
    <div className="flex-1 flex h-full relative">
      {/* Connection Info Bar - Slim bar below titlebar */}
      <div className="fixed top-0 left-0 right-0 h-8 border-b border-border bg-accent/30 z-40">
        <div className="flex items-center justify-between h-full px-4">
          {/* Left: Connection Info */}
          <div className="flex items-center gap-2">
            <Database className="size-3.5 text-muted-foreground" />
            <span className="text-xs font-medium">{connectionProfile.name}</span>
            {currentDatabase && (
              <>
                <span className="text-xs text-muted-foreground">/</span>
                <span className="text-xs text-muted-foreground">{currentDatabase}</span>
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

      {/* Main Content Area (with top padding for connection bar) */}
      <div className="flex-1 flex pt-8">
        {/* Left Sidebar - Schema Explorer */}
        <div className="w-80 border-r overflow-y-auto">
          <SchemaExplorer
            connectionId={connectionId}
            connectionProfile={connectionProfile}
            onDisconnect={handleDisconnect}
            onTableSelect={handleTableSelect}
            onDatabaseChange={setCurrentDatabase}
            onOpenERDiagram={handleOpenERDiagram}
          />
        </div>

        {/* Main Content - Child Routes */}
        <div className="flex-1 overflow-hidden">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
