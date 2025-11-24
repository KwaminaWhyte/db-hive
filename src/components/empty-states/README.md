# Empty State Components

A collection of reusable empty state components for DB-Hive, designed to provide a friendly and encouraging user experience when there's no data to display.

## Components

### Base Component: `EmptyState`

The foundation component that all other empty states use. It provides a consistent layout with animations, icons, and actions.

**Props:**

```typescript
interface EmptyStateProps {
  title: string;              // Main heading
  message: string;            // Descriptive text
  icon?: LucideIcon;          // Optional icon from lucide-react
  illustration?: ReactNode;   // Optional custom illustration
  actions?: EmptyStateAction[]; // Array of action buttons
  className?: string;         // Additional CSS classes
  iconClassName?: string;     // Custom icon styling
  size?: "sm" | "md" | "lg"; // Size variant (default: "md")
}

interface EmptyStateAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost";
  icon?: LucideIcon;
}
```

**Example:**

```tsx
import { EmptyState } from "@/components/EmptyState";
import { Database, Plus } from "lucide-react";

<EmptyState
  icon={Database}
  title="No Connections"
  message="Get started by adding your first connection."
  actions={[
    {
      label: "Add Connection",
      onClick: handleAdd,
      icon: Plus,
      variant: "default"
    }
  ]}
/>
```

### Specific Empty States

#### 1. `NoConnectionsEmpty`

Displayed when the user has no saved database connections.

**Props:**

```typescript
interface NoConnectionsEmptyProps {
  onAddConnection: () => void;
  className?: string;
}
```

**Usage:**

```tsx
import { NoConnectionsEmpty } from "@/components/empty-states";

<NoConnectionsEmpty onAddConnection={() => setShowConnectionDialog(true)} />
```

**Features:**
- Database icon with blue accent
- Clear call-to-action button
- Encouraging message about getting started

---

#### 2. `NoHistoryEmpty`

Displayed when query history is empty.

**Props:**

```typescript
interface NoHistoryEmptyProps {
  onRunQuery?: () => void; // Optional action
  className?: string;
}
```

**Usage:**

```tsx
import { NoHistoryEmpty } from "@/components/empty-states";

<NoHistoryEmpty
  onRunQuery={() => navigate('/query')}
/>
```

**Features:**
- History icon with violet accent
- Optional "Run Your First Query" button
- Context about how history works

---

#### 3. `NoTablesEmpty`

Displayed when a database has no tables.

**Props:**

```typescript
interface NoTablesEmptyProps {
  onCreateTable?: () => void;  // Optional action
  databaseName?: string;       // For personalized message
  className?: string;
}
```

**Usage:**

```tsx
import { NoTablesEmpty } from "@/components/empty-states";

<NoTablesEmpty
  databaseName="production_db"
  onCreateTable={() => setShowCreateTableDialog(true)}
/>
```

**Features:**
- Table icon with emerald accent
- Database name in message (if provided)
- Optional "Create Table" button

---

#### 4. `NoSearchResultsEmpty`

Displayed when search returns no results.

**Props:**

```typescript
interface NoSearchResultsEmptyProps {
  onClearSearch: () => void;
  searchQuery?: string;        // For personalized message
  className?: string;
}
```

**Usage:**

```tsx
import { NoSearchResultsEmpty } from "@/components/empty-states";

<NoSearchResultsEmpty
  searchQuery={searchTerm}
  onClearSearch={() => setSearchTerm('')}
/>
```

**Features:**
- SearchX icon with amber accent
- Shows search query in message
- "Clear Search" button
- Smaller size variant (`size="sm"`)

---

#### 5. `NoResultsEmpty`

Displayed when a query executes successfully but returns 0 rows.

**Props:**

```typescript
interface NoResultsEmptyProps {
  onRunQuery?: () => void;
  queryText?: string;
  className?: string;
}
```

**Usage:**

```tsx
import { NoResultsEmpty } from "@/components/empty-states";

<NoResultsEmpty
  queryText={sqlQuery}
  onRunQuery={() => focusEditor()}
/>
```

**Features:**
- FileQuestion icon with slate accent
- Context about why results might be empty
- Optional "Run Another Query" button
- Smaller size variant (`size="sm"`)

---

#### 6. `NoDataEmpty`

Displayed when a table exists but has no rows.

**Props:**

```typescript
interface NoDataEmptyProps {
  onAddData?: () => void;
  tableName?: string;
  className?: string;
}
```

**Usage:**

```tsx
import { NoDataEmpty } from "@/components/empty-states";

<NoDataEmpty
  tableName="users"
  onAddData={() => setShowInsertDialog(true)}
/>
```

**Features:**
- Inbox icon with cyan accent
- Table name in message (if provided)
- Optional "Insert Data" button

---

## Design Features

### Animations

All empty states include smooth entrance animations:

1. **Fade-in and slide-up** for the container (500ms)
2. **Zoom-in** for the icon (700ms with 100ms delay)
3. **Staggered fade-in** for content and actions (500ms with 200ms/300ms delays)
4. **Hover effects** on icons (scale + background opacity change)

