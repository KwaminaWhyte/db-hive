import type * as monaco from 'monaco-editor';
import type { AutocompleteMetadata, ColumnReference, TableReference } from '@/hooks/useAutocompleteMetadata';

// SQL keywords and functions for autocomplete
const SQL_KEYWORDS = [
  'SELECT', 'FROM', 'WHERE', 'INSERT', 'UPDATE', 'DELETE', 'CREATE', 'DROP',
  'ALTER', 'TABLE', 'DATABASE', 'INDEX', 'VIEW', 'JOIN', 'INNER', 'LEFT',
  'RIGHT', 'FULL', 'OUTER', 'ON', 'AS', 'AND', 'OR', 'NOT', 'IN', 'EXISTS',
  'BETWEEN', 'LIKE', 'IS', 'NULL', 'ORDER', 'BY', 'GROUP', 'HAVING', 'DISTINCT',
  'UNION', 'ALL', 'LIMIT', 'OFFSET', 'ASC', 'DESC', 'SET', 'VALUES', 'INTO',
  'PRIMARY', 'KEY', 'FOREIGN', 'REFERENCES', 'CONSTRAINT', 'UNIQUE', 'DEFAULT',
  'CHECK', 'CASCADE', 'BEGIN', 'COMMIT', 'ROLLBACK', 'TRANSACTION', 'CASE',
  'WHEN', 'THEN', 'ELSE', 'END', 'WITH', 'RECURSIVE', 'CAST', 'CROSS', 'USING',
];

const SQL_FUNCTIONS = [
  'COUNT', 'SUM', 'AVG', 'MIN', 'MAX', 'CONCAT', 'UPPER', 'LOWER', 'TRIM',
  'LENGTH', 'SUBSTRING', 'REPLACE', 'COALESCE', 'NULLIF', 'NOW', 'CURRENT_DATE',
  'CURRENT_TIME', 'CURRENT_TIMESTAMP', 'DATE', 'TIME', 'TIMESTAMP', 'EXTRACT',
  'DATE_ADD', 'DATE_SUB', 'DATEDIFF', 'ROUND', 'FLOOR', 'CEIL', 'ABS', 'POWER',
  'SQRT', 'MOD', 'RAND', 'GREATEST', 'LEAST', 'ROW_NUMBER', 'RANK', 'DENSE_RANK',
  'LAG', 'LEAD', 'FIRST_VALUE', 'LAST_VALUE', 'JSON_OBJECT', 'JSON_ARRAY',
  'JSON_EXTRACT', 'ARRAY_AGG', 'STRING_AGG', 'IF', 'IFNULL',
];

// Monaco completion providers are global per language, not per editor — so the
// provider is registered exactly once (PERF-16) and reads the current metadata
// from this module-level slot. Editors push fresh metadata via
// `setAutocompleteMetadata` instead of re-registering.
let currentMetadata: AutocompleteMetadata | null = null;
let registeredThisModule = false;

// Hot-reload guard: stash the disposable on globalThis so a re-evaluated
// module instance can dispose the provider registered by its predecessor
// instead of stacking a duplicate.
const HMR_DISPOSABLE_KEY = '__dbHiveSqlCompletionDisposable';

/**
 * Update the metadata the global SQL completion provider suggests from.
 * Call whenever the active connection/database metadata changes.
 */
export function setAutocompleteMetadata(metadata: AutocompleteMetadata | null): void {
  currentMetadata = metadata;
}

/**
 * Register the SQL autocomplete provider for Monaco Editor (idempotent).
 *
 * The provider is global per language, so this registers it once per module
 * instance and is a no-op on subsequent calls. No per-editor disposal is
 * needed — feed new metadata via `setAutocompleteMetadata` instead.
 *
 * The provider suggests:
 * - SQL keywords and functions
 * - Database names
 * - Schema names
 * - Table names (with schema context)
 * - Column names (with table and schema context)
 */
