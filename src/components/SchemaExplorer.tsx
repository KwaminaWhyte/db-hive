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
}

export function SchemaExplorer({
  connectionId,
  connectionProfile,
  onDisconnect,
  onTableSelect,
}: SchemaExplorerProps) {
  const [schemas, setSchemas] = useState<SchemaInfo[]>([]);
  const [selectedSchema, setSelectedSchema] = useState<string>("public");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [loadingSchemas, setLoadingSchemas] = useState(true);
  const [loadingTables, setLoadingTables] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Get the connected database name from the connection profile
  const connectedDatabase = connectionProfile.database || "postgres";

  // Fetch schemas on component mount
  useEffect(() => {
    fetchSchemas();
  }, [connectionId]);

  // Fetch tables when schema is selected
  useEffect(() => {
    if (selectedSchema) {
      fetchTables(selectedSchema);
    }
  }, [selectedSchema]);

  const fetchSchemas = async () => {
    setLoadingSchemas(true);
    setError(null);
    try {
      const schemasData = await invoke<SchemaInfo[]>("get_schemas", {
        connectionId,
        database: connectedDatabase,
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

        {/* Connected Database */}
        <div className="mb-3">
          <div className="text-xs text-muted-foreground mb-1">
            Connected Database
          </div>
          <div className="flex items-center gap-2 px-3 py-2 bg-muted rounded-md">
            <Database className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">{connectedDatabase}</span>
          </div>
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
                  {tables.map((table) => (
                    <button
                      key={`${table.schema}.${table.name}`}
                      className="w-full flex items-center gap-2 px-3 py-2 rounded-md hover:bg-accent transition-colors text-left group"
                      onClick={() => onTableSelect(selectedSchema, table.name)}
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
    </div>
  );
}
