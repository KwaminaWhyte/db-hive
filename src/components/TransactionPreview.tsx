import { FC } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Button } from './ui/button';
import { ScrollArea } from './ui/scroll-area';
import { Badge } from './ui/badge';
import { FileCode, X, AlertCircle } from 'lucide-react';
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
}

export const TransactionPreview: FC<TransactionPreviewProps> = ({
  statements,
  isExecuting,
  onCommit,
  onDiscard,
  onClose,
  error,
}) => {
  const totalStatements = statements.length;

  return (
    <div className="h-full flex flex-col border-l bg-background">
      {/* Header */}
      <div className="flex items-center justify-between p-3 border-b bg-muted/30">
        <div className="flex items-center gap-2">
          <FileCode className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-semibold">Transaction Preview</span>
          <Badge variant="secondary" className="ml-2">
            {totalStatements} {totalStatements === 1 ? 'statement' : 'statements'}
          </Badge>
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
            statements.map((stmt, idx) => (
              <Card key={idx} className="bg-muted/30">
                <CardHeader className="p-3 pb-2">
                  <CardTitle className="text-xs font-medium text-muted-foreground">
                    Statement {idx + 1}
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-3 pt-0">
                  <pre className="text-xs font-mono bg-background p-2 rounded border overflow-x-auto">
                    <code className="language-sql">{stmt}</code>
                  </pre>
                </CardContent>
              </Card>
            ))
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
