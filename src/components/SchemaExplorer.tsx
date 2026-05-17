import { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "@tanstack/react-router";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Database,
  Table2,
  Eye,
  ChevronRight,
  ChevronDown,
  Search,
  X,
  RefreshCw,
  Copy,
  FileCode,
  FileInput,
  FileSpreadsheet,
  FolderClosed,
  FolderOpen,
  Upload,
  Download,
  Network,
  MoreVertical,
  Workflow,
  Activity,
  Plus,
  FunctionSquare,
} from "lucide-react";
import { ConnectionProfile, SchemaInfo, TableInfo } from "@/types";
import { RedisSchemaTree } from "./RedisSchemaTree";
import { SqlExportDialog } from "./SqlExportDialog";
import { SqlImportDialog } from "./SqlImportDialog";
import { TableCreationDialog } from "./TableCreationDialog";
import { TableEditDialog } from "./TableEditDialog";
import { ConfirmDestructiveDialog } from "./ConfirmDestructiveDialog";
import { DataImportWizard } from "./DataImportWizard";
import { StoredProceduresPanel } from "./StoredProceduresPanel";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { NoTablesEmpty, NoSearchResultsEmpty } from "./empty-states";
import { ForeignKeyInfo } from "@/types/database";
import { dropTable } from "@/api/ddl";
import { useMetadataChangeListener, notifyMetadataChanged } from "@/hooks/useMetadataCache";
import { Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";

interface SchemaExplorerProps {
  connectionId: string;
  connectionProfile: ConnectionProfile;
  onDisconnect: () => void;
  onTableSelect: (schema: string, tableName: string) => void;
  onDatabaseChange?: (database: string) => void;
  onExecuteQuery?: (sql: string) => void;
  onOpenERDiagram?: (schema: string) => void;
  /** Open a Redis key's value in a main-area tab (Redis connections only) */
  onRedisKeySelect?: (key: string) => void;
}

export function SchemaExplorer({
  connectionId,
  connectionProfile,
  onDisconnect: _onDisconnect,
  onTableSelect,
  onDatabaseChange,
  onExecuteQuery,
  onOpenERDiagram,
  onRedisKeySelect,
}: SchemaExplorerProps) {
  const navigate = useNavigate();
  const isRedis = connectionProfile.driver === "Redis";
  const [activeRedisKey, setActiveRedisKey] = useState<string | null>(null);
  const [schemas, setSchemas] = useState<SchemaInfo[]>([]);
  const [selectedSchema, setSelectedSchema] = useState<string>("");
  // Store tables per schema to support lazy loading
  const [tablesBySchema, setTablesBySchema] = useState<
    Record<string, TableInfo[]>
  >({});
  const [databases, setDatabases] = useState<string[]>([]);
  const [selectedDatabase, setSelectedDatabase] = useState<string>("");
  const [loadingSchemas, setLoadingSchemas] = useState(true);
  const [loadingTablesForSchema, setLoadingTablesForSchema] = useState<
    Record<string, boolean>
  >({});
  const [loadingDatabases, setLoadingDatabases] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState<string>("");
  // Track which schemas are expanded
  const [expandedSchemas, setExpandedSchemas] = useState<Set<string>>(
    new Set(["public"]),
  );
  // Track the currently active/selected table for highlighting
  const [activeTable, setActiveTable] = useState<{
    schema: string;
    table: string;
  } | null>(null);
  // Import/Export dialog state
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [showImportDialog, setShowImportDialog] = useState(false);
  // Table creation dialog state
  const [showCreateTableDialog, setShowCreateTableDialog] = useState(false);
  const [createTableSchema, setCreateTableSchema] = useState<
    string | undefined
  >(undefined);
  // Table edit dialog state
  const [editTable, setEditTable] = useState<{ schema: string; name: string } | null>(null);
  // Drop table confirmation state
  const [dropTarget, setDropTarget] = useState<{
    schema: string;
    name: string;
    dependents: string[];
  } | null>(null);
  const [dropLoading, setDropLoading] = useState(false);
  // Data import wizard state
  const [showDataImportWizard, setShowDataImportWizard] = useState(false);
  // Stored procedures panel state
  const [showProceduresPanel, setShowProceduresPanel] = useState(false);
  const [importTargetSchema, setImportTargetSchema] = useState<string>("");
  const [importTargetTable, setImportTargetTable] = useState<string>("");

  // Get the connected database name from the connection profile
  const connectedDatabase = connectionProfile.database || "postgres";

  // Fetch schemas and databases on component mount
  useEffect(() => {
    setSelectedDatabase(connectedDatabase);
    // Notify parent of initial database
    onDatabaseChange?.(connectedDatabase);
    fetchSchemas();
    fetchDatabases();
  }, [connectionId]);

  // Lazy load tables when a schema is expanded
  const toggleSchemaExpansion = async (schemaName: string) => {
    const newExpanded = new Set(expandedSchemas);

    if (expandedSchemas.has(schemaName)) {
      // Collapse schema
      newExpanded.delete(schemaName);
    } else {
      // Expand schema - load tables if not already loaded
      newExpanded.add(schemaName);
      if (!tablesBySchema[schemaName]) {
        await fetchTables(schemaName);
      }
    }

    setExpandedSchemas(newExpanded);
  };

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

      // Load tables for initially expanded schemas (e.g., "public")
      expandedSchemas.forEach((schemaName) => {
        if (schemasData.find((s) => s.name === schemaName)) {
          fetchTables(schemaName);
        }
      });
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load schemas: ${errorMessage}`);
    } finally {
      setLoadingSchemas(false);
    }
  };

  const fetchTables = async (schema: string) => {
    setLoadingTablesForSchema((prev) => ({ ...prev, [schema]: true }));
    setError(null);
    try {
      const tablesData = await invoke<TableInfo[]>("get_tables", {
        connectionId,
        schema,
      });
      setTablesBySchema((prev) => ({ ...prev, [schema]: tablesData }));
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load tables: ${errorMessage}`);
    } finally {
      setLoadingTablesForSchema((prev) => ({ ...prev, [schema]: false }));
    }
  };

  // Refresh schemas/tables whenever a DDL operation dispatches a metadata-changed event
  useMetadataChangeListener(
    useCallback(
      (payload) => {
        fetchSchemas();
        if (payload.schema) {
          fetchTables(payload.schema);
        } else {
          expandedSchemas.forEach((s) => fetchTables(s));
        }
      },
      [expandedSchemas, connectionId, selectedDatabase],
    ),
  );

  // Also listen for a window-level "metadata:refresh" trigger from the titlebar.
  useEffect(() => {
    const handler = () => {
      fetchSchemas();
      expandedSchemas.forEach((s) => fetchTables(s));
    };
    window.addEventListener("metadata:refresh", handler);
    return () => window.removeEventListener("metadata:refresh", handler);
  }, [expandedSchemas]);

  // Prepare a drop confirmation by probing for FK dependents
  const requestDropTable = async (schemaName: string, tableNm: string) => {
    let dependents: string[] = [];
    try {
      const fks = await invoke<ForeignKeyInfo[]>("get_foreign_keys", {
        connectionId,
        schema: schemaName,
      });
      dependents = Array.from(
        new Set(
          fks
            .filter((fk) => fk.referencedTable === tableNm)
            .map((fk) => `${fk.schema}.${fk.table}`),
        ),
      );
    } catch (err) {
      // If the FK probe fails, proceed without dependents; the dialog still confirms.
      console.warn("Failed to probe foreign keys for drop:", err);
    }
    setDropTarget({ schema: schemaName, name: tableNm, dependents });
  };

  const confirmDropTable = async () => {
    if (!dropTarget) return;
    try {
      setDropLoading(true);
      await dropTable(connectionId, {
        schema: dropTarget.schema,
        name: dropTarget.name,
        cascade: dropTarget.dependents.length > 0,
        ifExists: true,
      });
      toast.success(`Dropped ${dropTarget.schema}.${dropTarget.name}`);
      notifyMetadataChanged({ schema: dropTarget.schema, reason: "drop-table" });
      setDropTarget(null);
    } catch (err: any) {
      toast.error("Drop Failed", {
        description: err?.message || err?.toString() || "Failed to drop table",
      });
    } finally {
      setDropLoading(false);
    }
  };

  const handleDatabaseChange = async (database: string) => {
    if (database === selectedDatabase) return;

    setSelectedDatabase(database);

    // Notify parent component about database change
    onDatabaseChange?.(database);

    // If switching to a different database, attempt to switch the connection
    if (database !== connectedDatabase) {
      setError(null);
      setLoadingSchemas(true);
      // Clear tables cache and expanded state when switching databases
      setTablesBySchema({});
      setExpandedSchemas(new Set(["public"]));

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

        // Auto-select "public" schema if it exists, otherwise clear selection
        const publicSchema = schemasData.find((s) => s.name === "public");
        const newSelectedSchema = publicSchema
          ? "public"
          : schemasData.length > 0
            ? schemasData[0].name
            : "";

        setSelectedSchema(newSelectedSchema);

        // Load tables for initially expanded schemas (e.g., "public")
        const newExpanded = new Set(["public"]);
        newExpanded.forEach((schemaName) => {
          if (schemasData.find((s) => s.name === schemaName)) {
            fetchTables(schemaName);
          }
        });
      } catch (err) {
        const errorMessage =
          typeof err === "string" ? err : (err as any)?.message || String(err);
        setError(`Failed to switch database: ${errorMessage}`);
        // Revert to the original database in the UI
        setSelectedDatabase(connectedDatabase);
      } finally {
        setLoadingSchemas(false);
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

  // Generate SELECT query for a table
  const generateSelectQuery = (schemaName: string, tableName: string) => {
    return `SELECT * FROM "${schemaName}"."${tableName}" LIMIT 100;`;
  };

  // Generate INSERT template for a table with actual column names
  const generateInsertTemplate = async (
    schemaName: string,
    tableName: string,
  ) => {
    try {
      // Fetch actual column names from the table
      const columns = await invoke<{ name: string; data_type: string }[]>(
        "get_table_columns_for_import",
        {
          connectionId,
          tableName,
          schema: schemaName,
        },
      );

      if (columns.length === 0) {
        return `INSERT INTO "${schemaName}"."${tableName}" (column1, column2, ...) \nVALUES (value1, value2, ...);`;
      }

      const columnNames = columns.map((c) => `"${c.name}"`).join(", ");
      const valuePlaceholders = columns
        .map((c) => {
          // Provide type hints as placeholders
          const type = c.data_type.toLowerCase();
          if (type.includes("int") || type.includes("serial")) return "0";
          if (type.includes("bool")) return "false";
          if (
            type.includes("float") ||
            type.includes("double") ||
            type.includes("numeric") ||
            type.includes("decimal")
          )
            return "0.0";
          if (type.includes("date") || type.includes("time"))
            return `'${new Date().toISOString().split("T")[0]}'`;
          if (type.includes("json")) return "'{}'";
          return "''";
        })
        .join(", ");

      return `INSERT INTO "${schemaName}"."${tableName}" (${columnNames})\nVALUES (${valuePlaceholders});`;
    } catch (err) {
      console.error("Failed to fetch columns for INSERT template:", err);
      return `INSERT INTO "${schemaName}"."${tableName}" (column1, column2, ...) \nVALUES (value1, value2, ...);`;
    }
  };

  // Copy table name to clipboard
  const copyTableName = async (tableName: string) => {
    try {
      await navigator.clipboard.writeText(tableName);
    } catch (err) {
      console.error("Failed to copy table name:", err);
    }
  };

  // Filter schemas and tables based on search query
  const filteredSchemas = useMemo(() => {
    if (!searchQuery.trim()) return schemas;

    const query = searchQuery.toLowerCase().trim();
    return schemas.filter((schema) => {
      // Include schema if its name matches
      if (schema.name.toLowerCase().includes(query)) return true;

      // Include schema if any of its tables match
      const schemaTables = tablesBySchema[schema.name] || [];
      return schemaTables.some((table) =>
        table.name.toLowerCase().includes(query),
      );
    });
  }, [schemas, tablesBySchema, searchQuery]);

  // Memoize filtered tables per schema to prevent recalculation on every render
  const filteredTablesBySchema = useMemo(() => {
    const result: Record<string, TableInfo[]> = {};
    const query = searchQuery.toLowerCase().trim();

    for (const schemaName of Object.keys(tablesBySchema)) {
      const schemaTables = tablesBySchema[schemaName] || [];
      if (!query) {
        result[schemaName] = schemaTables;
      } else {
        result[schemaName] = schemaTables.filter((table) =>
          table.name.toLowerCase().includes(query),
        );
      }
    }
    return result;
  }, [tablesBySchema, searchQuery]);

  // Get filtered tables for a specific schema (now just a lookup)
  const getFilteredTablesForSchema = useCallback(
    (schemaName: string) => {
      return filteredTablesBySchema[schemaName] || [];
    },
    [filteredTablesBySchema],
  );

  return (
    <div className="h-full flex flex-col">
      {/* Header with Database Selector and Actions */}
      <div className="border-b px-3 py-2">
        {/* Database Selector with Actions Menu */}
        <div className="mb-1.5">
          <div className="text-xs text-muted-foreground mb-1">Database</div>
          <div className="flex items-center gap-2">
            {loadingDatabases ? (
              <Skeleton className="h-10 w-full rounded-md" />
            ) : databases.length > 0 ? (
              <Select
                value={selectedDatabase}
                onValueChange={handleDatabaseChange}
              >
                <SelectTrigger className="flex-1">
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
              <div className="flex items-center gap-2 px-3 py-2 bg-muted rounded-md flex-1">
                <Database className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm font-medium">{connectedDatabase}</span>
              </div>
            )}

            {/* Actions Menu Popover */}
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  className="shrink-0"
                  title="Database actions"
                >
                  <MoreVertical className="h-4 w-4" />
                </Button>
              </PopoverTrigger>
              <PopoverContent className="w-56 p-2" align="end">
                <div className="space-y-1">
                  {/* View ER Diagram */}
                  {onOpenERDiagram && selectedSchema && (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-full justify-start"
                      onClick={() => onOpenERDiagram(selectedSchema)}
                    >
                      <Network className="h-4 w-4 mr-2" />
                      View ER Diagram
                    </Button>
                  )}

                  {/* Visual Query Builder */}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={() => navigate({ to: "/visual-query" })}
                  >
                    <Workflow className="h-4 w-4 mr-2" />
                    Visual Query Builder
                  </Button>

                  {/* Activity Monitor */}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={() => navigate({ to: "/activity" })}
                  >
                    <Activity className="h-4 w-4 mr-2" />
                    Activity Monitor
                  </Button>

                  {/* Export */}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={() => setShowExportDialog(true)}
                  >
                    <Download className="h-4 w-4 mr-2" />
                    Export SQL
                  </Button>

                  {/* Import SQL */}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={() => setShowImportDialog(true)}
                  >
                    <Upload className="h-4 w-4 mr-2" />
                    Import SQL
                  </Button>

                  {/* Import Data (CSV/Excel) */}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={() => {
                      setImportTargetSchema(selectedSchema);
                      setImportTargetTable("");
                      setShowDataImportWizard(true);
                    }}
                  >
                    <FileSpreadsheet className="h-4 w-4 mr-2" />
                    Import CSV/Excel
                  </Button>
                </div>
              </PopoverContent>
            </Popover>
          </div>
        </div>
      </div>

      {/* Error Display */}
      {error && (
        <div className="bg-destructive/10 text-destructive px-4 py-2 text-sm">
          {error}
        </div>
      )}

      {/* Search Input */}
      {!loadingSchemas && schemas.length > 0 && (
        <div className="px-3 py-2 border-b">
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search tables..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9 pr-9"
              />
              {searchQuery && (
                <button
                  onClick={() => setSearchQuery("")}
                  className="absolute right-3 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
                >
                  <X className="h-4 w-4" />
                </button>
              )}
            </div>
            <Button
              variant="ghost"
              size="icon"
              className="h-9 w-9 shrink-0"
              onClick={() => {
                fetchSchemas();
                expandedSchemas.forEach((schemaName) => {
                  fetchTables(schemaName);
                });
              }}
              disabled={
                loadingSchemas ||
                Object.values(loadingTablesForSchema).some(Boolean)
              }
              title="Refresh schemas & tables"
            >
              <RefreshCw
                className={`h-4 w-4 ${loadingSchemas || Object.values(loadingTablesForSchema).some(Boolean) ? "animate-spin" : ""}`}
              />
            </Button>
          </div>
        </div>
      )}

      {/* Schema & Tables Tree View */}
      <div className="flex-1 overflow-hidden min-h-0">
        {isRedis ? (
          <RedisSchemaTree
            connectionId={connectionId}
            searchQuery={searchQuery}
            activeKey={activeRedisKey}
            onSelectKey={(key) => {
              setActiveRedisKey(key);
              onRedisKeySelect?.(key);
            }}
          />
        ) : loadingSchemas ? (
          <div className="p-4 space-y-2">
            {[1, 2, 3, 4, 5, 6].map((i) => (
              <div key={i} className="flex items-center gap-2 px-3 py-2">
                <Skeleton className="h-4 w-4 rounded" />
                <Skeleton className="h-4 flex-1" />
              </div>
            ))}
          </div>
        ) : (
          <ScrollArea className="h-full">
            <div className="p-2">
              {schemas.length > 0 ? (
                filteredSchemas.length > 0 ? (
                  <div className="space-y-1">
                    {filteredSchemas.map((schema) => {
                      const isExpanded = expandedSchemas.has(schema.name);
                      const schemaTables = getFilteredTablesForSchema(
                        schema.name,
                      );
                      const isLoadingTables =
                        loadingTablesForSchema[schema.name];

                      return (
                        <Collapsible
                          key={schema.name}
                          open={isExpanded}
                          onOpenChange={() =>
                            toggleSchemaExpansion(schema.name)
                          }
                        >
                          {/* Schema Header */}
                          <ContextMenu>
                            <ContextMenuTrigger asChild>
                              <CollapsibleTrigger asChild>
                                <button
                                  className="w-full flex items-center gap-2 px-3 py-2 rounded-md transition-colors text-left hover:bg-accent/50 group"
                                  draggable={false}
                                >
                                  {isExpanded ? (
                                    <ChevronDown className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                                  ) : (
                                    <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                                  )}
                                  {isExpanded ? (
                                    <FolderOpen className="h-4 w-4 text-blue-500 flex-shrink-0" />
                                  ) : (
                                    <FolderClosed className="h-4 w-4 text-blue-500 flex-shrink-0" />
                                  )}
                                  <span className="flex-1 text-sm font-medium truncate">
                                    {schema.name}
                                  </span>
                                  {isLoadingTables && (
                                    <RefreshCw className="h-3 w-3 animate-spin text-muted-foreground" />
                                  )}
                                </button>
                              </CollapsibleTrigger>
                            </ContextMenuTrigger>
                            <ContextMenuContent>
                              <ContextMenuItem
                                onClick={() => {
                                  setCreateTableSchema(schema.name);
                                  setShowCreateTableDialog(true);
                                }}
                              >
                                <Plus className="h-4 w-4 mr-2" />
                                Create Table
                              </ContextMenuItem>
                              <ContextMenuItem
                                onClick={() => {
                                  setImportTargetSchema(schema.name);
                                  setImportTargetTable("");
                                  setShowDataImportWizard(true);
                                }}
                              >
                                <FileSpreadsheet className="h-4 w-4 mr-2" />
                                Import CSV/Excel
                              </ContextMenuItem>
                              <ContextMenuItem
                                onClick={() => fetchTables(schema.name)}
                              >
                                <RefreshCw className="h-4 w-4 mr-2" />
                                Refresh Tables
                              </ContextMenuItem>
                            </ContextMenuContent>
                          </ContextMenu>

                          {/* Tables under this schema */}
                          <CollapsibleContent>
                            <div className="ml-4 mt-1 space-y-1">
                              {isLoadingTables ? (
                                <div className="space-y-1">
                                  {[1, 2, 3].map((i) => (
                                    <div
                                      key={i}
                                      className="flex items-center gap-2 px-3 py-2"
                                    >
                                      <Skeleton className="h-4 w-4 rounded" />
                                      <Skeleton className="h-4 flex-1" />
                                    </div>
                                  ))}
                                </div>
                              ) : schemaTables.length > 0 ? (
                                schemaTables.map((table) => {
                                  const isSelected =
                                    activeTable?.schema === schema.name &&
                                    activeTable?.table === table.name;
                                  return (
                                    <ContextMenu
                                      key={`${schema.name}.${table.name}`}
                                    >
                                      <ContextMenuTrigger asChild>
                                        <button
                                          className={`w-full flex items-center gap-2 px-3 py-2 rounded-md transition-colors text-left group ${
                                            isSelected
                                              ? "bg-accent text-accent-foreground"
                                              : "hover:bg-accent/50"
                                          }`}
                                          onClick={() => {
                                            // Set active table for highlighting on single click
                                            setActiveTable({
                                              schema: schema.name,
                                              table: table.name,
                                            });
                                          }}
                                          onDoubleClick={() => {
                                            setSelectedSchema(schema.name);
                                            setActiveTable({
                                              schema: schema.name,
                                              table: table.name,
                                            });
                                            onTableSelect(
                                              schema.name,
                                              table.name,
                                            );
                                          }}
                                          draggable
                                          onDragStart={(e) => {
                                            e.dataTransfer.setData(
                                              "text/plain",
                                              `"${schema.name}"."${table.name}"`,
                                            );
                                            e.dataTransfer.effectAllowed =
                                              "copy";
                                          }}
                                        >
                                          {getTableIcon(table.tableType)}
                                          <span className="flex-1 text-sm truncate">
                                            {table.name}
                                          </span>
                                          {table.rowCount !== null &&
                                            table.rowCount !== undefined && (
                                              <span className="shrink-0 whitespace-nowrap text-xs text-muted-foreground">
                                                {table.rowCount.toLocaleString()}{" "}
                                                rows
                                              </span>
                                            )}
                                          <ChevronRight
                                            className={`h-4 w-4 text-muted-foreground transition-opacity flex-shrink-0 ${
                                              isSelected
                                                ? "opacity-100"
                                                : "opacity-0 group-hover:opacity-100"
                                            }`}
                                          />
                                        </button>
                                      </ContextMenuTrigger>
                                      <ContextMenuContent>
                                        <ContextMenuItem
                                          onClick={() => {
                                            setSelectedSchema(schema.name);
                                            setActiveTable({
                                              schema: schema.name,
                                              table: table.name,
                                            });
                                            onTableSelect(
                                              schema.name,
                                              table.name,
                                            );
                                          }}
                                        >
                                          <Eye className="h-4 w-4 mr-2" />
                                          View Data
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={() => {
                                            const sql = generateSelectQuery(
                                              schema.name,
                                              table.name,
                                            );
                                            onExecuteQuery?.(sql);
                                          }}
                                        >
                                          <FileCode className="h-4 w-4 mr-2" />
                                          Generate SELECT
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={async () => {
                                            const sql =
                                              await generateInsertTemplate(
                                                schema.name,
                                                table.name,
                                              );
                                            onExecuteQuery?.(sql);
                                          }}
                                        >
                                          <FileInput className="h-4 w-4 mr-2" />
                                          Generate INSERT
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={() =>
                                            copyTableName(table.name)
                                          }
                                        >
                                          <Copy className="h-4 w-4 mr-2" />
                                          Copy Name
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={() => {
                                            setImportTargetSchema(schema.name);
                                            setImportTargetTable(table.name);
                                            setShowDataImportWizard(true);
                                          }}
                                        >
                                          <Upload className="h-4 w-4 mr-2" />
                                          Import Data
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={() =>
                                            fetchTables(schema.name)
                                          }
                                        >
                                          <RefreshCw className="h-4 w-4 mr-2" />
                                          Refresh
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          onClick={() =>
                                            setEditTable({
                                              schema: schema.name,
                                              name: table.name,
                                            })
                                          }
                                        >
                                          <Pencil className="h-4 w-4 mr-2" />
                                          Edit Table
                                        </ContextMenuItem>
                                        <ContextMenuItem
                                          className="text-destructive focus:text-destructive"
                                          onClick={() =>
                                            requestDropTable(schema.name, table.name)
                                          }
                                        >
                                          <Trash2 className="h-4 w-4 mr-2" />
                                          Drop Table
                                        </ContextMenuItem>
                                      </ContextMenuContent>
                                    </ContextMenu>
                                  );
                                })
                              ) : (
                                <div className="px-3 py-6">
                                  <NoTablesEmpty
                                    databaseName={`${selectedDatabase}.${schema.name}`}
                                    onCreateTable={() => {
                                      setCreateTableSchema(schema.name);
                                      setShowCreateTableDialog(true);
                                    }}
                                  />
                                </div>
                              )}
                            </div>
                          </CollapsibleContent>
                        </Collapsible>
                      );
                    })}
                  </div>
                ) : (
                  <NoSearchResultsEmpty
                    searchQuery={searchQuery}
                    onClearSearch={() => setSearchQuery("")}
                  />
                )
              ) : (
                <p className="text-sm text-muted-foreground text-center py-8">
                  No schemas found
                </p>
              )}

              {/* Procedures & Functions */}
              <div className="mt-2 pt-2 border-t">
                <button
                  className="w-full flex items-center gap-2 px-3 py-2 rounded-md transition-colors text-left hover:bg-accent/50"
                  onClick={() => setShowProceduresPanel(true)}
                >
                  <FunctionSquare className="h-4 w-4 text-purple-500 flex-shrink-0" />
                  <span className="flex-1 text-sm font-medium truncate">
                    Procedures
                  </span>
                  <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                </button>
              </div>
            </div>
          </ScrollArea>
        )}
      </div>
      {/* SQL Export Dialog */}
      <SqlExportDialog
        open={showExportDialog}
        onClose={() => setShowExportDialog(false)}
        connectionId={connectionId}
        currentSchema={selectedDatabase}
      />

      {/* SQL Import Dialog */}
      <SqlImportDialog
        open={showImportDialog}
        onClose={() => setShowImportDialog(false)}
        connectionId={connectionId}
      />

      {/* Table Creation Dialog */}
      <TableCreationDialog
        open={showCreateTableDialog}
        onOpenChange={setShowCreateTableDialog}
        connectionId={connectionId}
        schema={createTableSchema}
        onSuccess={() => {
          // Refresh tables for the schema where the table was created
          if (createTableSchema) {
            fetchTables(createTableSchema);
          }
          notifyMetadataChanged({ schema: createTableSchema, reason: "create-table" });
        }}
      />

      {/* Table Edit Dialog */}
      {editTable && (
        <TableEditDialog
          open={!!editTable}
          onOpenChange={(open) => !open && setEditTable(null)}
          connectionId={connectionId}
          schema={editTable.schema}
          tableName={editTable.name}
          onSuccess={() => {
            fetchTables(editTable.schema);
          }}
        />
      )}

      {/* Drop Table Confirmation */}
      <ConfirmDestructiveDialog
        open={!!dropTarget}
        onOpenChange={(open) => !open && !dropLoading && setDropTarget(null)}
        title={
          dropTarget
            ? `Drop table "${dropTarget.schema}.${dropTarget.name}"?`
            : "Drop table?"
        }
        description={
          dropTarget && (
            <>
              <p>
                This action cannot be undone. All data in{" "}
                <span className="font-mono font-semibold text-foreground">
                  {dropTarget.schema}.{dropTarget.name}
                </span>{" "}
                will be permanently removed.
              </p>
              {dropTarget.dependents.length > 0 && (
                <div className="rounded-md border border-destructive/50 bg-destructive/10 p-2">
                  <p className="text-destructive font-medium text-xs mb-1">
                    {dropTarget.dependents.length} table
                    {dropTarget.dependents.length === 1 ? "" : "s"} reference this
                    table via foreign keys:
                  </p>
                  <ul className="list-disc list-inside text-xs font-mono">
                    {dropTarget.dependents.map((d) => (
                      <li key={d}>{d}</li>
                    ))}
                  </ul>
                  <p className="text-xs mt-1">
                    CASCADE will be used to also drop the dependent constraints.
                  </p>
                </div>
              )}
            </>
          )
        }
        confirmLabel={
          dropTarget && dropTarget.dependents.length > 0 ? "Drop with CASCADE" : "Drop Table"
        }
        loading={dropLoading}
        onConfirm={confirmDropTable}
      />

      {/* Stored Procedures / Functions */}
      <Dialog open={showProceduresPanel} onOpenChange={setShowProceduresPanel}>
        <DialogContent className="max-w-4xl h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>Procedures & Functions</DialogTitle>
          </DialogHeader>
          <div className="flex-1 overflow-hidden">
            <StoredProceduresPanel
              connectionId={connectionId}
              onExecute={(sql) => {
                onExecuteQuery?.(sql);
                setShowProceduresPanel(false);
              }}
            />
          </div>
        </DialogContent>
      </Dialog>

      {/* Data Import Wizard */}
      {showDataImportWizard && (
        <DataImportWizard
          connectionId={connectionId}
          defaultSchema={importTargetSchema}
          defaultTable={importTargetTable}
          onClose={() => {
            setShowDataImportWizard(false);
            setImportTargetSchema("");
            setImportTargetTable("");
          }}
          onSuccess={() => {
            // Refresh the table data after successful import
            if (importTargetSchema) {
              fetchTables(importTargetSchema);
            }
          }}
        />
      )}
    </div>
  );
}
