/**
 * ErrorStateExamples - Demo/Reference file showing usage of all error and empty state components
 *
 * This file serves as documentation and examples for developers.
 * It demonstrates various use cases and configurations.
 */

import { useState } from "react";
import {
  AlertCircle,
  FileSearch,
  Home,
  RefreshCw,
} from "lucide-react";
import { ErrorState } from "./ErrorState";
import { ConnectionLostError } from "./ConnectionLostError";
import { QueryErrorState } from "./QueryErrorState";
import { EmptyState } from "./EmptyState";
import { NoConnectionsEmpty } from "./empty-states/NoConnectionsEmpty";
import { NoResultsEmpty } from "./empty-states/NoResultsEmpty";

/**
 * Example 1: Basic ErrorState with custom icon and actions
 */
export const BasicErrorExample = () => {
  const handleRetry = () => {
    console.log("Retrying operation...");
  };

  const handleGoHome = () => {
    console.log("Navigating to home...");
  };

  return (
    <ErrorState
      title="Operation Failed"
      message="We encountered an error while processing your request. Please try again."
      icon={AlertCircle}
      actions={[
        { label: "Retry", onClick: handleRetry, icon: RefreshCw },
        {
          label: "Go Home",
          onClick: handleGoHome,
          variant: "outline",
          icon: Home,
        },
      ]}
    />
  );
};

/**
 * Example 2: Error state with different variants
 */
export const ErrorVariantsExample = () => {
  return (
    <div className="space-y-8">
      <ErrorState
        title="Error"
        message="This is an error variant with destructive styling."
        icon={AlertCircle}
        variant="error"
      />

      <ErrorState
        title="Warning"
        message="This is a warning variant with orange styling."
        icon={AlertCircle}
        variant="warning"
      />

      <ErrorState
        title="Information"
        message="This is an info variant with blue styling."
        icon={AlertCircle}
        variant="info"
      />
    </div>
  );
};

/**
 * Example 3: ConnectionLostError with database name
 */
export const ConnectionLostExample = () => {
  const handleReconnect = async () => {
    console.log("Attempting to reconnect...");
    // Simulate reconnection logic
  };

  const handleGoToDashboard = () => {
    console.log("Navigating to dashboard...");
  };

  return (
    <ConnectionLostError
      databaseName="PostgreSQL Production"
      onReconnect={handleReconnect}
      onGoToDashboard={handleGoToDashboard}
    />
  );
};

/**
 * Example 4: ConnectionLostError with custom message
 */
export const ConnectionLostCustomMessageExample = () => {
  return (
    <ConnectionLostError
      message="The database server is currently undergoing maintenance. Please try again in a few minutes."
      onReconnect={() => console.log("Reconnecting...")}
    />
  );
};

/**
 * Example 5: QueryErrorState with full details
 */
export const QueryErrorExample = () => {
  const handleRetry = () => {
    console.log("Retrying query...");
  };

  const handleViewDocs = () => {
    window.open("https://docs.db-hive.dev/errors", "_blank");
  };

  return (
    <QueryErrorState
      message='column "usre_name" does not exist'
      errorCode="42703"
      query="SELECT id, usre_name, email FROM users WHERE status = 'active';"
      onRetry={handleRetry}
      onViewDocs={handleViewDocs}
      showDetailsInitially={false}
    />
  );
};

/**
 * Example 6: QueryErrorState with syntax error
 */
export const QuerySyntaxErrorExample = () => {
  return (
    <QueryErrorState
      message='syntax error at or near "SELCT"'
      errorCode="42601"
      query="SELCT * FROM users;"
      onRetry={() => console.log("Retrying...")}
    />
  );
};

/**
 * Example 7: QueryErrorState with long query
 */
