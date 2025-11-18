import { useState } from "react";
import { ConnectionForm } from "./components/ConnectionForm";
import { ConnectionList } from "./components/ConnectionList";
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
  const [refreshKey, setRefreshKey] = useState(0);
  const [activeTab, setActiveTab] = useState<string>("connections");

  // Handle successful profile save
  const handleProfileSaved = () => {
    // Trigger refresh of connection list
    setRefreshKey((prev) => prev + 1);
    // Clear selected profile
    setSelectedProfile(undefined);
  };

  // Handle edit button click
  const handleEdit = (profile: ConnectionProfile) => {
    setSelectedProfile(profile);
    setActiveTab("connections");
  };

  // Handle successful connection
  const handleConnected = (connectionId: string) => {
    setActiveConnectionId(connectionId);
    // Switch to query editor tab
    setActiveTab("query");
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

      {/* Left Sidebar - Connection List */}
      <div className="w-80 border-r overflow-y-auto">
        <ConnectionList
          key={refreshKey}
          onEdit={handleEdit}
          onProfilesChange={() => setRefreshKey((prev) => prev + 1)}
          onConnected={handleConnected}
        />
      </div>

      {/* Main Content Area - Tabs */}
      <div className="flex-1 overflow-hidden">
        <Tabs value={activeTab} onValueChange={setActiveTab} className="h-full">
          <div className="border-b px-4">
            <TabsList>
              <TabsTrigger value="query">Query Editor</TabsTrigger>
              <TabsTrigger value="connections">Connections</TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="query" className="h-[calc(100%-3rem)] m-0">
            {activeConnectionId ? (
              <QueryPanel
                connectionId={activeConnectionId}
                onExecuteQuery={executeQuery}
              />
            ) : (
              <div className="flex items-center justify-center h-full text-muted-foreground">
                <div className="text-center space-y-2">
                  <p className="text-lg">No active connection</p>
                  <p className="text-sm">
                    Connect to a database from the Connections tab
                  </p>
                </div>
              </div>
            )}
          </TabsContent>

          <TabsContent value="connections" className="h-[calc(100%-3rem)] m-0 overflow-y-auto">
            <ConnectionForm
              profile={selectedProfile}
              onSuccess={handleProfileSaved}
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}

export default App;
