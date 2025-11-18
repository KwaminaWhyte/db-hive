---
name: react-component
description: Generate React component boilerplate with TypeScript, proper props typing, error handling, and TailwindCSS styling. Use when creating new UI components for the database client.
allowed-tools: Read, Write, Edit, Grep
---

# React Component Generator

This skill helps you create properly structured React components with TypeScript, state management, and modern patterns.

## When to Use

Use this skill when:
- Creating a new React component
- Building UI elements that interact with Tauri commands
- Need proper TypeScript typing
- Want consistent component structure
- Building components with async operations

## Component Template Structure

### Basic Component
```typescript
import { FC } from 'react';

interface ComponentNameProps {
    prop1: string;
    prop2?: number;
    onAction?: () => void;
}

export const ComponentName: FC<ComponentNameProps> = ({
    prop1,
    prop2 = 0,
    onAction
}) => {
    return (
        <div className="p-4">
            <h2 className="text-lg font-semibold">{prop1}</h2>
        </div>
    );
};
```

### Component with State
```typescript
import { FC, useState } from 'react';

interface ComponentNameProps {
    initialValue: string;
}

export const ComponentName: FC<ComponentNameProps> = ({ initialValue }) => {
    const [value, setValue] = useState(initialValue);
    const [loading, setLoading] = useState(false);

    const handleSubmit = () => {
        setLoading(true);
        // Handle action
        setLoading(false);
    };

    return (
        <div className="flex flex-col gap-4 p-4">
            <input
                value={value}
                onChange={(e) => setValue(e.target.value)}
                className="px-3 py-2 border rounded-md"
            />
            <button
                onClick={handleSubmit}
                disabled={loading}
                className="px-4 py-2 bg-primary text-primary-foreground rounded-md disabled:opacity-50"
            >
                {loading ? 'Loading...' : 'Submit'}
            </button>
        </div>
    );
};
```

### Component with Tauri Command
```typescript
import { FC, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ConnectionFormProps {
    onSuccess?: (connectionId: string) => void;
    onError?: (error: string) => void;
}

interface ConnectionProfile {
    name: string;
    host: string;
    port: number;
    username: string;
    password: string;
}

interface DbError {
    kind: 'connection' | 'auth' | 'config';
    message: string;
}

export const ConnectionForm: FC<ConnectionFormProps> = ({
    onSuccess,
    onError
}) => {
    const [profile, setProfile] = useState<ConnectionProfile>({
        name: '',
        host: 'localhost',
        port: 5432,
        username: '',
        password: '',
    });
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const handleConnect = async () => {
        setLoading(true);
        setError(null);

        try {
            const connectionId = await invoke<string>('create_connection', {
                profile,
            });
            onSuccess?.(connectionId);
        } catch (err) {
            const dbError = err as DbError;
            const errorMsg = `${dbError.kind}: ${dbError.message}`;
            setError(errorMsg);
            onError?.(errorMsg);
        } finally {
            setLoading(false);
        }
    };

    const handleChange = (field: keyof ConnectionProfile, value: string | number) => {
        setProfile(prev => ({
            ...prev,
            [field]: value,
        }));
    };

    return (
        <div className="flex flex-col gap-4 p-6 bg-background rounded-lg">
            <h2 className="text-2xl font-bold">New Connection</h2>

            {error && (
                <div className="p-3 bg-destructive/10 text-destructive rounded-md">
                    {error}
                </div>
            )}

            <div className="flex flex-col gap-2">
                <label className="text-sm font-medium">Name</label>
                <input
                    type="text"
                    value={profile.name}
                    onChange={(e) => handleChange('name', e.target.value)}
                    className="px-3 py-2 border rounded-md"
                    placeholder="My Database"
                />
            </div>

            <div className="flex flex-col gap-2">
                <label className="text-sm font-medium">Host</label>
                <input
                    type="text"
                    value={profile.host}
                    onChange={(e) => handleChange('host', e.target.value)}
                    className="px-3 py-2 border rounded-md"
                    placeholder="localhost"
                />
            </div>

            <div className="flex flex-col gap-2">
                <label className="text-sm font-medium">Port</label>
                <input
                    type="number"
                    value={profile.port}
                    onChange={(e) => handleChange('port', parseInt(e.target.value))}
                    className="px-3 py-2 border rounded-md"
                />
            </div>

            <div className="flex flex-col gap-2">
                <label className="text-sm font-medium">Username</label>
                <input
                    type="text"
                    value={profile.username}
                    onChange={(e) => handleChange('username', e.target.value)}
                    className="px-3 py-2 border rounded-md"
                />
            </div>

            <div className="flex flex-col gap-2">
                <label className="text-sm font-medium">Password</label>
                <input
                    type="password"
                    value={profile.password}
                    onChange={(e) => handleChange('password', e.target.value)}
                    className="px-3 py-2 border rounded-md"
                />
            </div>

            <div className="flex gap-2 mt-4">
                <button
                    onClick={handleConnect}
                    disabled={loading}
                    className="flex-1 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    {loading ? 'Connecting...' : 'Connect'}
                </button>
            </div>
        </div>
    );
};
```

