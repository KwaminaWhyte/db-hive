import { FC } from 'react';
import { CircleDot, Clock, Hash, Database } from 'lucide-react';
import { cn } from '@/lib/utils';

interface QueryStatusBarProps {
  connectionName?: string;
  databaseName?: string;
  rowCount?: number | null;
  rowsAffected?: number | null;
  executionTime?: number;
  queryType?: string;
  loading?: boolean;
}

export const QueryStatusBar: FC<QueryStatusBarProps> = ({
  connectionName,
  databaseName,
  rowCount,
  rowsAffected,
  executionTime,
  queryType,
  loading = false,
}) => {
  const formatTime = (ms?: number) => {
    if (ms === undefined) return null;
    return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(2)}s`;
  };

  const displayRows = rowCount !== null && rowCount !== undefined
    ? rowCount
    : rowsAffected !== null && rowsAffected !== undefined
    ? rowsAffected
    : null;

  return (
    <div className={cn(
      'flex items-center gap-4 px-3 h-6 border-t bg-muted/30 text-xs text-muted-foreground shrink-0',
      loading && 'animate-pulse'
    )}>
      {connectionName && (
        <span className="flex items-center gap-1">
          <CircleDot className="h-2.5 w-2.5 text-green-500 fill-green-500" />
          {connectionName}{databaseName ? ` / ${databaseName}` : ''}
        </span>
      )}
      {queryType && (
        <span className="flex items-center gap-1">
          <Database className="h-2.5 w-2.5" />
          {queryType}
        </span>
      )}
      {displayRows !== null && (
        <span className="flex items-center gap-1">
          <Hash className="h-2.5 w-2.5" />
          {displayRows.toLocaleString()} {displayRows === 1 ? 'row' : 'rows'}
        </span>
      )}
      {formatTime(executionTime) && (
        <span className="flex items-center gap-1">
          <Clock className="h-2.5 w-2.5" />
          {formatTime(executionTime)}
        </span>
      )}
    </div>
  );
};
