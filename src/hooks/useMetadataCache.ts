/**
 * useMetadataCache
 *
 * Lightweight event-bus helper to notify interested components (Schema Explorer,
 * table inspectors, etc.) that schema-level metadata has changed after a DDL
 * operation. There is no centralized cache abstraction in the app yet, so this
 * uses a window CustomEvent to coordinate refreshes.
 */

import { useCallback, useEffect } from "react";

export const METADATA_CHANGED_EVENT = "metadata-changed";

export interface MetadataChangePayload {
  /** Schema affected (optional). */
  schema?: string;
  /** Reason for the refresh (e.g., "create-table", "drop-table"). */
  reason?: string;
}

/**
 * Dispatch a metadata-changed event. Listeners (e.g. SchemaExplorer) will
 * refetch schemas/tables.
 */
export function notifyMetadataChanged(payload: MetadataChangePayload = {}): void {
  window.dispatchEvent(
    new CustomEvent<MetadataChangePayload>(METADATA_CHANGED_EVENT, {
      detail: payload,
    }),
  );
}

/**
 * Subscribe to metadata-changed events. Returns nothing; the handler is
 * cleaned up on unmount.
 */
export function useMetadataChangeListener(
  handler: (payload: MetadataChangePayload) => void,
): void {
  useEffect(() => {
    const listener = (e: Event) => {
      const detail = (e as CustomEvent<MetadataChangePayload>).detail ?? {};
      handler(detail);
    };
    window.addEventListener(METADATA_CHANGED_EVENT, listener);
    return () => window.removeEventListener(METADATA_CHANGED_EVENT, listener);
  }, [handler]);
}

/**
 * Hook returning a stable notifier callback for dispatching metadata refreshes.
 */
export function useMetadataCache() {
  const notify = useCallback((payload: MetadataChangePayload = {}) => {
    notifyMetadataChanged(payload);
  }, []);
  return { notify };
}
