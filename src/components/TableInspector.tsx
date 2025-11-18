import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Loader2,
  Table2,
  Key,
  Database,
  X,
  RefreshCw,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { TableSchema, QueryExecutionResult } from "@/types";

interface TableInspectorProps {
  connectionId: string;
  schema: string;
  tableName: string;
  onClose: () => void;
}

export function TableInspector({
  connectionId,
  schema,
  tableName,
  onClose,
}: TableInspectorProps) {
  const [tableSchema, setTableSchema] = useState<TableSchema | null>(null);
  const [sampleData, setSampleData] = useState<QueryExecutionResult | null>(
    null
  );
  const [loading, setLoading] = useState(true);
  const [loadingSampleData, setLoadingSampleData] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState("data");
  const [currentPage, setCurrentPage] = useState(1);
  const [pageSize] = useState(50);
  const [totalRows, setTotalRows] = useState<number | null>(null);

  useEffect(() => {
    // Reset state when table changes
    setTableSchema(null);
    setSampleData(null);
    setError(null);
    setActiveTab("data");
    setCurrentPage(1);
    setTotalRows(null);

    // Fetch new table schema
    fetchTableSchema();
  }, [connectionId, schema, tableName]);

  // Fetch sample data when Data tab is opened, schema is loaded, or page changes
  useEffect(() => {
    if (activeTab === "data" && tableSchema) {
      fetchSampleData();
    }
  }, [activeTab, tableName, schema, tableSchema, currentPage]);

  const fetchTableSchema = async () => {
    setLoading(true);
    setError(null);
    try {
      const schemaData = await invoke<TableSchema>("get_table_schema", {
        connectionId,
        schema,
        table: tableName,
      });
      setTableSchema(schemaData);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load table schema: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  const fetchSampleData = async () => {
    setLoadingSampleData(true);
    setError(null);
    try {
      // Calculate offset based on current page
      const offset = (currentPage - 1) * pageSize;

      // Fetch total row count if not already fetched
      if (totalRows === null) {
        try {
          const countResult = await invoke<QueryExecutionResult>("execute_query", {
            connectionId,
            sql: `SELECT COUNT(*) FROM "${schema}"."${tableName}"`,
          });
          const count = countResult.rows[0]?.[0];
          setTotalRows(typeof count === 'number' ? count : parseInt(String(count)) || 0);
        } catch (err) {
          console.error("Failed to fetch row count:", err);
          // Continue without total count
        }
      }

      // Fetch paginated data
      const result = await invoke<QueryExecutionResult>("execute_query", {
        connectionId,
        sql: `SELECT * FROM "${schema}"."${tableName}" LIMIT ${pageSize} OFFSET ${offset}`,
      });
      setSampleData(result);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load sample data: ${errorMessage}`);
    } finally {
      setLoadingSampleData(false);
    }
  };

  const handleRefresh = () => {
    fetchTableSchema();
    if (activeTab === "data") {
      setSampleData(null);
      setTotalRows(null);
      setCurrentPage(1);
      fetchSampleData();
    }
  };

  const handleNextPage = () => {
    if (totalRows === null) return;
    const totalPages = Math.ceil(totalRows / pageSize);
    if (currentPage < totalPages) {
      setCurrentPage(currentPage + 1);
    }
  };

  const handlePreviousPage = () => {
    if (currentPage > 1) {
      setCurrentPage(currentPage - 1);
    }
  };

  const totalPages = totalRows !== null ? Math.ceil(totalRows / pageSize) : null;

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (error && !tableSchema) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center justify-between p-4 border-b">
          <h3 className="font-semibold">Error</h3>
          <Button variant="ghost" size="sm" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </div>
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center space-y-2">
            <p className="text-destructive">{error}</p>
            <Button variant="outline" size="sm" onClick={fetchTableSchema}>
              <RefreshCw className="h-4 w-4 mr-2" />
              Retry
            </Button>
          </div>
        </div>
      </div>
    );
  }

  if (!tableSchema) return null;

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between p-4 border-b">
        <div className="flex items-center gap-2">
          <Table2 className="h-5 w-5" />
          <div>
            <h3 className="font-semibold">{tableName}</h3>
            <p className="text-xs text-muted-foreground">
              {schema}.{tableName}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={handleRefresh}>
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button variant="ghost" size="sm" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Error Display */}
      {error && (
        <div className="bg-destructive/10 text-destructive px-4 py-2 text-sm border-b">
          {error}
        </div>
      )}

      {/* Tabs */}
      <Tabs value={activeTab} onValueChange={setActiveTab} className="flex-1 flex flex-col">
        <div className="border-b px-4">
          <TabsList>
            <TabsTrigger value="data">Data</TabsTrigger>
            <TabsTrigger value="columns">
              Columns ({tableSchema.columns.length})
            </TabsTrigger>
            <TabsTrigger value="indexes">
              Indexes ({tableSchema.indexes.length})
            </TabsTrigger>
          </TabsList>
        </div>

        {/* Data Tab */}
        <TabsContent value="data" className="flex-1 m-0 overflow-hidden flex flex-col">
          {loadingSampleData ? (
            <div className="flex items-center justify-center flex-1">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : sampleData ? (
            <>
              <div className="flex-1 overflow-hidden">
                <ScrollArea className="h-full w-full">
                  <div className="min-w-max">
                    <Table>
                      <TableHeader className="sticky top-0 bg-background z-10">
                        <TableRow>
                          {sampleData.columns.map((col) => (
                            <TableHead key={col} className="whitespace-nowrap font-semibold">
                              {col}
                            </TableHead>
                          ))}
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {sampleData.rows.map((row, rowIndex) => (
                          <TableRow key={rowIndex}>
                            {row.map((cell, cellIndex) => (
                              <TableCell key={cellIndex} className="whitespace-nowrap">
                                {cell === null || cell === undefined ? (
                                  <span className="italic text-muted-foreground">
                                    NULL
                                  </span>
                                ) : typeof cell === "object" ? (
                                  <code className="text-xs bg-muted px-1 rounded">
                                    {JSON.stringify(cell)}
                                  </code>
                                ) : cell === "" ? (
                                  <span className="italic text-muted-foreground">
                                    (empty)
                                  </span>
                                ) : (
                                  <span>{String(cell)}</span>
                                )}
                              </TableCell>
                            ))}
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                </ScrollArea>
              </div>

              {/* Pagination Controls */}
              <div className="flex items-center justify-between px-4 py-3 border-t bg-background">
                <div className="text-sm text-muted-foreground">
                  {totalRows !== null ? (
                    <>
                      Showing {((currentPage - 1) * pageSize) + 1} to {Math.min(currentPage * pageSize, totalRows)} of {totalRows.toLocaleString()} rows
                    </>
                  ) : (
                    <>Showing {sampleData.rows.length} rows</>
                  )}
                </div>

                <div className="flex items-center gap-2">
                  {totalPages !== null && (
                    <span className="text-sm text-muted-foreground">
                      Page {currentPage} of {totalPages}
                    </span>
                  )}
                  <div className="flex gap-1">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handlePreviousPage}
                      disabled={currentPage === 1 || loadingSampleData}
                    >
                      <ChevronLeft className="h-4 w-4" />
                      Previous
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleNextPage}
                      disabled={totalPages === null || currentPage >= totalPages || loadingSampleData}
                    >
                      Next
                      <ChevronRight className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              </div>
            </>
          ) : (
            <div className="flex items-center justify-center flex-1">
              <div className="text-center space-y-2">
                <Database className="h-8 w-8 mx-auto text-muted-foreground" />
                <p className="text-muted-foreground">No data loaded</p>
                <Button variant="outline" size="sm" onClick={fetchSampleData}>
                  Load Sample Data
                </Button>
              </div>
            </div>
          )}
        </TabsContent>

        {/* Columns Tab */}
        <TabsContent value="columns" className="flex-1 m-0 overflow-hidden">
          <ScrollArea className="h-full w-full">
            <div className="min-w-max">
              <Table>
                <TableHeader className="sticky top-0 bg-background z-10">
                  <TableRow>
                    <TableHead className="whitespace-nowrap">Column</TableHead>
                    <TableHead className="whitespace-nowrap">Type</TableHead>
                    <TableHead className="whitespace-nowrap">Nullable</TableHead>
                    <TableHead className="whitespace-nowrap">Default</TableHead>
                    <TableHead className="whitespace-nowrap">Constraints</TableHead>
                  </TableRow>
                </TableHeader>
              <TableBody>
                {tableSchema.columns.map((column) => (
                  <TableRow key={column.name}>
                    <TableCell className="font-medium">
                      <div className="flex items-center gap-2">
                        {column.isPrimaryKey && (
                          <Key className="h-3 w-3 text-yellow-500" />
                        )}
                        {column.name}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{column.dataType}</Badge>
                    </TableCell>
                    <TableCell>
                      {column.nullable ? (
                        <Badge variant="secondary">NULL</Badge>
                      ) : (
                        <Badge variant="outline">NOT NULL</Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-sm">
                      {column.defaultValue || "-"}
                    </TableCell>
                    <TableCell>
                      {column.isPrimaryKey && (
                        <Badge variant="default">PRIMARY KEY</Badge>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
              </Table>
            </div>
          </ScrollArea>
        </TabsContent>

        {/* Indexes Tab */}
        <TabsContent value="indexes" className="flex-1 m-0 overflow-hidden">
          <ScrollArea className="h-full w-full">
            {tableSchema.indexes.length > 0 ? (
              <div className="min-w-max">
                <Table>
                  <TableHeader className="sticky top-0 bg-background z-10">
                    <TableRow>
                      <TableHead className="whitespace-nowrap">Index Name</TableHead>
                      <TableHead className="whitespace-nowrap">Columns</TableHead>
                      <TableHead className="whitespace-nowrap">Type</TableHead>
                    </TableRow>
                  </TableHeader>
                <TableBody>
                  {tableSchema.indexes.map((index) => (
                    <TableRow key={index.name}>
                      <TableCell className="font-medium">{index.name}</TableCell>
                      <TableCell>{index.columns.join(", ")}</TableCell>
                      <TableCell>
                        <div className="flex gap-1">
                          {index.isPrimary && (
                            <Badge variant="default">PRIMARY</Badge>
                          )}
                          {index.isUnique && (
                            <Badge variant="secondary">UNIQUE</Badge>
                          )}
                          {!index.isPrimary && !index.isUnique && (
                            <Badge variant="outline">INDEX</Badge>
                          )}
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
                </Table>
              </div>
            ) : (
              <div className="flex items-center justify-center h-full">
                <p className="text-muted-foreground">No indexes found</p>
              </div>
            )}
          </ScrollArea>
        </TabsContent>
      </Tabs>
    </div>
  );
}
