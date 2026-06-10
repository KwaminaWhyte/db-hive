import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AutocompleteMetadata } from './useAutocompleteMetadata';

/**
 * Schema context for the AI assistant, derived from the backend's cached
 * autocomplete metadata (`get_autocomplete_metadata`, 16-way concurrency +
 * 5-minute cache) instead of the old sequential per-table IPC waterfall
 * (PERF-01).
 *
 * Results are additionally cached at module level per connection + database
 * so the context is fetched once per connection, not once per mounted
 * QueryPanel (PERF-02).
 */

// Match the backend MetadataCache staleness window (5 minutes).
const CACHE_TTL_MS = 5 * 60 * 1000;

interface CacheEntry {
  promise: Promise<string>;
  timestamp: number;
}

const schemaContextCache = new Map<string, CacheEntry>();

const SYSTEM_SCHEMAS = [
  'information_schema',
  'pg_catalog',
  'mysql',
  'performance_schema',
  'sys',
];

/**
 * Build the AI schema context string from flattened autocomplete metadata.
 *
 * Keeps the same shape the AiAssistant has always received:
 *
 *   Database: <name>
 *
 *   Tables:
 *
 *   schema.table:
 *     - column: dataType
 */
function buildSchemaContext(
  metadata: AutocompleteMetadata,
  database?: string
): string {
  // Group columns by "schema.table" for quick lookup
  const columnsByTable = new Map<string, AutocompleteMetadata['columns']>();
  for (const col of metadata.columns) {
    const key = `${col.schema}.${col.table}`;
    const existing = columnsByTable.get(key);
    if (existing) {
      existing.push(col);
    } else {
      columnsByTable.set(key, [col]);
    }
  }

  // Skip system schemas, prioritize 'public' (PostgreSQL), then alphabetical
  const tables = metadata.tables
    .filter((t) => !SYSTEM_SCHEMAS.includes(t.schema))
    .sort((a, b) => {
      if (a.schema !== b.schema) {
        if (a.schema === 'public') return -1;
        if (b.schema === 'public') return 1;
        return a.schema.localeCompare(b.schema);
      }
      return a.table.localeCompare(b.table);
    });

  if (tables.length === 0) {
    return `Database: ${database || 'unknown'}\n\nNo tables found.`;
  }

  let context = `Database: ${database || 'unknown'}\n\nTables:\n`;
  for (const table of tables) {
    context += `\n${table.schema}.${table.table}:\n`;
    const columns = columnsByTable.get(`${table.schema}.${table.table}`) || [];
    for (const col of columns) {
      context += `  - ${col.column}: ${col.dataType}\n`;
    }
  }

  return context;
}

async function fetchSchemaContext(
  connectionId: string,
  database?: string
): Promise<string> {
  const metadata = await invoke<AutocompleteMetadata>(
    'get_autocomplete_metadata',
    {
      connectionId,
      database: database || '',
      forceRefresh: false,
    }
  );
  return buildSchemaContext(metadata, database);
}

/**
 * Get the schema context for a connection + database, deduplicating
 * concurrent callers and caching the result for the backend's TTL.
 */
export function getSchemaContext(
  connectionId: string,
  database?: string
): Promise<string> {
  const key = `${connectionId}::${database || ''}`;
  const entry = schemaContextCache.get(key);

  if (entry && Date.now() - entry.timestamp < CACHE_TTL_MS) {
    return entry.promise;
  }

  const promise = fetchSchemaContext(connectionId, database).catch((err) => {
    // Don't cache failures — allow the next caller to retry
    schemaContextCache.delete(key);
    throw err;
  });

  schemaContextCache.set(key, { promise, timestamp: Date.now() });
  return promise;
}

/**
 * React hook returning the AI schema context string for the given
 * connection + database. Empty string while loading, on error, or when
 * there is no connection.
 */
export function useSchemaContext(
  connectionId: string | null,
  database?: string
): string {
  const [context, setContext] = useState('');

  useEffect(() => {
    if (!connectionId) {
      setContext('');
      return;
    }

    let cancelled = false;
    getSchemaContext(connectionId, database)
      .then((ctx) => {
        if (!cancelled) setContext(ctx);
      })
      .catch((err) => {
        console.error('Failed to load schema context:', err);
        if (!cancelled) setContext('');
      });

    return () => {
      cancelled = true;
    };
  }, [connectionId, database]);

  return context;
}
