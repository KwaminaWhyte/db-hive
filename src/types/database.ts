/**
 * Database types for DB-Hive
 *
 * These types match the Rust backend models defined in src-tauri/src/models/
 * Note: Rust uses snake_case, TypeScript uses camelCase for field names
 */

/**
 * Supported database drivers
 */
export type DbDriver =
  | 'Postgres'
  | 'MySql'
  | 'Sqlite'
  | 'MongoDb'
  | 'SqlServer';

/**
 * SSL/TLS connection mode
 */
export type SslMode =
  | 'Disable'
  | 'Prefer'
  | 'Require';

/**
 * SSH authentication method
 */
export type SshAuthMethod = 'Password' | 'PrivateKey';

/**
 * SSH tunnel configuration
 */
export interface SshConfig {
  /** SSH server hostname or IP address */
  host: string;
  /** SSH server port (typically 22) */
  port: number;
  /** SSH username */
  username: string;
  /** Authentication method (password or private key) */
  authMethod: SshAuthMethod;
  /** Path to the private key file (only used with PrivateKey auth) */
  privateKeyPath?: string | null;
  /** Passphrase for encrypted private keys (optional) */
  keyPassphraseKeyringKey?: string | null;
  /** Local port to bind the tunnel to (0 = auto-assign) */
  localPort: number;
}

/**
 * Connection profile
 *
 * Represents a saved database connection with all necessary configuration.
 * Passwords are not stored directly but referenced via a keyring key for security.
 */
export interface ConnectionProfile {
  /** Unique identifier for this connection profile */
  id: string;

  /** User-friendly name for this connection */
  name: string;

  /** Database driver type */
  driver: DbDriver;

  /** Database server hostname or IP address (for SQLite, this is the file path) */
  host: string;

  /** Database server port */
  port: number;

  /** Username for database authentication */
  username: string;

  /** Key to retrieve the password from the OS keyring (if None, no password is required) */
  passwordKeyringKey?: string | null;

  /** Default database/schema to connect to */
  database?: string | null;

  /** SSL/TLS mode for the connection */
  sslMode: SslMode;

  /** Optional SSH tunnel configuration for accessing remote databases */
  sshTunnel?: SshConfig | null;

  /** Optional folder/group for organizing connections in the UI */
  folder?: string | null;
}

/**
 * Connection status
 */
export type ConnectionStatus =
  | 'Connected'
  | 'Disconnected'
  | { Error: string };

/**
 * Database error
 *
 * Represents errors returned from the Rust backend via Tauri IPC
 */
export interface DbError {
  kind:
    | 'connection'
    | 'query'
    | 'auth'
    | 'timeout'
    | 'invalid_input'
    | 'not_found'
    | 'internal';
  message: string;
}

/**
 * Get default port for a database driver
 */
export function getDefaultPort(driver: DbDriver): number {
  switch (driver) {
    case 'Postgres':
      return 5432;
    case 'MySql':
      return 3306;
    case 'Sqlite':
      return 0;
    case 'MongoDb':
      return 27017;
    case 'SqlServer':
      return 1433;
    default:
      return 0;
  }
}

/**
 * Get display name for a database driver
 */
export function getDriverDisplayName(driver: DbDriver): string {
  switch (driver) {
    case 'Postgres':
      return 'PostgreSQL';
    case 'MySql':
      return 'MySQL';
    case 'Sqlite':
      return 'SQLite';
    case 'MongoDb':
      return 'MongoDB';
    case 'SqlServer':
      return 'SQL Server';
    default:
      return driver;
  }
}

/**
 * Query execution result
 *
 * Represents the result of executing a SQL query
 */
export interface QueryExecutionResult {
  /** Column names in the result set */
  columns: string[];

  /** Row data as array of arrays */
  rows: any[][];

  /** Number of rows affected by DML statements (INSERT, UPDATE, DELETE) */
  rowsAffected: number | null;

  /** Query execution time in milliseconds */
  executionTime: number;
}

/**
 * Query error
 *
 * Extends DbError with query-specific information
 */
