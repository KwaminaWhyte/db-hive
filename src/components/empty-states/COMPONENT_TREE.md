# Empty State Component Architecture

## Component Hierarchy

```
EmptyState (Base Component)
├── Props
│   ├── title: string
│   ├── message: string
│   ├── icon?: LucideIcon
│   ├── illustration?: ReactNode
│   ├── actions?: EmptyStateAction[]
│   ├── className?: string
│   ├── iconClassName?: string
│   └── size?: "sm" | "md" | "lg"
│
├── Layout
│   ├── Icon/Illustration Container
│   │   └── Animated entrance (zoom-in, fade-in)
│   ├── Content Container
│   │   ├── Title (h3)
│   │   └── Message (p)
│   └── Actions Container
│       └── Button[] (with icons)
│
└── Variants
    ├── NoConnectionsEmpty
    │   ├── Icon: Database
    │   ├── Color: Blue
    │   └── Action: "Add Connection"
    │
    ├── NoHistoryEmpty
    │   ├── Icon: History
    │   ├── Color: Violet
    │   └── Action: "Run Your First Query" (optional)
    │
    ├── NoTablesEmpty
    │   ├── Icon: Table
    │   ├── Color: Emerald
    │   └── Action: "Create Table" (optional)
    │
    ├── NoSearchResultsEmpty
    │   ├── Icon: SearchX
    │   ├── Color: Amber
    │   ├── Size: Small
    │   └── Action: "Clear Search"
    │
    ├── NoResultsEmpty
    │   ├── Icon: FileQuestion
    │   ├── Color: Slate
    │   ├── Size: Small
    │   └── Action: "Run Another Query" (optional)
    │
    └── NoDataEmpty
        ├── Icon: Inbox
        ├── Color: Cyan
        └── Action: "Insert Data" (optional)
```

## Animation Timeline

```
Time (ms)  │ Component         │ Animation
───────────┼───────────────────┼──────────────────────────────
0          │ Container         │ Start fade-in + slide-up
100        │ Icon/Illustration │ Start zoom-in + fade-in
200        │ Content           │ Start fade-in + slide-up
300        │ Actions           │ Start fade-in + slide-up
500        │ Container         │ Complete
700        │ Icon/Illustration │ Complete
700        │ Content           │ Complete
800        │ Actions           │ Complete
```

## Color Palette

```
Component            │ Light Mode                  │ Dark Mode
─────────────────────┼─────────────────────────────┼──────────────────────────────
NoConnectionsEmpty   │ bg-blue-50, text-blue-600   │ bg-blue-950/30, text-blue-400
NoHistoryEmpty       │ bg-violet-50, text-violet-600│ bg-violet-950/30, text-violet-400
NoTablesEmpty        │ bg-emerald-50, text-emerald-600│ bg-emerald-950/30, text-emerald-400
NoSearchResultsEmpty │ bg-amber-50, text-amber-600 │ bg-amber-950/30, text-amber-400
NoResultsEmpty       │ bg-slate-50, text-slate-600 │ bg-slate-950/30, text-slate-400
NoDataEmpty          │ bg-cyan-50, text-cyan-600   │ bg-cyan-950/30, text-cyan-400
```

## Size Configuration

```
Size │ Icon   │ Title  │ Message │ Container Padding │ Gap
─────┼────────┼────────┼─────────┼───────────────────┼─────
sm   │ 48px   │ base   │ sm      │ py-8              │ gap-3
md   │ 64px   │ lg     │ base    │ py-12             │ gap-4
lg   │ 80px   │ xl     │ lg      │ py-16             │ gap-6
```

## Import Patterns

### Individual Import
```typescript
import { NoConnectionsEmpty } from "@/components/empty-states/NoConnectionsEmpty";
```

### Barrel Import (Recommended)
```typescript
import { NoConnectionsEmpty, NoHistoryEmpty } from "@/components/empty-states";
```

### Base Component Import
```typescript
import { EmptyState } from "@/components/EmptyState";
import type { EmptyStateProps, EmptyStateAction } from "@/components/EmptyState";
```

## Usage Flow

```
User Event / State Change
        ↓
Conditional Rendering
        ↓
Empty State Component
        ↓
EmptyState Base Component
        ↓
┌───────────────────────────┐
│   Render Animation        │
│   1. Container fade-in    │
│   2. Icon zoom-in         │
│   3. Content fade-in      │
│   4. Actions fade-in      │
└───────────────────────────┘
        ↓
User Interaction
        ↓
Action Callback
        ↓
State Update / Navigation
```

## File Dependencies

```
EmptyState.tsx
├── Dependencies
│   ├── react (FC, ReactNode)
│   ├── lucide-react (LucideIcon)
│   ├── @/components/ui/button (Button)
│   └── @/lib/utils (cn)
└── Exports
    ├── EmptyState (component)
    ├── EmptyStateProps (type)
    └── EmptyStateAction (type)

NoConnectionsEmpty.tsx
├── Dependencies
│   ├── react (FC)
│   ├── lucide-react (Database, Plus)
│   └── ../EmptyState (EmptyState, EmptyStateAction)
└── Exports
    ├── NoConnectionsEmpty (component)
    └── NoConnectionsEmptyProps (type)

[Similar structure for other variants...]

index.tsx
├── Dependencies
│   └── All variant components
└── Exports
    └── All variants + types (barrel export)
```

## State Management Pattern

```typescript
// Component State
const [showDialog, setShowDialog] = useState(false);
const [connections, setConnections] = useState<Connection[]>([]);

// Conditional Render
{connections.length === 0 ? (
  <NoConnectionsEmpty
    onAddConnection={() => setShowDialog(true)}
  />
) : (
  <ConnectionList connections={connections} />
)}

// Dialog opens when action clicked
<ConnectionDialog
  open={showDialog}
  onClose={() => setShowDialog(false)}
  onSave={handleSaveConnection}
/>
```

## Testing Strategy

```
Unit Tests
├── Base Component
│   ├── Renders with required props
│   ├── Renders with optional props
│   ├── Handles icon rendering
│   ├── Handles illustration rendering
│   ├── Renders multiple actions
│   ├── Applies size variants
│   └── Applies custom classes
│
└── Specific Variants
    ├── Renders correct icon
    ├── Renders correct message
    ├── Calls action callback
    ├── Shows/hides optional actions
    └── Personalizes message with props

Integration Tests
├── Empty state shows when data is empty
├── Content shows when data exists
├── Actions trigger correct behavior
└── State updates correctly

Visual Tests
├── Animations work correctly
├── Colors match design
├── Dark mode works
└── Responsive layout works
```

## Extension Points

Want to add a new empty state? Follow this pattern:

```typescript
import { FC } from "react";
import { YourIcon } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface YourEmptyStateProps {
  onAction: () => void;
  contextProp?: string;
  className?: string;
}

export const YourEmptyState: FC<YourEmptyStateProps> = ({
  onAction,
  contextProp,
  className,
}) => {
  const actions: EmptyStateAction[] = [
    {
      label: "Action Label",
      onClick: onAction,
      icon: ActionIcon,
      variant: "default",
    },
  ];

  return (
    <EmptyState
      icon={YourIcon}
      title="Your Title"
      message="Your helpful message here."
      actions={actions}
      className={className}
      iconClassName="bg-color-50 dark:bg-color-950/30 text-color-600 dark:text-color-400"
      size="md" // or "sm" / "lg"
    />
  );
};
```

Then add to `index.tsx`:
```typescript
export { YourEmptyState } from "./YourEmptyState";
export type { YourEmptyStateProps } from "./YourEmptyState";
```
