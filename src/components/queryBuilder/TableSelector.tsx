import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
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
  ChevronRight,
  ChevronDown,
  Search,
  X,
  RefreshCw,
  FolderClosed,
  FolderOpen,
  Table2,
  Eye,
  Plus,
  Trash2,
  AlertTriangle,
} from "lucide-react";
import type { QueryTable } from "@/types/queryBuilder";
import type { DbDriver } from "@/types/database";
import type { SchemaInfo, TableInfo, ColumnInfo } from "@/types";

interface TableSelectorProps {
  connectionId: string;
  currentDatabase: string | null;
  driver: DbDriver;
  selectedTables: QueryTable[];
  onAddTable: (table: QueryTable) => void;
  onRemoveTable: (alias: string) => void;
}

export function TableSelector({
  connectionId,
  currentDatabase,
  selectedTables,
  onAddTable,
  onRemoveTable,
}: TableSelectorProps) {
  const [schemas, setSchemas] = useState<SchemaInfo[]>([]);
  const [tablesBySchema, setTablesBySchema] = useState<Record<string, TableInfo[]>>({});
  const [loadingSchemas, setLoadingSchemas] = useState(true);
  const [loadingTablesForSchema, setLoadingTablesForSchema] = useState<Record<string, boolean>>({});
  const [loadingTableMetadata, setLoadingTableMetadata] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [expandedSchemas, setExpandedSchemas] = useState<Set<string>>(new Set(["public"]));

  // Table removal confirmation
  const [tableToRemove, setTableToRemove] = useState<{ alias: string; hasReferences: boolean } | null>(null);

  // Fetch schemas on component mount
  useEffect(() => {
    fetchSchemas();
  }, [connectionId, currentDatabase]);

  const fetchSchemas = async () => {
    setLoadingSchemas(true);
    setError(null);
    try {
      const schemasData = await invoke<SchemaInfo[]>("get_schemas", {
        connectionId,
        database: currentDatabase || "postgres",
      });
      setSchemas(schemasData);
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

  const toggleSchemaExpansion = async (schemaName: string) => {
    const newExpanded = new Set(expandedSchemas);

    if (expandedSchemas.has(schemaName)) {
      newExpanded.delete(schemaName);
    } else {
      newExpanded.add(schemaName);
      if (!tablesBySchema[schemaName]) {
        await fetchTables(schemaName);
      }
    }

    setExpandedSchemas(newExpanded);
  };

  const generateUniqueAlias = (tableName: string): string => {
    // First, try simple table name if not taken
    const simpleAlias = tableName.toLowerCase();
    const existingAliases = selectedTables.map(t => t.alias);

    if (!existingAliases.includes(simpleAlias)) {
      return simpleAlias;
    }

    // Try tableName_1, tableName_2, etc.
    let counter = 1;
    let alias = `${simpleAlias}_${counter}`;
    while (existingAliases.includes(alias)) {
      counter++;
      alias = `${simpleAlias}_${counter}`;
    }

    return alias;
  };

  const handleAddTable = async (schema: string, tableName: string) => {
    const tableKey = `${schema}.${tableName}`;
    setLoadingTableMetadata((prev) => ({ ...prev, [tableKey]: true }));
    setError(null);

    try {
      // Fetch column metadata for the table
      const tableSchemaData = await invoke<{
        table: TableInfo;
        columns: ColumnInfo[];
        indexes: any[];
      }>("get_table_schema", {
        connectionId,
        schema,
        table: tableName,
      });

      // Generate unique alias
      const alias = generateUniqueAlias(tableName);

      // Create QueryTable object
      const queryTable: QueryTable = {
        alias,
        schema,
        tableName,
        columns: tableSchemaData.columns.map((col) => ({
          name: col.name,
          dataType: col.dataType,
        })),
      };

      // Add table to query
      onAddTable(queryTable);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load table metadata: ${errorMessage}`);
    } finally {
      setLoadingTableMetadata((prev) => ({ ...prev, [tableKey]: false }));
    }
  };

  const handleRemoveTable = (alias: string) => {
    // Check if table is referenced in the query
    // For now, we'll just show a confirmation dialog
    // In a full implementation, you'd check for column/join references
    setTableToRemove({ alias, hasReferences: false });
  };

  const confirmRemoveTable = () => {
    if (tableToRemove) {
      onRemoveTable(tableToRemove.alias);
      setTableToRemove(null);
    }
  };

  const getTableIcon = (tableType: string) => {
    const type = tableType.toUpperCase();
    if (type.includes("VIEW")) {
      return <Eye className="h-4 w-4 text-blue-500" />;
    }
    return <Table2 className="h-4 w-4 text-green-500" />;
  };

  const getSelectedTableCount = (schema: string, tableName: string): number => {
    return selectedTables.filter(
      (t) => t.schema === schema && t.tableName === tableName
    ).length;
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
        table.name.toLowerCase().includes(query)
      );
    });
  }, [schemas, tablesBySchema, searchQuery]);

  const getFilteredTablesForSchema = (schemaName: string) => {
    const schemaTables = tablesBySchema[schemaName] || [];
    if (!searchQuery.trim()) return schemaTables;

    const query = searchQuery.toLowerCase().trim();
    return schemaTables.filter((table) =>
      table.name.toLowerCase().includes(query)
    );
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="border-b p-4">
        <div className="flex items-center justify-between mb-2">
          <h3 className="text-sm font-semibold">Tables</h3>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              fetchSchemas();
              expandedSchemas.forEach((schemaName) => {
                fetchTables(schemaName);
              });
            }}
            disabled={loadingSchemas}
          >
            <RefreshCw
              className={`h-4 w-4 ${loadingSchemas ? "animate-spin" : ""}`}
            />
          </Button>
        </div>

        {/* Search Input */}
        <div className="relative">
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
      </div>

      {/* Error Display */}
      {error && (
        <div className="bg-destructive/10 text-destructive px-4 py-2 text-sm">
          {error}
        </div>
      )}

      {/* Available Tables */}
      <div className="flex-1 overflow-hidden">
        {loadingSchemas ? (
          <div className="p-4 space-y-2">
            {[1, 2, 3, 4].map((i) => (
              <div key={i} className="flex items-center gap-2 px-3 py-2">
                <Skeleton className="h-4 w-4 rounded" />
                <Skeleton className="h-4 flex-1" />
              </div>
            ))}
          </div>
        ) : (
          <ScrollArea className="h-full">
            <div className="p-2">
              {filteredSchemas.length > 0 ? (
                <div className="space-y-1">
                  {filteredSchemas.map((schema) => {
                    const isExpanded = expandedSchemas.has(schema.name);
                    const schemaTables = getFilteredTablesForSchema(schema.name);
                    const isLoadingTables = loadingTablesForSchema[schema.name];

                    return (
                      <Collapsible
                        key={schema.name}
                        open={isExpanded}
                        onOpenChange={() => toggleSchemaExpansion(schema.name)}
                      >
                        {/* Schema Header */}
                        <CollapsibleTrigger asChild>
                          <button className="w-full flex items-center gap-2 px-3 py-2 rounded-md transition-colors text-left hover:bg-accent/50 group">
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

                        {/* Tables under this schema */}
                        <CollapsibleContent>
                          <div className="ml-4 mt-1 space-y-1">
                            {isLoadingTables ? (
                              <div className="space-y-1">
                                {[1, 2, 3].map((i) => (
                                  <div key={i} className="flex items-center gap-2 px-3 py-2">
                                    <Skeleton className="h-4 w-4 rounded" />
                                    <Skeleton className="h-4 flex-1" />
                                  </div>
                                ))}
                              </div>
                            ) : schemaTables.length > 0 ? (
                              schemaTables.map((table) => {
                                const tableKey = `${schema.name}.${table.name}`;
                                const isLoading = loadingTableMetadata[tableKey];
                                const selectedCount = getSelectedTableCount(
                                  schema.name,
                                  table.name
                                );

                                return (
                                  <div
                                    key={tableKey}
                                    className="flex items-center gap-2 px-3 py-2 rounded-md hover:bg-accent/50 group"
                                  >
                                    {getTableIcon(table.tableType)}
                                    <span className="flex-1 text-sm truncate">
                                      {table.name}
                                    </span>
                                    {selectedCount > 0 && (
                                      <Badge
                                        variant="secondary"
                                        className="text-xs px-1.5 py-0"
                                      >
                                        {selectedCount}
                                      </Badge>
                                    )}
                                    <Button
                                      variant="ghost"
                                      size="sm"
                                      className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                      onClick={() => handleAddTable(schema.name, table.name)}
                                      disabled={isLoading}
                                      title="Add table to query"
                                    >
                                      {isLoading ? (
                                        <RefreshCw className="h-3 w-3 animate-spin" />
                                      ) : (
                                        <Plus className="h-3 w-3" />
                                      )}
                                    </Button>
                                  </div>
                                );
                              })
                            ) : (
                              <p className="text-xs text-muted-foreground px-3 py-2">
                                No tables found
                              </p>
                            )}
                          </div>
                        </CollapsibleContent>
                      </Collapsible>
                    );
                  })}
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-8 text-center">
                  <Search className="h-8 w-8 text-muted-foreground mb-2" />
                  <p className="text-sm text-muted-foreground">
                    No tables match "{searchQuery}"
                  </p>
                  <button
                    onClick={() => setSearchQuery("")}
                    className="text-xs text-primary hover:underline mt-2"
                  >
                    Clear search
                  </button>
                </div>
              )}
            </div>
          </ScrollArea>
        )}
      </div>

      {/* Selected Tables Section */}
      {selectedTables.length > 0 && (
        <div className="border-t p-4">
          <h4 className="text-xs font-semibold text-muted-foreground mb-2">
            Selected Tables ({selectedTables.length})
          </h4>
          <ScrollArea className="max-h-48">
            <div className="space-y-1">
              {selectedTables.map((table) => (
                <div
                  key={table.alias}
                  className="flex items-center gap-2 px-3 py-2 rounded-md bg-accent/50 text-sm"
                >
                  <Table2 className="h-4 w-4 text-green-500 flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="truncate font-medium">
                      {table.schema}.{table.tableName}
                    </div>
                    <div className="text-xs text-muted-foreground truncate">
                      AS {table.alias} ({table.columns.length} columns)
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 w-6 p-0 text-destructive hover:text-destructive hover:bg-destructive/10"
                    onClick={() => handleRemoveTable(table.alias)}
                    title="Remove table from query"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              ))}
            </div>
          </ScrollArea>
        </div>
      )}

      {/* Remove Table Confirmation Dialog */}
      <AlertDialog open={tableToRemove !== null} onOpenChange={(open) => !open && setTableToRemove(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="flex items-center gap-2">
              {tableToRemove?.hasReferences && (
                <AlertTriangle className="h-5 w-5 text-destructive" />
              )}
              Remove Table
            </AlertDialogTitle>
            <AlertDialogDescription>
              {tableToRemove?.hasReferences ? (
                <>
                  This table is referenced in columns or joins. Removing it will also
                  remove those references. Are you sure you want to continue?
                </>
              ) : (
                <>
                  Are you sure you want to remove this table from the query?
                </>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmRemoveTable}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
