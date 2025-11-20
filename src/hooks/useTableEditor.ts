import { useState, useCallback } from 'react';
import { CellChange } from '@/components/EditableCell';
import { ColumnInfo } from '@/types';

export interface RowChanges {
  /** Map of column name to { oldValue, newValue } */
  changes: Map<string, { oldValue: any; newValue: any }>;
}

export interface NewRow {
  /** Temporary negative index for this new row */
  tempId: number;

  /** Map of column name to value */
  values: Map<string, any>;
}

export interface TableEditorState {
  /** Map of row index to row changes */
  changes: Map<number, RowChanges>;

  /** Currently editing cell { rowIndex, columnIndex } */
  editingCell: { rowIndex: number; columnIndex: number } | null;

  /** Selected rows for bulk operations */
  selectedRows: Set<number>;

  /** New rows to be inserted (using negative indices) */
  newRows: Map<number, NewRow>;

  /** Counter for generating unique negative IDs */
  nextNewRowId: number;
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
    newRows: new Map(),
    nextNewRowId: -1,
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
      // If this is a new row (negative index), update it directly
      if (change.rowIndex < 0) {
        const newRows = new Map(prev.newRows);
        const row = newRows.get(change.rowIndex);
        if (row) {
          const updatedValues = new Map(row.values);
          updatedValues.set(change.columnName, change.newValue);
          newRows.set(change.rowIndex, {
            ...row,
            values: updatedValues,
          });
        }
        return {
          ...prev,
          newRows,
          editingCell: null,
        };
      }

      // Handle existing row changes
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
   * Discard all changes and new rows
   */
  const discardChanges = useCallback(() => {
    setState((prev) => ({
      ...prev,
      changes: new Map(),
      newRows: new Map(),
    }));
  }, []);

