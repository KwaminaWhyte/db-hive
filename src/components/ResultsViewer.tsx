import { FC, useMemo, useState, useCallback } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  flexRender,
  ColumnDef,
  SortingState,
} from "@tanstack/react-table";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { Alert, AlertDescription } from "./ui/alert";
import { Button } from "./ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui/tabs";
import {
  Loader2,
  AlertCircle,
  CheckCircle2,
  Table as TableIcon,
  FileText,
  FileJson,
  Code,
  FileCode,
  Copy,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";

interface ResultsViewerProps {
  /** Column names */
  columns: string[];

  /** Row data (array of arrays) */
  rows: any[][];

  /** Number of rows affected by DML statements */
  rowsAffected: number | null;

  /** Loading state */
  loading: boolean;

  /** Error message */
  error: string | null;

  /** Execution time in milliseconds */
  executionTime?: number;
}

export const ResultsViewer: FC<ResultsViewerProps> = ({
  columns,
  rows,
  rowsAffected,
  loading,
  error,
  executionTime,
}) => {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [exporting, setExporting] = useState(false);
  const [viewMode, setViewMode] = useState<"grid" | "json" | "raw">("grid");

  console.log("ResultsViewer rendered with", rows.length, "rows");

  // Copy helper functions
  const copyToClipboard = useCallback(async (text: string, message: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(message);
    } catch (err) {
      toast.error("Failed to copy to clipboard");
    }
  }, []);

  const copyCellValue = useCallback(async (value: any) => {
    console.log("copyCellValue called with:", value);
    const text = value === null ? 'NULL' : String(value);
    await copyToClipboard(text, "Cell value copied to clipboard");
  }, [copyToClipboard]);

  const copyRowValues = useCallback(async (rowIndex: number) => {
    const row = rows[rowIndex];
    const text = row.map((v) => (v === null ? 'NULL' : String(v))).join('\t');
    await copyToClipboard(text, `Row ${rowIndex + 1} copied to clipboard`);
  }, [rows, copyToClipboard]);

  const copyColumnValues = useCallback(async (columnIndex: number) => {
    const columnValues = rows.map((row) => {
      const value = row[columnIndex];
      return value === null ? 'NULL' : String(value);
    });
    const text = columnValues.join('\n');
    await copyToClipboard(text, `Column "${columns[columnIndex]}" copied to clipboard`);
  }, [rows, columns, copyToClipboard]);

  // Handle CSV export
  const handleExportCSV = async () => {
    try {
      setExporting(true);
      const filePath = await save({
        defaultPath: "query_results.csv",
        filters: [
          {
            name: "CSV",
            extensions: ["csv"],
          },
        ],
      });

      if (filePath) {
        await invoke("export_to_csv", {
          filePath,
          columns,
          rows,
        });
        console.log("Exported to CSV:", filePath);
      }
    } catch (err) {
      console.error("Failed to export CSV:", err);
      toast.error(`Failed to export CSV: ${err}`);
    } finally {
      setExporting(false);
    }
  };

  // Handle JSON export
  const handleExportJSON = async () => {
    try {
      setExporting(true);
      const filePath = await save({
        defaultPath: "query_results.json",
        filters: [
          {
            name: "JSON",
            extensions: ["json"],
          },
        ],
      });

      if (filePath) {
        await invoke("export_to_json", {
          filePath,
          columns,
          rows,
        });
        console.log("Exported to JSON:", filePath);
      }
    } catch (err) {
      console.error("Failed to export JSON:", err);
      toast.error(`Failed to export JSON: ${err}`);
    } finally {
      setExporting(false);
    }
  };

  // Create column definitions from column names
  const columnDefs = useMemo<ColumnDef<any[]>[]>(() => {
    if (!columns.length) return [];

    return columns.map((colName, index) => ({
      id: colName,
      accessorFn: (row) => row[index],
      header: () => (
        <div className="flex items-center justify-between group">
          <span>{colName}</span>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
            onClick={(e) => {
              console.log("Column copy button clicked for:", colName);
              e.stopPropagation();
              copyColumnValues(index);
            }}
            title={`Copy column "${colName}"`}
          >
            <Copy className="h-3 w-3" />
          </Button>
        </div>
      ),
      cell: (info) => {
        const value = info.getValue();
        let displayValue: React.ReactNode;
        let cellString: string;
        let isTruncated = false;

        if (value === null) {
          cellString = 'NULL';
          displayValue = <span className="text-muted-foreground italic">NULL</span>;
        } else if (value === undefined) {
          cellString = 'undefined';
          displayValue = <span className="text-muted-foreground italic">undefined</span>;
        } else if (typeof value === "object") {
          cellString = JSON.stringify(value);
          isTruncated = cellString.length > 100;
          const displayString = isTruncated ? cellString.substring(0, 100) + '...' : cellString;
          displayValue = <span className="text-xs font-mono truncate">{displayString}</span>;
        } else {
          cellString = String(value);
          isTruncated = cellString.length > 100;
          const displayString = isTruncated ? cellString.substring(0, 100) + '...' : cellString;
          displayValue = <span className="truncate">{displayString}</span>;
        }

        const tooltipText = isTruncated
          ? `${cellString}\n\nClick to copy cell value`
          : "Click to copy cell value";

        return (
          <div
            className="cursor-pointer hover:bg-muted/50 -mx-4 -my-2 px-4 py-2 transition-colors"
            onClick={() => {
              console.log("Cell clicked, value:", value);
              copyCellValue(value);
            }}
            title={tooltipText}
          >
            <div className="truncate max-w-md">
              {displayValue}
            </div>
          </div>
        );
      },
    }));
  }, [columns, rows, copyCellValue, copyColumnValues]);

  const table = useReactTable({
    data: rows,
    columns: columnDefs,
    state: {
      sorting,
    },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  // Convert results to JSON format
  const resultsAsJSON = useMemo(() => {
    if (!columns.length || !rows.length) return "[]";

    const jsonRows = rows.map((row) => {
      const obj: Record<string, any> = {};
      columns.forEach((col, idx) => {
        obj[col] = row[idx];
      });
      return obj;
    });

    return JSON.stringify(jsonRows, null, 2);
  }, [columns, rows]);

  // Convert results to raw text format
  const resultsAsRaw = useMemo(() => {
    if (!columns.length) return "";

    // Create header row
    const header = columns.join("\t");

    // Create data rows
    const dataRows = rows.map((row) =>
      row
        .map((cell) => {
          if (cell === null) return "NULL";
          if (cell === undefined) return "undefined";
          if (typeof cell === "object") return JSON.stringify(cell);
          return String(cell);
        })
        .join("\t")
    );

    return [header, ...dataRows].join("\n");
  }, [columns, rows]);

  // Syntax highlight JSON
  const highlightJSON = (json: string) => {
    // First escape HTML to prevent XSS
    const escapeHtml = (str: string) => {
      return str
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
    };

    const escaped = escapeHtml(json);

    // Replace with spans for different token types
    return escaped
      // Numbers (must come before strings to avoid matching inside classes)
      .replace(/:\s*(-?\d+\.?\d*)/g, (_match, p1) => {
        return `: <span style="color: #fb923c">${p1}</span>`;
      })
      // Property keys (followed by colon)
      .replace(/"([^"]+)"(\s*):/g, (_match, p1, p2) => {
        return `<span style="color: #60a5fa">"${p1}"</span>${p2}:`;
      })
      // String values
      .replace(/:\s*"([^"]*)"/g, (_match, p1) => {
        return `: <span style="color: #4ade80">"${p1}"</span>`;
      })
      // Booleans
      .replace(/:\s*(true|false)/g, (_match, p1) => {
        return `: <span style="color: #c084fc">${p1}</span>`;
      })
      // Null
      .replace(/:\s*(null)/g, (_match, p1) => {
        return `: <span style="color: #f87171">${p1}</span>`;
      });
  };

  // Render loading state
  if (loading) {
    return (
      <Card className="h-full flex flex-col">
        <CardContent className="flex-1 flex items-center justify-center">
          <div className="flex flex-col items-center gap-3 text-muted-foreground">
            <Loader2 className="h-8 w-8 animate-spin" />
            <p>Executing query...</p>
          </div>
        </CardContent>
      </Card>
    );
  }

  // Render error state
  if (error) {
    return (
      <Card className="h-full flex flex-col">
        <CardContent className="flex-1 p-6">
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription className="ml-2">
              <div className="font-semibold mb-1">Query Error</div>
              <div className="text-sm font-mono whitespace-pre-wrap">
                {error}
              </div>
            </AlertDescription>
          </Alert>
        </CardContent>
      </Card>
    );
  }

  // Render empty state
  if (!columns.length && rowsAffected === null) {
    return (
      <Card className="h-full flex flex-col">
        <CardContent className="flex-1 flex items-center justify-center">
          <div className="flex flex-col items-center gap-3 text-muted-foreground">
            <TableIcon className="h-12 w-12 opacity-50" />
            <p>Execute a query to see results</p>
          </div>
        </CardContent>
      </Card>
    );
  }

  // Render DML result (rows affected)
  if (rowsAffected !== null) {
    return (
      <Card className="h-full flex flex-col">
        <CardContent className="flex-1 p-6">
          <Alert>
            <CheckCircle2 className="h-4 w-4 text-green-600" />
            <AlertDescription className="ml-2">
              <div className="flex items-center justify-between">
                <div>
                  <div className="font-semibold">
                    Query executed successfully
                  </div>
                  <div className="text-sm text-muted-foreground mt-1">
                    {rowsAffected} row{rowsAffected !== 1 ? "s" : ""} affected
                  </div>
                </div>
                {executionTime !== undefined && (
                  <div className="text-sm text-muted-foreground">
                    {executionTime < 1000
                      ? `${executionTime.toFixed(0)}ms`
                      : `${(executionTime / 1000).toFixed(2)}s`}
                  </div>
                )}
              </div>
            </AlertDescription>
          </Alert>
        </CardContent>
      </Card>
    );
  }

  // Render table results
  return (
    <Card className="h-full flex flex-col gap-0">
      <CardHeader className="border-b pb-0">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base font-medium">
            Results ({rows.length} row{rows.length !== 1 ? "s" : ""})
          </CardTitle>
          <div className="flex items-center gap-2">
            {executionTime !== undefined && (
              <div className="text-sm text-muted-foreground">
                {executionTime < 1000
                  ? `${executionTime.toFixed(0)}ms`
                  : `${(executionTime / 1000).toFixed(2)}s`}
              </div>
            )}
            {rows.length > 0 && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleExportCSV}
                  disabled={exporting}
                  className="gap-1"
                >
                  <FileText className="h-4 w-4" />
                  CSV
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleExportJSON}
                  disabled={exporting}
                  className="gap-1"
                >
                  <FileJson className="h-4 w-4" />
                  JSON
                </Button>
              </>
            )}
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex-1 p-0 overflow-hidden">
        <Tabs
          value={viewMode}
          onValueChange={(value) =>
            setViewMode(value as "grid" | "json" | "raw")
          }
          className="h-full flex flex-col"
        >
          <div className="border-b px-4 shrink-0">
            <TabsList className="h-10">
              <TabsTrigger value="grid" className="gap-1.5">
                <TableIcon className="h-3.5 w-3.5" />
                Grid
              </TabsTrigger>
              <TabsTrigger value="json" className="gap-1.5">
                <Code className="h-3.5 w-3.5" />
                JSON
              </TabsTrigger>
              <TabsTrigger value="raw" className="gap-1.5">
                <FileCode className="h-3.5 w-3.5" />
                Raw
              </TabsTrigger>
            </TabsList>
          </div>

          {/* Grid View */}
          <TabsContent value="grid" className="flex-1 m-0 overflow-auto">
            <div className="relative h-full">
              <table className="w-full border-collapse text-sm">
                <thead className="sticky top-0 bg-muted/50 backdrop-blur-sm z-10">
                  {table.getHeaderGroups().map((headerGroup) => (
                    <tr key={headerGroup.id} className="border-b">
                      <th className="text-left font-semibold px-2 py-3 border-r text-xs text-muted-foreground sticky left-0 bg-muted/50 backdrop-blur-sm">
                        #
                      </th>
                      {headerGroup.headers.map((header) => (
                        <th
                          key={header.id}
                          className={cn(
                            "text-left font-semibold px-4 py-3 border-r last:border-r-0",
                            header.column.getCanSort() &&
                              "cursor-pointer select-none"
                          )}
                          onClick={header.column.getToggleSortingHandler()}
                        >
                          <div className="flex items-center gap-2">
                            {flexRender(
                              header.column.columnDef.header,
                              header.getContext()
                            )}
                            {header.column.getIsSorted() && (
                              <span className="text-xs">
                                {header.column.getIsSorted() === "asc"
                                  ? "↑"
                                  : "↓"}
                              </span>
                            )}
                          </div>
                        </th>
                      ))}
                    </tr>
                  ))}
                </thead>
                <tbody>
                  {table.getRowModel().rows.map((row, rowIndex) => (
                    <tr
                      key={row.id}
                      className={cn(
                        "border-b hover:bg-muted/30 transition-colors group",
                        rowIndex % 2 === 0 ? "bg-background" : "bg-muted/10"
                      )}
                    >
                      <td className="px-2 py-2 border-r text-xs text-muted-foreground sticky left-0 bg-inherit">
                        <div className="flex items-center gap-1">
                          <span className="w-8 text-right">{rowIndex + 1}</span>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                            onClick={() => {
                              console.log("Row copy button clicked for row:", rowIndex);
                              copyRowValues(rowIndex);
                            }}
                            title={`Copy row ${rowIndex + 1}`}
                          >
                            <Copy className="h-3 w-3" />
                          </Button>
                        </div>
                      </td>
                      {row.getVisibleCells().map((cell) => (
                        <td
                          key={cell.id}
                          className="px-4 py-2 border-r last:border-r-0 max-w-md"
                        >
                          {flexRender(
                            cell.column.columnDef.cell,
                            cell.getContext()
                          )}
                        </td>
                      ))}
                    </tr>
                  ))}
                  {rows.length === 0 && (
                    <tr>
                      <td colSpan={columns.length + 1} className="text-center py-12">
                        <div className="flex flex-col items-center gap-3 text-muted-foreground">
                          <TableIcon className="h-12 w-12 opacity-50" />
                          <div>
                            <p className="font-medium">No results returned</p>
                            <p className="text-sm">The query executed successfully but returned no rows</p>
                          </div>
                        </div>
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </TabsContent>

          {/* JSON View */}
          <TabsContent value="json" className="flex-1 m-0 overflow-auto">
            <pre className="p-4 text-xs font-mono">
              <code
                dangerouslySetInnerHTML={{ __html: highlightJSON(resultsAsJSON) }}
              />
            </pre>
          </TabsContent>

          {/* Raw View */}
          <TabsContent value="raw" className="flex-1 m-0 overflow-auto">
            <pre className="p-4 text-xs font-mono whitespace-pre">
              <code>{resultsAsRaw}</code>
            </pre>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
};
