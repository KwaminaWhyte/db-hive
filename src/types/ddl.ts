/**
 * DDL (Data Definition Language) types for schema management
 *
 * These types match the Rust definitions in src-tauri/src/models/ddl.rs
 * and are used for creating, altering, and dropping database objects.
 */

/**
 * Column data type
 *
 * Represents common database column types. Each database driver will map these
 * to their native types (e.g., `integer` -> `INT` in MySQL, `INTEGER` in SQLite).
 */
export type ColumnType =
  // Integer types
  | { type: "smallInt" }
  | { type: "integer" }
  | { type: "bigInt" }
  // Decimal types
  | { type: "decimal"; precision: number; scale: number }
  | { type: "real" }
  | { type: "doublePrecision" }
  // String types
  | { type: "varchar"; length: number }
  | { type: "char"; length: number }
  | { type: "text" }
  // Binary types
  | { type: "bytea" }
  // Boolean
  | { type: "boolean" }
  // Date/Time types
  | { type: "date" }
  | { type: "time" }
  | { type: "timestamp" }
  | { type: "timestampTz" }
  // JSON types
  | { type: "json" }
  | { type: "jsonB" }
  // UUID
  | { type: "uuid" }
  // Array (PostgreSQL-specific)
  | { type: "array"; elementType: ColumnType }
  // Database-specific custom type
  | { type: "custom"; typeName: string };

/**
 * Foreign key action on DELETE or UPDATE
 */
export type ForeignKeyAction =
  | "NO_ACTION"
  | "RESTRICT"
  | "CASCADE"
  | "SET_NULL"
  | "SET_DEFAULT";

/**
 * Foreign key constraint definition
 */
export interface ForeignKeyConstraint {
  /** Name of the constraint (optional, will be auto-generated if not provided) */
  name?: string;
  /** Columns in this table that reference another table */
  columns: string[];
  /** Referenced table name */
  referencedTable: string;
  /** Referenced columns in the target table */
  referencedColumns: string[];
  /** Action on DELETE */
  onDelete: ForeignKeyAction;
  /** Action on UPDATE */
  onUpdate: ForeignKeyAction;
}

/**
 * Unique constraint definition
 */
export interface UniqueConstraint {
  /** Name of the constraint (optional) */
  name?: string;
  /** Columns that must be unique together */
  columns: string[];
}

/**
 * Check constraint definition
 */
export interface CheckConstraint {
  /** Name of the constraint (optional) */
  name?: string;
  /** SQL expression for the check (e.g., "age >= 18") */
  expression: string;
}

/**
 * Index type
 */
export type IndexType = "B_TREE" | "HASH" | "GIST" | "GIN";

/**
 * Index definition
 */
export interface IndexDefinition {
  /** Index name */
  name: string;
  /** Columns to index */
  columns: string[];
  /** Whether this is a unique index */
  unique: boolean;
  /** Index type (defaults to BTree) */
  indexType: IndexType;
}

/**
 * Column definition for table creation/alteration
 */
export interface ColumnDefinition {
  /** Column name */
  name: string;
  /** Column data type */
  columnType: ColumnType;
  /** Whether the column allows NULL values */
  nullable: boolean;
  /** Default value expression (SQL string, e.g., "0", "CURRENT_TIMESTAMP") */
  default?: string;
  /** Whether this column is part of the primary key */
  primaryKey: boolean;
  /** Whether this column is auto-incrementing (SERIAL in PostgreSQL, AUTO_INCREMENT in MySQL) */
  autoIncrement: boolean;
  /** Comment/description for the column */
  comment?: string;
}

/**
 * Table definition for creation
 */
export interface TableDefinition {
  /** Schema name (defaults to "public" in PostgreSQL, ignored in SQLite/MySQL without USE) */
  schema?: string;
  /** Table name */
  name: string;
  /** Column definitions */
  columns: ColumnDefinition[];
  /** Primary key columns (if composite, or if not set on individual columns) */
  primaryKey?: string[];
  /** Foreign key constraints */
  foreignKeys: ForeignKeyConstraint[];
  /** Unique constraints */
  uniqueConstraints: UniqueConstraint[];
  /** Check constraints */
  checkConstraints: CheckConstraint[];
  /** Table comment/description */
  comment?: string;
  /** If true, add "IF NOT EXISTS" clause */
  ifNotExists: boolean;
}