### Component with Streaming (Channel)
```typescript
import { FC, useState, useEffect } from 'react';
import { invoke, Channel } from '@tauri-apps/api/core';

interface QueryResultsProps {
    connectionId: string;
    sql: string;
}

interface ResultBatch {
    rows: Array<Record<string, any>>;
    hasMore: boolean;
}

interface QueryInfo {
    executionTime: number;
    totalRows: number;
}

export const QueryResults: FC<QueryResultsProps> = ({ connectionId, sql }) => {
    const [rows, setRows] = useState<Array<Record<string, any>>>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [info, setInfo] = useState<QueryInfo | null>(null);

    useEffect(() => {
        if (!sql) return;

        const executeQuery = async () => {
            setLoading(true);
            setError(null);
            setRows([]);

            try {
                const onBatch = new Channel<ResultBatch>();

                onBatch.onmessage = (batch) => {
                    setRows((prev) => [...prev, ...batch.rows]);
                };

                const queryInfo = await invoke<QueryInfo>('execute_query_streamed', {
                    connectionId,
                    sql,
                    onBatch,
                });

                setInfo(queryInfo);
            } catch (err) {
                setError(String(err));
            } finally {
                setLoading(false);
            }
        };

        executeQuery();
    }, [connectionId, sql]);

    if (loading) {
        return (
            <div className="flex items-center justify-center p-8">
                <div className="animate-spin h-8 w-8 border-4 border-primary border-t-transparent rounded-full" />
            </div>
        );
    }

    if (error) {
        return (
            <div className="p-4 bg-destructive/10 text-destructive rounded-md">
                {error}
            </div>
        );
    }

    return (
        <div className="flex flex-col gap-4">
            {info && (
                <div className="flex gap-4 text-sm text-muted-foreground">
                    <span>Execution time: {info.executionTime}ms</span>
                    <span>Rows: {info.totalRows}</span>
                </div>
            )}

            <div className="overflow-auto">
                <table className="w-full border-collapse">
                    <thead>
                        <tr className="border-b">
                            {rows[0] && Object.keys(rows[0]).map((key) => (
                                <th key={key} className="p-2 text-left font-medium">
                                    {key}
                                </th>
                            ))}
                        </tr>
                    </thead>
                    <tbody>
                        {rows.map((row, i) => (
                            <tr key={i} className="border-b hover:bg-muted/50">
                                {Object.values(row).map((value, j) => (
                                    <td key={j} className="p-2">
                                        {String(value)}
                                    </td>
                                ))}
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    );
};
```

## Custom Hooks Pattern

### useQuery Hook
```typescript
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface UseQueryResult<T> {
    data: T | null;
    loading: boolean;
    error: string | null;
    refetch: () => Promise<void>;
}

export function useQuery<T>(
    command: string,
    params: Record<string, any>
): UseQueryResult<T> {
    const [data, setData] = useState<T | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const execute = async () => {
        setLoading(true);
        setError(null);

        try {
            const result = await invoke<T>(command, params);
            setData(result);
        } catch (err) {
            setError(String(err));
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        execute();
    }, [command, JSON.stringify(params)]);

    return { data, loading, error, refetch: execute };
}

// Usage
const { data, loading, error } = useQuery<ConnectionProfile[]>('list_connections', {});
```

### useCommand Hook
```typescript
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface UseCommandResult<T> {
    execute: (params: Record<string, any>) => Promise<T>;
    data: T | null;
    loading: boolean;
    error: string | null;
}

export function useCommand<T>(command: string): UseCommandResult<T> {
    const [data, setData] = useState<T | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const execute = async (params: Record<string, any>): Promise<T> => {
        setLoading(true);
        setError(null);

        try {
            const result = await invoke<T>(command, params);
            setData(result);
            return result;
        } catch (err) {
            const errorMsg = String(err);
            setError(errorMsg);
            throw err;
        } finally {
            setLoading(false);
        }
    };

    return { execute, data, loading, error };
}

// Usage
const { execute, loading } = useCommand<string>('create_connection');

const handleConnect = async () => {
    try {
        const connectionId = await execute({ profile });
        console.log('Connected:', connectionId);
    } catch (err) {
        console.error('Failed:', err);
    }
};
```