export function registerSqlAutocomplete(monacoInstance: typeof monaco): void {
  if (registeredThisModule) return;
  registeredThisModule = true;

  // Dispose any provider left over from a previous module instance (hot reload)
  const globalSlot = globalThis as Record<string, unknown> & {
    [HMR_DISPOSABLE_KEY]?: monaco.IDisposable;
  };
  globalSlot[HMR_DISPOSABLE_KEY]?.dispose();

  globalSlot[HMR_DISPOSABLE_KEY] = monacoInstance.languages.registerCompletionItemProvider('sql', {
    provideCompletionItems: (model, position) => {
      // Read metadata at completion time so the single global provider always
      // reflects the most recently mounted editor's connection/database.
      const metadata = currentMetadata;

      const word = model.getWordUntilPosition(position);
      const range: monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };

      const suggestions: monaco.languages.CompletionItem[] = [];

      // Get the text before the cursor to determine context
      const textUntilPosition = model.getValueInRange({
        startLineNumber: 1,
        startColumn: 1,
        endLineNumber: position.lineNumber,
        endColumn: position.column,
      });

      const textBeforeCursor = textUntilPosition.toUpperCase();

      // Determine what kind of suggestions to show based on context
      const shouldShowTables = /\bFROM\s+\w*$|\bJOIN\s+\w*$|\bINTO\s+\w*$|\bUPDATE\s+\w*$/i.test(textBeforeCursor);
      const shouldShowColumns = /\bSELECT\s+.*$|\bWHERE\s+.*$|\bON\s+.*$|\bSET\s+.*$|\bGROUP\s+BY\s+.*$|\bORDER\s+BY\s+.*$/i.test(textBeforeCursor);

      // Always suggest keywords
      SQL_KEYWORDS.forEach(keyword => {
        suggestions.push({
          label: keyword,
          kind: monacoInstance.languages.CompletionItemKind.Keyword,
          insertText: keyword,
          range,
          sortText: `0_${keyword}`, // Sort keywords first
        });
      });

      // Always suggest functions
      SQL_FUNCTIONS.forEach(func => {
        suggestions.push({
          label: func,
          kind: monacoInstance.languages.CompletionItemKind.Function,
          insertText: `${func}()`,
          insertTextRules: monacoInstance.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          range,
          sortText: `1_${func}`,
        });
      });

      if (!metadata) {
        return { suggestions };
      }

      // Suggest tables when after FROM, JOIN, INTO, UPDATE
      if (shouldShowTables) {
        metadata.tables.forEach((table: TableReference) => {
          const fullName = table.schema ? `${table.schema}.${table.table}` : table.table;
          suggestions.push({
            label: fullName,
            kind: monacoInstance.languages.CompletionItemKind.Class,
            insertText: fullName,
            detail: `Table in ${table.schema}`,
            range,
            sortText: `2_${fullName}`,
          });

          // Also suggest just the table name
          suggestions.push({
            label: table.table,
            kind: monacoInstance.languages.CompletionItemKind.Class,
            insertText: table.table,
            detail: `Table in ${table.schema}`,
            range,
            sortText: `3_${table.table}`,
          });
        });
      }

      // Suggest columns when after SELECT, WHERE, ON, SET, GROUP BY, ORDER BY
      if (shouldShowColumns) {
        metadata.columns.forEach((column: ColumnReference) => {
          const fullName = `${column.table}.${column.column}`;
          suggestions.push({
            label: fullName,
            kind: monacoInstance.languages.CompletionItemKind.Field,
            insertText: fullName,
            detail: `${column.dataType} - ${column.schema}.${column.table}`,
            range,
            sortText: `4_${fullName}`,
          });

          // Also suggest just the column name
          suggestions.push({
            label: column.column,
            kind: monacoInstance.languages.CompletionItemKind.Field,
            insertText: column.column,
            detail: `${column.dataType} - ${column.schema}.${column.table}.${column.column}`,
            range,
            sortText: `5_${column.column}`,
          });
        });
      }

      // Always suggest databases and schemas
      metadata.databases.forEach((db: string) => {
        suggestions.push({
          label: db,
          kind: monacoInstance.languages.CompletionItemKind.Module,
          insertText: db,
          detail: 'Database',
          range,
          sortText: `6_${db}`,
        });
      });

      metadata.schemas.forEach((schema: string) => {
        suggestions.push({
          label: schema,
          kind: monacoInstance.languages.CompletionItemKind.Module,
          insertText: schema,
          detail: 'Schema',
          range,
          sortText: `7_${schema}`,
        });
      });

      return { suggestions };
    },
  });
}
