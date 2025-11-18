# Query Components

SQL Editor and Results Viewer components for DB-Hive database client.

## Components

### SQLEditor

Monaco Editor-based SQL editor with syntax highlighting, keyboard shortcuts, and connection status.

**Features:**
- SQL syntax highlighting and autocomplete
- Ctrl/Cmd+Enter to execute query
- Dark/light theme support
- Connection status indicator
- Clear button to reset editor
- Read-only mode when query is executing

**Props:**
```typescript
interface SQLEditorProps {
  connectionId: string | null;        // Active connection ID
  onExecuteQuery: (sql: string) => void;  // Execute callback
  value: string;                      // Current SQL text
  onChange: (value: string | undefined) => void;  // Text change handler
  loading?: boolean;                  // Query executing state
}
```

**Usage:**
```tsx
import { SQLEditor } from '@/components/SQLEditor';

<SQLEditor
  connectionId="conn-123"
  onExecuteQuery={(sql) => console.log('Execute:', sql)}
  value={sqlText}
  onChange={setSqlText}
  loading={false}
/>
```

### ResultsViewer

Virtualized table viewer for query results with sorting, loading states, and error handling.

**Features:**
- Virtualized table with TanStack Table for performance
- Column sorting
- NULL value indicators
- Loading spinner
- Error alerts
- DML result display (rows affected)
- Empty state
- Execution time display
- Sticky header
- Zebra striping

**Props:**
```typescript
interface ResultsViewerProps {
  columns: string[];              // Column names
  rows: any[][];                  // Row data (array of arrays)
  rowsAffected: number | null;    // For INSERT/UPDATE/DELETE
  loading: boolean;               // Loading state
  error: string | null;           // Error message
  executionTime?: number;         // Execution time in ms
}
```

**Usage:**
```tsx
import { ResultsViewer } from '@/components/ResultsViewer';

<ResultsViewer
  columns={['id', 'name', 'email']}
  rows={[
    [1, 'John', 'john@example.com'],
    [2, 'Jane', 'jane@example.com'],
  ]}
  rowsAffected={null}
  loading={false}
  error={null}
  executionTime={123}
/>
```

### QueryPanel

Combined SQL editor and results viewer with resizable panels and query execution management.

**Features:**
- Resizable split panel (editor top, results bottom)
- Query execution state management
- Error handling
- Loading states
- Automatic results display

**Props:**
```typescript
interface QueryPanelProps {
  connectionId: string | null;
  onExecuteQuery: (sql: string) => Promise<QueryExecutionResult>;
}
```

**Usage:**
```tsx
import { QueryPanel } from '@/components/QueryPanel';
import { invoke } from '@tauri-apps/api/core';

const executeQuery = async (sql: string) => {
  return await invoke('execute_query', {
    connectionId: activeConnection,
    sql,
  });
};

<QueryPanel
  connectionId="conn-123"
  onExecuteQuery={executeQuery}
/>
```

## Types

### QueryExecutionResult

```typescript
interface QueryExecutionResult {
  columns: string[];           // Column names in result set
  rows: any[][];              // Row data as array of arrays
  rowsAffected: number | null; // Rows affected by DML
  executionTime: number;       // Execution time in milliseconds
}
```

### QueryError

```typescript
interface QueryError extends DbError {
  sql?: string;      // SQL query that caused error
  line?: number;     // Line number of error
  column?: number;   // Column number of error
}
```

## Theming

All components support light/dark mode via the `ThemeProvider`:

```tsx
import { ThemeProvider } from '@/components/theme-provider';
import { QueryPanel } from '@/components/QueryPanel';

<ThemeProvider defaultTheme="dark">
  <QueryPanel ... />
</ThemeProvider>
```

The Monaco Editor automatically switches theme based on the current theme setting.

## Keyboard Shortcuts

### SQLEditor

- **Ctrl/Cmd + Enter**: Execute current query
- **Ctrl/Cmd + A**: Select all text
- **Ctrl/Cmd + Z**: Undo
- **Ctrl/Cmd + Shift + Z**: Redo
- **Ctrl/Cmd + F**: Find
- **Ctrl/Cmd + H**: Replace

## Performance

### Virtualization

The ResultsViewer uses TanStack Table which efficiently handles large result sets. However, for extremely large datasets (100k+ rows), consider:

1. **Server-side pagination**: Limit results on backend
2. **Streaming**: Use Tauri Channels to stream results in batches
3. **Virtual scrolling**: Already implemented via TanStack Table

### Monaco Editor

The Monaco Editor is lazy-loaded and uses web workers for syntax highlighting, ensuring smooth performance even with large SQL files.

## Styling

Components use:
- **TailwindCSS** for styling
- **shadcn/ui** for UI primitives (Card, Button, Alert)
- **lucide-react** for icons

Custom styles can be applied via `className` prop on all components.

## Example: Full Integration

See `/src/examples/QueryPanelExample.tsx` for complete integration examples:

- Basic QueryPanel usage
- Connection management integration
- Mock implementation for development

## Tauri Backend Integration

The QueryPanel expects the following Tauri command:

```rust
#[tauri::command]
async fn execute_query(
    connection_id: String,
    sql: String,
) -> Result<QueryExecutionResult, DbError> {
    // Execute query and return results
}
```

Where:
```rust
#[derive(Serialize)]
pub struct QueryExecutionResult {
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    rows_affected: Option<u64>,
    execution_time: u64,
}
```

## Error Handling

Errors are displayed in the ResultsViewer as alerts:

```tsx
// Error from Tauri command
{
  kind: 'query',
  message: 'Syntax error near "FROM"'
}
```

The component handles both:
- Tauri command errors (DbError)
- Network/connection errors
- JavaScript errors

## Accessibility

- Semantic HTML (`table`, `th`, `td`)
- ARIA labels on buttons
- Keyboard navigation
- Focus management
- Color contrast compliant

## Testing

Example test setup:

```tsx
import { render, screen } from '@testing-library/react';
import { vi } from 'vitest';
import { QueryPanel } from './QueryPanel';

test('executes query on button click', async () => {
  const mockExecute = vi.fn().mockResolvedValue({
    columns: ['id'],
    rows: [[1]],
    rowsAffected: null,
    executionTime: 100,
  });

  render(
    <QueryPanel
      connectionId="test-conn"
      onExecuteQuery={mockExecute}
    />
  );

  // Test implementation
});
```