export const QueryErrorLongQueryExample = () => {
  const longQuery = `
SELECT
  u.id,
  u.username,
  u.email,
  p.title AS profile_title,
  COUNT(o.id) AS order_count,
  SUM(o.total_amount) AS total_spent
FROM
  users u
  LEFT JOIN profiles p ON u.id = p.user_id
  LEFT JOIN orders o ON u.id = o.user_id
WHERE
  u.status = 'active'
  AND u.created_at >= NOW() - INTERVAL '1 year'
GROUP BY
  u.id, u.username, u.email, p.title
HAVING
  COUNT(o.id) > 10
ORDER BY
  total_spent DESC
LIMIT 100;
  `.trim();

  return (
    <QueryErrorState
      message="Permission denied for relation users"
      errorCode="42501"
      query={longQuery}
      onRetry={() => console.log("Retrying...")}
      onViewDocs={() => console.log("Opening docs...")}
      showDetailsInitially={true}
    />
  );
};

/**
 * Example 8: NoConnectionsEmpty
 */
export const NoConnectionsExample = () => {
  const handleAddConnection = () => {
    console.log("Opening connection form...");
  };

  return <NoConnectionsEmpty onAddConnection={handleAddConnection} />;
};

/**
 * Example 9: NoConnectionsEmpty - Basic usage
 * Note: The existing NoConnectionsEmpty has simpler props
 */
export const NoConnectionsCustomExample = () => {
  return (
    <NoConnectionsEmpty
      onAddConnection={() => console.log("Adding connection...")}
    />
  );
};

/**
 * Example 10: NoResultsEmpty - Basic usage
 * Note: The existing NoResultsEmpty has simpler props (no noQueryExecuted flag)
 */
export const NoResultsNoQueryExample = () => {
  return (
    <NoResultsEmpty
      onRunQuery={() => console.log("Running query...")}
    />
  );
};

/**
 * Example 11: NoResultsEmpty - With custom query text
 */
export const NoResultsEmptyExample = () => {
  return (
    <NoResultsEmpty
      queryText="SELECT * FROM users WHERE status = 'active'"
      onRunQuery={() => console.log("Running another query...")}
    />
  );
};

/**
 * Example 12: EmptyState with custom illustration
 */
export const EmptyStateWithCustomContentExample = () => {
  const customIllustration = (
    <div className="text-left space-y-2 p-4 bg-muted/50 rounded-lg text-sm">
      <p className="font-semibold">Quick tips:</p>
      <ul className="list-disc list-inside space-y-1 text-muted-foreground">
        <li>Write a query in the SQL editor</li>
        <li>Click the bookmark icon to save it</li>
        <li>Access saved queries anytime from the sidebar</li>
      </ul>
    </div>
  );

  return (
    <EmptyState
      title="No Saved Queries"
      message="You haven't saved any queries yet."
      icon={FileSearch}
      size="md"
      illustration={customIllustration}
    />
  );
};

/**
 * Example 13: Error state in try-catch block
 */
export const ErrorInTryCatchExample = () => {
  const [error, setError] = useState<string | null>(null);

  const executeQuery = async () => {
    try {
      // Simulating a query execution
      throw new Error("Connection timeout");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    }
  };

  if (error) {
    return (
      <ErrorState
        title="Query Failed"
        message={error}
        icon={AlertCircle}
        actions={[
          {
            label: "Try Again",
            onClick: () => {
              setError(null);
              executeQuery();
            },
          },
          {
            label: "Cancel",
            onClick: () => setError(null),
            variant: "outline",
          },
        ]}
      />
    );
  }

  return <button onClick={executeQuery}>Execute Query</button>;
};

/**
 * Example 14: Using with React Error Boundary
 */
export const ErrorBoundaryFallbackExample = () => {
  return (
    <ErrorState
      title="Something Went Wrong"
      message="An unexpected error occurred in the application. This has been logged for debugging."
      icon={AlertCircle}
      variant="error"
      actions={[
        {
          label: "Reload Application",
          onClick: () => window.location.reload(),
          variant: "default",
        },
        {
          label: "Report Issue",
          onClick: () => console.log("Opening issue reporter..."),
          variant: "outline",
        },
      ]}
    />
  );
};

/**
 * Example 15: Compact empty state
 */
export const CompactEmptyStateExample = () => {
  return (
    <EmptyState
      title="No History"
      message="Your query history is empty."
      icon={FileSearch}
      size="sm"
      actions={[
        {
          label: "Run a Query",
          onClick: () => console.log("Navigate to editor..."),
        },
      ]}
    />
  );
};
