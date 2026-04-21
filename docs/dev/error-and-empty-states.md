# Error & Empty State Components

This document provides comprehensive documentation for DB-Hive's error and empty state components (Milestone 3.9).

## Overview

These components provide consistent, user-friendly error and empty state handling across the application. They feature:

- Smooth animations and transitions
- Dark/light theme support
- Responsive, mobile-friendly layouts
- Accessible design
- Reusable, composable architecture
- TypeScript type safety

## Components

### 1. ErrorState (Base Component)

The foundational error state component that other error components build upon.

#### Props

```typescript
interface ErrorStateProps {
  title: string;              // Error title
  message: string;            // Error message
  icon?: LucideIcon;          // Optional icon
  actions?: ErrorAction[];    // Array of action buttons
  className?: string;         // Custom CSS classes
  children?: ReactNode;       // Custom content
  variant?: "error" | "warning" | "info"; // Visual variant
}

interface ErrorAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost" | "destructive";
  icon?: LucideIcon;
}
```

#### Usage

```tsx
import { ErrorState } from "@/components";
import { AlertCircle, RefreshCw } from "lucide-react";

<ErrorState
  title="Operation Failed"
  message="We encountered an error while processing your request."
  icon={AlertCircle}
  variant="error"
  actions={[
    { label: "Retry", onClick: handleRetry, icon: RefreshCw },
    { label: "Cancel", onClick: handleCancel, variant: "outline" }
  ]}
/>
```

#### Variants

- **error** (default): Red/destructive styling for errors
- **warning**: Orange styling for warnings
- **info**: Blue styling for informational messages

---

### 2. ConnectionLostError

Specialized component for database connection failures.

#### Props

```typescript
interface ConnectionLostErrorProps {
  onReconnect?: () => void;        // Reconnect handler
  onGoToDashboard?: () => void;    // Dashboard navigation
  message?: string;                 // Custom error message
  databaseName?: string;            // DB name that lost connection
  additionalActions?: ErrorAction[]; // Extra action buttons
  className?: string;
}
```

#### Usage

```tsx
import { ConnectionLostError } from "@/components";

<ConnectionLostError
  databaseName="PostgreSQL Production"
  onReconnect={async () => {
    await reconnectToDatabase();
  }}
  onGoToDashboard={() => navigate("/dashboard")}
/>
```

#### Features

- WifiOff icon by default
- Contextual message with database name
- "Reconnect" and "Go to Dashboard" action buttons
- Friendly, non-technical error message

---

### 3. QueryErrorState

Specialized component for SQL query execution errors.

#### Props

```typescript
interface QueryErrorStateProps {
  message: string;                   // Error message from database
  query?: string;                    // SQL query that caused error
  onRetry?: () => void;              // Retry handler
  onViewDocs?: () => void;           // Documentation link handler
  errorCode?: string;                // Database error code
  additionalActions?: ErrorAction[];  // Extra action buttons
  className?: string;
  showDetailsInitially?: boolean;    // Show query details by default
}
```

#### Usage

```tsx
import { QueryErrorState } from "@/components";

<QueryErrorState
  message='column "usre_name" does not exist'
  errorCode="42703"
  query="SELECT id, usre_name FROM users;"
  onRetry={handleRetry}
  onViewDocs={() => window.open("https://docs.db-hive.dev")}
/>
```

#### Features

- AlertCircle icon
- Error code display (if available)
- Collapsible query details with syntax highlighting
- Copy error details to clipboard
- Helpful tips section
- "Try Again" and "View Documentation" buttons

---

### 4. EmptyState (Base Component)

The foundational empty state component for when there's no data to display.

#### Props

```typescript
interface EmptyStateProps {
  title: string;                 // Title text
  message: string;               // Description message
  icon?: LucideIcon;             // Optional icon
  actions?: EmptyStateAction[];  // Action buttons
  className?: string;
  size?: "sm" | "md" | "lg";     // Size variant
}

interface EmptyStateAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost";
  icon?: LucideIcon;
}
```

#### Usage