  /**
   * Get the effective value for a cell (with changes applied)
   */
  const getCellValue = useCallback(
    (rowIndex: number, columnName: string): any => {
      // Check if this is a new row (negative index)
      if (rowIndex < 0) {
        const newRow = state.newRows.get(rowIndex);
        return newRow?.values.get(columnName) ?? null;
      }

      // Check for changes in existing rows
      const rowChanges = state.changes.get(rowIndex);
      if (rowChanges?.changes.has(columnName)) {
        return rowChanges.changes.get(columnName)!.newValue;
      }

      // Return original value
      const columnIndex = columns.findIndex((col) => col.name === columnName);
      if (columnIndex === -1) return null;
      return rows[rowIndex]?.[columnIndex];
    },
    [state.changes, state.newRows, columns, rows]
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
          const newValue = change.newValue;

          let value: string;
          if (newValue === null || newValue === undefined) {
            value = 'NULL';
          } else if (typeof newValue === 'boolean') {
            value = newValue ? 'true' : 'false';
          } else if (typeof newValue === 'string') {
            // Check if this is a boolean column
            const col = columns.find(c => c.name === columnName);
            const lowerType = col?.dataType.toLowerCase() || '';
            if (lowerType.includes('bool') && (newValue === 'true' || newValue === 'false')) {
              value = newValue;  // Unquoted for boolean
            } else {
              value = `'${newValue.replace(/'/g, "''")}'`;  // Quoted for string
            }
          } else if (typeof newValue === 'number') {
            value = String(newValue);
          } else {
            value = `'${String(newValue).replace(/'/g, "''")}'`;
          }

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

  /**
   * Generate DELETE statements for selected rows
   */
  const generateDeleteStatements = useCallback(
    (schema: string, tableName: string, quoteIdentifier: (id: string) => string): string[] => {
      const statements: string[] = [];
      const pkColumns = getPrimaryKeyColumns();

      if (pkColumns.length === 0) {
        throw new Error('Cannot generate DELETE statements: table has no primary key');
      }

      state.selectedRows.forEach((rowIndex) => {
        const row = rows[rowIndex];
        if (!row) return;

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

        const statement = `DELETE FROM ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)} WHERE ${whereClauses.join(' AND ')};`;
        statements.push(statement);
      });

      return statements;
    },
    [state.selectedRows, rows, columns, getPrimaryKeyColumns]
  );

  /**
   * Add a new row with default values
   */
  const addRow = useCallback(() => {
    setState((prev) => {
      const newRowId = prev.nextNewRowId;
      const values = new Map<string, any>();

      // Initialize with default values or NULL
      columns.forEach((col) => {
        if (col.defaultValue !== null && col.defaultValue !== undefined) {
          values.set(col.name, col.defaultValue);
        } else {
          values.set(col.name, null);
        }
      });

      const newRow: NewRow = {
        tempId: newRowId,
        values,
      };

      const newRows = new Map(prev.newRows);
      newRows.set(newRowId, newRow);

      return {
        ...prev,
        newRows,
        nextNewRowId: prev.nextNewRowId - 1,
      };
    });
  }, [columns]);

  /**
   * Remove a new row (before it's committed)
   */
  const removeNewRow = useCallback((tempId: number) => {
    setState((prev) => {
      const newRows = new Map(prev.newRows);
      newRows.delete(tempId);
      return {
        ...prev,
        newRows,
      };
    });
  }, []);

  /**
   * Update a value in a new row
   */
  const updateNewRowValue = useCallback((tempId: number, columnName: string, value: any) => {
    setState((prev) => {
      const newRows = new Map(prev.newRows);
      const row = newRows.get(tempId);
      if (!row) return prev;

      const updatedValues = new Map(row.values);
      updatedValues.set(columnName, value);

      newRows.set(tempId, {
        ...row,
        values: updatedValues,
      });

      return {
        ...prev,
        newRows,
      };
    });
  }, []);

  /**
   * Check if a column should be auto-generated by the database
   */
  const shouldSkipColumnInInsert = useCallback((col: ColumnInfo): boolean => {
    const lowerName = col.name.toLowerCase();
    const lowerType = col.dataType.toLowerCase();

    console.log(`[shouldSkipColumnInInsert] Checking column: ${col.name}`, {
      name: col.name,
      dataType: col.dataType,
      isPrimaryKey: col.isPrimaryKey,
      isAutoIncrement: col.isAutoIncrement,
      defaultValue: col.defaultValue,
      nullable: col.nullable,
    });

    // Skip auto-increment columns (detected by backend drivers)
    if (col.isAutoIncrement) {
      console.log(`[shouldSkipColumnInInsert] ✓ Skipping ${col.name}: auto-increment column`);
      return true;
    }

    // Skip if column has a default value that's a function/expression
    if (col.defaultValue) {
      const defaultLower = col.defaultValue.toLowerCase();
      if (
        defaultLower.includes('uuid_generate') ||
        defaultLower.includes('gen_random_uuid') ||
        defaultLower.includes('current_timestamp') ||
        defaultLower.includes('now()') ||
        defaultLower.includes('getdate()')
      ) {
        console.log(`[shouldSkipColumnInInsert] ✓ Skipping ${col.name}: has function default`);
        return true;
      }
    }

    // Skip UUID primary keys ONLY if they have a default value (like DEFAULT UUID())
    // If no default, we need to generate the UUID in the application
    if (col.isPrimaryKey &&
        (lowerType.includes('char') || lowerType.includes('uuid')) &&
        (lowerName === 'id' || lowerName === 'uuid' || lowerName === 'guid' || lowerName.endsWith('_id'))) {
      // Only skip if there's a default UUID generator
      if (col.defaultValue &&
          (col.defaultValue.toLowerCase().includes('uuid') ||
           col.defaultValue.toLowerCase().includes('gen_random_uuid'))) {
        console.log(`[shouldSkipColumnInInsert] ✓ Skipping ${col.name}: UUID/GUID primary key with default`);
        return true;
      }
      // Don't skip - need to provide UUID value
      console.log(`[shouldSkipColumnInInsert] ✗ NOT skipping ${col.name}: UUID primary key WITHOUT default (must provide value)`);
      return false;
    }

    // Skip common auto-generated timestamp columns
    if ((lowerName === 'created_at' || lowerName === 'updated_at') &&
        (lowerType.includes('timestamp') || lowerType.includes('datetime'))) {
      console.log(`[shouldSkipColumnInInsert] ✓ Skipping ${col.name}: auto-timestamp column`);
      return true;
    }

    console.log(`[shouldSkipColumnInInsert] ✗ NOT skipping ${col.name}`);
    return false;
  }, []);

  /**
   * Generate INSERT statements for new rows
   */
  const generateInsertStatements = useCallback(
    (schema: string, tableName: string, quoteIdentifier: (id: string) => string): string[] => {
      console.log('[generateInsertStatements] Called with:', {
        schema,
        tableName,
        newRowsCount: state.newRows.size,
        newRowsKeys: Array.from(state.newRows.keys()),
      });

      const statements: string[] = [];

      state.newRows.forEach((newRow, tempId) => {
        console.log(`[generateInsertStatements] Processing row ${tempId}:`, {
          rowData: Object.fromEntries(newRow.values),
        });

        const columnNames: string[] = [];
        const values: string[] = [];

        columns.forEach((col) => {
          // Skip auto-generated columns
          if (shouldSkipColumnInInsert(col)) {
            console.log(`[generateInsertStatements] Skipping auto-generated column: ${col.name}`);
            return;
          }

          let value = newRow.values.get(col.name);

          // Generate UUID for UUID primary keys that don't have a value
          if ((value === null || value === undefined) &&
              col.isPrimaryKey &&
              (col.dataType.toLowerCase().includes('char') || col.dataType.toLowerCase().includes('uuid')) &&
              (col.name.toLowerCase() === 'id' || col.name.toLowerCase() === 'uuid' ||
               col.name.toLowerCase() === 'guid' || col.name.toLowerCase().endsWith('_id'))) {
            // Generate a v4 UUID
            value = crypto.randomUUID();
            console.log(`[generateInsertStatements] Generated UUID for ${col.name}:`, value);
          }

          console.log(`[generateInsertStatements] Column ${col.name} value:`, value);

          columnNames.push(quoteIdentifier(col.name));

          if (value === null || value === undefined) {
            values.push('NULL');
          } else if (typeof value === 'boolean') {
            // Boolean values should be unquoted (true/false, not 'true'/'false')
            values.push(value ? 'true' : 'false');
          } else if (typeof value === 'string') {
            // Check if this is a boolean column with string value
            const lowerType = col.dataType.toLowerCase();
            if (lowerType.includes('bool') && (value === 'true' || value === 'false')) {
              // PostgreSQL boolean: unquoted
              values.push(value);
            } else {
              // Regular string: quoted with escaped single quotes
              values.push(`'${value.replace(/'/g, "''")}'`);
            }
          } else if (typeof value === 'number') {
            // Numbers: unquoted
            values.push(String(value));
          } else {
            // Everything else: convert to string and quote
            values.push(`'${String(value).replace(/'/g, "''")}'`);
          }
        });

        // If all columns were skipped (only auto-generated), insert with DEFAULT VALUES
        if (columnNames.length === 0) {
          const statement = `INSERT INTO ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)} DEFAULT VALUES;`;
          console.log(`[generateInsertStatements] All columns skipped, using DEFAULT VALUES:`, statement);
          statements.push(statement);
        } else {
          const statement = `INSERT INTO ${quoteIdentifier(schema)}.${quoteIdentifier(tableName)} (${columnNames.join(', ')}) VALUES (${values.join(', ')});`;
          console.log(`[generateInsertStatements] Generated INSERT:`, statement);
          statements.push(statement);
        }
      });

      console.log(`[generateInsertStatements] Total statements generated: ${statements.length}`);
      return statements;
    },
    [state.newRows, columns, shouldSkipColumnInInsert]
  );

  return {
    // State
    changes: state.changes,
    editingCell: state.editingCell,
    selectedRows: state.selectedRows,
    newRows: state.newRows,

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

    // New rows
    addRow,
    removeNewRow,
    updateNewRowValue,

    // Changes
    discardChanges,
    getTotalChanges,
    generateUpdateStatements,
    generateDeleteStatements,
    generateInsertStatements,
    getPrimaryKeyColumns,
  };
}
