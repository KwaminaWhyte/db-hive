import { createContext, useContext, useState, ReactNode, useCallback, useMemo } from "react";
import { ConnectionProfile } from "@/types/database";

interface ConnectionContextValue {
  connectionId: string | null;
  connectionProfile: ConnectionProfile | undefined;
  currentDatabase: string;
  setConnection: (id: string, profile: ConnectionProfile) => void;
  setCurrentDatabase: (db: string) => void;
  disconnect: () => void;
}

const ConnectionContext = createContext<ConnectionContextValue | undefined>(
  undefined
);

export function ConnectionProvider({ children }: { children: ReactNode }) {
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const [connectionProfile, setConnectionProfile] = useState<
    ConnectionProfile | undefined
  >(undefined);
  const [currentDatabase, setCurrentDatabaseState] = useState<string>("");

  // Memoize callbacks to prevent unnecessary re-renders
  const setConnection = useCallback((id: string, profile: ConnectionProfile) => {
    setConnectionId(id);
    setConnectionProfile(profile);
  }, []);

  const setCurrentDatabase = useCallback((db: string) => {
    setCurrentDatabaseState(db);
  }, []);

  const disconnect = useCallback(() => {
    setConnectionId(null);
    setConnectionProfile(undefined);
    setCurrentDatabaseState("");
  }, []);

  // Memoize context value to prevent unnecessary re-renders of consumers
  const contextValue = useMemo(() => ({
    connectionId,
    connectionProfile,
    currentDatabase,
    setConnection,
    setCurrentDatabase,
    disconnect,
  }), [connectionId, connectionProfile, currentDatabase, setConnection, setCurrentDatabase, disconnect]);

  return (
    <ConnectionContext.Provider value={contextValue}>
      {children}
    </ConnectionContext.Provider>
  );
}

export function useConnectionContext() {
  const context = useContext(ConnectionContext);
  if (!context) {
    throw new Error(
      "useConnectionContext must be used within ConnectionProvider"
    );
  }
  return context;
}
