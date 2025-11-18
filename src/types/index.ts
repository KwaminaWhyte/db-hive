/**
 * Type exports for DB-Hive
 */

export type {
  DbDriver,
  SslMode,
  SshConfig,
  ConnectionProfile,
  ConnectionStatus,
  DbError,
  QueryExecutionResult,
  QueryError,
  DatabaseInfo,
  SchemaInfo,
  TableInfo,
  ColumnInfo,
  IndexInfo,
  TableSchema,
} from './database';

export { getDefaultPort, getDriverDisplayName } from './database';