```tsx
import { EmptyState } from "@/components";
import { Database, Plus } from "lucide-react";

<EmptyState
  title="No Data"
  message="There's nothing to display yet."
  icon={Database}
  size="md"
  actions={[
    { label: "Add Data", onClick: handleAdd, icon: Plus }
  ]}
/>
```

---

### 5. NoConnectionsEmpty

Specialized empty state for when no database connections exist.

#### Props

```typescript
interface NoConnectionsEmptyProps {
  onAddConnection?: () => void;      // Add connection handler
  message?: string;                   // Custom message
  additionalActions?: EmptyStateAction[];
  className?: string;
  size?: "sm" | "md" | "lg";
}
```

#### Usage

```tsx
import { NoConnectionsEmpty } from "@/components";

<NoConnectionsEmpty
  onAddConnection={() => setShowConnectionForm(true)}
/>
```

#### Features

- Database icon
- Friendly message about creating first connection
- "Add Connection" button with Plus icon
- Supports all database types in message

---

### 6. NoResultsEmpty

Specialized empty state for query results.

#### Props

```typescript
interface NoResultsEmptyProps {
  onRunQuery?: () => void;           // Run query handler
  message?: string;                   // Custom message
  additionalActions?: EmptyStateAction[];
  className?: string;
  size?: "sm" | "md" | "lg";
  noQueryExecuted?: boolean;         // Whether this is pre-query state
}
```

#### Usage

```tsx
import { NoResultsEmpty } from "@/components";

// Before query execution
<NoResultsEmpty
  noQueryExecuted={true}
  onRunQuery={handleRunQuery}
/>

// After query returns no results
<NoResultsEmpty
  noQueryExecuted={false}
  message="No users match your search criteria."
  onRunQuery={handleRunQuery}
/>
```

#### Features

- FileSearch icon
- Different messages for pre/post query states
- "Run Query" or "Run Another Query" button
- Compact by default (size="sm")

---

## Styling & Theming

All components support:

- **Dark/Light Mode**: Automatic theme adaptation using Tailwind's dark mode
- **Animations**: Smooth fade-in, slide-in, and zoom-in animations
- **Responsive Design**: Mobile-friendly layouts
- **Custom Classes**: `className` prop for additional styling

### Animation Timeline

1. Container fades in (0ms)
2. Icon zooms in (100ms delay)
3. Title/message slides in (200ms delay)
4. Custom content appears (300ms delay)
5. Action buttons slide in (400ms delay)

### Color Variants

Error states use theme-aware colors:

```css
/* Error */
text-destructive
bg-destructive/10 dark:bg-destructive/20

/* Warning */
text-orange-500 dark:text-orange-400
bg-orange-500/10 dark:bg-orange-500/20

/* Info */
text-blue-500 dark:text-blue-400
bg-blue-500/10 dark:bg-blue-500/20
```

---

## Integration Patterns

### With React Error Boundary

```tsx
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { ErrorState } from "@/components";
import { AlertCircle } from "lucide-react";

<ErrorBoundary
  fallback={
    <ErrorState
      title="Something Went Wrong"
      message="An unexpected error occurred."
      icon={AlertCircle}
      actions={[
        {
          label: "Reload",
          onClick: () => window.location.reload()
        }
      ]}
    />
  }
>
  <YourComponent />
</ErrorBoundary>
```

### With Try-Catch

```tsx
const [error, setError] = useState<string | null>(null);

const executeQuery = async () => {
  try {
    await invoke("execute_query", { sql, connectionId });
  } catch (err) {
    setError(err instanceof Error ? err.message : "Unknown error");
  }
};

if (error) {
  return (
    <QueryErrorState
      message={error}
      onRetry={() => {
        setError(null);
        executeQuery();
      }}
    />
  );
}
```

### With Conditional Rendering

```tsx
const { connections, loading } = useConnectionStore();

if (loading) {
  return <LoadingSpinner />;
}

if (connections.length === 0) {
  return <NoConnectionsEmpty onAddConnection={handleAdd} />;
}

return <ConnectionList connections={connections} />;
```

### With Tauri Commands

