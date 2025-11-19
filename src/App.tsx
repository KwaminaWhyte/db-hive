import { useState } from "react";
import { ConnectionForm } from "./components/ConnectionForm";
import { ConnectionList } from "./components/ConnectionList";
import { SchemaExplorer } from "./components/SchemaExplorer";
import { TableInspector } from "./components/TableInspector";
import { QueryPanel } from "./components/QueryPanel";
import { ModeToggle } from "./components/mode-toggle";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConnectionProfile, QueryExecutionResult } from "./types/database";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [selectedProfile, setSelectedProfile] = useState<
    ConnectionProfile | undefined
  >(undefined);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(
    null
  );
  const [activeConnectionProfile, setActiveConnectionProfile] = useState<
    ConnectionProfile | undefined
  >(undefined);
  const [openTables, setOpenTables] = useState<Array<{
    schema: string;
    tableName: string;
    id: string; // Unique identifier for the tab
  }>>([]);
  const [activeTableId, setActiveTableId] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [activeTab, setActiveTab] = useState<string>("connections");

  // Handle successful profile save
  const handleProfileSaved = () => {
    // Trigger refresh of connection list
    setRefreshKey((prev) => prev + 1);
    // Clear selected profile
    setSelectedProfile(undefined);
  };

  // Handle edit button click (or new connection when null)
  const handleEdit = (profile: ConnectionProfile | null) => {
    setSelectedProfile(profile ?? undefined);
    setActiveTab("connections");
  };

  // Handle successful connection
  const handleConnected = (connectionId: string, profile: ConnectionProfile) => {
    setActiveConnectionId(connectionId);
    setActiveConnectionProfile(profile);
    // Switch to query editor tab
    setActiveTab("query");
  };

  // Handle disconnect
  const handleDisconnect = () => {
    setActiveConnectionId(null);
    setActiveConnectionProfile(undefined);
    setOpenTables([]);
    setActiveTableId(null);
    // Switch back to connections tab
    setActiveTab("connections");
  };

  // Handle table selection - open in new tab or switch to existing tab
  const handleTableSelect = (schema: string, tableName: string) => {
    const tableId = `${schema}.${tableName}`;

    // Check if table is already open
    const existingTable = openTables.find(t => t.id === tableId);

    if (existingTable) {
      // Table already open, just switch to it
      setActiveTableId(tableId);
    } else {
      // Open new table tab
      setOpenTables(prev => [...prev, { schema, tableName, id: tableId }]);
      setActiveTableId(tableId);
    }
  };

  // Handle closing a table tab
  const handleCloseTable = (tableId: string) => {
    setOpenTables(prev => {
      const newTables = prev.filter(t => t.id !== tableId);

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

  // Execute query via Tauri command
  const executeQuery = async (
    sql: string
  ): Promise<QueryExecutionResult> => {
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
            selectedTable={activeTableId ? activeTableId.split('.')[1] : null}
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
                <Tabs value={activeTableId || undefined} onValueChange={setActiveTableId} className="flex-1 flex flex-col overflow-hidden">
                  <div className="border-b px-4 shrink-0">
                    <TabsList className="h-10">
                      {openTables.map((table) => (
                        <TabsTrigger
                          key={table.id}
                          value={table.id}
                          className="relative group pr-8"
                        >
                          <span className="truncate max-w-[150px]">
                            {table.tableName}
                          </span>
                          <button
                            className="absolute right-1 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 hover:bg-destructive/20 rounded p-0.5 transition-opacity"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleCloseTable(table.id);
                            }}
                          >
                            <span className="sr-only">Close</span>
                            <svg
                              xmlns="http://www.w3.org/2000/svg"
                              width="14"
                              height="14"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="2"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <line x1="18" y1="6" x2="6" y2="18"></line>
                              <line x1="6" y1="6" x2="18" y2="18"></line>
                            </svg>
                          </button>
                        </TabsTrigger>
                      ))}
                    </TabsList>
                  </div>

                  {/* Table Inspector for each tab */}
                  {openTables.map((table) => (
                    <TabsContent
                      key={table.id}
                      value={table.id}
                      className="flex-1 m-0 overflow-hidden"
                    >
                      <TableInspector
                        connectionId={activeConnectionId}
                        schema={table.schema}
                        tableName={table.tableName}
                        onClose={() => handleCloseTable(table.id)}
                      />
                    </TabsContent>
                  ))}
                </Tabs>
              </>
            ) : (
              <QueryPanel
                connectionId={activeConnectionId}
                connectionProfile={activeConnectionProfile}
                onExecuteQuery={executeQuery}
              />
            )}
          </div>
        ) : (
          // When not connected, show tabs for Connections and Query Editor
          <Tabs value={activeTab} onValueChange={setActiveTab} className="h-full">
            <div className="border-b px-4">
              <TabsList>
                <TabsTrigger value="connections">Connections</TabsTrigger>
                <TabsTrigger value="query">Query Editor</TabsTrigger>
              </TabsList>
            </div>

            <TabsContent value="connections" className="h-[calc(100%-3rem)] m-0 overflow-y-auto">
              <ConnectionForm
                profile={selectedProfile}
                onSuccess={handleProfileSaved}
              />
            </TabsContent>

            <TabsContent value="query" className="h-[calc(100%-3rem)] m-0">
              <div className="flex items-center justify-center h-full text-muted-foreground">
                <div className="text-center space-y-2">
                  <p className="text-lg">No active connection</p>
                  <p className="text-sm">
                    Connect to a database from the Connections tab
                  </p>
                </div>
              </div>
            </TabsContent>
          </Tabs>
        )}
      </div>
    </div>
  );
}

export default App;
