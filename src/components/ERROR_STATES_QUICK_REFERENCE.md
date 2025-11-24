# Error & Empty States - Quick Reference

Quick reference guide for using error and empty state components in DB-Hive.

---

## Import Statement

```typescript
import {
  ErrorState,
  ConnectionLostError,
  QueryErrorState,
  EmptyState,
  NoConnectionsEmpty,
  NoResultsEmpty,
} from "@/components";
```

---

## ErrorState

**When to use:** General purpose errors

```tsx
<ErrorState
  title="Operation Failed"
  message="Brief description of what went wrong"
  icon={AlertCircle}
  variant="error" // "error" | "warning" | "info"
  actions={[
    { label: "Retry", onClick: handleRetry, icon: RefreshCw }
  ]}
/>
```

---

## ConnectionLostError

**When to use:** Database connection failures

```tsx
<ConnectionLostError
  databaseName="PostgreSQL Production"
  onReconnect={handleReconnect}
  onGoToDashboard={() => navigate("/dashboard")}
/>
```

---

## QueryErrorState

**When to use:** SQL query execution errors

```tsx
<QueryErrorState
  message="column \"name\" does not exist"
  errorCode="42703"
  query="SELECT name FROM users;"
  onRetry={handleRetry}
  onViewDocs={() => window.open("/docs")}
/>
```

---

## NoConnectionsEmpty

**When to use:** No database connections configured

```tsx
<NoConnectionsEmpty
  onAddConnection={() => setShowForm(true)}
/>
```

---

## NoResultsEmpty

**When to use:** Query returns no results

```tsx
// Before query executed
<NoResultsEmpty
  noQueryExecuted={true}
  onRunQuery={handleRun}
/>

// After query returns empty
<NoResultsEmpty
  noQueryExecuted={false}
  message="No records found."
  onRunQuery={handleRun}
/>
```

---

## EmptyState

**When to use:** Generic empty states

```tsx
<EmptyState
  title="No Data"
  message="There's nothing to display yet."
  icon={Database}
  size="md" // "sm" | "md" | "lg"
  actions={[
    { label: "Add Data", onClick: handleAdd }
  ]}
/>
```

---

## Common Patterns

### With Try-Catch

```tsx
const [error, setError] = useState<string | null>(null);

try {
  await invoke("command", params);
} catch (err) {
  setError(err.message);
}

if (error) {
  return (
    <ErrorState
      title="Error"
      message={error}
      actions={[
        { label: "Retry", onClick: () => setError(null) }
      ]}
    />
  );
}
```

### With Conditional Rendering

```tsx
if (loading) return <Spinner />;
if (error) return <ErrorState {...errorProps} />;
if (data.length === 0) return <EmptyState {...emptyProps} />;
return <DataView data={data} />;
```

---

## Icon Reference (lucide-react)

```typescript
import {
  AlertCircle,    // Generic errors
  WifiOff,        // Connection errors
  FileSearch,     // Empty results
  Database,       // Database-related
  RefreshCw,      // Retry action
  Home,           // Go home action
  BookOpen,       // Documentation
} from "lucide-react";
```

---

## Action Button Variants

```typescript
variant: "default"      // Primary (filled)
variant: "outline"      // Secondary (outlined)
variant: "secondary"    // Tertiary
variant: "ghost"        // Minimal
variant: "destructive"  // Dangerous actions
```

---

## Size Variants (EmptyState)

```typescript
size: "sm"  // Compact (min-h-[200px])
size: "md"  // Medium (min-h-[400px]) - Default
size: "lg"  // Large (min-h-[400px], larger text)
```

---

## Color Variants (ErrorState)

```typescript
variant: "error"    // Red/destructive (default)
variant: "warning"  // Orange
variant: "info"     // Blue
```

---

## TypeScript Types

```typescript
interface ErrorAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost" | "destructive";
  icon?: LucideIcon;
}

interface EmptyStateAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost";
  icon?: LucideIcon;
}
```

---

## Full Examples

See:
- `ErrorStateExamples.tsx` - 15+ code examples
- `ErrorStatesDemo.tsx` - Visual showcase page
- `ERROR_AND_EMPTY_STATES.md` - Complete documentation