### Color Coding

Each empty state uses a specific color accent for better visual recognition:

- **NoConnectionsEmpty**: Blue (`bg-blue-50`, `text-blue-600`)
- **NoHistoryEmpty**: Violet (`bg-violet-50`, `text-violet-600`)
- **NoTablesEmpty**: Emerald (`bg-emerald-50`, `text-emerald-600`)
- **NoSearchResultsEmpty**: Amber (`bg-amber-50`, `text-amber-600`)
- **NoResultsEmpty**: Slate (`bg-slate-50`, `text-slate-600`)
- **NoDataEmpty**: Cyan (`bg-cyan-50`, `text-cyan-600`)

Colors automatically adapt for dark mode with reduced opacity.

### Responsive Design

- Maximum width: `max-w-md` (448px)
- Centered alignment with auto margins
- Padding adapts to content size
- Button layout wraps on small screens

### Accessibility

- Semantic HTML structure
- Proper heading hierarchy
- Clear, descriptive text
- Keyboard-accessible buttons
- Sufficient color contrast
- Screen reader friendly

---

## Size Variants

The base `EmptyState` component supports three size variants:

| Size | Icon | Title   | Message  | Padding | Use Case |
|------|------|---------|----------|---------|----------|
| `sm` | 48px | `base`  | `sm`     | `py-8`  | Inline contexts, search results |
| `md` | 64px | `lg`    | `base`   | `py-12` | Default, main content areas |
| `lg` | 80px | `xl`    | `lg`     | `py-16` | Full-page empty states |

---

## Best Practices

### When to Use

1. **NoConnectionsEmpty**: First-time users, connection list is empty
2. **NoHistoryEmpty**: Query history panel, activity log
3. **NoTablesEmpty**: Database explorer, schema browser
4. **NoSearchResultsEmpty**: Search/filter results, live search
5. **NoResultsEmpty**: Query result grid, data viewer
6. **NoDataEmpty**: Table data view, row browser

### When NOT to Use

- Loading states (use `Spinner` or `Skeleton` instead)
- Error states (use error components)
- Intentionally hidden content
- Temporary states during transitions

### Customization

You can customize any empty state by:

1. **Adding className**: For positioning or layout adjustments
2. **Custom icon colors**: Using `iconClassName` prop
3. **Multiple actions**: Pass array of actions
4. **Custom illustrations**: Use `illustration` prop instead of `icon`

Example with custom illustration:

```tsx
<EmptyState
  title="Welcome to DB-Hive"
  message="Your database management journey starts here."
  illustration={
    <img src="/welcome.svg" alt="" className="w-32 h-32" />
  }
  actions={[
    { label: "Get Started", onClick: handleStart },
    { label: "Learn More", onClick: handleLearn, variant: "outline" }
  ]}
/>
```

---

## Integration Examples

### In a Query Results Component

```tsx
import { NoResultsEmpty } from "@/components/empty-states";

const QueryResults = ({ data, isLoading, query }) => {
  if (isLoading) return <Spinner />;

  if (!data || data.length === 0) {
    return <NoResultsEmpty queryText={query} />;
  }

  return <DataTable data={data} />;
};
```

### In a Connection List

```tsx
import { NoConnectionsEmpty } from "@/components/empty-states";

const ConnectionList = ({ connections }) => {
  if (connections.length === 0) {
    return (
      <NoConnectionsEmpty
        onAddConnection={() => setShowDialog(true)}
      />
    );
  }

  return <div>{/* Connection items */}</div>;
};
```

### In a Search Component

```tsx
import { NoSearchResultsEmpty } from "@/components/empty-states";

const SearchResults = ({ results, query, onClear }) => {
  if (query && results.length === 0) {
    return (
      <NoSearchResultsEmpty
        searchQuery={query}
        onClearSearch={onClear}
      />
    );
  }

  return <ResultsList items={results} />;
};
```

---

## Testing

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { NoConnectionsEmpty } from "./NoConnectionsEmpty";

test("calls onAddConnection when button clicked", () => {
  const handleAdd = vi.fn();
  render(<NoConnectionsEmpty onAddConnection={handleAdd} />);

  fireEvent.click(screen.getByText("Add Connection"));
  expect(handleAdd).toHaveBeenCalledOnce();
});

test("displays correct message", () => {
  render(<NoConnectionsEmpty onAddConnection={() => {}} />);

  expect(screen.getByText("No Connections Yet")).toBeInTheDocument();
  expect(screen.getByText(/Get started by creating/)).toBeInTheDocument();
});
```

---

## Migration from Generic Empty States

If you have existing empty states, migrate them to these components:

**Before:**

```tsx
<div className="text-center p-8">
  <p className="text-muted-foreground">No connections found</p>
  <Button onClick={handleAdd}>Add Connection</Button>
</div>
```

**After:**

```tsx
<NoConnectionsEmpty onAddConnection={handleAdd} />
```

Benefits:
- Consistent design across the app
- Built-in animations
- Accessible by default
- Responsive layout
- Easier to maintain