export interface QueryError extends DbError {
  /** The SQL query that caused the error */
  sql?: string;

  /** Line number where error occurred (if available) */
  line?: number;

  /** Column number where error occurred (if available) */
  column?: number;
}

/**
 * Database information
 *
 * Represents a database/catalog within a database server
 */
export interface DatabaseInfo {
  /** Database name */
  name: string;

  /** Database owner/creator (if available) */
  owner?: string | null;

  /** Database size in bytes (if available) */
  size?: number | null;
}

/**
 * Schema information
 *
 * Represents a schema/namespace within a database
 */
export interface SchemaInfo {
  /** Schema name */
  name: string;

  /** Parent database name */
  database: string;
}

/**
 * Table information
 *
 * Represents a table or view within a schema
 */
export interface TableInfo {
  /** Table name */
  name: string;

  /** Parent schema name */
  schema: string;

  /** Approximate row count (if available) */
  rowCount?: number | null;

  /** Table type: "TABLE", "VIEW", "MATERIALIZED VIEW", etc. */
  tableType: string;
}

/**
 * Column information
 *
 * Represents a column within a table
 */
export interface ColumnInfo {
  /** Column name */
  name: string;

  /** Data type (e.g., "VARCHAR(255)", "INTEGER", "TIMESTAMP") */
  dataType: string;

  /** Whether the column accepts NULL values */
  nullable: boolean;

  /** Default value expression (if any) */
  defaultValue?: string | null;

  /** Whether this column is part of the primary key */
  isPrimaryKey: boolean;

  /** Whether this column is auto-increment/serial (MySQL AUTO_INCREMENT, PostgreSQL SERIAL) */
  isAutoIncrement: boolean;
}

/**
 * Index information
 *
 * Represents an index on a table
 */
export interface IndexInfo {
  /** Index name */
  name: string;

  /** Columns included in the index (in order) */
  columns: string[];

  /** Whether this is a unique index */
  isUnique: boolean;

  /** Whether this is the primary key index */
  isPrimary: boolean;
}

/**
 * Foreign key information
 *
 * Represents a foreign key constraint that references another table.
 * This is used for ER diagrams and understanding table relationships.
 */
export interface ForeignKeyInfo {
  /** Foreign key constraint name */
  name: string;

  /** Table that contains this foreign key */
  table: string;

  /** Schema of the table that contains this foreign key */
  schema: string;

  /** Columns in this table that make up the foreign key */
  columns: string[];

  /** Referenced table name */
  referencedTable: string;

  /** Schema of the referenced table */
  referencedSchema: string;

  /** Columns in the referenced table */
  referencedColumns: string[];

  /** ON DELETE action (CASCADE, SET NULL, RESTRICT, NO ACTION, etc.) */
  onDelete?: string | null;

  /** ON UPDATE action (CASCADE, SET NULL, RESTRICT, NO ACTION, etc.) */
  onUpdate?: string | null;
}

/**
 * Complete table schema
 *
 * Contains all metadata about a table
 */
export interface TableSchema {
  /** Table metadata */
  table: TableInfo;

  /** Column definitions */
  columns: ColumnInfo[];

  /** Indexes defined on the table */
  indexes: IndexInfo[];
}

/**
 * SQL Export Options
 *
 * Configuration for exporting database structure and data to SQL dump files
 */
export interface SqlExportOptions {
  /** Include DROP TABLE statements before CREATE TABLE */
  includeDrop: boolean;

  /** Include CREATE TABLE statements */
  includeCreate: boolean;

  /** Include INSERT statements (data) */
  includeData: boolean;

  /** Filter by specific tables (empty array = all tables) */
  tables: string[];

  /** Schema to export from (PostgreSQL/MySQL). Defaults to "public" if not specified */
  schema?: string | null;
}

/**
 * SQL Import Options
 *
 * Configuration for importing SQL dump files into a database
 */
export interface SqlImportOptions {
  /** Continue importing even if some statements fail */
  continueOnError: boolean;

  /** Use a transaction (rollback all changes on error) */
  useTransaction: boolean;
}
