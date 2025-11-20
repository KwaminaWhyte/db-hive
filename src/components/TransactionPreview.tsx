import { FC, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Button } from './ui/button';
import { ScrollArea } from './ui/scroll-area';
import { Badge } from './ui/badge';
import { FileCode, X, AlertCircle, FileEdit, Database } from 'lucide-react';
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

export const TransactionPreview: FC<TransactionPreviewProps> = ({
  statements,
  isExecuting,
  onCommit,
  onDiscard,
  onClose,
  error,
  totalChanges = 0,
  modifiedRows = 0,
}) => {
  const totalStatements = statements.length;

  // Calculate statistics
  const stats = useMemo(() => {
    let updates = 0;
    let inserts = 0;
    let deletes = 0;

    statements.forEach((stmt) => {
      const upperStmt = stmt.trim().toUpperCase();
      if (upperStmt.startsWith('UPDATE')) updates++;
      else if (upperStmt.startsWith('INSERT')) inserts++;
      else if (upperStmt.startsWith('DELETE')) deletes++;
    });

    return { updates, inserts, deletes };
  }, [statements]);

  return (
    <div className="h-full flex flex-col border-l bg-background">
      {/* Header */}
      <div className="flex items-center justify-between p-3 border-b bg-muted/30">
        <div className="flex items-center gap-2">
          <FileCode className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-semibold">Transaction Preview</span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={onClose}
          className="h-7 w-7 p-0"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      {/* Statistics Bar */}
      {totalStatements > 0 && (
        <div className="p-3 border-b bg-muted/20">
          <div className="flex items-center gap-3 flex-wrap">
            <div className="flex items-center gap-1.5">
              <Database className="h-3.5 w-3.5 text-blue-500" />
              <span className="text-xs font-medium">{modifiedRows} {modifiedRows === 1 ? 'Row' : 'Rows'}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <FileEdit className="h-3.5 w-3.5 text-amber-500" />
              <span className="text-xs font-medium">{totalChanges} {totalChanges === 1 ? 'Cell' : 'Cells'}</span>
            </div>
            {stats.updates > 0 && (
              <Badge variant="outline" className="text-xs">
                {stats.updates} UPDATE{stats.updates > 1 ? 'S' : ''}
              </Badge>
            )}
            {stats.inserts > 0 && (
              <Badge variant="outline" className="text-xs bg-green-500/10 text-green-600 border-green-600/30">
                {stats.inserts} INSERT{stats.inserts > 1 ? 'S' : ''}
              </Badge>
            )}
            {stats.deletes > 0 && (
              <Badge variant="outline" className="text-xs bg-red-500/10 text-red-600 border-red-600/30">
                {stats.deletes} DELETE{stats.deletes > 1 ? 'S' : ''}
              </Badge>
            )}
          </div>
        </div>
      )}

      {/* Error Display */}
      {error && (
        <div className="p-3 border-b">
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription className="text-sm">{error}</AlertDescription>
          </Alert>
        </div>
      )}

      {/* SQL Statements */}
      <ScrollArea className="flex-1 p-4">
        <div className="space-y-3">
          {statements.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground">
              <p className="text-sm">No changes to commit</p>
              <p className="text-xs mt-1">Edit cells to see SQL statements here</p>
            </div>
          ) : (
            statements.map((stmt, idx) => {
              // Determine statement type for badge color
              const upperStmt = stmt.trim().toUpperCase();
              const stmtType = upperStmt.startsWith('UPDATE')
                ? 'UPDATE'
                : upperStmt.startsWith('INSERT')
                ? 'INSERT'
                : upperStmt.startsWith('DELETE')
                ? 'DELETE'
                : 'SQL';

              const badgeColor =
                stmtType === 'UPDATE'
                  ? 'bg-blue-500/10 text-blue-600 border-blue-600/30'
                  : stmtType === 'INSERT'
                  ? 'bg-green-500/10 text-green-600 border-green-600/30'
                  : 'bg-red-500/10 text-red-600 border-red-600/30';

              return (
                <Card key={idx} className="bg-muted/30">
                  <CardHeader className="p-3 pb-2">
                    <div className="flex items-center justify-between">
                      <CardTitle className="text-xs font-medium text-muted-foreground">
                        Statement {idx + 1}
                      </CardTitle>
                      <Badge variant="outline" className={`text-xs ${badgeColor}`}>
                        {stmtType}
                      </Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="p-3 pt-0">
                    <pre className="text-xs font-mono bg-background p-3 rounded border overflow-x-auto">
                      <code className="language-sql" style={{ color: '#D4D4D4' }}>
                        {highlightSQL(stmt)}
                      </code>
                    </pre>
                  </CardContent>
                </Card>
              );
            })
          )}
        </div>
      </ScrollArea>

      {/* Action Buttons */}
      <div className="p-3 border-t bg-muted/30 space-y-2">
        <div className="flex gap-2">
          <Button
            variant="default"
            size="sm"
            onClick={onCommit}
            disabled={totalStatements === 0 || isExecuting}
            className="flex-1"
          >
            {isExecuting ? 'Committing...' : `Commit ${totalStatements} ${totalStatements === 1 ? 'Change' : 'Changes'}`}
          </Button>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={onDiscard}
          disabled={totalStatements === 0 || isExecuting}
          className="w-full"
        >
          Discard All Changes
        </Button>
      </div>
    </div>
  );
};