## TailwindCSS Styling Patterns

### Button Variants
```typescript
// Primary button
<button className="px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90">
    Primary
</button>

// Secondary button
<button className="px-4 py-2 bg-secondary text-secondary-foreground rounded-md hover:bg-secondary/80">
    Secondary
</button>

// Destructive button
<button className="px-4 py-2 bg-destructive text-destructive-foreground rounded-md hover:bg-destructive/90">
    Delete
</button>

// Outline button
<button className="px-4 py-2 border border-input bg-background rounded-md hover:bg-accent hover:text-accent-foreground">
    Outline
</button>

// Ghost button
<button className="px-4 py-2 hover:bg-accent hover:text-accent-foreground rounded-md">
    Ghost
</button>
```

### Card Layouts
```typescript
<div className="rounded-lg border bg-card text-card-foreground shadow-sm">
    <div className="p-6 space-y-1.5">
        <h3 className="text-2xl font-semibold">Card Title</h3>
        <p className="text-sm text-muted-foreground">Card description</p>
    </div>
    <div className="p-6 pt-0">
        {/* Card content */}
    </div>
    <div className="flex items-center p-6 pt-0">
        {/* Card footer */}
    </div>
</div>
```

### Form Inputs
```typescript
// Text input
<input
    type="text"
    className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
    placeholder="Enter value"
/>

// Select
<select className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50">
    <option>Option 1</option>
    <option>Option 2</option>
</select>
```

## Testing Pattern

```typescript
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { ConnectionForm } from './ConnectionForm';

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(),
}));

describe('ConnectionForm', () => {
    beforeEach(() => {
        vi.clearAllMocks();
    });

    it('renders form fields', () => {
        render(<ConnectionForm />);

        expect(screen.getByLabelText(/name/i)).toBeInTheDocument();
        expect(screen.getByLabelText(/host/i)).toBeInTheDocument();
        expect(screen.getByLabelText(/port/i)).toBeInTheDocument();
    });

    it('submits form successfully', async () => {
        const { invoke } = await import('@tauri-apps/api/core');
        const mockInvoke = invoke as any;
        mockInvoke.mockResolvedValue('conn-123');

        const onSuccess = vi.fn();
        render(<ConnectionForm onSuccess={onSuccess} />);

        await userEvent.type(screen.getByLabelText(/name/i), 'Test DB');
        await userEvent.click(screen.getByRole('button', { name: /connect/i }));

        await waitFor(() => {
            expect(onSuccess).toHaveBeenCalledWith('conn-123');
        });
    });

    it('shows error on failure', async () => {
        const { invoke } = await import('@tauri-apps/api/core');
        const mockInvoke = invoke as any;
        mockInvoke.mockRejectedValue({
            kind: 'connection',
            message: 'Connection refused',
        });

        render(<ConnectionForm />);

        await userEvent.click(screen.getByRole('button', { name: /connect/i }));

        await waitFor(() => {
            expect(screen.getByText(/connection refused/i)).toBeInTheDocument();
        });
    });
});
```

## File Organization

```
components/
├── [feature]/
│   ├── ComponentName.tsx
│   ├── ComponentName.test.tsx
│   └── index.ts
└── ui/              # Reusable primitives
    ├── button.tsx
    ├── input.tsx
    ├── card.tsx
    └── index.ts
```

## Checklist

When creating a new component:

1. [ ] Create component file in appropriate directory
2. [ ] Define TypeScript interface for props
3. [ ] Implement component with proper typing
4. [ ] Add loading and error states for async operations
5. [ ] Style with TailwindCSS
6. [ ] Handle edge cases (empty state, errors)
7. [ ] Add accessibility attributes (aria-*, role)
8. [ ] Export from index file
9. [ ] Write tests
10. [ ] Document props and usage

## Remember

- Use TypeScript for type safety
- Handle loading and error states
- Keep components small and focused
- Use semantic HTML
- Add aria attributes for accessibility
- Test components thoroughly
- Use TailwindCSS utility classes
- Leverage custom hooks for reusable logic
