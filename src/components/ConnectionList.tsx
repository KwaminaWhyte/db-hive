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
import { Pencil, Trash, Database, Server, Plus, RefreshCw } from "lucide-react";
import { Skeleton } from "@/components/ui/skeleton";

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
  const [passwordPrompt, setPasswordPrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [deletePrompt, setDeletePrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [password, setPassword] = useState("");

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

  // Handle connect button click
  const handleConnectClick = async (profile: ConnectionProfile) => {
    try {
      const savedPassword = await invoke<string | null>("get_saved_password", {
        profileId: profile.id,
      });

      if (savedPassword) {
        setConnectingId(profile.id);
        setError(null);

        try {
          const connectionId = await invoke<string>("connect_to_database", {
            profileId: profile.id,
            password: savedPassword,
          });

          onConnected?.(connectionId, profile);
        } catch (err) {
          const errorMessage =
            typeof err === "string"
              ? err
              : (err as any)?.message || String(err);
          setError(`Failed to connect: ${errorMessage}`);
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
            });

            onConnected?.(connectionId, profile);
          } catch (err) {
            const errorMessage =
              typeof err === "string"
                ? err
                : (err as any)?.message || String(err);
            setError(`Failed to connect: ${errorMessage}`);
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
          });

          onConnected?.(connectionId, profile);
        } catch (err) {
          const errorMessage =
            typeof err === "string"
              ? err
              : (err as any)?.message || String(err);
          setError(`Failed to connect: ${errorMessage}`);
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

    try {
      const connectionId = await invoke<string>("connect_to_database", {
        profileId: passwordPrompt.profileId,
        password,
      });

      const profile = profiles.find((p) => p.id === passwordPrompt.profileId);
      if (profile) {
        onConnected?.(connectionId, profile);
      }

      setPasswordPrompt(null);
      setPassword("");
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to connect: ${errorMessage}`);
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
        <div className="p-4 border-b border-slate-800/70">
          <Skeleton className="h-10 w-full rounded-xl bg-slate-800/50" />
          <div className="mt-4 space-y-2">
            <Skeleton className="h-6 w-48 rounded-lg bg-slate-800/40" />
            <Skeleton className="h-4 w-32 rounded-md bg-slate-800/30" />
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-20 rounded-xl bg-slate-800/40" />
          ))}
        </div>
      </div>
    );
  }

  // Helper to get badge color for database driver
  const getDriverColor = (driver: DbDriver) => {
    const driverName = getDriverDisplayName(driver).toLowerCase();
    if (driverName.includes("postgresql")) {
      return "bg-blue-500/20 border-blue-500/40 text-blue-300";
    } else if (driverName.includes("mysql")) {
      return "bg-orange-500/20 border-orange-500/40 text-orange-300";
    } else if (driverName.includes("sqlite")) {
      return "bg-green-500/20 border-green-500/40 text-green-300";
    } else if (driverName.includes("mongodb")) {
      return "bg-emerald-500/20 border-emerald-500/40 text-emerald-300";
    } else if (driverName.includes("sql server")) {
      return "bg-red-500/20 border-red-500/40 text-red-300";
    }
    return "bg-amber-300/10 border-amber-300/30 text-amber-200";
  };

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="p-4 border-b border-slate-800/70">
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
            <h2 className="text-lg font-semibold text-slate-100">Connections</h2>
            <p className="text-xs text-slate-400">
              {profiles.length} saved
            </p>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={loadProfiles}
            className="h-8 w-8 rounded-lg border border-slate-700/60 bg-slate-900/60 hover:bg-slate-900/90 hover:border-amber-300/40 transition-colors"
          >
            <RefreshCw className="h-4 w-4 text-slate-300" strokeWidth={1.5} />
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 rounded-xl bg-red-500/10 border border-red-500/30 p-3 text-sm text-red-200">
          {error}
        </div>
      )}

      {/* Connection List */}
      <div className="flex-1 overflow-y-auto">
        {profiles.length === 0 ? (
          <div className="p-8 text-center">
            <div className="inline-flex h-16 w-16 items-center justify-center rounded-2xl border-2 border-amber-300/30 bg-amber-300/10 mb-4">
              <Database className="h-8 w-8 text-amber-300" strokeWidth={1.5} />
            </div>
            <p className="text-slate-300 text-sm font-medium">No connections yet</p>
            <p className="text-slate-400 text-xs mt-1">
              Create your first connection to get started
            </p>
          </div>
        ) : (
          <div className="p-4 space-y-2">
            {profiles.map((profile) => {
              const colorClass = getDriverColor(profile.driver);
              const driverName = getDriverDisplayName(profile.driver);
              const isConnecting = connectingId === profile.id;

              return (
                <div
                  key={profile.id}
                  className="group relative rounded-xl border border-slate-800/70 bg-slate-900/60 hover:bg-slate-900/90 hover:border-amber-300/30 transition-all cursor-pointer"
                  onDoubleClick={() => handleConnectClick(profile)}
                  title="Double-click to connect"
                >
                  <div className="p-3">
                    <div className="flex items-start justify-between">
                      <div className="flex items-start gap-3 flex-1">
                        <div className="mt-0.5 h-5 w-5 rounded-lg border border-amber-300/40 bg-amber-300/10 flex items-center justify-center">
                          <Server className="h-3 w-3 text-amber-300" strokeWidth={1.5} />
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="font-medium text-sm text-slate-100 truncate">
                              {profile.name}
                            </span>
                            <span className={`text-[10px] px-2 py-0.5 rounded-full border ${colorClass} font-medium`}>
                              {driverName}
                            </span>
                            {isConnecting && (
                              <span className="text-xs text-amber-300 animate-pulse">
                                Connecting...
                              </span>
                            )}
                          </div>
                          <div className="text-xs text-slate-400 flex items-center gap-2">
                            <span>
                              {profile.host}:{profile.port}
                            </span>
                            {profile.database && (
                              <>
                                <span className="text-slate-600">•</span>
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
                          className="h-7 w-7 rounded-lg hover:bg-slate-800/80 hover:text-amber-300"
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
                          className="h-7 w-7 rounded-lg hover:bg-red-500/20 hover:text-red-300"
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
            })}
          </div>
        )}
      </div>

      {/* Password Prompt Dialog */}
      <Dialog
        open={!!passwordPrompt}
        onOpenChange={(open) => !open && setPasswordPrompt(null)}
      >
        <DialogContent className="bg-slate-900 border-slate-800/70">
          <DialogHeader>
            <DialogTitle className="text-slate-100">
              Connect to {passwordPrompt?.profileName}
            </DialogTitle>
            <DialogDescription className="text-slate-400">
              Enter the password to connect to this database.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2 py-4">
            <Label htmlFor="connect-password" className="text-slate-200">
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
              className="rounded-lg border-slate-700/60 bg-slate-950/60 text-slate-100"
            />
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setPasswordPrompt(null)}
              className="rounded-lg border-slate-700/60 bg-slate-900/60 text-slate-200 hover:bg-slate-900/90"
            >
              Cancel
            </Button>
            <Button
              onClick={handleConnect}
              disabled={!password || connectingId !== null}
              className="rounded-lg bg-gradient-to-r from-amber-400 via-amber-300 to-amber-400 text-slate-950 font-medium shadow-[0_4px_12px_rgba(251,191,36,0.3)]"
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
        <DialogContent className="bg-slate-900 border-slate-800/70">
          <DialogHeader>
            <DialogTitle className="text-slate-100">Delete Connection</DialogTitle>
            <DialogDescription className="text-slate-400">
              Are you sure you want to delete "{deletePrompt?.profileName}"?
              This action cannot be undone.
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setDeletePrompt(null)}
              className="rounded-lg border-slate-700/60 bg-slate-900/60 text-slate-200 hover:bg-slate-900/90"
            >
              Cancel
            </Button>
            <Button
              onClick={handleDeleteConfirm}
              className="rounded-lg bg-red-500/80 hover:bg-red-500 text-white font-medium"
            >
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};
