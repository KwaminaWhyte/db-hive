import { createContext, useContext, useState, useRef, ReactNode, useCallback, useEffect, useMemo } from "react";

/**
 * Tab State Interface
 *
 * Stores state specific to each tab (query content, table filters, etc.)
 */
export interface TabState {
  id: string;
  type: "query" | "table" | "redis";
  label: string;

  // For query tabs
  sql?: string;
  results?: any;

  // For redis key-value tabs
  redisKey?: string;

  // For table tabs
  schema?: string;
  tableName?: string;
  page?: number;
  limit?: number;
  filter?: string;
  sortBy?: string;
  sortOrder?: "asc" | "desc";
}

interface TabContextValue {
  // Get state for a specific tab
  getTabState: (tabId: string) => TabState | undefined;

  // Update state for a specific tab
  updateTabState: (tabId: string, updates: Partial<TabState>) => void;

  // Create new tab state
  createTabState: (state: TabState) => void;

  // Remove tab state
  removeTabState: (tabId: string) => void;

  // Get all tab states
  getAllTabStates: () => Record<string, TabState>;

  // Restore tab states (for persistence)
  restoreTabStates: (states: Record<string, TabState>) => void;
}

const TabContext = createContext<TabContextValue | undefined>(undefined);

interface TabProviderProps {
  children: ReactNode;
  connectionId: string | null;
  currentDatabase: string | null;
}

/**
 * Strip non-persistable fields (e.g. query results) before serializing tab
 * states to localStorage. `results` can hold megabytes of rows — persisting it
 * would synchronously stringify all of it on every tab mutation (PERF-15).
 */
function toPersistableStates(states: Record<string, TabState>): Record<string, TabState> {
  const persistable: Record<string, TabState> = {};
  for (const [id, state] of Object.entries(states)) {
    const { results: _results, ...rest } = state;
    persistable[id] = rest;
  }
  return persistable;
}

export function TabProvider({ children, connectionId, currentDatabase }: TabProviderProps) {
  const [tabStates, setTabStates] = useState<Record<string, TabState>>({});

  // Ref mirror of tabStates — the source of truth for getters so that
  // `getTabState`/`getAllTabStates` are identity-stable ([] deps) and don't
  // re-trigger consumer effects/callbacks on every tab mutation (PERF-15).
  // All mutators update the ref synchronously before scheduling the state
  // update, so getters always see fresh values within the same tick.
  const tabStatesRef = useRef<Record<string, TabState>>(tabStates);

  const applyTabStates = useCallback((next: Record<string, TabState>) => {
    tabStatesRef.current = next;
    setTabStates(next);
  }, []);

  // Load tab states from localStorage when connection OR database changes
  useEffect(() => {
    if (connectionId && currentDatabase) {
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      const saved = localStorage.getItem(storageKey);

      if (saved) {
        try {
          const parsed = JSON.parse(saved);
          applyTabStates(parsed.states || {});
        } catch (error) {
          console.error("Failed to parse saved tab states:", error);
        }
      } else {
        // No saved tabs for this database, clear current tabs
        applyTabStates({});
      }
    } else {
      // No connection or database, clear tabs
      applyTabStates({});
    }
  }, [connectionId, currentDatabase, applyTabStates]);

  // Save tab states to localStorage whenever they change
  useEffect(() => {
    if (connectionId && currentDatabase && Object.keys(tabStates).length > 0) {
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      localStorage.setItem(storageKey, JSON.stringify({
        states: toPersistableStates(tabStates),
        timestamp: Date.now(),
      }));
    }
  }, [tabStates, connectionId, currentDatabase]);

  const getTabState = useCallback((tabId: string) => {
    return tabStatesRef.current[tabId];
  }, []);

  const updateTabState = useCallback((tabId: string, updates: Partial<TabState>) => {
    const prev = tabStatesRef.current;
    applyTabStates({
      ...prev,
      [tabId]: {
        ...prev[tabId],
        ...updates,
      } as TabState,
    });
  }, [applyTabStates]);

  const createTabState = useCallback((state: TabState) => {
    applyTabStates({
      ...tabStatesRef.current,
      [state.id]: state,
    });
  }, [applyTabStates]);

  const removeTabState = useCallback((tabId: string) => {
    const newStates = { ...tabStatesRef.current };
    delete newStates[tabId];
    applyTabStates(newStates);
  }, [applyTabStates]);

  const getAllTabStates = useCallback(() => {
    return tabStatesRef.current;
  }, []);

  const restoreTabStates = useCallback((states: Record<string, TabState>) => {
    applyTabStates(states);
  }, [applyTabStates]);

  // All callbacks above are identity-stable, but the context value must still
  // change identity when tabStates changes: consumers (e.g. the query route)
  // read tab state during render via `getTabState` and rely on the context
  // update for re-render. Keeping `tabStates` in the dep list preserves that
  // reactivity while the stable getters keep consumer effects/memos from
  // re-firing on every mutation.
  const contextValue = useMemo(() => ({
    getTabState,
    updateTabState,
    createTabState,
    removeTabState,
    getAllTabStates,
    restoreTabStates,
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), [tabStates, getTabState, updateTabState, createTabState, removeTabState, getAllTabStates, restoreTabStates]);

  return (
    <TabContext.Provider value={contextValue}>
      {children}
    </TabContext.Provider>
  );
}

export function useTabContext() {
  const context = useContext(TabContext);
  if (!context) {
    throw new Error("useTabContext must be used within TabProvider");
  }
  return context;
}
