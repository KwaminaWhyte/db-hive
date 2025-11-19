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
import {
  Loader2,
  Database,
  Table2,
  Eye,
  LogOut,
  ChevronRight,
} from "lucide-react";
import { ConnectionProfile, SchemaInfo, TableInfo } from "@/types";

interface SchemaExplorerProps {
  connectionId: string;
  connectionProfile: ConnectionProfile;
  onDisconnect: () => void;
  onTableSelect: (schema: string, tableName: string) => void;
  selectedTable?: string | null;
}

export function SchemaExplorer({
  connectionId,
  connectionProfile,
  onDisconnect,
  onTableSelect,
  selectedTable,
}: SchemaExplorerProps) {
  const [schemas, setSchemas] = useState<SchemaInfo[]>([]);
  const [selectedSchema, setSelectedSchema] = useState<string>("public");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [databases, setDatabases] = useState<string[]>([]);
  const [selectedDatabase, setSelectedDatabase] = useState<string>("");
  const [loadingSchemas, setLoadingSchemas] = useState(true);
  const [loadingTables, setLoadingTables] = useState(false);
  const [loadingDatabases, setLoadingDatabases] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Get the connected database name from the connection profile
  const connectedDatabase = connectionProfile.database || "postgres";

  // Fetch schemas and databases on component mount
  useEffect(() => {
    setSelectedDatabase(connectedDatabase);
    fetchSchemas();
    fetchDatabases();
  }, [connectionId]);

  // Fetch tables when schema is selected
  useEffect(() => {
    if (selectedSchema) {
      fetchTables(selectedSchema);
    }
  }, [selectedSchema]);

  const fetchDatabases = async () => {
    setLoadingDatabases(true);
    try {
      const dbsData = await invoke<{ name: string }[]>("get_databases", {
        connectionId,
      });
      const dbNames = dbsData.map((db) => db.name);
      setDatabases(dbNames);
    } catch (err) {
      console.error("Failed to fetch databases:", err);
      // Don't show error, databases dropdown is optional
    } finally {
      setLoadingDatabases(false);
    }
  };

  const fetchSchemas = async () => {
    setLoadingSchemas(true);
    setError(null);
    try {
      const schemasData = await invoke<SchemaInfo[]>("get_schemas", {
        connectionId,
        database: selectedDatabase || connectedDatabase,
      });
      setSchemas(schemasData);

      // Auto-select "public" schema if it exists, otherwise select the first one
      const publicSchema = schemasData.find((s) => s.name === "public");
      if (publicSchema) {
        setSelectedSchema("public");
      } else if (schemasData.length > 0) {
        setSelectedSchema(schemasData[0].name);
      }
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load schemas: ${errorMessage}`);
    } finally {
      setLoadingSchemas(false);
    }
  };

  const fetchTables = async (schema: string) => {
    setLoadingTables(true);
    setError(null);
    try {
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

  const handleDatabaseChange = async (database: string) => {
    if (database === selectedDatabase) return;

    setSelectedDatabase(database);

    // If switching to a different database, attempt to switch the connection
    if (database !== connectedDatabase) {
      setError(null);
      setLoadingSchemas(true);
      setLoadingTables(true);

      try {
        // Switch to the new database using the same credentials
        await invoke("switch_database", {
          connectionId,
          newDatabase: database,
        });

        // Refresh schemas for the new database
        const schemasData = await invoke<SchemaInfo[]>("get_schemas", {
          connectionId,
          database,
        });
        setSchemas(schemasData);

        // Auto-select "public" schema if it exists, otherwise select the first one
        const publicSchema = schemasData.find((s) => s.name === "public");
        const newSelectedSchema = publicSchema
          ? "public"
          : schemasData.length > 0
          ? schemasData[0].name
          : "";

        setSelectedSchema(newSelectedSchema);

        // Fetch tables for the selected schema
        if (newSelectedSchema) {
          await fetchTables(newSelectedSchema);
        }
      } catch (err) {
        const errorMessage =
          typeof err === "string" ? err : (err as any)?.message || String(err);
        setError(`Failed to switch database: ${errorMessage}`);
        // Revert to the original database in the UI
        setSelectedDatabase(connectedDatabase);
      } finally {
        setLoadingSchemas(false);
        setLoadingTables(false);
      }
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
        {/* Database Selector */}
        <div className="mb-3">
          <div className="text-xs text-muted-foreground mb-1">Database</div>
          {loadingDatabases ? (
            <div className="flex items-center gap-2 px-3 py-2 bg-muted rounded-md">
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
              <span className="text-sm">Loading databases...</span>
            </div>
          ) : databases.length > 0 ? (
            <Select
              value={selectedDatabase}
              onValueChange={handleDatabaseChange}
            >
              <SelectTrigger className="w-full">
                <div className="flex items-center gap-2">
                  <Database className="h-4 w-4 text-muted-foreground" />
                  <SelectValue placeholder="Select a database" />
                </div>
              </SelectTrigger>
              <SelectContent>
                {databases.map((db) => (
                  <SelectItem key={db} value={db}>
                    <div className="flex items-center gap-2">
                      {db === connectedDatabase && (
                        <span className="text-xs text-green-600">●</span>
                      )}
                      <span>{db}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : (
            <div className="flex items-center gap-2 px-3 py-2 bg-muted rounded-md">
              <Database className="h-4 w-4 text-muted-foreground" />
              <span className="text-sm font-medium">{connectedDatabase}</span>
            </div>
          )}
        </div>

        {/* Schema Selector */}
        {loadingSchemas ? (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : schemas.length > 0 ? (
          <div>
            <div className="text-xs text-muted-foreground mb-1">Schema</div>
            <Select value={selectedSchema} onValueChange={setSelectedSchema}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select a schema" />
              </SelectTrigger>
              <SelectContent>
                {schemas.map((schema) => (
                  <SelectItem key={schema.name} value={schema.name}>
                    {schema.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">No schemas found</p>
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
                  {tables.map((table) => {
                    const isSelected = selectedTable === table.name;
                    return (
                      <button
                        key={`${table.schema}.${table.name}`}
                        className={`w-full flex items-center gap-2 px-3 py-2 rounded-md transition-colors text-left group ${
                          isSelected
                            ? "bg-accent text-accent-foreground"
                            : "hover:bg-accent/50"
                        }`}
                        onDoubleClick={() =>
                          onTableSelect(selectedSchema, table.name)
                        }
                      >
                        {getTableIcon(table.tableType)}
                        <span className="flex-1 text-sm font-medium">
                          {table.name}
                        </span>
                        {table.rowCount !== null &&
                          table.rowCount !== undefined && (
                            <span className="text-xs text-muted-foreground">
                              {table.rowCount.toLocaleString()} rows
                            </span>
                          )}
                        <ChevronRight
                          className={`h-4 w-4 text-muted-foreground transition-opacity ${
                            isSelected
                              ? "opacity-100"
                              : "opacity-0 group-hover:opacity-100"
                          }`}
                        />
                      </button>
                    );
                  })}
                </div>
              ) : selectedSchema ? (
                <p className="text-sm text-muted-foreground text-center py-8">
                  No tables found in this schema
                </p>
              ) : (
                <p className="text-sm text-muted-foreground text-center py-8">
                  Select a schema to view tables
                </p>
              )}
            </div>
          </ScrollArea>
        )}
      </div>
      <div className="p-4 border-t">
        <Button
          variant="outline"
          size="sm"
          onClick={handleDisconnect}
          color="danger"
        >
          <LogOut className="h-4 w-4 mr-2" />
          Disconnect
        </Button>
      </div>
    </div>
  );
}
