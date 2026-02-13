import { FC, useMemo } from 'react';
import { Button } from './ui/button';
import { ScrollArea } from './ui/scroll-area';
import { Tabs, TabsList, TabsTrigger, TabsContent } from './ui/tabs';
import { X, AlertCircle, Eye, Code } from 'lucide-react';
import { Alert, AlertDescription } from './ui/alert';

interface TransactionPreviewProps {
  /** SQL statements to preview */
  statements: string[];

  /** Whether statements are being executed */
  isExecuting: boolean;

  /** Callback to execute statements */
  onCommit: () => void;

  /** Callback to discard changes */
  onDiscard: () => void;

  /** Callback to close panel */
  onClose: () => void;

  /** Error message if any */
  error?: string | null;

  /** Total number of cell changes */
  totalChanges?: number;

  /** Number of modified rows */
  modifiedRows?: number;

  /** Map of rowIndex -> { changes: Map<columnName, { oldValue, newValue }> } */
  changes?: Map<number, { changes: Map<string, { oldValue: any; newValue: any }> }>;

  /** Map of tempId -> { tempId: number, values: Map<string, any> } */
  newRows?: Map<number, { tempId: number; values: Map<string, any> }>;

  /** Column schema information */
  columns?: Array<{
    name: string;
    dataType: string;
    isPrimaryKey: boolean;
    nullable: boolean;
    defaultValue: string | null;
    isAutoIncrement: boolean;
  }>;

  /** Current page data rows */
  rows?: any[][];

  /** Table name */
  tableName?: string;

  /** Schema name */
  schemaName?: string;
}

/**
 * Apply basic SQL syntax highlighting using inline styles
 */
function highlightSQL(sql: string): React.ReactNode {
  const keywords = /\b(UPDATE|SET|WHERE|AND|OR|INSERT|INTO|VALUES|DELETE|FROM|SELECT|BEGIN|COMMIT|ROLLBACK|IS|NULL|NOT)\b/gi;
  const strings = /'[^']*'/g;
  const numbers = /\b\d+(\.\d+)?\b/g;
  const operators = /(=|!=|<>|<=|>=|<|>)/g;

  // Create a list of all matches with their positions
  interface Match {
    start: number;
    end: number;
    type: 'keyword' | 'string' | 'number' | 'operator';
    value: string;
  }

  const matches: Match[] = [];

  // Find all keywords
  let match;
  while ((match = keywords.exec(sql)) !== null) {
    matches.push({
      start: match.index,
      end: match.index + match[0].length,
      type: 'keyword',
      value: match[0],
    });
  }

  // Find all strings
  strings.lastIndex = 0;
  while ((match = strings.exec(sql)) !== null) {
    matches.push({
      start: match.index,
      end: match.index + match[0].length,
      type: 'string',
      value: match[0],
    });
  }

  // Find all numbers
  numbers.lastIndex = 0;
  while ((match = numbers.exec(sql)) !== null) {
    matches.push({
      start: match.index,
      end: match.index + match[0].length,
      type: 'number',
      value: match[0],
    });
  }

  // Find all operators
  operators.lastIndex = 0;
  while ((match = operators.exec(sql)) !== null) {
    matches.push({
      start: match.index,
      end: match.index + match[0].length,
      type: 'operator',
      value: match[0],
    });
  }

  // Sort matches by position
  matches.sort((a, b) => a.start - b.start);

  // Remove overlapping matches (keep first match)
  const filteredMatches: Match[] = [];
  let lastEnd = -1;
  for (const m of matches) {
    if (m.start >= lastEnd) {
      filteredMatches.push(m);
      lastEnd = m.end;
    }
  }

  // Build highlighted output
  const result: React.ReactNode[] = [];
  let currentPos = 0;

  filteredMatches.forEach((m, idx) => {
    // Add text before match
    if (m.start > currentPos) {
      result.push(<span key={`text-${idx}`}>{sql.substring(currentPos, m.start)}</span>);
    }

    // Add highlighted match
    const style =
      m.type === 'keyword'
        ? { color: '#569CD6', fontWeight: 600 }
        : m.type === 'string'
        ? { color: '#CE9178' }
        : m.type === 'number'
        ? { color: '#B5CEA8' }
        : { color: '#D4D4D4' };

    result.push(
      <span key={`match-${idx}`} style={style}>
        {m.value}
      </span>
    );

    currentPos = m.end;
  });

  // Add remaining text
  if (currentPos < sql.length) {
    result.push(<span key="text-end">{sql.substring(currentPos)}</span>);
  }

  return result;
}

/** Format a cell value for display */
function formatValue(value: any): string {
  if (value === null || value === undefined) return 'NULL';
  if (typeof value === 'string') return `'${value}'`;
  return String(value);
}

