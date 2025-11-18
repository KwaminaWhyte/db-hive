# Query Components Architecture

## Component Hierarchy

```
┌─────────────────────────────────────────────────────────────┐
│                        QueryPanel                            │
│  (State Management + Resizable Layout)                       │
│                                                               │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                  PanelGroup (Vertical)                 │  │
│  │                                                         │  │
│  │  ┌───────────────────────────────────────────────┐    │  │
│  │  │              Panel (40%, min 20%)              │    │  │
│  │  │  ┌──────────────────────────────────────────┐ │    │  │
│  │  │  │           SQLEditor                       │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Card > CardHeader (Toolbar)       │  │ │    │  │
│  │  │  │  │  - Execute Button (Play Icon)      │  │ │    │  │
│  │  │  │  │  - Clear Button (Trash Icon)       │  │ │    │  │
│  │  │  │  │  - Connection Status (CircleDot)   │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Card > CardContent                │  │ │    │  │
│  │  │  │  │  ┌──────────────────────────────┐ │  │ │    │  │
│  │  │  │  │  │   Monaco Editor              │ │  │ │    │  │
│  │  │  │  │  │   - SQL Language             │ │  │ │    │  │
│  │  │  │  │  │   - Theme Support            │ │  │ │    │  │
│  │  │  │  │  │   - Ctrl+Enter Shortcut      │ │  │ │    │  │
│  │  │  │  │  │   - Autocomplete             │ │  │ │    │  │
│  │  │  │  │  └──────────────────────────────┘ │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  └──────────────────────────────────────────┘ │    │  │
│  │  └───────────────────────────────────────────────┘    │  │
│  │                                                         │  │
│  │  ┌───────────────────────────────────────────────┐    │  │
│  │  │       PanelResizeHandle (Draggable)           │    │  │
│  │  │       [GripHorizontal Icon on Hover]          │    │  │
│  │  └───────────────────────────────────────────────┘    │  │
│  │                                                         │  │
│  │  ┌───────────────────────────────────────────────┐    │  │
│  │  │              Panel (60%, min 30%)              │    │  │
│  │  │  ┌──────────────────────────────────────────┐ │    │  │
│  │  │  │         ResultsViewer                     │ │    │  │
│  │  │  │                                            │ │    │  │
│  │  │  │  [If Loading]                             │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Loader2 (Spinning)                │  │ │    │  │
│  │  │  │  │  "Executing query..."              │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │                                            │ │    │  │
│  │  │  │  [If Error]                               │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Alert (Destructive)               │  │ │    │  │
│  │  │  │  │  AlertCircle Icon                  │  │ │    │  │
│  │  │  │  │  Error Message                     │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │                                            │ │    │  │
│  │  │  │  [If Empty]                               │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  TableIcon                         │  │ │    │  │
│  │  │  │  │  "Execute a query to see results"  │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │                                            │ │    │  │
│  │  │  │  [If DML Result]                          │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Alert (Success)                   │  │ │    │  │
│  │  │  │  │  CheckCircle2 Icon                 │  │ │    │  │
│  │  │  │  │  "3 rows affected"                 │  │ │    │  │
│  │  │  │  │  Execution Time                    │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │                                            │ │    │  │
│  │  │  │  [If Results]                             │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Card > CardHeader                 │  │ │    │  │
│  │  │  │  │  - "Results (N rows)"              │  │ │    │  │
│  │  │  │  │  - Execution Time                  │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  │  ┌────────────────────────────────────┐  │ │    │  │
│  │  │  │  │  Card > CardContent                │  │ │    │  │
│  │  │  │  │  ┌──────────────────────────────┐ │  │ │    │  │
│  │  │  │  │  │  TanStack Table              │ │  │ │    │  │
│  │  │  │  │  │  - Sticky Header (Sortable)  │ │  │ │    │  │
│  │  │  │  │  │  - Virtualized Rows          │ │  │ │    │  │
│  │  │  │  │  │  - Zebra Striping            │ │  │ │    │  │
│  │  │  │  │  │  - Hover Effects             │ │  │ │    │  │
│  │  │  │  │  │  - NULL Indicators           │ │  │ │    │  │
│  │  │  │  │  │  - Scrollable                │ │  │ │    │  │
│  │  │  │  │  └──────────────────────────────┘ │  │ │    │  │
│  │  │  │  └────────────────────────────────────┘  │ │    │  │
│  │  │  └──────────────────────────────────────────┘ │    │  │
│  │  └───────────────────────────────────────────────┘    │  │
│  └─────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────┘
```

## Data Flow

```
┌──────────────┐
│     User     │
└──────┬───────┘
       │ Types SQL
       ▼
┌──────────────────┐
│   SQLEditor      │
│  (Monaco Editor) │
└──────┬───────────┘
       │ Ctrl+Enter or Click Execute
       ▼
┌──────────────────┐
│   QueryPanel     │───────┐ setState(loading: true)
│ (State Manager)  │       │ setState(error: null)
└──────┬───────────┘       │ setState(results: null)
       │                   │
       │ onExecuteQuery()  ▼
       ▼              ┌──────────────────┐
┌──────────────────┐ │  ResultsViewer   │
│  Tauri Command   │ │  (Shows Loading) │
│  invoke(         │ └──────────────────┘
│   'execute_query'│
│  )               │
└──────┬───────────┘
       │
       ├─── Success ──────────┐
       │                      │
       │                      ▼
       │              ┌──────────────────┐
       │              │   QueryPanel     │
       │              │ setState({       │
       │              │   results,       │
       │              │   loading: false │
       │              │ })               │
       │              └──────┬───────────┘
       │                     │
       │                     ▼
       │              ┌──────────────────┐
       │              │  ResultsViewer   │
       │              │  (Shows Table or │
       │              │   Rows Affected) │
       │              └──────────────────┘
       │
       └─── Error ───────────┐
                             │
                             ▼
                     ┌──────────────────┐
                     │   QueryPanel     │
                     │ setState({       │
                     │   error,         │
                     │   loading: false │
                     │ })               │
                     └──────┬───────────┘
                            │
                            ▼
                     ┌──────────────────┐
                     │  ResultsViewer   │
                     │  (Shows Alert    │
                     │   with Error)    │
                     └──────────────────┘
```