```tsx
import { invoke } from "@tauri-apps/api/core";
import { QueryErrorState } from "@/components";

const [queryError, setQueryError] = useState<{
  message: string;
  code?: string;
} | null>(null);

const executeQuery = async (sql: string) => {
  try {
    const result = await invoke("execute_query", {
      connectionId: activeConnection,
      sql,
    });
    return result;
  } catch (err) {
    // Tauri errors are serialized with kind and message
    const dbError = err as { kind: string; message: string };
    setQueryError({
      message: dbError.message,
      code: dbError.kind,
    });
  }
};

if (queryError) {
  return (
    <QueryErrorState
      message={queryError.message}
      errorCode={queryError.code}
      query={currentQuery}
      onRetry={() => {
        setQueryError(null);
        executeQuery(currentQuery);
      }}
    />
  );
}
```

---

## Accessibility

All components follow accessibility best practices:

- Semantic HTML structure
- Proper heading hierarchy
- Keyboard navigation support
- Focus management for buttons
- Screen reader friendly
- Sufficient color contrast
- Clear, descriptive text

---

## Examples

See `ErrorStateExamples.tsx` for 15+ comprehensive examples including:

1. Basic error states
2. Error variants (error, warning, info)
3. Connection lost scenarios
4. Query errors with syntax highlighting
5. Empty states for various scenarios
6. Custom content and actions
7. Integration with Error Boundaries
8. Compact layouts

---

## Best Practices

### DO

- Use specific error components when available (ConnectionLostError, QueryErrorState)
- Provide actionable buttons (Retry, Go Back, etc.)
- Keep messages user-friendly and non-technical
- Include error codes when available
- Use appropriate icons from lucide-react
- Handle loading states before showing empty states

### DON'T

- Don't show stack traces to end users (use details/collapsible sections)
- Don't use overly technical language
- Don't create empty states without action buttons
- Don't mix error and empty states
- Don't ignore error codes from the database

---

## Testing

Example test patterns:

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { ErrorState } from "@/components";

test("renders error state with actions", () => {
  const handleRetry = vi.fn();

  render(
    <ErrorState
      title="Error"
      message="Something went wrong"
      actions={[
        { label: "Retry", onClick: handleRetry }
      ]}
    />
  );

  expect(screen.getByText("Error")).toBeInTheDocument();
  expect(screen.getByText("Something went wrong")).toBeInTheDocument();

  fireEvent.click(screen.getByText("Retry"));
  expect(handleRetry).toHaveBeenCalled();
});
```

---

## File Structure

```
src/components/
├── ErrorState.tsx              # Base error state component
├── ConnectionLostError.tsx     # Connection failure errors
├── QueryErrorState.tsx         # Query execution errors
├── EmptyState.tsx              # Base empty state component
├── NoConnectionsEmpty.tsx      # No connections empty state
├── NoResultsEmpty.tsx          # No query results empty state
├── ErrorStateExamples.tsx      # 15+ usage examples
├── ERROR_AND_EMPTY_STATES.md   # This documentation
└── index.ts                    # Component exports
```

---

## Dependencies

- `lucide-react`: Icons
- `@/components/ui/button`: Button component
- `@/components/ui/card`: Card component
- `@/lib/utils`: Utility functions (cn)
- React 19+
- TypeScript

---

## Future Enhancements

Potential improvements for future versions:

- [ ] Animation preferences (respect prefers-reduced-motion)
- [ ] Internationalization (i18n) support
- [ ] More specialized error types (permission denied, timeout, etc.)
- [ ] Integration with error tracking services (Sentry, etc.)
- [ ] Custom illustration support beyond icons
- [ ] Loading state variations
- [ ] Retry with exponential backoff
- [ ] Error analytics

---

## Changelog

### v0.8.0-beta (Milestone 3.9)
- Initial implementation of error and empty state components
- ErrorState, ConnectionLostError, QueryErrorState
- EmptyState, NoConnectionsEmpty, NoResultsEmpty
- Comprehensive documentation and examples
- Dark/light theme support
- Smooth animations
- Full TypeScript support

---

For more information, see:
- Component examples: `ErrorStateExamples.tsx`
- Design system: `src/components/ui/`
- Project documentation: `CLAUDE.md`
