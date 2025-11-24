import { FC, useState } from "react";
import { AlertCircle, RefreshCw, BookOpen, Copy, Check } from "lucide-react";
import { ErrorState, ErrorAction } from "./ErrorState";
import { Button } from "./ui/button";

export interface QueryErrorStateProps {
  /**
   * The error message from the database
   */
  message: string;

  /**
   * Optional SQL query that caused the error
   */
  query?: string;

  /**
   * Callback when the user clicks "Try Again"
   */
  onRetry?: () => void;

  /**
   * Callback when the user clicks "View Documentation"
   */
  onViewDocs?: () => void;

  /**
   * Error code (if available from database)
   */
  errorCode?: string;

  /**
   * Additional custom actions
   */
  additionalActions?: ErrorAction[];

  /**
   * Custom className for container
   */
  className?: string;

  /**
   * Show the full error details by default
   */
  showDetailsInitially?: boolean;
}

/**
 * QueryErrorState - Specialized error component for SQL query execution errors
 *
 * Displays detailed error information when a query fails, including the error
 * message, error code, and optionally the query that caused the error.
 * Provides options to retry or view documentation.
 *
 * @example
 * ```tsx
 * <QueryErrorState
 *   message="Syntax error near 'SELCT'"
 *   query="SELCT * FROM users;"
 *   errorCode="42601"
 *   onRetry={handleRetry}
 *   onViewDocs={() => window.open('https://docs.db-hive.dev')}
 * />
 * ```
 */
export const QueryErrorState: FC<QueryErrorStateProps> = ({
  message,
  query,
  onRetry,
  onViewDocs,
  errorCode,
  additionalActions = [],
  className,
  showDetailsInitially = false,
}) => {
  const [showDetails, setShowDetails] = useState(showDetailsInitially);
  const [copied, setCopied] = useState(false);

  // Build actions array
  const actions: ErrorAction[] = [];

  if (onRetry) {
    actions.push({
      label: "Try Again",
      onClick: onRetry,
      variant: "default",
      icon: RefreshCw,
    });
  }

  if (onViewDocs) {
    actions.push({
      label: "View Documentation",
      onClick: onViewDocs,
      variant: "outline",
      icon: BookOpen,
    });
  }

  // Add any additional actions
  actions.push(...additionalActions);

  // Handle copy error details
  const handleCopyError = async () => {
    const errorDetails = [
      errorCode ? `Error Code: ${errorCode}` : "",
      `Message: ${message}`,
      query ? `\nQuery:\n${query}` : "",
    ]
      .filter(Boolean)
      .join("\n");

    try {
      await navigator.clipboard.writeText(errorDetails);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy error details:", err);
    }
  };

  return (
    <ErrorState
      title="Query Error"
      message="The query could not be executed. Please review the error details below."
      icon={AlertCircle}
      actions={actions}
      variant="error"
      className={className}
    >
      {/* Error Details Section */}
      <div className="w-full space-y-3 text-left">
        {/* Error Code */}
        {errorCode && (
          <div className="flex items-center gap-2 text-xs">
            <span className="font-semibold text-muted-foreground">
              Error Code:
            </span>
            <code className="px-2 py-1 bg-muted rounded text-destructive font-mono">
              {errorCode}
            </code>
          </div>
        )}

        {/* Error Message */}
        <div className="p-4 bg-destructive/10 dark:bg-destructive/20 border border-destructive/20 rounded-lg">
          <div className="flex items-start justify-between gap-2 mb-2">
            <span className="text-xs font-semibold text-destructive uppercase tracking-wide">
              Error Message
            </span>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={handleCopyError}
              className="h-6 w-6 hover:bg-destructive/20"
            >
              {copied ? (
                <Check className="h-3 w-3 text-green-500" />
              ) : (
                <Copy className="h-3 w-3" />
              )}
            </Button>
          </div>
          <p className="text-sm text-foreground font-mono leading-relaxed break-words">
            {message}
          </p>
        </div>

        {/* Query Details (collapsible) */}
        {query && (
          <div className="space-y-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowDetails(!showDetails)}
              className="text-xs text-muted-foreground hover:text-foreground p-0 h-auto font-normal"
            >
              {showDetails ? "Hide" : "Show"} query details
            </Button>

            {showDetails && (
              <div className="p-4 bg-muted/50 border border-border/50 rounded-lg animate-in slide-in-from-top-2 duration-200">
                <div className="text-xs font-semibold text-muted-foreground mb-2 uppercase tracking-wide">
                  Query
                </div>
                <pre className="text-xs font-mono text-foreground overflow-x-auto leading-relaxed whitespace-pre-wrap break-words">
                  {query}
                </pre>
              </div>
            )}
          </div>
        )}

        {/* Helpful tips */}
        <div className="pt-2 border-t border-border/50">
          <p className="text-xs text-muted-foreground leading-relaxed">
            <strong className="font-semibold">Tip:</strong> Check your SQL
            syntax, table names, and column references. You can also view the
            documentation for common error codes and solutions.
          </p>
        </div>
      </div>
    </ErrorState>
  );
};
