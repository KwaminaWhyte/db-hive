/**
 * Schema migration types. Mirrors src-tauri/src/migrations/diff.rs and
 * src-tauri/src/commands/migrations.rs.
 */

export interface ColumnInfo {
  name: string;
  dataType: string;
  nullable: boolean;
  defaultValue: string | null;
  isPrimaryKey: boolean;
  isAutoIncrement: boolean;
}

export interface IndexInfo {
  name: string;
  columns: string[];
  isUnique: boolean;
  isPrimary: boolean;
}

export interface ForeignKeyInfo {
  name: string;
  table: string;
  schema: string;
  columns: string[];
  referencedTable: string;
  referencedSchema: string;
  referencedColumns: string[];
  onDelete: string | null;
  onUpdate: string | null;
}

export interface TableInfo {
  name: string;
  schema: string;
  rowCount: number | null;
  tableType: string;
}

export interface TableSchema {
  table: TableInfo;
  columns: ColumnInfo[];
  indexes: IndexInfo[];
}

export interface ColumnChange {
  name: string;
  source: ColumnInfo;
  target: ColumnInfo;
}

export interface TableDiff {
  schema: string;
  name: string;
  addedColumns: ColumnInfo[];
  removedColumns: ColumnInfo[];
  modifiedColumns: ColumnChange[];
  addedIndexes: IndexInfo[];
  removedIndexes: IndexInfo[];
  addedFks: ForeignKeyInfo[];
  removedFks: ForeignKeyInfo[];
}

export interface SchemaDiff {
  addedTables: TableSchema[];
  removedTables: TableSchema[];
  modifiedTables: TableDiff[];
}

export interface ApplyResult {
  executed: number;
  succeeded: number;
  failedStatement: string | null;
  error: string | null;
}
