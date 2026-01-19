import { createContext, useContext, useState, ReactNode, useCallback, useEffect, useMemo } from "react";

/**
 * Tab State Interface
 *
 * Stores state specific to each tab (query content, table filters, etc.)
 */
export interface TabState {
  id: string;
  type: "query" | "table";
  label: string;

  // For query tabs
  sql?: string;
  results?: any;

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

export function TabProvider({ children, connectionId, currentDatabase }: TabProviderProps) {
  const [tabStates, setTabStates] = useState<Record<string, TabState>>({});

  // Load tab states from localStorage when connection OR database changes
  useEffect(() => {
    if (connectionId && currentDatabase) {
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      const saved = localStorage.getItem(storageKey);

      if (saved) {
        try {
          const parsed = JSON.parse(saved);
          setTabStates(parsed.states || {});
        } catch (error) {
          console.error("Failed to parse saved tab states:", error);
        }
      } else {
        // No saved tabs for this database, clear current tabs
        setTabStates({});
      }
    } else {
      // No connection or database, clear tabs
      setTabStates({});
    }
  }, [connectionId, currentDatabase]);

  // Save tab states to localStorage whenever they change
  useEffect(() => {
    if (connectionId && currentDatabase && Object.keys(tabStates).length > 0) {
      const storageKey = `db-hive-tabs-${connectionId}-${currentDatabase}`;
      localStorage.setItem(storageKey, JSON.stringify({
        states: tabStates,
        timestamp: Date.now(),
      }));
    }
  }, [tabStates, connectionId, currentDatabase]);

  const getTabState = useCallback((tabId: string) => {
    return tabStates[tabId];
  }, [tabStates]);

  const updateTabState = useCallback((tabId: string, updates: Partial<TabState>) => {
    setTabStates((prev) => ({
      ...prev,
      [tabId]: {
        ...prev[tabId],
        ...updates,
      } as TabState,
    }));
  }, []);

  const createTabState = useCallback((state: TabState) => {
    setTabStates((prev) => ({
      ...prev,
      [state.id]: state,
    }));
  }, []);

  const removeTabState = useCallback((tabId: string) => {
    setTabStates((prev) => {
      const newStates = { ...prev };
      delete newStates[tabId];
      return newStates;
    });
  }, []);

  const getAllTabStates = useCallback(() => {
    return tabStates;
  }, [tabStates]);

  const restoreTabStates = useCallback((states: Record<string, TabState>) => {
    setTabStates(states);
  }, []);

  // Memoize context value to prevent unnecessary re-renders of consumers
  const contextValue = useMemo(() => ({
    getTabState,
    updateTabState,
    createTabState,
    removeTabState,
    getAllTabStates,
    restoreTabStates,
  }), [getTabState, updateTabState, createTabState, removeTabState, getAllTabStates, restoreTabStates]);

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