/**
 * Operation for altering a table column
 */
export type AlterColumnOperation =
  | {
      type: "addColumn";
      column: ColumnDefinition;
    }
  | {
      type: "dropColumn";
      columnName: string;
      cascade: boolean;
    }
  | {
      type: "renameColumn";
      oldName: string;
      newName: string;
    }
  | {
      type: "alterType";
      columnName: string;
      newType: ColumnType;
    }
  | {
      type: "setNotNull";
      columnName: string;
      notNull: boolean;
    }
  | {
      type: "setDefault";
      columnName: string;
      default?: string;
    };

/**
 * Table alteration definition
 */
export interface AlterTableDefinition {
  /** Schema name */
  schema?: string;
  /** Table name */
  name: string;
  /** Column operations */
  operations: AlterColumnOperation[];
}

/**
 * Request to drop a table
 */
export interface DropTableDefinition {
  /** Schema name */
  schema?: string;
  /** Table name */
  name: string;
  /** If true, also drop dependent objects (CASCADE) */
  cascade: boolean;
  /** If true, add "IF EXISTS" clause */
  ifExists: boolean;
}

/**
 * DDL operation result with generated SQL
 */
export interface DdlResult {
  /** Generated SQL statement(s) */
  sql: string[];
  /** Success message */
  message: string;
}

/**
 * Helper functions for creating column types
 */
export const ColumnTypes = {
  smallInt: (): ColumnType => ({ type: "smallInt" }),
  integer: (): ColumnType => ({ type: "integer" }),
  bigInt: (): ColumnType => ({ type: "bigInt" }),
  decimal: (precision: number, scale: number): ColumnType => ({
    type: "decimal",
    precision,
    scale,
  }),
  real: (): ColumnType => ({ type: "real" }),
  doublePrecision: (): ColumnType => ({ type: "doublePrecision" }),
  varchar: (length: number): ColumnType => ({ type: "varchar", length }),
  char: (length: number): ColumnType => ({ type: "char", length }),
  text: (): ColumnType => ({ type: "text" }),
  bytea: (): ColumnType => ({ type: "bytea" }),
  boolean: (): ColumnType => ({ type: "boolean" }),
  date: (): ColumnType => ({ type: "date" }),
  time: (): ColumnType => ({ type: "time" }),
  timestamp: (): ColumnType => ({ type: "timestamp" }),
  timestampTz: (): ColumnType => ({ type: "timestampTz" }),
  json: (): ColumnType => ({ type: "json" }),
  jsonB: (): ColumnType => ({ type: "jsonB" }),
  uuid: (): ColumnType => ({ type: "uuid" }),
  array: (elementType: ColumnType): ColumnType => ({
    type: "array",
    elementType,
  }),
  custom: (typeName: string): ColumnType => ({ type: "custom", typeName }),
};

/**
 * Get a human-readable label for a column type
 */
export function getColumnTypeLabel(columnType: ColumnType): string {
  switch (columnType.type) {
    case "smallInt":
      return "Small Integer";
    case "integer":
      return "Integer";
    case "bigInt":
      return "Big Integer";
    case "decimal":
      return `Decimal(${columnType.precision}, ${columnType.scale})`;
    case "real":
      return "Real";
    case "doublePrecision":
      return "Double Precision";
    case "varchar":
      return `VARCHAR(${columnType.length})`;
    case "char":
      return `CHAR(${columnType.length})`;
    case "text":
      return "Text";
    case "bytea":
      return "Bytea";
    case "boolean":
      return "Boolean";
    case "date":
      return "Date";
    case "time":
      return "Time";
    case "timestamp":
      return "Timestamp";
    case "timestampTz":
      return "Timestamp with Timezone";
    case "json":
      return "JSON";
    case "jsonB":
      return "JSONB";
    case "uuid":
      return "UUID";
    case "array":
      return `${getColumnTypeLabel(columnType.elementType)}[]`;
    case "custom":
      return columnType.typeName;
  }
}
