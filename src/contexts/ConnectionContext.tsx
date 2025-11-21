import { createContext, useContext, useState, ReactNode } from "react";
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
  const [currentDatabase, setCurrentDatabase] = useState<string>("");

  const setConnection = (id: string, profile: ConnectionProfile) => {
    setConnectionId(id);
    setConnectionProfile(profile);
  };

  const disconnect = () => {
    setConnectionId(null);
    setConnectionProfile(undefined);
    setCurrentDatabase("");
  };

  return (
    <ConnectionContext.Provider
      value={{
        connectionId,
        connectionProfile,
        currentDatabase,
        setConnection,
        setCurrentDatabase,
        disconnect,
      }}
    >
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
