import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface TableReference {
  schema: string;
  table: string;
}

export interface ColumnReference {
  schema: string;
  table: string;
  column: string;
  dataType: string;
}

export interface AutocompleteMetadata {
  databases: string[];
  schemas: string[];
  tables: TableReference[];
  columns: ColumnReference[];
}

interface UseAutocompleteMetadataOptions {
  connectionId: string | null;
  database: string | null;
  enabled?: boolean;
}

export function useAutocompleteMetadata({
  connectionId,
  database,
  enabled = true,
}: UseAutocompleteMetadataOptions) {
  const [metadata, setMetadata] = useState<AutocompleteMetadata | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchMetadata = useCallback(async (forceRefresh = false) => {
    if (!connectionId || !database || !enabled) {
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const result = await invoke<AutocompleteMetadata>('get_autocomplete_metadata', {
        connectionId,
        database,
        forceRefresh,
      });

      setMetadata(result);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      console.error('Failed to fetch autocomplete metadata:', errorMessage);
    } finally {
      setLoading(false);
    }
  }, [connectionId, database, enabled]);

  // Fetch metadata when connection or database changes
  useEffect(() => {
    if (connectionId && database && enabled) {
      fetchMetadata(false);
    }
  }, [connectionId, database, enabled, fetchMetadata]);

  const refresh = useCallback(() => {
    return fetchMetadata(true);
  }, [fetchMetadata]);

  return {
    metadata,
    loading,
    error,
    refresh,
  };
}
