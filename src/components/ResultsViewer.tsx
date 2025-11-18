import { FC, useMemo } from 'react';
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  flexRender,
  ColumnDef,
  SortingState,
} from '@tanstack/react-table';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Alert, AlertDescription } from './ui/alert';
import { Loader2, AlertCircle, CheckCircle2, Table as TableIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useState } from 'react';

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

  // Create column definitions from column names
  const columnDefs = useMemo<ColumnDef<any[]>[]>(() => {
    if (!columns.length) return [];

    return columns.map((colName, index) => ({
      id: colName,
      accessorFn: (row) => row[index],
      header: colName,
      cell: (info) => {
        const value = info.getValue();
        if (value === null) {
          return <span className="text-muted-foreground italic">NULL</span>;
        }
        if (value === undefined) {
          return <span className="text-muted-foreground italic">undefined</span>;
        }
        if (typeof value === 'object') {
          return (
            <span className="text-xs font-mono">
              {JSON.stringify(value)}
            </span>
          );
        }
        return <span className="truncate">{String(value)}</span>;
      },
    }));
  }, [columns]);

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
                  <div className="font-semibold">Query executed successfully</div>
                  <div className="text-sm text-muted-foreground mt-1">
                    {rowsAffected} row{rowsAffected !== 1 ? 's' : ''} affected
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
    <Card className="h-full flex flex-col">
      <CardHeader className="pb-3 border-b">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base font-medium">
            Results ({rows.length} row{rows.length !== 1 ? 's' : ''})
          </CardTitle>
          {executionTime !== undefined && (
            <div className="text-sm text-muted-foreground">
              {executionTime < 1000
                ? `${executionTime.toFixed(0)}ms`
                : `${(executionTime / 1000).toFixed(2)}s`}
            </div>
          )}
        </div>
      </CardHeader>

      <CardContent className="flex-1 p-0 overflow-auto">
        <div className="relative h-full">
          <table className="w-full border-collapse text-sm">
            <thead className="sticky top-0 bg-muted/50 backdrop-blur-sm z-10">
              {table.getHeaderGroups().map((headerGroup) => (
                <tr key={headerGroup.id} className="border-b">
                  {headerGroup.headers.map((header) => (
                    <th
                      key={header.id}
                      className={cn(
                        'text-left font-semibold px-4 py-3 border-r last:border-r-0',
                        header.column.getCanSort() && 'cursor-pointer select-none'
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
                            {header.column.getIsSorted() === 'asc' ? '↑' : '↓'}
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
                    'border-b hover:bg-muted/30 transition-colors',
                    rowIndex % 2 === 0 ? 'bg-background' : 'bg-muted/10'
                  )}
                >
                  {row.getVisibleCells().map((cell) => (
                    <td
                      key={cell.id}
                      className="px-4 py-2 border-r last:border-r-0 max-w-md"
                    >
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  );
};
