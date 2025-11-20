import { useState } from "react";
import { ConnectionForm } from "./components/ConnectionForm";
import { ConnectionList } from "./components/ConnectionList";
import { SchemaExplorer } from "./components/SchemaExplorer";
import { TableInspector } from "./components/TableInspector";
import { QueryPanel } from "./components/QueryPanel";
import { ModeToggle } from "./components/mode-toggle";
import { WelcomeScreen } from "./components/WelcomeScreen";
import { ConnectionProfile, QueryExecutionResult } from "./types/database";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import { Toaster } from "sonner";
import { useTheme } from "./components/theme-provider";

function App() {
  const { theme } = useTheme();
  const [selectedProfile, setSelectedProfile] = useState<
    ConnectionProfile | null | undefined
  >(undefined);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(
    null
  );
  const [activeConnectionProfile, setActiveConnectionProfile] = useState<
    ConnectionProfile | undefined
  >(undefined);
  const [currentDatabase, setCurrentDatabase] = useState<string>("");
  const [openTables, setOpenTables] = useState<
    Array<{
      schema: string;
      tableName: string;
      id: string; // Unique identifier for the tab
    }>
  >([]);
  const [activeTableId, setActiveTableId] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [pendingQuery, setPendingQuery] = useState<string | null>(null);

  // Handle successful profile save
  const handleProfileSaved = () => {
    // Trigger refresh of connection list
    setRefreshKey((prev) => prev + 1);
    // Clear selected profile
    setSelectedProfile(undefined);
  };

  // Handle edit button click (or new connection when null)
  const handleEdit = (profile: ConnectionProfile | null) => {
    // Use null for new connection, undefined for no selection (show logo)
    setSelectedProfile(profile);
  };

  // Handle successful connection
  const handleConnected = (
    connectionId: string,
    profile: ConnectionProfile
  ) => {
    setActiveConnectionId(connectionId);
    setActiveConnectionProfile(profile);
    // Don't set currentDatabase here - let SchemaExplorer tell us the actual database
  };

  // Handle database change from SchemaExplorer
  const handleDatabaseChange = (database: string) => {
    setCurrentDatabase(database);
  };

  // Handle disconnect
  const handleDisconnect = () => {
    setActiveConnectionId(null);
    setActiveConnectionProfile(undefined);
    setOpenTables([]);
    setActiveTableId(null);
  };

  // Handle table selection - open in new tab or switch to existing tab
  const handleTableSelect = (schema: string, tableName: string) => {
    const tableId = `${schema}.${tableName}`;

    // Check if table is already open
    const existingTable = openTables.find((t) => t.id === tableId);

    if (existingTable) {
      // Table already open, just switch to it
      setActiveTableId(tableId);
    } else {
      // Open new table tab
      setOpenTables((prev) => [...prev, { schema, tableName, id: tableId }]);
      setActiveTableId(tableId);
    }
  };

  // Handle closing a table tab
  const handleCloseTable = (tableId: string) => {
    setOpenTables((prev) => {
      const newTables = prev.filter((t) => t.id !== tableId);

      // If closing the active table, switch to another table or null
      if (activeTableId === tableId) {
        if (newTables.length > 0) {
          // Switch to the last table in the list
          setActiveTableId(newTables[newTables.length - 1].id);
        } else {
          setActiveTableId(null);
        }
      }

      return newTables;
    });
  };

  // Handle query execution from context menu
  const handleExecuteGeneratedQuery = (sql: string) => {
    // Close all table tabs to show QueryPanel
    setOpenTables([]);
    setActiveTableId(null);
    // Set pending query for QueryPanel to pick up
    setPendingQuery(sql);
  };

  // Execute query via Tauri command
  const executeQuery = async (sql: string): Promise<QueryExecutionResult> => {
    if (!activeConnectionId) {
      throw new Error("No active connection");
    }

    return await invoke<QueryExecutionResult>("execute_query", {
      connectionId: activeConnectionId,
      sql,
    });
  };

  return (
    <div className="flex h-screen w-full bg-background">
      {/* Theme Toggle - Top Right Corner */}
      <div className="fixed top-4 right-4 z-50">
        <ModeToggle />
      </div>

      {/* Left Sidebar - Connection List or Schema Explorer */}
      <div className="w-80 border-r overflow-y-auto">
        {activeConnectionId && activeConnectionProfile ? (
          <SchemaExplorer
            connectionId={activeConnectionId}
            connectionProfile={activeConnectionProfile}
            onDisconnect={handleDisconnect}
            onTableSelect={handleTableSelect}
            onDatabaseChange={handleDatabaseChange}
            selectedTable={activeTableId ? activeTableId.split(".")[1] : null}
            onExecuteQuery={handleExecuteGeneratedQuery}
          />
        ) : (
          <ConnectionList
            key={refreshKey}
            onEdit={handleEdit}
            onProfilesChange={() => setRefreshKey((prev) => prev + 1)}
            onConnected={handleConnected}
          />
        )}
      </div>

      {/* Main Content Area - Tabs */}
      <div className="flex-1 overflow-hidden">
        {activeConnectionId ? (
          // When connected, show table tabs and table inspector
          <div className="h-full flex flex-col">
            {openTables.length > 0 ? (
              <>
                {/* Table Tabs */}
                <div className="flex-1 flex flex-col overflow-hidden">
                  {/* Tab Bar */}
                  <div className="flex items-center gap-1 bg-muted/30 border-b px-2 py-1 overflow-x-auto shrink-0">
                    {openTables.map((table) => (
                      <div
                        key={table.id}
                        className={`
                          group flex items-center gap-2 px-3 py-1.5 rounded-t-md text-sm cursor-pointer transition-colors
                          ${
                            table.id === activeTableId
                              ? "bg-background border-t border-x text-foreground"
                              : "text-muted-foreground hover:text-foreground hover:bg-muted/50"
                          }
                        `}
                        onClick={() => setActiveTableId(table.id)}
                      >
                        <span className="select-none truncate max-w-[150px]">
                          {table.tableName}
                        </span>
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleCloseTable(table.id);
                          }}
                          className="opacity-0 group-hover:opacity-100 hover:bg-muted rounded p-0.5 transition-opacity"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ))}
                  </div>

                  {/* Table Inspector for active tab */}
                  <div className="flex-1 overflow-hidden">
                    {openTables.map((table) => (
                      <div
                        key={table.id}
                        className={
                          table.id === activeTableId ? "h-full" : "hidden"
                        }
                      >
                        <TableInspector
                          connectionId={activeConnectionId}
                          schema={table.schema}
                          tableName={table.tableName}
                          onClose={() => handleCloseTable(table.id)}
                          driverType={activeConnectionProfile?.driver}
                        />
                      </div>
                    ))}
                  </div>
                </div>
              </>
            ) : (
              <QueryPanel
                connectionId={activeConnectionId}
                connectionProfile={activeConnectionProfile}
                currentDatabase={currentDatabase}
                onExecuteQuery={executeQuery}
                pendingQuery={pendingQuery}
                onQueryLoaded={() => setPendingQuery(null)}
              />
            )}
          </div>
        ) : selectedProfile !== undefined ? (
          // When not connected but showing connection form (selectedProfile is null or ConnectionProfile)
          <div className="h-full overflow-y-auto">
            <ConnectionForm
              profile={selectedProfile ?? undefined}
              onSuccess={handleProfileSaved}
            />
          </div>
        ) : (
          // When not connected and selectedProfile is undefined - show welcome screen
          <WelcomeScreen
            onNewConnection={() => handleEdit(null)}
            onRecentConnections={() => {
              // TODO: Implement recent connections feature
              console.log("Recent connections clicked");
            }}
            onViewSample={() => {
              // TODO: Implement sample database feature
              console.log("View sample clicked");
            }}
            onOpenDocs={() => {
              // TODO: Open documentation
              window.open("https://github.com/anthropics/db-hive/wiki", "_blank");
            }}
          />
        )}
      </div>
      <Toaster
        richColors
        position="bottom-right"
        theme={theme === "system" ? undefined : theme}
      />
    </div>
  );
}

export default App;
