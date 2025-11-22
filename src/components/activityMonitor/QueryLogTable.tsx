import { FC, useMemo, useState } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  flexRender,
  ColumnDef,
  SortingState,
} from "@tanstack/react-table";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  Database,
  ChevronDown,
  ChevronUp,
  CheckCircle2,
  XCircle,
  Clock,
  Ban,
  Activity,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { QueryLog, QueryLogSort, QueryType, QueryStatus } from "@/types/activity";

interface QueryLogTableProps {
  logs: QueryLog[];
  total: number;
  page: number;
  pageSize: number;
  sort: QueryLogSort;
  loading: boolean;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
  onSortChange: (sort: QueryLogSort) => void;
}

/**
 * Get badge variant and icon for query type
 */
const getQueryTypeStyle = (type: QueryType) => {
  switch (type) {
    case "SELECT":
      return {
        variant: "default" as const,
        className: "bg-blue-500/10 text-blue-600 border-blue-500/20",
      };
    case "INSERT":
      return {
        variant: "default" as const,
        className: "bg-green-500/10 text-green-600 border-green-500/20",
      };
    case "UPDATE":
      return {
        variant: "default" as const,
        className: "bg-yellow-500/10 text-yellow-600 border-yellow-500/20",
      };
    case "DELETE":
      return {
        variant: "default" as const,
        className: "bg-red-500/10 text-red-600 border-red-500/20",
      };
    case "CREATE":
    case "ALTER":
    case "DROP":
      return {
        variant: "default" as const,
        className: "bg-purple-500/10 text-purple-600 border-purple-500/20",
      };
    case "TRANSACTION":
      return {
        variant: "default" as const,
        className: "bg-indigo-500/10 text-indigo-600 border-indigo-500/20",
      };
    default:
      return {
        variant: "outline" as const,
        className: "",
      };
  }
};

/**
 * Get badge variant and icon for query status
 */
const getQueryStatusStyle = (status: QueryStatus) => {
  switch (status) {
    case "completed":
      return {
        variant: "default" as const,
        className: "bg-green-500/10 text-green-600 border-green-500/20",
        icon: CheckCircle2,
      };
    case "failed":
      return {
        variant: "destructive" as const,
        className: "",
        icon: XCircle,
      };
    case "running":
      return {
        variant: "default" as const,
        className: "bg-yellow-500/10 text-yellow-600 border-yellow-500/20",
        icon: Clock,
      };
    case "cancelled":
      return {
        variant: "secondary" as const,
        className: "",
        icon: Ban,
      };
  }
};

/**
 * Format timestamp to readable format
 */
const formatTimestamp = (timestamp: string): string => {
  const date = new Date(timestamp);
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

  const month = months[date.getMonth()];
  const day = date.getDate().toString().padStart(2, "0");
  const hours = date.getHours().toString().padStart(2, "0");
  const minutes = date.getMinutes().toString().padStart(2, "0");
  const seconds = date.getSeconds().toString().padStart(2, "0");

  return `${month} ${day}, ${hours}:${minutes}:${seconds}`;
};

/**
 * Format duration in milliseconds to readable format
 */
const formatDuration = (durationMs?: number): string => {
  if (durationMs === undefined || durationMs === null) return "-";

  if (durationMs < 1000) {
    return `${durationMs.toFixed(0)}ms`;
  }

  return `${(durationMs / 1000).toFixed(2)}s`;
};

/**
 * Truncate text with ellipsis
 */
const truncateText = (text: string, maxLength: number): string => {
  if (text.length <= maxLength) return text;
  return text.substring(0, maxLength) + "...";
};

