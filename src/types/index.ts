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
  SqlExportOptions,
  SqlImportOptions,
} from './database';

export { getDefaultPort, getDriverDisplayName } from './database';

export type {
  AppSettings,
  GeneralSettings,
  ThemeSettings,
  QuerySettings,
  ShortcutsSettings,
  StartupBehavior,
  ThemeMode,
} from './settings';

export { defaultSettings } from './settings';
