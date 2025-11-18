/**
 * QueryPanel Integration Example
 *
 * This file demonstrates how to integrate the QueryPanel component
 * with Tauri backend commands for database query execution.
 *
 * NOTE: This is an example file. The actual Tauri commands need to be
 * implemented in src-tauri/src/commands/ first.
 */

import { FC, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { QueryPanel } from '@/components/QueryPanel';
import { ConnectionProfile, QueryExecutionResult } from '@/types/database';

/**
 * Example component showing QueryPanel integration
 */
export const QueryPanelExample: FC = () => {
  // In a real app, this would come from a connection store or state management
  const [activeConnectionId] = useState<string | null>(null);

  /**
   * Execute query via Tauri command
   *
   * This function calls the Rust backend to execute the SQL query
   * against the active database connection.
   *
   * Expected Rust command signature:
   * ```rust
   * #[tauri::command]
   * async fn execute_query(
   *     connection_id: String,
   *     sql: String,
   * ) -> Result<QueryExecutionResult, DbError> {
   *     // Implementation
   * }
   * ```
   */
  const executeQuery = async (sql: string): Promise<QueryExecutionResult> => {
    if (!activeConnectionId) {
      throw new Error('No active connection');
    }

    try {
      // Call Tauri command to execute query
      const result = await invoke<QueryExecutionResult>('execute_query', {
        connectionId: activeConnectionId,
        sql,
      });

      return result;
    } catch (error: any) {
      // Handle Tauri errors
      console.error('Query execution failed:', error);
      throw error;
    }
  };

  return (
    <div className="h-screen w-full">
      {/* Connection selector (simplified for example) */}
      <div className="h-12 border-b px-4 flex items-center gap-4 bg-background">
        <h1 className="font-semibold">DB-Hive Query Editor</h1>
        <div className="text-sm text-muted-foreground">
          {activeConnectionId
            ? `Connected to: ${activeConnectionId}`
            : 'No connection selected'}
        </div>
      </div>

      {/* Query Panel */}
      <div className="h-[calc(100vh-3rem)]">
        <QueryPanel
          connectionId={activeConnectionId}
          onExecuteQuery={executeQuery}
        />
      </div>
    </div>
  );
};

/**
 * Example with connection management
 *
 * This shows a more complete integration with connection selection
 */
export const FullQueryExample: FC = () => {
  const [connections, setConnections] = useState<ConnectionProfile[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(null);

  // Load connections on mount
  const loadConnections = async () => {
    try {
      const profiles = await invoke<ConnectionProfile[]>('list_connections');
      setConnections(profiles);
    } catch (error) {
      console.error('Failed to load connections:', error);
    }
  };

  // Connect to a database
  const connectToDatabase = async (connectionId: string) => {
    try {
      await invoke('connect_database', { connectionId });
      setActiveConnectionId(connectionId);
    } catch (error) {
      console.error('Connection failed:', error);
      throw error;
    }
  };

  // Execute query
  const executeQuery = async (sql: string): Promise<QueryExecutionResult> => {
    if (!activeConnectionId) {
      throw new Error('No active connection');
    }

    const result = await invoke<QueryExecutionResult>('execute_query', {
      connectionId: activeConnectionId,
      sql,
    });

    return result;
  };

  return (
    <div className="flex h-screen">
      {/* Sidebar with connections */}
      <div className="w-64 border-r bg-background">
        <div className="p-4">
          <h2 className="font-semibold mb-4">Connections</h2>
          <button
            onClick={loadConnections}
            className="text-sm text-blue-600 hover:underline mb-4"
          >
            Refresh
          </button>
          <div className="space-y-2">
            {connections.map((conn) => (
              <button
                key={conn.id}
                onClick={() => connectToDatabase(conn.id)}
                className={`w-full text-left px-3 py-2 rounded text-sm ${
                  activeConnectionId === conn.id
                    ? 'bg-primary text-primary-foreground'
                    : 'hover:bg-muted'
                }`}
              >
                {conn.name}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Query Panel */}
      <div className="flex-1">
        <QueryPanel
          connectionId={activeConnectionId}
          onExecuteQuery={executeQuery}
        />
      </div>
    </div>
  );
};

/**
 * Mock implementation for testing without backend
 *
 * This can be used during development to test the UI before
 * the Rust backend is ready.
 */
export const MockQueryPanel: FC = () => {
  const [activeConnectionId] = useState<string>('mock-connection-1');

  // Mock query execution
  const mockExecuteQuery = async (sql: string): Promise<QueryExecutionResult> => {
    // Simulate network delay
    await new Promise((resolve) => setTimeout(resolve, 500));

    const sqlUpper = sql.toUpperCase().trim();

    // Mock SELECT query
    if (sqlUpper.startsWith('SELECT')) {
      return {
        columns: ['id', 'name', 'email', 'created_at'],
        rows: [
          [1, 'John Doe', 'john@example.com', '2024-01-15'],
          [2, 'Jane Smith', 'jane@example.com', '2024-01-16'],
          [3, 'Bob Johnson', 'bob@example.com', '2024-01-17'],
          [4, 'Alice Brown', 'alice@example.com', '2024-01-18'],
          [5, 'Charlie Wilson', 'charlie@example.com', '2024-01-19'],
        ],
        rowsAffected: null,
        executionTime: 123,
      };
    }

    // Mock INSERT/UPDATE/DELETE query
    if (
      sqlUpper.startsWith('INSERT') ||
      sqlUpper.startsWith('UPDATE') ||
      sqlUpper.startsWith('DELETE')
    ) {
      return {
        columns: [],
        rows: [],
        rowsAffected: 3,
        executionTime: 45,
      };
    }

    // Mock error for invalid SQL
    throw new Error('Syntax error near "' + sql.substring(0, 20) + '"');
  };

  return (
    <div className="h-screen w-full">
      <div className="h-12 border-b px-4 flex items-center gap-4 bg-background">
        <h1 className="font-semibold">Mock Query Panel (Development)</h1>
        <div className="text-sm text-muted-foreground">
          Mock Connection - Try: SELECT * FROM users
        </div>
      </div>
      <div className="h-[calc(100vh-3rem)]">
        <QueryPanel
          connectionId={activeConnectionId}
          onExecuteQuery={mockExecuteQuery}
        />
      </div>
    </div>
  );
};
