import { FC, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ConnectionProfile,
  DbDriver,
  getDriverDisplayName,
} from "../types/database";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Pencil, Trash, Server, Plus, RefreshCw, ChevronRight, ChevronDown, FolderOpen, Folder } from "lucide-react";
import { Skeleton } from "@/components/ui/skeleton";
import { NoConnectionsEmpty } from "@/components/empty-states";
import { ConnectionLostError } from "@/components/ConnectionLostError";

interface ConnectionListProps {
  /** Callback when a profile is selected for editing or null for new connection */
  onEdit?: (profile: ConnectionProfile | null) => void;
  /** Callback when profiles list changes */
  onProfilesChange?: () => void;
  /** Callback when successfully connected to a database */
  onConnected?: (connectionId: string, profile: ConnectionProfile) => void;
}

export const ConnectionList: FC<ConnectionListProps> = ({
  onEdit,
  onProfilesChange,
  onConnected,
}) => {
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<{
    profileId: string;
    profileName: string;
    message: string;
  } | null>(null);
  const [passwordPrompt, setPasswordPrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [deletePrompt, setDeletePrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [password, setPassword] = useState("");
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set());

  // Load profiles on mount
  useEffect(() => {
    loadProfiles();
  }, []);

  // Load connection profiles
  const loadProfiles = async () => {
    setLoading(true);
    setError(null);

    try {
      const result = await invoke<ConnectionProfile[]>(
        "list_connection_profiles"
      );
      setProfiles(result);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load profiles: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Organize profiles by folder
  const organizeProfilesByFolder = () => {
    const folderMap = new Map<string, ConnectionProfile[]>();
    const noFolderProfiles: ConnectionProfile[] = [];

    profiles.forEach((profile) => {
      if (profile.folder) {
        if (!folderMap.has(profile.folder)) {
          folderMap.set(profile.folder, []);
        }
        folderMap.get(profile.folder)!.push(profile);
      } else {
        noFolderProfiles.push(profile);
      }
    });

    return { folderMap, noFolderProfiles };
  };

  // Toggle folder expansion
  const toggleFolder = (folderName: string) => {
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(folderName)) {
        next.delete(folderName);
      } else {
        next.add(folderName);
      }
      return next;
    });
  };

  // Handle connect button click
  const handleConnectClick = async (profile: ConnectionProfile) => {
    try {
      const savedPassword = await invoke<string | null>("get_saved_password", {
        profileId: profile.id,
      });

      // Get SSH password if SSH tunnel is configured with password auth
      let sshPassword: string | null = null;
      if (profile.sshTunnel && profile.sshTunnel.authMethod === "Password") {
        try {
          sshPassword = await invoke<string | null>("get_ssh_password", {
            profileId: profile.id,
          });
        } catch (err) {
          console.error("Failed to get SSH password:", err);
        }
      }

      if (savedPassword) {
        setConnectingId(profile.id);
        setError(null);

        try {
          const connectionId = await invoke<string>("connect_to_database", {
            profileId: profile.id,
            password: savedPassword,
            sshPassword,
          });

          onConnected?.(connectionId, profile);
        } catch (err) {
          const errorMessage =
            typeof err === "string"
              ? err
              : (err as any)?.message || String(err);

          // Check if this is a connection error from Rust DbError
          const isConnectionError = (err as any)?.kind === "connection" ||
                                   errorMessage.toLowerCase().includes("connection");

          if (isConnectionError) {
            setConnectionError({
              profileId: profile.id,
              profileName: profile.name,
              message: errorMessage,
            });
          } else {
            setError(`Failed to connect: ${errorMessage}`);
          }
        } finally {
          setConnectingId(null);
        }
      } else {
        if (profile.driver === "MongoDb" || profile.driver === "Sqlite") {
          setConnectingId(profile.id);
          setError(null);

          try {
            const connectionId = await invoke<string>("connect_to_database", {
              profileId: profile.id,
              password: "",
              sshPassword,
            });

            onConnected?.(connectionId, profile);
          } catch (err) {
            const errorMessage =
              typeof err === "string"
                ? err
                : (err as any)?.message || String(err);

            // Check if this is a connection error from Rust DbError
            const isConnectionError = (err as any)?.kind === "connection" ||
                                     errorMessage.toLowerCase().includes("connection");

            if (isConnectionError) {
              setConnectionError({
                profileId: profile.id,
                profileName: profile.name,
                message: errorMessage,
              });
            } else {
              setError(`Failed to connect: ${errorMessage}`);
            }
          } finally {
            setConnectingId(null);
          }
        } else {
          setPasswordPrompt({
            profileId: profile.id,
            profileName: profile.name,
          });
          setPassword("");
        }
      }
    } catch (err) {
      if (profile.driver === "MongoDb" || profile.driver === "Sqlite") {
        setConnectingId(profile.id);
        setError(null);

        try {
          const connectionId = await invoke<string>("connect_to_database", {
            profileId: profile.id,
            password: "",
            sshPassword: null,
          });

          onConnected?.(connectionId, profile);
        } catch (err) {
          const errorMessage =
            typeof err === "string"
              ? err
              : (err as any)?.message || String(err);

          // Check if this is a connection error from Rust DbError
          const isConnectionError = (err as any)?.kind === "connection" ||
                                   errorMessage.toLowerCase().includes("connection");

          if (isConnectionError) {
            setConnectionError({
              profileId: profile.id,
              profileName: profile.name,
              message: errorMessage,
            });
          } else {
            setError(`Failed to connect: ${errorMessage}`);
          }
        } finally {
          setConnectingId(null);
        }
      } else {
        setPasswordPrompt({
          profileId: profile.id,
          profileName: profile.name,
        });
        setPassword("");
      }
    }
  };

  // Handle connect with password
  const handleConnect = async () => {
    if (!passwordPrompt || !password) return;

    setConnectingId(passwordPrompt.profileId);
    setError(null);

    // Get profile before try block so it's accessible in catch
    const profile = profiles.find((p) => p.id === passwordPrompt.profileId);

    try {
      // Get SSH password if SSH tunnel is configured with password auth
      let sshPassword: string | null = null;
      if (profile?.sshTunnel && profile.sshTunnel.authMethod === "Password") {
        try {
          sshPassword = await invoke<string | null>("get_ssh_password", {
            profileId: passwordPrompt.profileId,
          });
        } catch (err) {
          console.error("Failed to get SSH password:", err);
        }
      }

      const connectionId = await invoke<string>("connect_to_database", {
        profileId: passwordPrompt.profileId,
        password,
        sshPassword,
      });

      if (profile) {
        onConnected?.(connectionId, profile);
      }

      setPasswordPrompt(null);
      setPassword("");
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);

      // Check if this is a connection error from Rust DbError
      const isConnectionError = (err as any)?.kind === "connection" ||
                               errorMessage.toLowerCase().includes("connection");

      if (isConnectionError && profile) {
        setConnectionError({
          profileId: profile.id,
          profileName: profile.name,
          message: errorMessage,
        });
      } else {
        setError(`Failed to connect: ${errorMessage}`);
      }
    } finally {
      setConnectingId(null);
    }
  };

  // Handle delete button click
  const handleDeleteClick = (profile: ConnectionProfile) => {
    setDeletePrompt({
      profileId: profile.id,
      profileName: profile.name,
    });
  };

  // Handle delete confirmation
  const handleDeleteConfirm = async () => {
    if (!deletePrompt) return;

    setError(null);

    try {
      await invoke("delete_connection_profile", {
        profileId: deletePrompt.profileId,
      });
      await loadProfiles();
      onProfilesChange?.();
      setDeletePrompt(null);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to delete profile: ${errorMessage}`);
    }
  };

  if (loading) {
    return (
      <div className="h-full flex flex-col bg-background">
        <div className="p-4 border-b border-border">
          <Skeleton className="h-10 w-full rounded-xl" />
          <div className="mt-4 space-y-2">
            <Skeleton className="h-6 w-48 rounded-lg" />
            <Skeleton className="h-4 w-32 rounded-md" />
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-20 rounded-xl" />
          ))}
        </div>
      </div>
    );
  }

  // Helper to get badge color for database driver
  const getDriverColor = (driver: DbDriver) => {
    const driverName = getDriverDisplayName(driver).toLowerCase();
    if (driverName.includes("postgresql")) {
      return "bg-blue-500/20 border-blue-500/40 text-blue-600 dark:text-blue-300";
    } else if (driverName.includes("mysql")) {
      return "bg-orange-500/20 border-orange-500/40 text-orange-600 dark:text-orange-300";
    } else if (driverName.includes("sqlite")) {
      return "bg-green-500/20 border-green-500/40 text-green-600 dark:text-green-300";
    } else if (driverName.includes("mongodb")) {
      return "bg-emerald-500/20 border-emerald-500/40 text-emerald-600 dark:text-emerald-300";
    } else if (driverName.includes("sql server")) {
      return "bg-red-500/20 border-red-500/40 text-red-600 dark:text-red-300";
    }
    return "bg-amber-300/10 border-amber-300/30 text-amber-700 dark:text-amber-200";
  };

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="p-4 border-b border-border">
        <Button
          size="sm"
          onClick={() => onEdit?.(null)}
          className="w-full"
        >
          <Plus className="h-4 w-4 mr-2" strokeWidth={1.5} />
          New Connection
        </Button>

        <div className="flex justify-between items-center w-full mt-4">
          <div>
            <h2 className="text-lg font-semibold text-foreground">Connections</h2>
            <p className="text-xs text-muted-foreground">
              {profiles.length} saved
            </p>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={loadProfiles}
            className="h-8 w-8 rounded-lg border border-border bg-background/60 hover:bg-accent hover:border-amber-500/40 transition-colors"
          >
            <RefreshCw className="h-4 w-4 text-muted-foreground" strokeWidth={1.5} />
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 rounded-xl bg-red-500/10 border border-red-500/30 p-3 text-sm text-red-900 dark:text-red-200">
          {error}
        </div>
      )}

      {connectionError && (
        <div className="mx-4 mt-4">
          <ConnectionLostError
            databaseName={connectionError.profileName}
            message={connectionError.message}
            onReconnect={async () => {
              const profile = profiles.find((p) => p.id === connectionError.profileId);
              if (profile) {
                setConnectionError(null);
                await handleConnectClick(profile);
              }
            }}
            onGoToDashboard={() => {
              setConnectionError(null);
            }}
          />
        </div>
      )}

      {/* Connection List */}
      <div className="flex-1 overflow-y-auto">
        {profiles.length === 0 ? (
          <NoConnectionsEmpty
            onAddConnection={() => onEdit?.(null)}
          />
        ) : (
          <div className="p-4 space-y-2">
            {(() => {
              const { folderMap, noFolderProfiles } = organizeProfilesByFolder();

              // Render helper for connection profile
              const renderProfile = (profile: ConnectionProfile) => {
                const colorClass = getDriverColor(profile.driver);
                const driverName = getDriverDisplayName(profile.driver);
                const isConnecting = connectingId === profile.id;

                return (
                  <div
                    key={profile.id}
                    className="group relative rounded-xl border border-border bg-card hover:bg-accent hover:border-amber-500/30 transition-all cursor-pointer"
                    onDoubleClick={() => handleConnectClick(profile)}
                    title="Double-click to connect"
                  >
                    <div className="p-3">
                      <div className="flex items-start justify-between">
                        <div className="flex items-start gap-3 flex-1">
                          <div className="mt-0.5 h-5 w-5 rounded-lg border border-amber-500/40 bg-amber-500/10 flex items-center justify-center">
                            <Server className="h-3 w-3 text-amber-600 dark:text-amber-300" strokeWidth={1.5} />
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-1">
                              <span className="font-medium text-sm text-foreground truncate">
                                {profile.name}
                              </span>
                              <span className={`text-[10px] px-2 py-0.5 rounded-full border ${colorClass} font-medium`}>
                                {driverName}
                              </span>
                              {isConnecting && (
                                <span className="text-xs text-amber-600 dark:text-amber-300 animate-pulse">
                                  Connecting...
                                </span>
                              )}
                            </div>
                            <div className="text-xs text-muted-foreground flex items-center gap-2">
                              <span>
                                {profile.host}:{profile.port}
                              </span>
                              {profile.database && (
                                <>
                                  <span className="text-border">•</span>
                                  <span>{profile.database}</span>
                                </>
                              )}
                            </div>
                          </div>
                        </div>
                        <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 rounded-lg hover:bg-accent hover:text-amber-600 dark:hover:text-amber-300"
                            onClick={(e) => {
                              e.stopPropagation();
                              onEdit?.(profile);
                            }}
                            title="Edit"
                          >
                            <Pencil className="h-3.5 w-3.5" strokeWidth={1.5} />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 rounded-lg hover:bg-red-500/20 hover:text-red-600 dark:hover:text-red-300"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleDeleteClick(profile);
                            }}
                            title="Delete"
                          >
                            <Trash className="h-3.5 w-3.5" strokeWidth={1.5} />
                          </Button>
                        </div>
                      </div>
                    </div>
                  </div>
                );
              };

              return (
                <>
                  {/* Render folders */}
                  {Array.from(folderMap.entries())
                    .sort(([a], [b]) => a.localeCompare(b))
                    .map(([folderName, folderProfiles]) => {
                      const isExpanded = expandedFolders.has(folderName);

                      return (
                        <div key={`folder-${folderName}`}>
                          {/* Folder Header */}
                          <div
                            className="flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-accent cursor-pointer transition-colors"
                            onClick={() => toggleFolder(folderName)}
                          >
                            {isExpanded ? (
                              <ChevronDown className="h-4 w-4 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="h-4 w-4 text-muted-foreground" />
                            )}
                            {isExpanded ? (
                              <FolderOpen className="h-4 w-4 text-amber-600 dark:text-amber-300" />
                            ) : (
                              <Folder className="h-4 w-4 text-amber-600 dark:text-amber-300" />
                            )}
                            <span className="text-sm font-medium text-foreground">
                              {folderName}
                            </span>
                            <span className="text-xs text-muted-foreground">
                              ({folderProfiles.length})
                            </span>
                          </div>

                          {/* Folder Contents */}
                          {isExpanded && (
                            <div className="ml-6 mt-1 space-y-2">
                              {folderProfiles.map(renderProfile)}
                            </div>
                          )}
                        </div>
                      );
                    })}

                  {/* Render connections without folders */}
                  {noFolderProfiles.map(renderProfile)}
                </>
              );
            })()}
          </div>
        )}
      </div>

      {/* Password Prompt Dialog */}
      <Dialog
        open={!!passwordPrompt}
        onOpenChange={(open) => !open && setPasswordPrompt(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              Connect to {passwordPrompt?.profileName}
            </DialogTitle>
            <DialogDescription>
              Enter the password to connect to this database.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2 py-4">
            <Label htmlFor="connect-password">
              Password
            </Label>
            <Input
              type="password"
              id="connect-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  handleConnect();
                }
              }}
              placeholder="Enter password"
              autoFocus
            />
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setPasswordPrompt(null)}
            >
              Cancel
            </Button>
            <Button
              onClick={handleConnect}
              disabled={!password || connectingId !== null}
            >
              {connectingId ? "Connecting..." : "Connect"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog
        open={!!deletePrompt}
        onOpenChange={(open) => !open && setDeletePrompt(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Connection</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete "{deletePrompt?.profileName}"?
              This action cannot be undone.
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setDeletePrompt(null)}
            >
              Cancel
            </Button>
            <Button
              onClick={handleDeleteConfirm}
              variant="destructive"
            >
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};
