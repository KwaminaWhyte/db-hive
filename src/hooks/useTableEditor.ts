import { useState, useCallback } from 'react';
import { CellChange } from '@/components/EditableCell';
import { ColumnInfo } from '@/types';

export interface RowChanges {
  /** Map of column name to { oldValue, newValue } */
  changes: Map<string, { oldValue: any; newValue: any }>;
}

export interface TableEditorState {
  /** Map of row index to row changes */
  changes: Map<number, RowChanges>;

  /** Currently editing cell { rowIndex, columnIndex } */
  editingCell: { rowIndex: number; columnIndex: number } | null;

  /** Selected rows for bulk operations */
  selectedRows: Set<number>;
}

export interface UseTableEditorOptions {
  /** Table columns schema */
  columns: ColumnInfo[];

  /** Current page data */
  rows: any[][];
}

export function useTableEditor({ columns, rows }: UseTableEditorOptions) {
  const [state, setState] = useState<TableEditorState>({
    changes: new Map(),
    editingCell: null,
    selectedRows: new Set(),
  });

  /**
   * Start editing a cell
   */
  const startEdit = useCallback((rowIndex: number, columnIndex: number) => {
    setState((prev) => ({
      ...prev,
      editingCell: { rowIndex, columnIndex },
    }));
  }, []);

  /**
   * Cancel cell editing
   */
  const cancelEdit = useCallback(() => {
    setState((prev) => ({
      ...prev,
      editingCell: null,
    }));
  }, []);

  /**
   * Apply a cell change
   */
  const applyChange = useCallback((change: CellChange) => {
    setState((prev) => {
      const newChanges = new Map(prev.changes);

      // Get or create row changes
      let rowChanges = newChanges.get(change.rowIndex);
      if (!rowChanges) {
        rowChanges = { changes: new Map() };
        newChanges.set(change.rowIndex, rowChanges);
      }

      // Add or update column change
      rowChanges.changes.set(change.columnName, {
        oldValue: change.oldValue,
        newValue: change.newValue,
      });

      // If new value equals old value, remove the change
      if (change.newValue === change.oldValue) {
        rowChanges.changes.delete(change.columnName);
        // If no more changes for this row, remove row entry
        if (rowChanges.changes.size === 0) {
          newChanges.delete(change.rowIndex);
        }
      }

      return {
        ...prev,
        changes: newChanges,
        editingCell: null,
      };
    });
  }, []);

  /**
   * Toggle row selection
   */
  const toggleRowSelection = useCallback((rowIndex: number) => {
    setState((prev) => {
      const newSelection = new Set(prev.selectedRows);
      if (newSelection.has(rowIndex)) {
        newSelection.delete(rowIndex);
      } else {
        newSelection.add(rowIndex);
      }
      return {
        ...prev,
        selectedRows: newSelection,
      };
    });
  }, []);

  /**
   * Clear all selections
   */
  const clearSelection = useCallback(() => {
    setState((prev) => ({
      ...prev,
      selectedRows: new Set(),
    }));
  }, []);

  /**
   * Select all rows
   */
  const selectAll = useCallback(() => {
    setState((prev) => ({
      ...prev,
      selectedRows: new Set(rows.map((_, idx) => idx)),
    }));
  }, [rows]);

  /**
   * Discard all changes
   */
  const discardChanges = useCallback(() => {
    setState((prev) => ({
      ...prev,
      changes: new Map(),
    }));
  }, []);

  /**
   * Get the effective value for a cell (with changes applied)
   */
  const getCellValue = useCallback(
    (rowIndex: number, columnName: string): any => {
      const rowChanges = state.changes.get(rowIndex);
      if (rowChanges?.changes.has(columnName)) {
        return rowChanges.changes.get(columnName)!.newValue;
      }

      // Return original value
      const columnIndex = columns.findIndex((col) => col.name === columnName);
      if (columnIndex === -1) return null;
      return rows[rowIndex]?.[columnIndex];
    },
    [state.changes, columns, rows]
  );

  /**
   * Check if a cell has been modified
   */
  const isCellModified = useCallback(
    (rowIndex: number, columnName: string): boolean => {
      const rowChanges = state.changes.get(rowIndex);
      return rowChanges?.changes.has(columnName) ?? false;
    },
    [state.changes]
  );

  /**
   * Get total number of changes
   */
  const getTotalChanges = useCallback((): number => {
    let total = 0;
    state.changes.forEach((rowChanges) => {
      total += rowChanges.changes.size;
    });
    return total;
  }, [state.changes]);

  /**
   * Get primary key columns
   */
  const getPrimaryKeyColumns = useCallback((): ColumnInfo[] => {
    return columns.filter((col) => col.isPrimaryKey);
  }, [columns]);

  /**
   * Generate UPDATE statements for all changes
   */
  const generateUpdateStatements = useCallback(
    (schema: string, tableName: string, quoteIdentifier: (id: string) => string): string[] => {
      const statements: string[] = [];
      const pkColumns = getPrimaryKeyColumns();

      if (pkColumns.length === 0) {
        throw new Error('Cannot generate UPDATE statements: table has no primary key');
      }

      state.changes.forEach((rowChanges, rowIndex) => {
        const row = rows[rowIndex];
        if (!row) return;

        // Build SET clause
        const setClauses: string[] = [];
        rowChanges.changes.forEach((change, columnName) => {
          const quotedColumn = quoteIdentifier(columnName);
          const value = change.newValue === null ? 'NULL' :
                        typeof change.newValue === 'string' ? `'${change.newValue.replace(/'/g, "''")}'` :
                        String(change.newValue);
          setClauses.push(`${quotedColumn} = ${value}`);
        });

        // Build WHERE clause using primary key
        const whereClauses: string[] = [];
        pkColumns.forEach((pkCol) => {
          const columnIndex = columns.findIndex((col) => col.name === pkCol.name);
          const pkValue = row[columnIndex];
          const quotedColumn = quoteIdentifier(pkCol.name);
          const value = pkValue === null ? 'NULL' :
                        typeof pkValue === 'string' ? `'${pkValue.replace(/'/g, "''")}'` :
                        String(pkValue);

          if (pkValue === null) {
            whereClauses.push(`${quotedColumn} IS NULL`);
          } else {
            whereClauses.push(`${quotedColumn} = ${value}`);
          }
        });

        const statement = `UPDATE ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)} SET ${setClauses.join(', ')} WHERE ${whereClauses.join(' AND ')};`;
        statements.push(statement);
      });

      return statements;
    },
    [state.changes, rows, columns, getPrimaryKeyColumns]
  );

  return {
    // State
    changes: state.changes,
    editingCell: state.editingCell,
    selectedRows: state.selectedRows,

    // Cell editing
    startEdit,
    cancelEdit,
    applyChange,
    isCellModified,
    getCellValue,

    // Row selection
    toggleRowSelection,
    clearSelection,
    selectAll,

    // Changes
    discardChanges,
    getTotalChanges,
    generateUpdateStatements,
    getPrimaryKeyColumns,
  };
}