export const QueryLogTable: FC<QueryLogTableProps> = ({
  logs,
  total,
  page,
  pageSize,
  sort,
  loading,
  onPageChange,
  onPageSizeChange,
  onSortChange,
}) => {
  const [expandedRows, setExpandedRows] = useState<Set<string>>(new Set());
  const [sorting, setSorting] = useState<SortingState>([
    {
      id: sort.field,
      desc: sort.direction === "desc",
    },
  ]);

  // Toggle row expansion
  const toggleRowExpansion = (id: string) => {
    setExpandedRows((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  // Handle sorting change
  const handleSortingChange = (updaterOrValue: SortingState | ((old: SortingState) => SortingState)) => {
    const newSorting = typeof updaterOrValue === 'function' ? updaterOrValue(sorting) : updaterOrValue;
    setSorting(newSorting);

    if (newSorting.length > 0) {
      const sortField = newSorting[0].id as QueryLogSort["field"];
      const sortDirection = newSorting[0].desc ? "desc" : "asc";

      onSortChange({
        field: sortField,
        direction: sortDirection,
      });
    }
  };

  // Column definitions
  const columns = useMemo<ColumnDef<QueryLog>[]>(
    () => [
      {
        id: "expand",
        header: "",
        cell: ({ row }) => {
          const isExpanded = expandedRows.has(row.original.id);
          return (
            <Button
              variant="ghost"
              size="sm"
              className="h-6 w-6 p-0"
              onClick={() => toggleRowExpansion(row.original.id)}
            >
              {isExpanded ? (
                <ChevronUp className="h-4 w-4" />
              ) : (
                <ChevronDown className="h-4 w-4" />
              )}
            </Button>
          );
        },
      },
      {
        id: "startedAt",
        accessorKey: "startedAt",
        header: "Timestamp",
        cell: ({ row }) => (
          <span className="text-sm font-mono">
            {formatTimestamp(row.original.startedAt)}
          </span>
        ),
      },
      {
        id: "connectionName",
        accessorKey: "connectionName",
        header: "Connection",
        cell: ({ row }) => (
          <span className="text-sm truncate max-w-[150px] inline-block" title={row.original.connectionName}>
            {truncateText(row.original.connectionName, 20)}
          </span>
        ),
      },
      {
        id: "database",
        accessorKey: "database",
        header: "Database",
        cell: ({ row }) => (
          <span className="text-sm text-muted-foreground">
            {row.original.database || "-"}
          </span>
        ),
      },
      {
        id: "query",
        accessorKey: "sql",
        header: "Query",
        enableSorting: false,
        cell: ({ row }) => (
          <div className="max-w-[300px]">
            <code
              className="text-xs font-mono text-muted-foreground bg-muted px-2 py-1 rounded inline-block truncate max-w-full"
              title={row.original.sql}
            >
              {truncateText(row.original.sql, 50)}
            </code>
          </div>
        ),
      },
      {
        id: "queryType",
        accessorKey: "queryType",
        header: "Type",
        enableSorting: false,
        cell: ({ row }) => {
          const style = getQueryTypeStyle(row.original.queryType);
          return (
            <Badge variant={style.variant} className={style.className}>
              {row.original.queryType}
            </Badge>
          );
        },
      },
      {
        id: "status",
        accessorKey: "status",
        header: "Status",
        enableSorting: false,
        cell: ({ row }) => {
          const style = getQueryStatusStyle(row.original.status);
          const Icon = style.icon;
          return (
            <Badge variant={style.variant} className={style.className}>
              <Icon className="h-3 w-3" />
              {row.original.status}
            </Badge>
          );
        },
      },
      {
        id: "durationMs",
        accessorKey: "durationMs",
        header: "Duration",
        cell: ({ row }) => (
          <span className="text-sm font-mono text-right inline-block w-full">
            {formatDuration(row.original.durationMs)}
          </span>
        ),
      },
      {
        id: "rowCount",
        accessorKey: "rowCount",
        header: "Rows",
        cell: ({ row }) => (
          <span className="text-sm font-mono text-right inline-block w-full">
            {row.original.rowCount !== undefined && row.original.rowCount !== null
              ? row.original.rowCount.toLocaleString()
              : "-"}
          </span>
        ),
      },
    ],
    [expandedRows]
  );

  const table = useReactTable({
    data: logs,
    columns,
    state: {
      sorting,
    },
    onSortingChange: handleSortingChange,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    manualSorting: true,
  });

  const totalPages = Math.ceil(total / pageSize);
  const startRow = page * pageSize + 1;
  const endRow = Math.min((page + 1) * pageSize, total);

  // Render loading state
  if (loading && logs.length === 0) {
    return (
      <Card>
        <CardContent className="p-6">
          <div className="space-y-3">
            {Array.from({ length: 5 }).map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </div>
        </CardContent>
      </Card>
    );
  }

  // Render empty state
  if (!loading && logs.length === 0) {
    return (
      <Card>
        <CardContent className="p-12">
          <div className="flex flex-col items-center justify-center gap-3 text-muted-foreground">
            <Activity className="h-12 w-12 opacity-50" />
            <div className="text-center">
              <p className="font-medium text-lg">No query logs found</p>
              <p className="text-sm mt-1">
                Query execution logs will appear here
              </p>
            </div>
          </div>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      <Card>
        <div className="relative">
          <Table>
            <TableHeader>
              {table.getHeaderGroups().map((headerGroup) => (
                <TableRow key={headerGroup.id}>
                  {headerGroup.headers.map((header) => (
                    <TableHead
                      key={header.id}
                      className={cn(
                        header.column.getCanSort() && "cursor-pointer select-none",
                        header.id === "expand" && "w-10",
                        header.id === "durationMs" && "text-right",
                        header.id === "rowCount" && "text-right"
                      )}
                      onClick={header.column.getToggleSortingHandler()}
                    >
                      {header.isPlaceholder ? null : (
                        <div className="flex items-center gap-2">
                          {flexRender(
                            header.column.columnDef.header,
                            header.getContext()
                          )}
                          {header.column.getIsSorted() && (
                            <span className="text-xs">
                              {header.column.getIsSorted() === "asc" ? "↑" : "↓"}
                            </span>
                          )}
                        </div>
                      )}
                    </TableHead>
                  ))}
                </TableRow>
              ))}
            </TableHeader>
            <TableBody>
              {table.getRowModel().rows.map((row) => {
                const isExpanded = expandedRows.has(row.original.id);
                return (
                  <>
                    <TableRow key={row.id}>
                      {row.getVisibleCells().map((cell) => (
                        <TableCell key={cell.id}>
                          {flexRender(
                            cell.column.columnDef.cell,
                            cell.getContext()
                          )}
                        </TableCell>
                      ))}
                    </TableRow>
                    {isExpanded && (
                      <TableRow key={`${row.id}-expanded`}>
                        <TableCell colSpan={columns.length} className="bg-muted/30 p-4">
                          <div className="space-y-2">
                            <div className="flex items-center gap-2 text-sm font-medium">
                              <Database className="h-4 w-4" />
                              Full Query
                            </div>
                            <pre className="bg-background border rounded-md p-4 text-xs font-mono overflow-x-auto">
                              <code>{row.original.sql}</code>
                            </pre>
                            {row.original.error && (
                              <div className="mt-3">
                                <div className="flex items-center gap-2 text-sm font-medium text-destructive mb-2">
                                  <XCircle className="h-4 w-4" />
                                  Error
                                </div>
                                <pre className="bg-destructive/10 border border-destructive/20 rounded-md p-4 text-xs font-mono overflow-x-auto text-destructive">
                                  <code>{row.original.error}</code>
                                </pre>
                              </div>
                            )}
                            {row.original.tags && row.original.tags.length > 0 && (
                              <div className="mt-3">
                                <div className="text-sm font-medium mb-2">Tags</div>
                                <div className="flex flex-wrap gap-2">
                                  {row.original.tags.map((tag) => (
                                    <Badge key={tag} variant="outline">
                                      {tag}
                                    </Badge>
                                  ))}
                                </div>
                              </div>
                            )}
                          </div>
                        </TableCell>
                      </TableRow>
                    )}
                  </>
                );
              })}
            </TableBody>
          </Table>
          {loading && (
            <div className="absolute inset-0 bg-background/50 backdrop-blur-sm flex items-center justify-center">
              <div className="flex items-center gap-2 text-muted-foreground">
                <Clock className="h-5 w-5 animate-spin" />
                <span className="text-sm">Loading...</span>
              </div>
            </div>
          )}
        </div>
      </Card>

      {/* Pagination Controls */}
      <Card>
        <CardContent className="p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className="text-sm text-muted-foreground">
                Showing {startRow} to {endRow} of {total} queries
              </div>
              <div className="flex items-center gap-2">
                <span className="text-sm text-muted-foreground">Rows per page:</span>
                <select
                  value={pageSize}
                  onChange={(e) => onPageSizeChange(Number(e.target.value))}
                  className="h-8 w-20 rounded-md border border-input bg-background px-2 text-sm"
                >
                  <option value="10">10</option>
                  <option value="25">25</option>
                  <option value="50">50</option>
                  <option value="100">100</option>
                </select>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => onPageChange(0)}
                disabled={page === 0}
              >
                <ChevronsLeft className="h-4 w-4" />
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => onPageChange(page - 1)}
                disabled={page === 0}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <div className="flex items-center gap-2 px-2">
                <span className="text-sm text-muted-foreground">
                  Page {page + 1} of {totalPages}
                </span>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => onPageChange(page + 1)}
                disabled={page >= totalPages - 1}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => onPageChange(totalPages - 1)}
                disabled={page >= totalPages - 1}
              >
                <ChevronsRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
};
