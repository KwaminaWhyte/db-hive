import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Loader2, Database, Table2, Eye, LogOut, ChevronRight } from "lucide-react";
import { DatabaseInfo, TableInfo } from "@/types";

interface SchemaExplorerProps {
  connectionId: string;
  onDisconnect: () => void;
}

export function SchemaExplorer({
  connectionId,
  onDisconnect,
}: SchemaExplorerProps) {
  const [databases, setDatabases] = useState<DatabaseInfo[]>([]);
  const [selectedDatabase, setSelectedDatabase] = useState<string>("");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [loadingDatabases, setLoadingDatabases] = useState(true);
  const [loadingTables, setLoadingTables] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fetch databases on component mount
  useEffect(() => {
    fetchDatabases();
  }, [connectionId]);

  // Fetch tables when database is selected
  useEffect(() => {
    if (selectedDatabase) {
      fetchTables(selectedDatabase);
    }
  }, [selectedDatabase]);

  const fetchDatabases = async () => {
    setLoadingDatabases(true);
    setError(null);
    try {
      const dbs = await invoke<DatabaseInfo[]>("get_databases", {
        connectionId,
      });
      setDatabases(dbs);

      // Auto-select first database
      if (dbs.length > 0) {
        setSelectedDatabase(dbs[0].name);
      }
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load databases: ${errorMessage}`);
    } finally {
      setLoadingDatabases(false);
    }
  };

  const fetchTables = async (_database: string) => {
    setLoadingTables(true);
    setError(null);
    try {
      // For PostgreSQL, we need to specify a schema
      // Default to "public" schema for now
      // TODO: Add schema selection UI for databases that support schemas
      const schema = "public";
      const tablesData = await invoke<TableInfo[]>("get_tables", {
        connectionId,
        schema,
      });
      setTables(tablesData);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load tables: ${errorMessage}`);
    } finally {
      setLoadingTables(false);
    }
  };

  const handleDisconnect = async () => {
    try {
      await invoke("disconnect_from_database", { connectionId });
      onDisconnect();
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to disconnect: ${errorMessage}`);
    }
  };

  const getTableIcon = (tableType: string) => {
    const type = tableType.toUpperCase();
    if (type.includes("VIEW")) {
      return <Eye className="h-4 w-4 text-blue-500" />;
    }
    return <Table2 className="h-4 w-4 text-green-500" />;
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header with Disconnect Button */}
      <div className="border-b p-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <Database className="h-5 w-5" />
            Schema Explorer
          </h2>
          <Button variant="outline" size="sm" onClick={handleDisconnect}>
            <LogOut className="h-4 w-4 mr-2" />
            Disconnect
          </Button>
        </div>

        {/* Database Selector */}
        {loadingDatabases ? (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : databases.length > 0 ? (
          <Select value={selectedDatabase} onValueChange={setSelectedDatabase}>
            <SelectTrigger className="w-full">
              <SelectValue placeholder="Select a database" />
            </SelectTrigger>
            <SelectContent>
              {databases.map((db) => (
                <SelectItem key={db.name} value={db.name}>
                  <div className="flex items-center gap-2">
                    <Database className="h-4 w-4" />
                    <span>{db.name}</span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <p className="text-sm text-muted-foreground">No databases found</p>
        )}
      </div>

      {/* Error Display */}
      {error && (
        <div className="bg-destructive/10 text-destructive px-4 py-2 text-sm">
          {error}
        </div>
      )}

      {/* Tables List */}
      <div className="flex-1 overflow-hidden">
        {loadingTables ? (
          <div className="flex items-center justify-center h-full">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <ScrollArea className="h-full">
            <div className="p-2">
              {tables.length > 0 ? (
                <div className="space-y-1">
                  {tables.map((table) => (
                    <button
                      key={`${table.schema}.${table.name}`}
                      className="w-full flex items-center gap-2 px-3 py-2 rounded-md hover:bg-accent transition-colors text-left group"
                      onClick={() => {
                        // TODO: Handle table click - could populate SQL editor
                        console.log("Selected table:", table);
                      }}
                    >
                      {getTableIcon(table.tableType)}
                      <span className="flex-1 text-sm">{table.name}</span>
                      {table.rowCount !== null &&
                        table.rowCount !== undefined && (
                          <span className="text-xs text-muted-foreground">
                            {table.rowCount.toLocaleString()} rows
                          </span>
                        )}
                      <ChevronRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                    </button>
                  ))}
                </div>
              ) : selectedDatabase ? (
                <p className="text-sm text-muted-foreground text-center py-8">
                  No tables found in this database
                </p>
              ) : (
                <p className="text-sm text-muted-foreground text-center py-8">
                  Select a database to view tables
                </p>
              )}
            </div>
          </ScrollArea>
        )}
      </div>
    </div>
  );
}
