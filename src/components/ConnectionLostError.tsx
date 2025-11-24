import { FC } from "react";
import { WifiOff, Home, RefreshCw } from "lucide-react";
import { ErrorState, ErrorAction } from "./ErrorState";

export interface ConnectionLostErrorProps {
  /**
   * Callback when the user clicks "Reconnect"
   */
  onReconnect?: () => void;

  /**
   * Callback when the user clicks "Go to Dashboard"
   */
  onGoToDashboard?: () => void;

  /**
   * Custom error message (optional)
   */
  message?: string;

  /**
   * Database name that lost connection (optional)
   */
  databaseName?: string;

  /**
   * Additional custom actions
   */
  additionalActions?: ErrorAction[];

  /**
   * Custom className for container
   */
  className?: string;
}

/**
 * ConnectionLostError - Specialized error component for connection failures
 *
 * Displays a friendly error message when database connection is lost,
 * with options to reconnect or return to the dashboard.
 *
 * @example
 * ```tsx
 * <ConnectionLostError
 *   databaseName="PostgreSQL Production"
 *   onReconnect={handleReconnect}
 *   onGoToDashboard={() => navigate('/dashboard')}
 * />
 * ```
 */
export const ConnectionLostError: FC<ConnectionLostErrorProps> = ({
  onReconnect,
  onGoToDashboard,
  message,
  databaseName,
  additionalActions = [],
  className,
}) => {
  // Build default message
  const defaultMessage = databaseName
    ? `We couldn't maintain the connection to ${databaseName}. This might be due to network issues, server downtime, or connection timeout.`
    : "We couldn't connect to the database. Please check your connection settings and try again.";

  // Build actions array
  const actions: ErrorAction[] = [];

  if (onReconnect) {
    actions.push({
      label: "Reconnect",
      onClick: onReconnect,
      variant: "default",
      icon: RefreshCw,
    });
  }

  if (onGoToDashboard) {
    actions.push({
      label: "Go to Dashboard",
      onClick: onGoToDashboard,
      variant: "outline",
      icon: Home,
    });
  }

  // Add any additional actions
  actions.push(...additionalActions);

  return (
    <ErrorState
      title="Connection Lost"
      message={message || defaultMessage}
      icon={WifiOff}
      actions={actions}
      variant="error"
      className={className}
    />
  );
};
