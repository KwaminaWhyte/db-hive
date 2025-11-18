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
 * SSH tunnel configuration
 */
export interface SshConfig {
  host: string;
  port: number;
  username: string;
  privateKeyPath: string;
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
