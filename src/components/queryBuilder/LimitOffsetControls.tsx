import { FC, useState, useEffect } from 'react';
import { Hash, X, Info } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { cn } from '@/lib/utils';

interface LimitOffsetControlsProps {
  limit: number | undefined;
  offset: number | undefined;
  onSetLimit: (limit: number | undefined) => void;
  onSetOffset: (offset: number | undefined) => void;
}

export const LimitOffsetControls: FC<LimitOffsetControlsProps> = ({
  limit,
  offset,
  onSetLimit,
  onSetOffset,
}) => {
  const [limitValue, setLimitValue] = useState<string>(limit?.toString() || '');
  const [offsetValue, setOffsetValue] = useState<string>(offset?.toString() || '');
  const [limitError, setLimitError] = useState<string | null>(null);
  const [offsetError, setOffsetError] = useState<string | null>(null);

  // Sync internal state with props
  useEffect(() => {
    setLimitValue(limit?.toString() || '');
  }, [limit]);

  useEffect(() => {
    setOffsetValue(offset?.toString() || '');
  }, [offset]);

  const validateLimit = (value: string): boolean => {
    if (value === '') {
      setLimitError(null);
      return true;
    }

    const num = parseInt(value, 10);
    if (isNaN(num)) {
      setLimitError('Must be a valid number');
      return false;
    }

    if (num <= 0) {
      setLimitError('Must be greater than 0');
      return false;
    }

    if (!Number.isInteger(num)) {
      setLimitError('Must be a whole number');
      return false;
    }

    setLimitError(null);
    return true;
  };

  const validateOffset = (value: string): boolean => {
    if (value === '') {
      setOffsetError(null);
      return true;
    }

    const num = parseInt(value, 10);
    if (isNaN(num)) {
      setOffsetError('Must be a valid number');
      return false;
    }

    if (num < 0) {
      setOffsetError('Must be 0 or greater');
      return false;
    }

    if (!Number.isInteger(num)) {
      setOffsetError('Must be a whole number');
      return false;
    }

    setOffsetError(null);
    return true;
  };

  const handleLimitChange = (value: string) => {
    setLimitValue(value);

    if (value === '') {
      onSetLimit(undefined);
      setLimitError(null);
      return;
    }

    if (validateLimit(value)) {
      const num = parseInt(value, 10);
      onSetLimit(num);
    }
  };

  const handleOffsetChange = (value: string) => {
    setOffsetValue(value);

    if (value === '') {
      onSetOffset(undefined);
      setOffsetError(null);
      return;
    }

    if (validateOffset(value)) {
      const num = parseInt(value, 10);
      onSetOffset(num);
    }
  };

  const clearLimit = () => {
    setLimitValue('');
    setLimitError(null);
    onSetLimit(undefined);
  };

  const clearOffset = () => {
    setOffsetValue('');
    setOffsetError(null);
    onSetOffset(undefined);
  };

  return (
    <div className="flex flex-col gap-4">
      {/* Info message */}
      <Alert>
        <Info className="size-4" />
        <AlertDescription>
          Use LIMIT to control how many rows are returned. Use OFFSET to skip rows for pagination.
        </AlertDescription>
      </Alert>

      {/* Controls */}
      <div className="flex flex-wrap gap-4">
        {/* LIMIT control */}
        <div className="flex-1 min-w-[200px]">
          <div className="flex items-center gap-2 mb-2">
            <Label htmlFor="limit-input" className="text-sm font-medium">
              <Hash className="size-3.5" />
              Limit results to:
            </Label>
            {limitValue && (
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={clearLimit}
                aria-label="Clear limit"
                className="ml-auto"
              >
                <X className="size-3.5" />
              </Button>
            )}
          </div>
          <div className="relative">
            <Input
              id="limit-input"
              type="number"
              min="1"
              step="1"
              placeholder="No limit"
              value={limitValue}
              onChange={(e) => handleLimitChange(e.target.value)}
              aria-invalid={!!limitError}
              className={cn(limitError && 'border-destructive')}
            />
          </div>
          {limitError && (
            <p className="text-xs text-destructive mt-1.5">{limitError}</p>
          )}
          <p className="text-xs text-muted-foreground mt-1.5">
            Maximum number of rows to return
          </p>
        </div>

        {/* OFFSET control */}
        <div className="flex-1 min-w-[200px]">
          <div className="flex items-center gap-2 mb-2">
            <Label htmlFor="offset-input" className="text-sm font-medium">
              <Hash className="size-3.5" />
              Skip first:
            </Label>
            {offsetValue && (
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={clearOffset}
                aria-label="Clear offset"
                className="ml-auto"
              >
                <X className="size-3.5" />
              </Button>
            )}
          </div>
          <div className="relative">
            <Input
              id="offset-input"
              type="number"
              min="0"
              step="1"
              placeholder="Start from beginning"
              value={offsetValue}
              onChange={(e) => handleOffsetChange(e.target.value)}
              aria-invalid={!!offsetError}
              className={cn(offsetError && 'border-destructive')}
            />
          </div>
          {offsetError && (
            <p className="text-xs text-destructive mt-1.5">{offsetError}</p>
          )}
          <p className="text-xs text-muted-foreground mt-1.5">
            Number of rows to skip before returning results
          </p>
        </div>
      </div>

      {/* Pagination preview */}
      {(limit !== undefined || offset !== undefined) && (
        <div className="p-3 rounded-lg bg-accent/30 border border-input">
          <p className="text-sm font-medium mb-1">Pagination Summary:</p>
          <p className="text-xs text-muted-foreground">
            {offset !== undefined && offset > 0 && (
              <span>Skip the first {offset} row{offset !== 1 ? 's' : ''}</span>
            )}
            {offset !== undefined && offset > 0 && limit !== undefined && (
              <span>, then </span>
            )}
            {limit !== undefined && (
              <span>return up to {limit} row{limit !== 1 ? 's' : ''}</span>
            )}
            {offset === undefined && limit === undefined && (
              <span>No pagination applied</span>
            )}
          </p>
        </div>
      )}
    </div>
  );
};
