/**
 * Activity Logging & Monitoring Types
 *
 * Types for tracking query execution, session monitoring,
 * and performance metrics.
 */

/**
 * Query execution status
 */
export type QueryStatus = 'running' | 'completed' | 'failed' | 'cancelled';

/**
 * Query type classification
 */
export type QueryType =
  | 'SELECT'
  | 'INSERT'
  | 'UPDATE'
  | 'DELETE'
  | 'CREATE'
  | 'ALTER'
  | 'DROP'
  | 'TRANSACTION'
  | 'OTHER';

/**
 * Individual query log entry
 */
export interface QueryLog {
  /** Unique log entry ID */
  id: string;
  /** Connection ID this query was executed on */
  connectionId: string;
  /** Connection profile name */
  connectionName: string;
  /** Database name (if applicable) */
  database?: string;
  /** SQL query text */
  sql: string;
  /** Query type (SELECT, INSERT, etc.) */
  queryType: QueryType;
  /** Execution status */
  status: QueryStatus;
  /** Start timestamp (ISO 8601) */
  startedAt: string;
  /** End timestamp (ISO 8601) */
  completedAt?: string;
  /** Execution duration in milliseconds */
  durationMs?: number;
  /** Number of rows affected/returned */
  rowCount?: number;
  /** Error message if failed */
  error?: string;
  /** User-added tags for categorization */
  tags?: string[];
}

/**
 * Performance metrics snapshot
 */
export interface PerformanceMetrics {
  /** Timestamp of this snapshot */
  timestamp: string;
  /** CPU usage percentage (0-100) */
  cpuUsage: number;
  /** Memory usage in bytes */
  memoryUsage: number;
  /** Memory usage percentage (0-100) */
  memoryPercent: number;
  /** Number of active connections */
  activeConnections: number;
  /** Number of running queries */
  runningQueries: number;
}

/**
 * Database session information
 */
export interface DatabaseSession {
  /** Session ID */
  id: string;
  /** Connection ID */
  connectionId: string;
  /** Database name */
  database?: string;
  /** Session state (active, idle, etc.) */
  state: string;
  /** Current query being executed */
  currentQuery?: string;
  /** Session start time */
  startedAt: string;
  /** Last activity timestamp */
  lastActivityAt: string;
  /** Client application name */
  application?: string;
  /** Client host/IP */
  clientHost?: string;
}

/**
 * Query log filter options
 */
export interface QueryLogFilter {
  /** Filter by connection ID */
  connectionId?: string;
  /** Filter by database name */
  database?: string;
  /** Filter by query type */
  queryType?: QueryType;
  /** Filter by status */
  status?: QueryStatus;
  /** Filter by minimum duration (ms) */
  minDuration?: number;
  /** Filter by maximum duration (ms) */
  maxDuration?: number;
  /** Filter by date range start */
  startDate?: string;
  /** Filter by date range end */
  endDate?: string;
  /** Search text in query SQL */
  searchText?: string;
  /** Filter by tags */
  tags?: string[];
}

/**
 * Sort options for query logs
 */
export interface QueryLogSort {
  /** Field to sort by */
  field: 'startedAt' | 'durationMs' | 'rowCount' | 'connectionName';
  /** Sort direction */
  direction: 'asc' | 'desc';
}

/**
 * Paginated query log response
 */
export interface QueryLogResponse {
  /** Query logs for this page */
  logs: QueryLog[];
  /** Total number of logs matching filter */
  total: number;
  /** Current page number (0-indexed) */
  page: number;
  /** Page size */
  pageSize: number;
  /** Total number of pages */
  totalPages: number;
}

/**
 * Activity statistics
 */
export interface ActivityStats {
  /** Total queries executed */
  totalQueries: number;
  /** Total failed queries */
  failedQueries: number;
  /** Average query duration (ms) */
  avgDuration: number;
  /** Total rows affected */
  totalRows: number;
  /** Queries by type */
  queriesByType: Record<QueryType, number>;
  /** Queries by status */
  queriesByStatus: Record<QueryStatus, number>;
}

/**
 * Export format options
 */
export type ExportFormat = 'json' | 'csv' | 'txt';

/**
 * Export options
 */
export interface ExportOptions {
  /** Export format */
  format: ExportFormat;
  /** Include filters in export */
  includeFilters?: boolean;
  /** File path to save to */
  filePath?: string;
}
