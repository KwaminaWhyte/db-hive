import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Table2,
  Key,
  Database,
  X,
  RefreshCw,
  ChevronLeft,
  ChevronRight,
  Copy,
  Edit3,
  Save,
} from "lucide-react";
import { TableSchema, QueryExecutionResult } from "@/types";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { RowJsonViewer } from "./RowJsonViewer";
import { EditableCell, CellChange } from "./EditableCell";
import { TransactionPreview } from "./TransactionPreview";
import { useTableEditor } from "@/hooks/useTableEditor";
import { toast } from "sonner";

interface TableInspectorProps {
  connectionId: string;
  schema: string;
  tableName: string;
  onClose: () => void;
  driverType?: string;
}

export function TableInspector({
  connectionId,
  schema,
  tableName,
  onClose,
  driverType = 'Postgres',
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
  const [pageSize] = useState(20);
  const [totalRows, setTotalRows] = useState<number | null>(null);
  const [selectedRow, setSelectedRow] = useState<any[] | null>(null);
  const [showRowViewer, setShowRowViewer] = useState(false);
  const [editMode, setEditMode] = useState(false);
  const [showTransactionPreview, setShowTransactionPreview] = useState(false);
  const [isCommitting, setIsCommitting] = useState(false);
  const [commitError, setCommitError] = useState<string | null>(null);

  // Initialize table editor hook
  const editor = useTableEditor({
    columns: tableSchema?.columns || [],
    rows: sampleData?.rows || [],
  });

  // Helper function to quote identifiers based on database driver
  const quoteIdentifier = (identifier: string) => {
    if (driverType === 'MySql') {
      return `\`${identifier}\``;
    }
    // PostgreSQL, SQLite use double quotes
    return `"${identifier}"`;
  };

  // Copy helper functions
  const copyToClipboard = useCallback(async (text: string, message: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(message);
    } catch (err) {
      toast.error("Failed to copy to clipboard");
    }
  }, []);

  const copyRowValues = useCallback(async (row: any[]) => {
    const text = row.map((v) => (v === null ? 'NULL' : String(v))).join('\t');
    await copyToClipboard(text, "Row copied");
  }, [copyToClipboard]);

  const copyColumnValues = useCallback(async (columnName: string) => {
    if (!sampleData) return;
    const columnIndex = sampleData.columns.indexOf(columnName);
    if (columnIndex === -1) return;

    const columnValues = sampleData.rows.map((row) => {
      const value = row[columnIndex];
      return value === null ? 'NULL' : String(value);
    });
    const text = columnValues.join('\n');
    await copyToClipboard(text, `Column "${columnName}" copied`);
  }, [sampleData, copyToClipboard]);

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

      let result: QueryExecutionResult;

      if (driverType === 'MongoDb') {
        // MongoDB uses different query syntax
        // Collection name is the tableName
        result = await invoke<QueryExecutionResult>("execute_query", {
          connectionId,
          sql: `db.${tableName}.find({})`,
        });
        // Note: MongoDB pagination will be added when we support .limit() and .skip()
        // For now, return all documents (up to driver's internal limit)
        setTotalRows(null);
      } else {
        // SQL databases
        // Fetch total row count if not already fetched
        if (totalRows === null) {
          try {
            const countResult = await invoke<QueryExecutionResult>("execute_query", {
              connectionId,
              sql: `SELECT COUNT(*) FROM ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)}`,
            });
            const count = countResult.rows[0]?.[0];
            setTotalRows(typeof count === 'number' ? count : parseInt(String(count)) || 0);
          } catch (err) {
            console.error("Failed to fetch row count:", err);
            // Continue without total count
          }
        }

        // Fetch paginated data
        result = await invoke<QueryExecutionResult>("execute_query", {
          connectionId,
          sql: `SELECT * FROM ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)} LIMIT ${pageSize} OFFSET ${offset}`,
        });
      }

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

  // Handle commit changes
  const handleCommit = async () => {
    if (editor.getTotalChanges() === 0) {
      toast.error('No changes to commit');
      return;
    }

    let combinedSQL = ''; // Declare outside try block for error logging

    try {
      setIsCommitting(true);
      setCommitError(null);

      // Generate UPDATE statements
      const statements = editor.generateUpdateStatements(schema, tableName, quoteIdentifier);

      if (driverType === 'MySql') {
        combinedSQL = `START TRANSACTION;\n${statements.join('\n')}\nCOMMIT;`;
      } else if (driverType === 'Sqlite') {
        combinedSQL = `BEGIN TRANSACTION;\n${statements.join('\n')}\nCOMMIT;`;
      } else if (driverType === 'MongoDb') {
        // MongoDB doesn't use SQL transactions in the same way
        // For now, execute statements individually (will be updated when MongoDB transactions are properly implemented)
        toast.error('Table editing is not yet supported for MongoDB');
        return;
      } else {
        // PostgreSQL and SQL Server
        combinedSQL = `BEGIN;\n${statements.join('\n')}\nCOMMIT;`;
      }

      await invoke('execute_query', {
        connectionId,
        sql: combinedSQL,
      });

      toast.success(`Successfully committed ${editor.getTotalChanges()} changes`);

      // Clear changes and refresh data
      editor.discardChanges();
      setShowTransactionPreview(false);
      fetchSampleData();
    } catch (err) {
      const errorMessage = typeof err === 'string' ? err : (err as any)?.message || String(err);
      console.error('Commit failed:', errorMessage);
      console.error('SQL that failed:', combinedSQL);
      setCommitError(`Failed to commit changes: ${errorMessage}`);
      toast.error(`Failed to commit changes: ${errorMessage}`);
    } finally {
      setIsCommitting(false);
    }
  };

  // Handle discard changes
  const handleDiscard = () => {
    editor.discardChanges();
    setShowTransactionPreview(false);
    toast.info('Changes discarded');
  };

  // Toggle edit mode
  const handleToggleEditMode = () => {
    if (editMode && editor.getTotalChanges() > 0) {
      // Warn user they have unsaved changes
      toast.warning('You have unsaved changes. Please commit or discard them first.');
      setShowTransactionPreview(true);
      return;
    }
    setEditMode(!editMode);
    if (!editMode) {
      toast.info('Edit mode enabled - double-click cells to edit');
    }
  };

  if (loading) {
    return (
      <div className="h-full flex flex-col">
        {/* Header Skeleton */}
        <div className="flex items-center justify-between p-4 border-b">
          <div className="flex items-center gap-2">
            <Skeleton className="h-6 w-6 rounded" />
            <Skeleton className="h-6 w-48" />
          </div>
          <Skeleton className="h-8 w-8 rounded" />
        </div>

        {/* Tabs Skeleton */}
        <div className="border-b px-4">
          <div className="flex gap-4 h-10">
            <Skeleton className="h-full w-20" />
            <Skeleton className="h-full w-24" />
            <Skeleton className="h-full w-20" />
          </div>
        </div>

        {/* Content Skeleton */}
        <div className="flex-1 p-4 space-y-3">
          {[1, 2, 3, 4, 5, 6].map((i) => (
            <Skeleton key={i} className="h-12 w-full" />
          ))}
        </div>
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
          {editor.getTotalChanges() > 0 && (
            <>
              <Badge variant="default" className="mr-2">
                {editor.getTotalChanges()} {editor.getTotalChanges() === 1 ? 'change' : 'changes'}
              </Badge>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowTransactionPreview(!showTransactionPreview)}
              >
                <Save className="h-4 w-4 mr-1" />
                Review Changes
              </Button>
            </>
          )}
          <Button
            variant={editMode ? "default" : "outline"}
            size="sm"
            onClick={handleToggleEditMode}
            title={editMode ? "Exit edit mode" : "Enter edit mode"}
          >
            <Edit3 className="h-4 w-4 mr-1" />
            {editMode ? 'Editing' : 'Edit'}
          </Button>
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
      <Tabs value={activeTab} onValueChange={setActiveTab} className="flex-1 flex flex-col overflow-hidden">
        <div className="border-b px-4 shrink-0">
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
        <TabsContent value="data" className="flex-1 m-0 flex flex-col overflow-hidden">
          {loadingSampleData ? (
            <div className="flex-1 p-4 space-y-2">
              <div className="flex gap-2 mb-4">
                {[1, 2, 3, 4, 5].map((i) => (
                  <Skeleton key={i} className="h-8 flex-1" />
                ))}
              </div>
              {[1, 2, 3, 4, 5, 6, 7, 8].map((i) => (
                <div key={i} className="flex gap-2">
                  {[1, 2, 3, 4, 5].map((j) => (
                    <Skeleton key={j} className="h-10 flex-1" />
                  ))}
                </div>
              ))}
            </div>
          ) : sampleData ? (
            <>
              {showTransactionPreview ? (
                <PanelGroup direction="horizontal" className="flex-1">
                  <Panel defaultSize={70} minSize={50}>
                    {showRowViewer && selectedRow ? (
                      <PanelGroup direction="horizontal" className="h-full">
                        <Panel defaultSize={65} minSize={40}>
                          <div className="h-full overflow-auto">
                            <div className="min-w-max h-full">
                              <Table>
                          <TableHeader className="sticky top-0 bg-background border-b z-10">
                            <TableRow>
                              <TableHead className="w-12 text-center font-normal text-xs text-muted-foreground">#</TableHead>
                              {sampleData.columns.map((col) => {
                                const columnInfo = tableSchema?.columns.find(c => c.name === col);
                                return (
                                  <TableHead key={col} className="whitespace-nowrap group">
                                    <div className="flex items-center justify-between gap-2">
                                      <div className="flex flex-col gap-0.5">
                                        <span className="font-medium text-foreground">{col}</span>
                                        {columnInfo && (
                                          <span className="text-[10px] font-normal text-muted-foreground">
                                            {columnInfo.dataType.toUpperCase()}
                                          </span>
                                        )}
                                      </div>
                                      <Button
                                        variant="ghost"
                                        size="sm"
                                        className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          copyColumnValues(col);
                                        }}
                                        title={`Copy column "${col}"`}
                                      >
                                        <Copy className="h-3 w-3" />
                                      </Button>
                                    </div>
                                  </TableHead>
                                );
                              })}
                            </TableRow>
                          </TableHeader>
                          <TableBody>
                            {sampleData.rows.map((row, rowIndex) => {
                              const absoluteRowNumber = ((currentPage - 1) * pageSize) + rowIndex + 1;
                              return (
                                <TableRow
                                  key={rowIndex}
                                  className="hover:bg-muted/50 group"
                                  title="Double-click to view row details"
                                >
                                  <TableCell
                                    className="w-12 text-center text-xs text-muted-foreground font-mono"
                                    onDoubleClick={!editMode ? () => {
                                      setSelectedRow(row);
                                      setShowRowViewer(true);
                                    } : undefined}
                                  >
                                    <div className="flex items-center justify-center gap-1">
                                      <span>{absoluteRowNumber}</span>
                                      <Button
                                        variant="ghost"
                                        size="sm"
                                        className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                        onClick={() => copyRowValues(row)}
                                        title="Copy row"
                                      >
                                        <Copy className="h-3 w-3" />
                                      </Button>
                                    </div>
                                  </TableCell>
                                  {row.map((cell, cellIndex) => (
                                    <TableCell
                                      key={cellIndex}
                                      className="whitespace-nowrap font-mono text-sm cursor-text select-text hover:bg-accent/50"
                                      onDoubleClick={() => {
                                        setSelectedRow(row);
                                        setShowRowViewer(true);
                                      }}
                                      title="Double-click for JSON viewer"
                                    >
                                      {cell === null || cell === undefined ? (
                                        <span className="italic text-muted-foreground opacity-50">
                                          NULL
                                        </span>
                                      ) : typeof cell === "object" ? (
                                        <code className="text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
                                          {JSON.stringify(cell)}
                                        </code>
                                      ) : typeof cell === "boolean" ? (
                                        <span className={cell ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
                                          {String(cell)}
                                        </span>
                                      ) : typeof cell === "number" ? (
                                        <span className="text-blue-600 dark:text-blue-400">
                                          {cell.toLocaleString()}
                                        </span>
                                      ) : cell === "" ? (
                                        <span className="italic text-muted-foreground opacity-50">
                                          (empty)
                                        </span>
                                      ) : (
                                        <span className="text-foreground">{String(cell)}</span>
                                      )}
                                    </TableCell>
                                  ))}
                                </TableRow>
                              );
                            })}
                          </TableBody>
                        </Table>
                      </div>
                    </div>
                        </Panel>
                        <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />
                        <Panel defaultSize={35} minSize={25} maxSize={60}>
                          <RowJsonViewer
                            columns={sampleData.columns}
                            row={selectedRow}
                            onClose={() => {
                              setShowRowViewer(false);
                              setSelectedRow(null);
                            }}
                          />
                        </Panel>
                      </PanelGroup>
                    ) : (
                <div className="flex-1 overflow-auto">
                <div className="min-w-max h-full">
                  <Table>
                      <TableHeader className="sticky top-0 bg-background border-b z-10">
                        <TableRow>
                          <TableHead className="w-12 text-center font-normal text-xs text-muted-foreground">#</TableHead>
                          {sampleData.columns.map((col) => {
                            const columnInfo = tableSchema?.columns.find(c => c.name === col);
                            return (
                              <TableHead key={col} className="whitespace-nowrap group">
                                <div className="flex items-center justify-between gap-2">
                                  <div className="flex flex-col gap-0.5">
                                    <span className="font-medium text-foreground">{col}</span>
                                    {columnInfo && (
                                      <span className="text-[10px] font-normal text-muted-foreground">
                                        {columnInfo.dataType.toUpperCase()}
                                      </span>
                                    )}
                                  </div>
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      copyColumnValues(col);
                                    }}
                                    title={`Copy column "${col}"`}
                                  >
                                    <Copy className="h-3 w-3" />
                                  </Button>
                                </div>
                              </TableHead>
                            );
                          })}
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {sampleData.rows.map((row, rowIndex) => {
                          const absoluteRowNumber = ((currentPage - 1) * pageSize) + rowIndex + 1;
                          return (
                            <TableRow
                              key={rowIndex}
                              className="hover:bg-muted/50 group"
                              title="Double-click to view row details"
                            >
                              <TableCell
                                className="w-12 text-center text-xs text-muted-foreground font-mono"
                                onDoubleClick={!editMode ? () => {
                                  setSelectedRow(row);
                                  setShowRowViewer(true);
                                } : undefined}
                              >
                                <div className="flex items-center justify-center gap-1">
                                  <span>{absoluteRowNumber}</span>
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                    onClick={() => copyRowValues(row)}
                                    title="Copy row"
                                  >
                                    <Copy className="h-3 w-3" />
                                  </Button>
                                </div>
                              </TableCell>
                              {row.map((cell, cellIndex) => {
                                const columnName = sampleData.columns[cellIndex];
                                const columnInfo = tableSchema?.columns.find(c => c.name === columnName);

                                if (!columnInfo) return null;

                                // In edit mode, use EditableCell component
                                if (editMode) {
                                  const isEditing = editor.editingCell?.rowIndex === rowIndex &&
                                                   editor.editingCell?.columnIndex === cellIndex;
                                  const isModified = editor.isCellModified(rowIndex, columnName);

                                  return (
                                    <TableCell key={cellIndex} className="p-0">
                                      <EditableCell
                                        value={cell}
                                        rowIndex={rowIndex}
                                        columnName={columnName}
                                        columnIndex={cellIndex}
                                        isEditing={isEditing}
                                        isModified={isModified}
                                        dataType={columnInfo.dataType}
                                        nullable={columnInfo.nullable}
                                        onStartEdit={editor.startEdit}
                                        onChange={editor.applyChange}
                                        onCancelEdit={editor.cancelEdit}
                                      />
                                    </TableCell>
                                  );
                                }

                                // Read-only mode (original rendering)
                                const cellString = cell === null || cell === undefined ? 'NULL' :
                                                  typeof cell === "object" ? JSON.stringify(cell) :
                                                  String(cell);
                                const isTruncated = cellString.length > 100;
                                const displayValue = isTruncated ? cellString.substring(0, 100) + '...' : cellString;

                                return (
                                  <TableCell
                                    key={cellIndex}
                                    className="whitespace-nowrap font-mono text-sm cursor-text select-text hover:bg-accent/50 max-w-md"
                                    onDoubleClick={() => {
                                      setSelectedRow(row);
                                      setShowRowViewer(true);
                                    }}
                                    title={isTruncated ? `${cellString}\n\nDouble-click for JSON viewer` : "Double-click for JSON viewer"}
                                  >
                                    <div className="truncate">
                                      {cell === null || cell === undefined ? (
                                        <span className="italic text-muted-foreground opacity-50">
                                          NULL
                                        </span>
                                      ) : typeof cell === "object" ? (
                                        <code className="text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
                                          {displayValue}
                                        </code>
                                      ) : typeof cell === "boolean" ? (
                                        <span className={cell ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
                                          {String(cell)}
                                        </span>
                                      ) : typeof cell === "number" ? (
                                        <span className="text-blue-600 dark:text-blue-400">
                                          {cell.toLocaleString()}
                                        </span>
                                      ) : cell === "" ? (
                                        <span className="italic text-muted-foreground opacity-50">
                                          (empty)
                                        </span>
                                      ) : (
                                        <span className="text-foreground">{displayValue}</span>
                                      )}
                                    </div>
                                  </TableCell>
                                );
                              })}
                            </TableRow>
                          );
                        })}
                      </TableBody>
                    </Table>
                    {sampleData.rows.length === 0 && (
                      <div className="flex items-center justify-center py-12">
                        <div className="text-center space-y-2">
                          <Database className="h-12 w-12 mx-auto text-muted-foreground opacity-50" />
                          <p className="text-muted-foreground">No data found in this table</p>
                          <p className="text-sm text-muted-foreground">This collection/table is empty</p>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}
                    </Panel>
                    <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />
                    <Panel defaultSize={30} minSize={20} maxSize={50}>
                      <TransactionPreview
                        statements={editor.getTotalChanges() > 0 ? editor.generateUpdateStatements(schema, tableName, quoteIdentifier) : []}
                        isExecuting={isCommitting}
                        onCommit={handleCommit}
                        onDiscard={handleDiscard}
                        onClose={() => setShowTransactionPreview(false)}
                        error={commitError}
                      />
                    </Panel>
                  </PanelGroup>
                ) : (
                  showRowViewer && selectedRow ? (
                    <PanelGroup direction="horizontal" className="flex-1">
                      <Panel defaultSize={65} minSize={40}>
                        <div className="h-full overflow-auto">
                          <div className="min-w-max h-full">
                            <Table>
                              <TableHeader className="sticky top-0 bg-background border-b z-10">
                                <TableRow>
                                  <TableHead className="w-12 text-center font-normal text-xs text-muted-foreground">#</TableHead>
                                  {sampleData.columns.map((col) => {
                                    const columnInfo = tableSchema?.columns.find(c => c.name === col);
                                    return (
                                      <TableHead key={col} className="whitespace-nowrap group">
                                        <div className="flex items-center justify-between gap-2">
                                          <div className="flex flex-col gap-0.5">
                                            <span className="font-medium text-foreground">{col}</span>
                                            {columnInfo && (
                                              <span className="text-[10px] font-normal text-muted-foreground">
                                                {columnInfo.dataType.toUpperCase()}
                                              </span>
                                            )}
                                          </div>
                                          <Button
                                            variant="ghost"
                                            size="sm"
                                            className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              copyColumnValues(col);
                                            }}
                                            title={`Copy column "${col}"`}
                                          >
                                            <Copy className="h-3 w-3" />
                                          </Button>
                                        </div>
                                      </TableHead>
                                    );
                                  })}
                                </TableRow>
                              </TableHeader>
                              <TableBody>
                                {sampleData.rows.map((row, rowIndex) => {
                                  const absoluteRowNumber = ((currentPage - 1) * pageSize) + rowIndex + 1;
                                  return (
                                    <TableRow
                                      key={rowIndex}
                                      className="hover:bg-muted/50 group"
                                      title="Double-click to view row details"
                                    >
                                      <TableCell
                                        className="w-12 text-center text-xs text-muted-foreground font-mono"
                                        onDoubleClick={() => {
                                          setSelectedRow(row);
                                          setShowRowViewer(true);
                                        }}
                                      >
                                        <div className="flex items-center justify-center gap-1">
                                          <span>{absoluteRowNumber}</span>
                                          <Button
                                            variant="ghost"
                                            size="sm"
                                            className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                            onClick={() => copyRowValues(row)}
                                            title="Copy row"
                                          >
                                            <Copy className="h-3 w-3" />
                                          </Button>
                                        </div>
                                      </TableCell>
                                      {row.map((cell, cellIndex) => {
                                        const cellString = cell === null || cell === undefined ? 'NULL' :
                                                          typeof cell === "object" ? JSON.stringify(cell) :
                                                          String(cell);
                                        const isTruncated = cellString.length > 100;
                                        const displayValue = isTruncated ? cellString.substring(0, 100) + '...' : cellString;

                                        return (
                                          <TableCell
                                            key={cellIndex}
                                            className="whitespace-nowrap font-mono text-sm cursor-text select-text hover:bg-accent/50 max-w-md"
                                            onDoubleClick={() => {
                                              setSelectedRow(row);
                                              setShowRowViewer(true);
                                            }}
                                            title={isTruncated ? `${cellString}\n\nDouble-click for JSON viewer` : "Double-click for JSON viewer"}
                                          >
                                            <div className="truncate">
                                              {cell === null || cell === undefined ? (
                                                <span className="italic text-muted-foreground opacity-50">
                                                  NULL
                                                </span>
                                              ) : typeof cell === "object" ? (
                                                <code className="text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
                                                  {displayValue}
                                                </code>
                                              ) : typeof cell === "boolean" ? (
                                                <span className={cell ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
                                                  {String(cell)}
                                                </span>
                                              ) : typeof cell === "number" ? (
                                                <span className="text-blue-600 dark:text-blue-400">
                                                  {cell.toLocaleString()}
                                                </span>
                                              ) : cell === "" ? (
                                                <span className="italic text-muted-foreground opacity-50">
                                                  (empty)
                                                </span>
                                              ) : (
                                                <span className="text-foreground">{displayValue}</span>
                                              )}
                                            </div>
                                          </TableCell>
                                        );
                                      })}
                                    </TableRow>
                                  );
                                })}
                              </TableBody>
                            </Table>
                          </div>
                        </div>
                      </Panel>
                      <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />
                      <Panel defaultSize={35} minSize={25} maxSize={60}>
                        <RowJsonViewer
                          columns={sampleData.columns}
                          row={selectedRow}
                          onClose={() => {
                            setShowRowViewer(false);
                            setSelectedRow(null);
                          }}
                        />
                      </Panel>
                    </PanelGroup>
                  ) : (
                    <div className="flex-1 overflow-auto">
                      <div className="min-w-max h-full">
                        <Table>
                          <TableHeader className="sticky top-0 bg-background border-b z-10">
                            <TableRow>
                              <TableHead className="w-12 text-center font-normal text-xs text-muted-foreground">#</TableHead>
                              {sampleData.columns.map((col) => {
                                const columnInfo = tableSchema?.columns.find(c => c.name === col);
                                return (
                                  <TableHead key={col} className="whitespace-nowrap group">
                                    <div className="flex items-center justify-between gap-2">
                                      <div className="flex flex-col gap-0.5">
                                        <span className="font-medium text-foreground">{col}</span>
                                        {columnInfo && (
                                          <span className="text-[10px] font-normal text-muted-foreground">
                                            {columnInfo.dataType.toUpperCase()}
                                          </span>
                                        )}
                                      </div>
                                      <Button
                                        variant="ghost"
                                        size="sm"
                                        className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          copyColumnValues(col);
                                        }}
                                        title={`Copy column "${col}"`}
                                      >
                                        <Copy className="h-3 w-3" />
                                      </Button>
                                    </div>
                                  </TableHead>
                                );
                              })}
                            </TableRow>
                          </TableHeader>
                          <TableBody>
                            {sampleData.rows.map((row, rowIndex) => {
                              const absoluteRowNumber = ((currentPage - 1) * pageSize) + rowIndex + 1;
                              return (
                                <TableRow
                                  key={rowIndex}
                                  className="hover:bg-muted/50 group"
                                  title="Double-click to view row details"
                                >
                                  <TableCell
                                    className="w-12 text-center text-xs text-muted-foreground font-mono"
                                    onDoubleClick={!editMode ? () => {
                                      setSelectedRow(row);
                                      setShowRowViewer(true);
                                    } : undefined}
                                  >
                                    <div className="flex items-center justify-center gap-1">
                                      <span>{absoluteRowNumber}</span>
                                      <Button
                                        variant="ghost"
                                        size="sm"
                                        className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                        onClick={() => copyRowValues(row)}
                                        title="Copy row"
                                      >
                                        <Copy className="h-3 w-3" />
                                      </Button>
                                    </div>
                                  </TableCell>
                                  {row.map((cell, cellIndex) => {
                                    const columnName = sampleData.columns[cellIndex];
                                    const columnInfo = tableSchema?.columns.find(c => c.name === columnName);

                                    if (!columnInfo) return null;

                                    // In edit mode, use EditableCell component
                                    if (editMode) {
                                      const isEditing = editor.editingCell?.rowIndex === rowIndex &&
                                                       editor.editingCell?.columnIndex === cellIndex;
                                      const isModified = editor.isCellModified(rowIndex, columnName);

                                      return (
                                        <TableCell key={cellIndex} className="p-0">
                                          <EditableCell
                                            value={cell}
                                            rowIndex={rowIndex}
                                            columnName={columnName}
                                            columnIndex={cellIndex}
                                            isEditing={isEditing}
                                            isModified={isModified}
                                            dataType={columnInfo.dataType}
                                            nullable={columnInfo.nullable}
                                            onStartEdit={editor.startEdit}
                                            onChange={editor.applyChange}
                                            onCancelEdit={editor.cancelEdit}
                                          />
                                        </TableCell>
                                      );
                                    }

                                    // Read-only mode (original rendering)
                                    const cellString = cell === null || cell === undefined ? 'NULL' :
                                                      typeof cell === "object" ? JSON.stringify(cell) :
                                                      String(cell);
                                    const isTruncated = cellString.length > 100;
                                    const displayValue = isTruncated ? cellString.substring(0, 100) + '...' : cellString;

                                    return (
                                      <TableCell
                                        key={cellIndex}
                                        className="whitespace-nowrap font-mono text-sm cursor-text select-text hover:bg-accent/50 max-w-md"
                                        onDoubleClick={() => {
                                          setSelectedRow(row);
                                          setShowRowViewer(true);
                                        }}
                                        title={isTruncated ? `${cellString}\n\nDouble-click for JSON viewer` : "Double-click for JSON viewer"}
                                      >
                                        <div className="truncate">
                                          {cell === null || cell === undefined ? (
                                            <span className="italic text-muted-foreground opacity-50">
                                              NULL
                                            </span>
                                          ) : typeof cell === "object" ? (
                                            <code className="text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
                                              {displayValue}
                                            </code>
                                          ) : typeof cell === "boolean" ? (
                                            <span className={cell ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
                                              {String(cell)}
                                            </span>
                                          ) : typeof cell === "number" ? (
                                            <span className="text-blue-600 dark:text-blue-400">
                                              {cell.toLocaleString()}
                                            </span>
                                          ) : cell === "" ? (
                                            <span className="italic text-muted-foreground opacity-50">
                                              (empty)
                                            </span>
                                          ) : (
                                            <span className="text-foreground">{displayValue}</span>
                                          )}
                                        </div>
                                      </TableCell>
                                    );
                                  })}
                                </TableRow>
                              );
                            })}
                          </TableBody>
                        </Table>
                        {sampleData.rows.length === 0 && (
                          <div className="flex items-center justify-center py-12">
                            <div className="text-center space-y-2">
                              <Database className="h-12 w-12 mx-auto text-muted-foreground opacity-50" />
                              <p className="text-muted-foreground">No data found in this table</p>
                              <p className="text-sm text-muted-foreground">This collection/table is empty</p>
                            </div>
                          </div>
                        )}
                      </div>
                    </div>
                  )
                )}

              {/* Pagination Controls */}
              <div className="flex items-center justify-between px-4 py-3 border-t bg-background shrink-0">
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
