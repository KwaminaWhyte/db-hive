import React, { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Checkbox } from "@/components/ui/checkbox";
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
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Table2,
  Key,
  Database,
  X,
  RefreshCw,
  ChevronLeft,
  ChevronRight,
  Copy,
  Save,
  Trash2,
  Plus,
  XCircle,
  ClipboardCopy,
  Pencil,
  Ban,
} from "lucide-react";
import { TableSchema, QueryExecutionResult } from "@/types";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { RowJsonViewer } from "./RowJsonViewer";
import { EditableCell } from "./EditableCell";
import { TransactionPreview } from "./TransactionPreview";
import { useTableEditor } from "@/hooks/useTableEditor";
import { toast } from "sonner";

/** Returns a small styled badge indicating the column's data type */
function getColumnTypeIcon(dataType: string): React.ReactNode {
  const type = dataType.toLowerCase();
  if (
    type.includes("int") ||
    type.includes("serial") ||
    type.includes("numeric") ||
    type.includes("decimal") ||
    type.includes("float") ||
    type.includes("double") ||
    type.includes("real") ||
    type.includes("money")
  ) {
    return (
      <span className="text-[10px] font-bold text-blue-500 bg-blue-500/10 px-1 rounded leading-none">
        #
      </span>
    );
  }
  if (type.includes("bool")) {
    return (
      <span className="text-[10px] font-bold text-purple-500 bg-purple-500/10 px-1 rounded leading-none">
        B
      </span>
    );
  }
  if (type.includes("json")) {
    return (
      <span className="text-[10px] font-bold text-orange-500 bg-orange-500/10 px-1 rounded leading-none">
        {"{}"}
      </span>
    );
  }
  if (
    type.includes("date") ||
    type.includes("time") ||
    type.includes("timestamp")
  ) {
    return (
      <span className="text-[10px] font-bold text-green-500 bg-green-500/10 px-1 rounded leading-none">
        D
      </span>
    );
  }
  if (type.includes("uuid")) {
    return (
      <span className="text-[10px] font-bold text-pink-500 bg-pink-500/10 px-1 rounded leading-none">
        U
      </span>
    );
  }
  if (type.includes("bytea") || type.includes("blob") || type.includes("binary")) {
    return (
      <span className="text-[10px] font-bold text-yellow-600 bg-yellow-500/10 px-1 rounded leading-none">
        0x
      </span>
    );
  }
  if (type.includes("array") || type.includes("[]")) {
    return (
      <span className="text-[10px] font-bold text-cyan-500 bg-cyan-500/10 px-1 rounded leading-none">
        []
      </span>
    );
  }
  // Default: text/varchar/char type
  return (
    <span className="text-[10px] font-bold text-muted-foreground bg-muted px-1 rounded leading-none">
      T
    </span>
  );
}

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
  const [pageSize] = useState(35);
  const [totalRows, setTotalRows] = useState<number | null>(null);
  const [selectedRow, setSelectedRow] = useState<any[] | null>(null);
  const [showRowViewer, setShowRowViewer] = useState(false);
  const [showTransactionPreview, setShowTransactionPreview] = useState(false);
  const [isCommitting, setIsCommitting] = useState(false);
  const [commitError, setCommitError] = useState<string | null>(null);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);

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

  // Copy a single cell value to clipboard
  const copyCellValue = useCallback(async (value: any) => {
    const text = value === null || value === undefined ? 'NULL' :
                 typeof value === 'object' ? JSON.stringify(value) : String(value);
    await copyToClipboard(text, "Cell value copied");
  }, [copyToClipboard]);

  // Copy a row as JSON to clipboard
  const copyRowAsJson = useCallback(async (row: any[]) => {
    if (!sampleData) return;
    const obj: Record<string, any> = {};
    sampleData.columns.forEach((col, i) => {
      obj[col] = row[i];
    });
    const json = JSON.stringify(obj, null, 2);
    await copyToClipboard(json, "Row copied as JSON");
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
    const totalChanges = editor.getTotalChanges();
    const newRowsCount = editor.newRows.size;

    if (totalChanges === 0 && newRowsCount === 0) {
      toast.error('No changes to commit');
      return;
    }

    let combinedSQL = ''; // Declare outside try block for error logging

    try {
      setIsCommitting(true);
      setCommitError(null);

      // Generate all statements
      const statements: string[] = [];

      // Add INSERT statements for new rows
      if (newRowsCount > 0) {
        const insertStatements = editor.generateInsertStatements(schema, tableName, quoteIdentifier);
        statements.push(...insertStatements);
      }

      // Add UPDATE statements for modified cells
      if (totalChanges > 0) {
        const updateStatements = editor.generateUpdateStatements(schema, tableName, quoteIdentifier);
        statements.push(...updateStatements);
      }

      if (statements.length === 0) {
        toast.error('No changes to commit');
        return;
      }

      // Wrap in transaction
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

      const message = `Successfully committed ${newRowsCount + totalChanges} ${newRowsCount + totalChanges === 1 ? 'change' : 'changes'}`;
      toast.success(message);

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

  // Handle bulk delete
  const handleBulkDelete = async () => {
    if (editor.selectedRows.size === 0) {
      toast.error('No rows selected');
      return;
    }

    try {
      setIsDeleting(true);
      setCommitError(null);

      // Generate DELETE statements
      const statements = editor.generateDeleteStatements(schema, tableName, quoteIdentifier);

      // Execute statements in a transaction
      let combinedSQL = '';
      if (driverType === 'MySql') {
        combinedSQL = `START TRANSACTION;\n${statements.join('\n')}\nCOMMIT;`;
      } else if (driverType === 'Sqlite') {
        combinedSQL = `BEGIN TRANSACTION;\n${statements.join('\n')}\nCOMMIT;`;
      } else if (driverType === 'MongoDb') {
        toast.error('Bulk delete is not yet supported for MongoDB');
        return;
      } else {
        // PostgreSQL and SQL Server
        combinedSQL = `BEGIN;\n${statements.join('\n')}\nCOMMIT;`;
      }

      await invoke('execute_query', {
        connectionId,
        sql: combinedSQL,
      });

      toast.success(`Successfully deleted ${editor.selectedRows.size} row${editor.selectedRows.size > 1 ? 's' : ''}`);

      // Clear selection and refresh data
      editor.clearSelection();
      setShowDeleteConfirm(false);
      fetchSampleData();
    } catch (err) {
      const errorMessage = typeof err === 'string' ? err : (err as any)?.message || String(err);
      console.error('Bulk delete failed:', errorMessage);
      setCommitError(`Failed to delete rows: ${errorMessage}`);
      toast.error(`Failed to delete rows: ${errorMessage}`);
    } finally {
      setIsDeleting(false);
    }
  };

  // Clear selection when page changes
  useEffect(() => {
    editor.clearSelection();
  }, [currentPage]);

  // Clear selection when data refreshes
  useEffect(() => {
    editor.clearSelection();
  }, [sampleData]);

  // Compute header checkbox state (for indeterminate support)
  const totalSelectableRows = (sampleData?.rows.length || 0) + editor.newRows.size;
  const allRowsSelected = totalSelectableRows > 0 && editor.selectedRows.size === totalSelectableRows;
  const someRowsSelected = editor.selectedRows.size > 0 && !allRowsSelected;
  const headerCheckboxState: boolean | "indeterminate" = allRowsSelected
    ? true
    : someRowsSelected
      ? "indeterminate"
      : false;

  // Copy selected rows as JSON to clipboard
  const copySelectedRowsAsJson = useCallback(async () => {
    if (!sampleData || editor.selectedRows.size === 0) return;

    const selectedData: Record<string, any>[] = [];

    editor.selectedRows.forEach((rowIndex) => {
      if (rowIndex < 0) {
        // New row
        const newRow = editor.newRows.get(rowIndex);
        if (newRow) {
          const obj: Record<string, any> = {};
          newRow.values.forEach((value, key) => {
            obj[key] = value;
          });
          selectedData.push(obj);
        }
      } else {
        // Existing row
        const row = sampleData.rows[rowIndex];
        if (row) {
          const obj: Record<string, any> = {};
          sampleData.columns.forEach((col, i) => {
            obj[col] = row[i];
          });
          selectedData.push(obj);
        }
      }
    });

    const json = JSON.stringify(selectedData, null, 2);
    await copyToClipboard(json, `${selectedData.length} row${selectedData.length === 1 ? '' : 's'} copied as JSON`);
  }, [sampleData, editor.selectedRows, editor.newRows, copyToClipboard]);

  // Bulk delete selected rows by queuing them through the editor's deleteRow pattern
  const handleBulkDeleteFromBar = useCallback(() => {
    if (editor.selectedRows.size === 0) return;
    setShowDeleteConfirm(true);
  }, [editor.selectedRows.size]);

  // ---------- Shared rendering helpers ----------

  /** Render the table header row (checkbox + # + column headers) */
  const renderTableHeader = () => (
    <TableHeader className="sticky top-0 bg-background border-b z-10">
      <TableRow>
        <TableHead className="w-12 text-center">
          <Checkbox
            checked={headerCheckboxState}
            onCheckedChange={(checked) => {
              if (checked) {
                editor.selectAll();
              } else {
                editor.clearSelection();
              }
            }}
            aria-label="Select all rows"
          />
        </TableHead>
        <TableHead className="w-12 text-center font-normal text-xs text-muted-foreground">#</TableHead>
        {(tableSchema?.columns || []).map((columnInfo) => (
          <TableHead key={columnInfo.name} className="whitespace-nowrap group">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5">
                {columnInfo.isPrimaryKey && <Key className="h-3 w-3 text-amber-500 shrink-0" />}
                <span className="shrink-0">{getColumnTypeIcon(columnInfo.dataType)}</span>
                <span className="font-medium text-foreground truncate">{columnInfo.name}</span>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                onClick={(e) => {
                  e.stopPropagation();
                  copyColumnValues(columnInfo.name);
                }}
                title={`Copy column "${columnInfo.name}"`}
              >
                <Copy className="h-3 w-3" />
              </Button>
            </div>
          </TableHead>
        ))}
      </TableRow>
    </TableHeader>
  );

  /** Render new rows (inserted but not yet committed) */
  const renderNewRows = () => (
    <>
      {Array.from(editor.newRows.values()).map((newRow) => {
        const newRowIndex = newRow.tempId; // negative number
        const isSelected = editor.selectedRows.has(newRowIndex);
        return (
          <TableRow
            key={`new-${newRowIndex}`}
            className={`hover:bg-muted/50 group ${isSelected ? 'bg-muted' : ''} bg-green-500/10 border-l-2 border-l-green-500`}
            title="New row (not yet committed)"
          >
            <TableCell className="w-12 text-center">
              <Checkbox
                checked={isSelected}
                onCheckedChange={() => editor.toggleRowSelection(newRowIndex)}
                aria-label={`Select new row`}
              />
            </TableCell>
            <TableCell className="w-12 text-center text-xs text-muted-foreground font-mono">
              <div className="flex items-center justify-center gap-1">
                <Plus className="h-3 w-3 text-green-600" />
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-5 w-5 p-0 opacity-100 hover:bg-destructive/20"
                  onClick={() => {
                    editor.removeNewRow(newRowIndex);
                    toast.info('New row removed');
                  }}
                  title="Remove new row"
                >
                  <X className="h-3 w-3 text-destructive" />
                </Button>
              </div>
            </TableCell>
            {(tableSchema?.columns || []).map((columnInfo, cellIndex) => {
              const isEditing = editor.editingCell?.rowIndex === newRowIndex &&
                               editor.editingCell?.columnIndex === cellIndex;
              const cellValue = newRow.values.get(columnInfo.name);

              return (
                <TableCell key={cellIndex} className="p-0">
                  <EditableCell
                    value={cellValue}
                    rowIndex={newRowIndex}
                    columnName={columnInfo.name}
                    columnIndex={cellIndex}
                    isEditing={isEditing}
                    isModified={false}
                    dataType={columnInfo.dataType}
                    nullable={columnInfo.nullable}
                    onStartEdit={editor.startEdit}
                    onChange={editor.applyChange}
                    onCancelEdit={editor.cancelEdit}
                  />
                </TableCell>
              );
            })}
          </TableRow>
        );
      })}
    </>
  );

  /** Render existing data rows with EditableCell and context menu */
  const renderExistingRows = () => (
    <>
      {sampleData?.rows.map((row, rowIndex) => {
        const absoluteRowNumber = ((currentPage - 1) * pageSize) + rowIndex + 1;
        const isSelected = editor.selectedRows.has(rowIndex);
        return (
          <TableRow
            key={rowIndex}
            className={`hover:bg-muted/50 group ${isSelected ? 'bg-muted' : ''}`}
            title="Double-click cells to edit"
          >
            <TableCell className="w-12 text-center">
              <Checkbox
                checked={isSelected}
                onCheckedChange={() => editor.toggleRowSelection(rowIndex)}
                aria-label={`Select row ${absoluteRowNumber}`}
              />
            </TableCell>
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
              const columnName = sampleData!.columns[cellIndex];
              const columnInfo = tableSchema?.columns.find(c => c.name === columnName);

              if (!columnInfo) return null;

              const isEditing = editor.editingCell?.rowIndex === rowIndex &&
                               editor.editingCell?.columnIndex === cellIndex;
              const isModified = editor.isCellModified(rowIndex, columnName);

              return (
                <ContextMenu key={cellIndex}>
                  <ContextMenuTrigger asChild>
                    <TableCell className="p-0">
                      <EditableCell
                        value={cell}
                        rowIndex={rowIndex}
                        columnName={columnName}
                        columnIndex={cellIndex}
                        isEditing={isEditing}
                        isModified={isModified}
                        dataType={columnInfo.dataType}
                        nullable={columnInfo.nullable}
                        isPrimaryKey={columnInfo.isPrimaryKey}
                        onStartEdit={editor.startEdit}
                        onChange={editor.applyChange}
                        onCancelEdit={editor.cancelEdit}
                      />
                    </TableCell>
                  </ContextMenuTrigger>
                  <ContextMenuContent>
                    <ContextMenuItem onClick={() => copyCellValue(cell)}>
                      <ClipboardCopy className="h-4 w-4 mr-2" />
                      Copy Cell
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => copyRowAsJson(row)}>
                      <Copy className="h-4 w-4 mr-2" />
                      Copy Row as JSON
                    </ContextMenuItem>
                    {!columnInfo.isPrimaryKey && (
                      <ContextMenuItem onClick={() => editor.startEdit(rowIndex, cellIndex)}>
                        <Pencil className="h-4 w-4 mr-2" />
                        Edit Cell
                      </ContextMenuItem>
                    )}
                    {!columnInfo.isPrimaryKey && columnInfo.nullable && (
                      <ContextMenuItem onClick={() => {
                        editor.applyChange({
                          rowIndex,
                          columnName,
                          columnIndex: cellIndex,
                          oldValue: cell,
                          newValue: null,
                        });
                      }}>
                        <Ban className="h-4 w-4 mr-2" />
                        Set to NULL
                      </ContextMenuItem>
                    )}
                    <ContextMenuSeparator />
                    <ContextMenuItem
                      className="text-destructive focus:text-destructive"
                      onClick={() => {
                        editor.toggleRowSelection(rowIndex);
                        setShowDeleteConfirm(true);
                      }}
                    >
                      <Trash2 className="h-4 w-4 mr-2" />
                      Delete Row
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              );
            })}
          </TableRow>
        );
      })}
    </>
  );

  /** Render the full data table (header + new rows + existing rows) */
  const renderDataTable = () => (
    <div className="h-full overflow-auto">
      <div className="min-w-max h-full select-none">
        <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
          {renderTableHeader()}
          <TableBody>
            {renderNewRows()}
            {renderExistingRows()}
          </TableBody>
        </Table>
        {sampleData && sampleData.rows.length === 0 && editor.newRows.size === 0 && (
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
  );

  /** Render the TransactionPreview panel */
  const renderTransactionPreview = () => (
    <TransactionPreview
      statements={(() => {
        const statements: string[] = [];
        // Add INSERT statements for new rows
        if (editor.newRows.size > 0) {
          statements.push(...editor.generateInsertStatements(schema, tableName, quoteIdentifier));
        }
        // Add UPDATE statements for modified cells
        if (editor.getTotalChanges() > 0) {
          statements.push(...editor.generateUpdateStatements(schema, tableName, quoteIdentifier));
        }
        return statements;
      })()}
      isExecuting={isCommitting}
      onCommit={handleCommit}
      onDiscard={handleDiscard}
      onClose={() => setShowTransactionPreview(false)}
      error={commitError}
      totalChanges={editor.getTotalChanges() + editor.newRows.size}
      modifiedRows={editor.changes.size + editor.newRows.size}
      {...{
        changes: editor.changes,
        newRows: editor.newRows,
        columns: tableSchema?.columns || [],
        rows: sampleData?.rows || [],
        tableName,
        schemaName: schema,
      } as any}
    />
  );

  if (loading) {
    return (
      <div className="h-full flex flex-col">
        {/* Header Skeleton */}
        <div className="flex items-center justify-between px-3 py-2 border-b">
          <div className="flex items-center gap-2">
            <Skeleton className="h-6 w-6 rounded" />
            <Skeleton className="h-6 w-48" />
          </div>
          <Skeleton className="h-8 w-8 rounded" />
        </div>

        {/* Content Skeleton */}
        <div className="flex-1 p-4 space-y-3">
          {[1, 2, 3, 4, 5, 6].map((i) => (
            <Skeleton key={i} className="h-12 w-full" />
          ))}
        </div>

        {/* Tabs Skeleton at bottom */}
        <div className="border-t px-4">
          <div className="flex gap-4 h-10">
            <Skeleton className="h-full w-20" />
            <Skeleton className="h-full w-24" />
            <Skeleton className="h-full w-20" />
          </div>
        </div>
      </div>
    );
  }

  if (error && !tableSchema) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center justify-between px-3 py-2 border-b">
          <h3 className="text-sm font-semibold">Error</h3>
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
      <div className="flex items-center justify-between px-3 py-2 border-b">
        <div className="flex items-center gap-2">
          <Table2 className="h-4 w-4" />
          <div>
            <h3 className="text-sm font-semibold">{tableName}</h3>
            <p className="text-xs text-muted-foreground">
              {schema}.{tableName}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {(editor.getTotalChanges() > 0 || editor.newRows.size > 0) && (
            <>
              <Badge variant="default" className="mr-2">
                {editor.getTotalChanges() + editor.newRows.size} {editor.getTotalChanges() + editor.newRows.size === 1 ? 'change' : 'changes'}
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
          {editor.selectedRows.size > 0 && (
            <>
              <Badge variant="secondary" className="mr-2">
                {editor.selectedRows.size} {editor.selectedRows.size === 1 ? 'row' : 'rows'} selected
              </Badge>
              <Button
                variant="destructive"
                size="sm"
                onClick={() => setShowDeleteConfirm(true)}
                disabled={isDeleting}
              >
                <Trash2 className="h-4 w-4 mr-1" />
                {isDeleting ? 'Deleting...' : 'Delete Selected'}
              </Button>
            </>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              editor.addRow();
              toast.success('New row added');
            }}
            title="Add a new row"
          >
            <Plus className="h-4 w-4 mr-1" />
            Add Row
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

      {/* Tabs - TabsList moved to bottom */}
      <Tabs value={activeTab} onValueChange={setActiveTab} className="flex-1 flex flex-col overflow-hidden">
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
                          {renderDataTable()}
                        </Panel>
                        <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />
                        <Panel defaultSize={35} minSize={25} maxSize={60}>
                          <RowJsonViewer
                            columns={sampleData.columns.length > 0 ? sampleData.columns : tableSchema?.columns.map(c => c.name) || []}
                            row={selectedRow}
                            onClose={() => {
                              setShowRowViewer(false);
                              setSelectedRow(null);
                            }}
                          />
                        </Panel>
                      </PanelGroup>
                    ) : (
                      renderDataTable()
                    )}
                  </Panel>
                  <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />
                  <Panel defaultSize={30} minSize={20} maxSize={50}>
                    {renderTransactionPreview()}
                  </Panel>
                </PanelGroup>
              ) : (
                showRowViewer && selectedRow ? (
                  <PanelGroup direction="horizontal" className="flex-1">
                    <Panel defaultSize={65} minSize={40}>
                      {renderDataTable()}
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
                  <div className="flex-1 overflow-hidden">
                    {renderDataTable()}
                  </div>
                )
              )}

              {/* Floating Bulk Action Bar */}
              {editor.selectedRows.size > 0 && (
                <div className="relative shrink-0">
                  <div className="flex items-center justify-center px-4 py-2">
                    <div className="flex items-center gap-3 px-4 py-2 bg-background border border-border rounded-lg shadow-lg">
                      <span className="text-sm font-medium text-foreground">
                        {editor.selectedRows.size} {editor.selectedRows.size === 1 ? 'row' : 'rows'} selected
                      </span>
                      <div className="h-4 w-px bg-border" />
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={copySelectedRowsAsJson}
                      >
                        <Copy className="h-3 w-3 mr-1" />
                        Copy JSON
                      </Button>
                      <Button
                        variant="destructive"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={handleBulkDeleteFromBar}
                        disabled={isDeleting}
                      >
                        <Trash2 className="h-3 w-3 mr-1" />
                        Delete
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={() => editor.clearSelection()}
                      >
                        <XCircle className="h-3 w-3 mr-1" />
                        Deselect All
                      </Button>
                    </div>
                  </div>
                </div>
              )}

              {/* Pagination Controls - shares footer with TabsList */}
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
              <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
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
                <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
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

        {/* Footer: Tabs + Pagination in one bar */}
        <div className="flex items-center justify-between px-4 py-1.5 border-t bg-background shrink-0">
          <TabsList>
            <TabsTrigger value="data">Data</TabsTrigger>
            <TabsTrigger value="columns">
              Columns ({tableSchema.columns.length})
            </TabsTrigger>
            <TabsTrigger value="indexes">
              Indexes ({tableSchema.indexes.length})
            </TabsTrigger>
          </TabsList>

          {/* Pagination (only visible on Data tab) */}
          {activeTab === "data" && sampleData && (
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">
                {totalRows !== null ? `${totalRows.toLocaleString()} rows` : `${sampleData.rows.length} rows`}
              </span>
              <span className="text-muted-foreground">|</span>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handlePreviousPage}
                disabled={currentPage === 1 || loadingSampleData}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <div className="flex items-center gap-1 text-xs">
                <input
                  type="number"
                  value={currentPage}
                  onChange={(e) => {
                    const page = parseInt(e.target.value);
                    if (!isNaN(page) && page >= 1) {
                      if (totalPages && page <= totalPages) {
                        setCurrentPage(page);
                      } else if (!totalPages) {
                        setCurrentPage(page);
                      }
                    }
                  }}
                  className="w-12 h-7 text-center bg-muted border border-border rounded text-xs select-text [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none focus:outline-none focus:ring-1 focus:ring-ring"
                  min={1}
                  max={totalPages || undefined}
                />
                <span className="text-muted-foreground">of {totalPages ?? "?"}</span>
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleNextPage}
                disabled={totalPages === null || currentPage >= totalPages || loadingSampleData}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
              <span className="text-xs text-muted-foreground border-l border-border pl-2 ml-1">
                {pageSize} / page
              </span>
            </div>
          )}
        </div>
      </Tabs>

      {/* Bulk Delete Confirmation Dialog */}
      <AlertDialog open={showDeleteConfirm} onOpenChange={setShowDeleteConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {editor.selectedRows.size} {editor.selectedRows.size === 1 ? 'Row' : 'Rows'}?</AlertDialogTitle>
            <AlertDialogDescription>
              This action cannot be undone. This will permanently delete the selected {editor.selectedRows.size === 1 ? 'row' : 'rows'} from the table.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleBulkDelete}
              disabled={isDeleting}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? 'Deleting...' : 'Delete'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
