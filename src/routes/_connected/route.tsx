import { createFileRoute, Outlet, useNavigate } from "@tanstack/react-router";
import { SchemaExplorer } from "@/components/SchemaExplorer";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { Settings, LogOut, Database } from "lucide-react";

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
    // Get current tabs from query route search params
    const tableId = `${schema}.${tableName}`;

    // Navigate to query route with the table added to tabs
    // We'll check if it's already open in the query route component
    navigate({
      to: "/query",
      search: (prev) => {
        const currentTabs = prev.tabs ? prev.tabs.split(",") : ["query"];

        // Check if table is already open
        const existingIndex = currentTabs.indexOf(tableId);

        if (existingIndex >= 0) {
          // Table already open, just switch to it
          return {
            tabs: prev.tabs || "query",
            active: existingIndex,
          };
        }

        // Add new table tab
        const newTabs = [...currentTabs, tableId];
        return {
          tabs: newTabs.join(","),
          active: newTabs.length - 1, // Switch to new tab
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
      {/* Top Bar */}
      <div className="fixed top-0 left-0 right-0 h-14 border-b border-border bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 z-50">
        <div className="flex items-center justify-between h-full px-4">
          {/* Left: Connection Info */}
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent/50 border border-border">
              <Database className="size-4 text-muted-foreground" />
              <span className="text-sm font-medium">{connectionProfile.name}</span>
              {currentDatabase && (
                <>
                  <span className="text-muted-foreground">/</span>
                  <span className="text-sm text-muted-foreground">{currentDatabase}</span>
                </>
              )}
            </div>
          </div>

          {/* Right: Controls */}
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={handleDisconnect}
              title="Disconnect"
            >
              <LogOut className="size-4 mr-2" />
              Disconnect
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate({ to: "/settings" })}
            >
              <Settings className="size-4" />
            </Button>
            <ModeToggle />
          </div>
        </div>
      </div>

      {/* Main Content Area (with top padding for fixed header) */}
      <div className="flex-1 flex pt-14">
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
