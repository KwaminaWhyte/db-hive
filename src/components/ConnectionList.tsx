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
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Pencil, Trash, Database, Server, Plus, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";

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
      // Handle error - could be string or DbError object
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load profiles: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Handle connect button click
  const handleConnectClick = async (profile: ConnectionProfile) => {
    // Check if there's a saved password for this profile
    try {
      const savedPassword = await invoke<string | null>("get_saved_password", {
        profileId: profile.id,
      });

      if (savedPassword) {
        // Auto-connect with saved password
        setConnectingId(profile.id);
        setError(null);

        try {
          const connectionId = await invoke<string>("connect_to_database", {
            profileId: profile.id,
            password: savedPassword,
          });

          // Success - notify parent component
          onConnected?.(connectionId, profile);
        } catch (err) {
          const errorMessage =
            typeof err === "string" ? err : (err as any)?.message || String(err);
          setError(`Failed to connect: ${errorMessage}`);
        } finally {
          setConnectingId(null);
        }
      } else {
        // No saved password, show password prompt
        setPasswordPrompt({
          profileId: profile.id,
          profileName: profile.name,
        });
        setPassword("");
      }
    } catch (err) {
      // If fetching saved password fails, show password prompt
      setPasswordPrompt({
        profileId: profile.id,
        profileName: profile.name,
      });
      setPassword("");
    }
  };

  // Handle connect with password
  const handleConnect = async () => {
    if (!passwordPrompt) return;

    const profile = profiles.find((p) => p.id === passwordPrompt.profileId);
    if (!profile) {
      setError("Profile not found");
      return;
    }

    setConnectingId(passwordPrompt.profileId);
    setError(null);

    try {
      const connectionId = await invoke<string>("connect_to_database", {
        profileId: passwordPrompt.profileId,
        password,
      });

      // Success - close password prompt
      setPasswordPrompt(null);
      setPassword("");

      // Notify parent component with connection ID and profile
      onConnected?.(connectionId, profile);
    } catch (err) {
      // Handle error - could be string or DbError object
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
      // Handle error - could be string or DbError object
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to delete profile: ${errorMessage}`);
    }
  };

  if (loading) {
    return (
      <Card className="m-6">
        <CardHeader>
          <CardTitle>Connections</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground">Loading...</p>
        </CardContent>
      </Card>
    );
  }

  // Helper to get badge variant and color for database driver
  const getDriverBadge = (driver: DbDriver) => {
    const driverName = getDriverDisplayName(driver).toLowerCase();
    if (driverName.includes("postgresql")) {
      return {
        variant: "default" as const,
        className: "bg-blue-500 hover:bg-blue-600",
      };
    } else if (driverName.includes("mysql")) {
      return {
        variant: "default" as const,
        className: "bg-orange-500 hover:bg-orange-600",
      };
    } else if (driverName.includes("sqlite")) {
      return {
        variant: "default" as const,
        className: "bg-green-500 hover:bg-green-600",
      };
    } else if (driverName.includes("mongodb")) {
      return {
        variant: "default" as const,
        className: "bg-emerald-600 hover:bg-emerald-700",
      };
    } else if (driverName.includes("sql server")) {
      return {
        variant: "default" as const,
        className: "bg-red-500 hover:bg-red-600",
      };
    }
    return { variant: "secondary" as const, className: "" };
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-4 border-b">
        <div className="flex justify-between flex-col">
          <Button
            size="sm"
            onClick={() => onEdit?.(null)}
            className="w-full md:w-auto"
          >
            <Plus className="h-4 w-4 mr-1" />
            New Connection
          </Button>
          <div className="flex justify-between items-center w-full mt-4">
            <div>
              <h2 className="text-lg font-semibold">Saved Connections</h2>
              <p className="text-xs text-muted-foreground">
                {profiles.length} connection{profiles.length !== 1 ? "s" : ""}{" "}
                saved
              </p>
            </div>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={loadProfiles}>
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 rounded-md bg-destructive/10 p-3 text-sm text-destructive border border-destructive/20">
          {error}
        </div>
      )}

      {/* Connection List */}
      <div className="flex-1 overflow-y-auto">
        {profiles.length === 0 ? (
          <div className="p-8 text-center">
            <Database className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
            <p className="text-muted-foreground text-sm">
              No connections saved yet
            </p>
            <p className="text-muted-foreground text-xs mt-1">
              Create a new connection to get started
            </p>
          </div>
        ) : (
          <div className="p-4 space-y-2">
            {profiles.map((profile) => {
              const badgeProps = getDriverBadge(profile.driver);
              const driverName = getDriverDisplayName(profile.driver);
              return (
                <div
                  key={profile.id}
                  className="group relative border rounded-lg p-3 hover:bg-muted/50 transition-colors cursor-pointer select-none"
                  onDoubleClick={() => handleConnectClick(profile)}
                  title="Double-click to connect"
                >
                  <div className="flex items-start justify-between">
                    <div className="flex items-start gap-3 flex-1">
                      <Server className="h-5 w-5 text-muted-foreground mt-0.5" />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <span className="font-medium text-sm truncate">
                            {profile.name}
                          </span>
                          <Badge
                            variant={badgeProps.variant}
                            className={`text-[10px] px-1.5 py-0 ${badgeProps.className}`}
                          >
                            {driverName}
                          </Badge>
                          {connectingId === profile.id && (
                            <span className="text-xs text-muted-foreground animate-pulse">
                              Connecting...
                            </span>
                          )}
                        </div>
                        <div className="text-xs text-muted-foreground space-y-0.5">
                          <div className="flex items-center gap-4">
                            <span>
                              {profile.host}:{profile.port}
                            </span>
                            {profile.database && (
                              <>
                                <span className="text-muted-foreground/50">
                                  •
                                </span>
                                <span>{profile.database}</span>
                              </>
                            )}
                          </div>
                          <div>{profile.username}</div>
                        </div>
                      </div>
                    </div>
                    <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={(e) => {
                          e.stopPropagation();
                          onEdit?.(profile);
                        }}
                        title="Edit connection"
                      >
                        <Pencil className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 hover:text-destructive"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDeleteClick(profile);
                        }}
                        title="Delete connection"
                      >
                        <Trash className="h-4 w-4" />
                      </Button>
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
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Connect to {passwordPrompt?.profileName}</DialogTitle>
            <DialogDescription>
              Enter the password to connect to this database.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2 py-4">
            <Label htmlFor="connect-password">Password</Label>
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
            <Button variant="outline" onClick={() => setPasswordPrompt(null)}>
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
            <Button variant="outline" onClick={() => setDeletePrompt(null)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};