## State Management

### QueryPanel Internal State

```typescript
const [sql, setSql] = useState('');                    // Current SQL text
const [loading, setLoading] = useState(false);         // Query executing
const [results, setResults] = useState<QueryExecutionResult | null>(null);
const [error, setError] = useState<string | null>(null);
```

### State Transitions

```
Initial State:
  sql: ''
  loading: false
  results: null
  error: null

User Types:
  sql: 'SELECT * FROM users'
  loading: false
  results: null
  error: null

Execute Query:
  sql: 'SELECT * FROM users'
  loading: true     ◄── SET
  results: null     ◄── CLEAR
  error: null       ◄── CLEAR

Success:
  sql: 'SELECT * FROM users'
  loading: false    ◄── CLEAR
  results: { ... }  ◄── SET
  error: null

Error:
  sql: 'INVALID SQL'
  loading: false    ◄── CLEAR
  results: null
  error: 'Syntax error near "INVALID"'  ◄── SET
```

## Theme Integration

```
┌──────────────────┐
│  ThemeProvider   │
│  (Root Level)    │
└────────┬─────────┘
         │
         ├─── theme: "dark" ────────┐
         │                          │
         ▼                          ▼
┌────────────────┐         ┌────────────────┐
│   SQLEditor    │         │ ResultsViewer  │
│ useTheme()     │         │ Uses Tailwind  │
│                │         │ CSS Variables  │
│ Monaco:        │         │                │
│ - vs-dark      │         │ - bg-background│
│                │         │ - text-foreground
└────────────────┘         │ - border       │
                           └────────────────┘

         ├─── theme: "light" ───────┐
         │                          │
         ▼                          ▼
┌────────────────┐         ┌────────────────┐
│   SQLEditor    │         │ ResultsViewer  │
│ Monaco:        │         │ (Auto-switches │
│ - vs           │         │  via CSS vars) │
└────────────────┘         └────────────────┘
```

## Component Communication

```
Parent Component (e.g., App)
│
├─ Manages Connection State
│  const [connectionId, setConnectionId] = useState(null)
│
└─ Provides Query Executor
   const executeQuery = async (sql) => {
     return await invoke('execute_query', { connectionId, sql })
   }

   ▼ Props
┌──────────────────────────────┐
│       QueryPanel             │
│ - connectionId (from parent) │
│ - onExecuteQuery (callback)  │
└──────┬───────────────────────┘
       │
       ├─ Passes to SQLEditor ───────────┐
       │  - connectionId                 │
       │  - onExecute (wrapped)          │
       │  - value (internal state)       │
       │  - onChange (internal setState) │
       │                                 ▼
       │                         ┌──────────────┐
       │                         │  SQLEditor   │
       │                         │              │
       │                         │ - Shows conn │
       │                         │   status     │
       │                         │ - Calls      │
       │                         │   onExecute  │
       │                         └──────────────┘
       │
       └─ Passes to ResultsViewer ───────┐
          - columns (from state)         │
          - rows (from state)            │
          - error (from state)           │
          - loading (from state)         │
                                         ▼
                                 ┌──────────────┐
                                 │ResultsViewer │
                                 │              │
                                 │ - Displays   │
                                 │   results    │
                                 └──────────────┘
```

## File Dependencies

```
SQLEditor.tsx
├─ Imports
│  ├─ react
│  ├─ @monaco-editor/react
│  ├─ ./theme-provider (useTheme)
│  ├─ ./ui/card
│  ├─ ./ui/button
│  ├─ lucide-react (icons)
│  └─ @/lib/utils (cn)

ResultsViewer.tsx
├─ Imports
│  ├─ react
│  ├─ @tanstack/react-table
│  ├─ ./ui/card
│  ├─ ./ui/alert
│  ├─ lucide-react (icons)
│  └─ @/lib/utils (cn)

QueryPanel.tsx
├─ Imports
│  ├─ react
│  ├─ react-resizable-panels
│  ├─ ./SQLEditor
│  ├─ ./ResultsViewer
│  ├─ @/types/database (QueryExecutionResult)
│  └─ lucide-react (GripHorizontal)
```

## Performance Optimizations

### SQLEditor
- Monaco Editor uses web workers for syntax highlighting
- Lazy loading of Monaco (only loads when component mounts)
- Debounced change events (built into Monaco)

### ResultsViewer
- TanStack Table virtualizes rows (only renders visible rows)
- Column memoization prevents unnecessary re-renders
- Sticky header uses CSS (no JavaScript scroll listeners)

### QueryPanel
- State updates batched in async function
- Results only re-render when data changes
- Resizable panels use CSS transforms (GPU accelerated)

## Accessibility Features

### SQLEditor
- Keyboard shortcuts (Ctrl+Enter)
- Focus management
- ARIA labels on buttons
- Keyboard navigation in Monaco

### ResultsViewer
- Semantic HTML (`<table>`, `<th>`, `<td>`)
- ARIA role="alert" for errors
- Color contrast compliant
- Keyboard-navigable table

### QueryPanel
- Resizable panels keyboard accessible
- Focus trap when appropriate
- Screen reader announcements for state changes