interface VisualChange {
  type: 'update' | 'insert' | 'delete';
  breadcrumb: string;
  details: Array<{
    column?: string;
    oldValue?: string;
    newValue?: string;
    summary?: string;
  }>;
}

export const TransactionPreview: FC<TransactionPreviewProps> = ({
  statements,
  isExecuting,
  onCommit,
  onDiscard,
  onClose,
  error,
  totalChanges = 0,
  changes,
  newRows,
  columns,
  rows,
  tableName,
  schemaName,
}) => {
  const totalStatements = statements.length;

  // Determine whether we have rich change data for the visual tab
  const hasVisualData = !!(changes || newRows);

  // Find the primary key column index for row identification
  const pkColumnIndex = useMemo(() => {
    if (!columns) return -1;
    return columns.findIndex((col) => col.isPrimaryKey);
  }, [columns]);

  // Build a human-readable row identifier
  const getRowLabel = (rowIndex: number): string => {
    if (rows && pkColumnIndex >= 0 && rows[rowIndex]) {
      const pkCol = columns![pkColumnIndex];
      const pkValue = rows[rowIndex][pkColumnIndex];
      return `${pkCol.name} = ${formatValue(pkValue)}`;
    }
    return `row ${rowIndex + 1}`;
  };

  const qualifiedTable = useMemo(() => {
    if (schemaName && tableName) return `${schemaName}.${tableName}`;
    if (tableName) return tableName;
    return 'table';
  }, [schemaName, tableName]);

  // Compute visual diff entries from the changes and newRows maps
  const visualChanges = useMemo<VisualChange[]>(() => {
    const entries: VisualChange[] = [];

    // Process UPDATE changes
    if (changes) {
      Array.from(changes.entries()).forEach(([rowIndex, rowChange]) => {
        Array.from(rowChange.changes.entries()).forEach(([columnName, { oldValue, newValue }]) => {
          entries.push({
            type: 'update',
            breadcrumb: `${qualifiedTable} > ${getRowLabel(rowIndex)} > ${columnName}`,
            details: [
              { oldValue: formatValue(oldValue) },
              { newValue: formatValue(newValue) },
            ],
          });
        });
      });
    }

    // Process INSERT new rows
    if (newRows) {
      Array.from(newRows.entries()).forEach(([, newRow]) => {
        const valueParts: string[] = [];
        newRow.values.forEach((value, colName) => {
          valueParts.push(`${colName}: ${formatValue(value)}`);
        });

        entries.push({
          type: 'insert',
          breadcrumb: `${qualifiedTable} > new row`,
          details: [
            { summary: valueParts.length > 0 ? valueParts.join(', ') : '(empty row)' },
          ],
        });
      });
    }

    // Detect DELETE statements from SQL (no rich data available for deletes)
    statements.forEach((stmt) => {
      const upperStmt = stmt.trim().toUpperCase();
      if (upperStmt.startsWith('DELETE')) {
        entries.push({
          type: 'delete',
          breadcrumb: `${qualifiedTable} > row`,
          details: [{ summary: stmt.trim() }],
        });
      }
    });

    return entries;
  }, [changes, newRows, statements, qualifiedTable, rows, pkColumnIndex, columns]);

  return (
    <div className="h-full flex flex-col border-l bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b bg-muted/30">
        <span className="text-sm font-semibold">Pending Changes</span>
        <Button
          variant="ghost"
          size="sm"
          onClick={onClose}
          className="h-7 w-7 p-0"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      {/* Error Display */}
      {error && (
        <div className="p-3 border-b">
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription className="text-sm">{error}</AlertDescription>
          </Alert>
        </div>
      )}

      {/* Tabs: Visual / SQL */}
      {totalStatements === 0 ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center py-8 text-muted-foreground">
            <p className="text-sm">No changes to commit</p>
            <p className="text-xs mt-1">Edit cells to see changes here</p>
          </div>
        </div>
      ) : (
        <Tabs defaultValue={hasVisualData ? 'visual' : 'sql'} className="flex-1 flex flex-col min-h-0">
          <div className="px-3 pt-2 border-b">
            <TabsList className="h-8">
              <TabsTrigger value="visual" className="text-xs gap-1.5 px-3">
                <Eye className="h-3.5 w-3.5" />
                Visual
              </TabsTrigger>
              <TabsTrigger value="sql" className="text-xs gap-1.5 px-3">
                <Code className="h-3.5 w-3.5" />
                SQL
              </TabsTrigger>
            </TabsList>
          </div>

          {/* Visual Tab */}
          <TabsContent value="visual" className="flex-1 min-h-0">
            <ScrollArea className="h-full">
              <div className="p-3 space-y-3">
                {hasVisualData && visualChanges.length > 0 ? (
                  visualChanges.map((change, idx) => (
                    <div key={idx} className="space-y-1">
                      {/* Breadcrumb */}
                      <div className="text-xs text-muted-foreground font-mono px-1">
                        {change.breadcrumb.split(' > ').map((segment, segIdx, arr) => (
                          <span key={segIdx}>
                            <span className={segIdx === arr.length - 1 ? 'text-foreground font-medium' : ''}>
                              {segment}
                            </span>
                            {segIdx < arr.length - 1 && (
                              <span className="mx-1 text-muted-foreground/60">&rsaquo;</span>
                            )}
                          </span>
                        ))}
                      </div>

                      {/* Diff lines */}
                      <div className="rounded-md overflow-hidden border border-border/50">
                        {change.type === 'update' && change.details.map((detail, dIdx) => (
                          <div key={dIdx}>
                            {detail.oldValue !== undefined && (
                              <div className="bg-red-500/10 text-red-600 dark:text-red-400 border-l-2 border-red-500 px-3 py-1.5 font-mono text-xs">
                                <span className="select-none mr-2">-</span>
                                {detail.oldValue}
                              </div>
                            )}
                            {detail.newValue !== undefined && (
                              <div className="bg-green-500/10 text-green-600 dark:text-green-400 border-l-2 border-green-500 px-3 py-1.5 font-mono text-xs">
                                <span className="select-none mr-2">+</span>
                                {detail.newValue}
                              </div>
                            )}
                          </div>
                        ))}

                        {change.type === 'insert' && change.details.map((detail, dIdx) => (
                          <div key={dIdx} className="bg-green-500/10 text-green-600 dark:text-green-400 border-l-2 border-green-500 px-3 py-1.5 font-mono text-xs">
                            <span className="select-none mr-2">+</span>
                            INSERT {detail.summary}
                          </div>
                        ))}

                        {change.type === 'delete' && change.details.map((_detail, dIdx) => (
                          <div key={dIdx} className="bg-red-500/10 text-red-600 dark:text-red-400 border-l-2 border-red-500 px-3 py-1.5 font-mono text-xs">
                            <span className="select-none mr-2">-</span>
                            DELETE row
                          </div>
                        ))}
                      </div>
                    </div>
                  ))
                ) : (
                  /* Fallback: no rich data, show SQL statement summaries */
                  statements.map((stmt, idx) => {
                    const upperStmt = stmt.trim().toUpperCase();
                    const isInsert = upperStmt.startsWith('INSERT');
                    const isDelete = upperStmt.startsWith('DELETE');

                    return (
                      <div key={idx} className="space-y-1">
                        <div className="text-xs text-muted-foreground font-mono px-1">
                          {qualifiedTable} <span className="mx-1 text-muted-foreground/60">&rsaquo;</span>
                          <span className="text-foreground font-medium">statement {idx + 1}</span>
                        </div>
                        <div className="rounded-md overflow-hidden border border-border/50">
                          <div
                            className={`px-3 py-1.5 font-mono text-xs border-l-2 ${
                              isDelete
                                ? 'bg-red-500/10 text-red-600 dark:text-red-400 border-red-500'
                                : isInsert
                                ? 'bg-green-500/10 text-green-600 dark:text-green-400 border-green-500'
                                : 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500'
                            }`}
                          >
                            <span className="select-none mr-2">{isDelete ? '-' : isInsert ? '+' : '~'}</span>
                            {stmt.trim().substring(0, 120)}{stmt.trim().length > 120 ? '...' : ''}
                          </div>
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </ScrollArea>
          </TabsContent>

          {/* SQL Tab */}
          <TabsContent value="sql" className="flex-1 min-h-0">
            <ScrollArea className="h-full">
              <div className="p-3 space-y-2">
                {statements.map((stmt, idx) => (
                  <pre
                    key={idx}
                    className="text-xs font-mono bg-muted/50 p-3 rounded-md border overflow-x-auto"
                  >
                    <code style={{ color: '#D4D4D4' }}>
                      {highlightSQL(stmt)}
                    </code>
                  </pre>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>
        </Tabs>
      )}

      {/* Footer */}
      {totalStatements > 0 && (
        <div className="flex items-center justify-between px-4 py-3 border-t bg-muted/30">
          <Button
            variant="ghost"
            size="sm"
            onClick={onDiscard}
            disabled={isExecuting}
            className="text-muted-foreground hover:text-destructive text-xs"
          >
            Clear All
          </Button>
          <Button
            variant="default"
            size="sm"
            onClick={onCommit}
            disabled={isExecuting}
            className="text-xs"
          >
            {isExecuting ? 'Committing...' : `Commit All (${totalChanges || totalStatements})`}
          </Button>
        </div>
      )}
    </div>
  );
};
