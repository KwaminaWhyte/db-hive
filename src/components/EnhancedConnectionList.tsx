import { FC, useState, useEffect, useMemo } from "react";
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
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Pencil,
  Trash,
  Database,
  Server,
  Plus,
  RefreshCw,
  Grid3x3,
  List,
  Search,
  Star,
  Clock,
  HardDrive,
  Cloud,
} from "lucide-react";
import { Skeleton } from "@/components/ui/skeleton";
import { ConnectionCard } from "./ConnectionCard";
import { NoConnectionsEmpty, NoSearchResultsEmpty } from "@/components/empty-states";

interface EnhancedConnectionListProps {
  /** Callback when a profile is selected for editing or null for new connection */
  onEdit?: (profile: ConnectionProfile | null) => void;
  /** Callback when profiles list changes */
  onProfilesChange?: () => void;
  /** Callback when successfully connected to a database */
  onConnected?: (connectionId: string, profile: ConnectionProfile) => void;
}

type ViewMode = "list" | "grid";
type CategoryFilter = "all" | "favorites" | "recent" | "local" | "cloud";

export const EnhancedConnectionList: FC<EnhancedConnectionListProps> = ({
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

  // New state for enhanced features
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [category, setCategory] = useState<CategoryFilter>("all");
  const [searchQuery, setSearchQuery] = useState("");

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

  // Toggle favorite status
  const handleToggleFavorite = async (profileId: string) => {
    const profile = profiles.find((p) => p.id === profileId);
    if (!profile) return;

    try {
      // Update profile with toggled favorite status
      await invoke("update_connection_profile", {
        profileId,
        profile: {
          ...profile,
          isFavorite: !profile.isFavorite,
        },
      });

      await loadProfiles();
      onProfilesChange?.();
    } catch (err) {
      console.error("Failed to toggle favorite:", err);
    }
  };

  // Filter and categorize profiles
  const filteredProfiles = useMemo(() => {
    let filtered = [...profiles];

    // Apply category filter
    if (category === "favorites") {
      filtered = filtered.filter((p) => p.isFavorite);
    } else if (category === "recent") {
      filtered = filtered
        .filter((p) => p.lastConnectedAt && p.lastConnectedAt > 0)
        .sort((a, b) => (b.lastConnectedAt || 0) - (a.lastConnectedAt || 0))
        .slice(0, 10);
    } else if (category === "local") {
      filtered = filtered.filter(
        (p) => p.host === "localhost" || p.host === "127.0.0.1" || p.driver === "Sqlite"
      );
    } else if (category === "cloud") {
      filtered = filtered.filter(
        (p) => p.host !== "localhost" && p.host !== "127.0.0.1" && p.driver !== "Sqlite"
      );
    }

    // Apply search filter
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (p) =>
          p.name.toLowerCase().includes(query) ||
          p.host.toLowerCase().includes(query) ||
          (p.folder && p.folder.toLowerCase().includes(query)) ||
          getDriverDisplayName(p.driver).toLowerCase().includes(query)
      );
    }

    return filtered;
  }, [profiles, category, searchQuery]);

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
              sshPassword,
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
            sshPassword: null,
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
      const profile = profiles.find((p) => p.id === passwordPrompt.profileId);
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
      setError(`Failed to connect: ${errorMessage}`);
    } finally {
      setConnectingId(null);
    }
  };

  // Handle delete button click
  const handleDeleteClick = (profileId: string) => {
    const profile = profiles.find((p) => p.id === profileId);
    if (!profile) return;

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

  // Render list item
  const renderListItem = (profile: ConnectionProfile) => {
    const colorClass = getDriverColor(profile.driver);
    const driverName = getDriverDisplayName(profile.driver);
    const isConnecting = connectingId === profile.id;

    return (
      <div
        key={profile.id}
        className="group relative rounded-xl border border-border bg-card hover:bg-accent hover:border-primary/30 transition-all cursor-pointer"
        onDoubleClick={() => handleConnectClick(profile)}
        title="Double-click to connect"
      >
        <div className="p-3">
          <div className="flex items-start justify-between">
            <div className="flex items-start gap-3 flex-1">
              <div className="mt-0.5 h-5 w-5 rounded-lg border border-primary/40 bg-primary/10 flex items-center justify-center">
                <Server className="h-3 w-3 text-primary" strokeWidth={1.5} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  {profile.isFavorite && (
                    <Star className="h-3 w-3 text-yellow-500 fill-current" />
                  )}
                  <span className="font-medium text-sm text-foreground truncate">
                    {profile.name}
                  </span>
                  <span className={`text-[10px] px-2 py-0.5 rounded-full border ${colorClass} font-medium`}>
                    {driverName}
                  </span>
                  {isConnecting && (
                    <span className="text-xs text-primary animate-pulse">
                      Connecting...
                    </span>
                  )}
                </div>
                <p className="text-xs text-muted-foreground truncate">
                  {profile.host}:{profile.port}
                  {profile.database && ` • ${profile.database}`}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={(e) => {
                  e.stopPropagation();
                  handleToggleFavorite(profile.id);
                }}
                title={profile.isFavorite ? "Remove from favorites" : "Add to favorites"}
              >
                <Star className={`h-3 w-3 ${profile.isFavorite ? "text-yellow-500 fill-current" : ""}`} strokeWidth={1.5} />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={(e) => {
                  e.stopPropagation();
                  onEdit?.(profile);
                }}
                title="Edit connection"
              >
                <Pencil className="h-3 w-3" strokeWidth={1.5} />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-destructive hover:text-destructive"
                onClick={(e) => {
                  e.stopPropagation();
                  handleDeleteClick(profile.id);
                }}
                title="Delete connection"
              >
                <Trash className="h-3 w-3" strokeWidth={1.5} />
              </Button>
            </div>
          </div>
        </div>
      </div>
    );
  };

  if (loading) {
    return (
      <div className="h-full flex flex-col bg-background">
        <div className="p-4 border-b border-border space-y-4">
          <Skeleton className="h-10 w-full rounded-xl" />
          <Skeleton className="h-10 w-full rounded-xl" />
        </div>
        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-20 rounded-xl" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header with Actions */}
      <div className="p-4 border-b border-border space-y-4">
        {/* New Connection & Refresh */}
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            onClick={() => onEdit?.(null)}
            className="flex-1"
          >
            <Plus className="h-4 w-4 mr-2" strokeWidth={1.5} />
            New Connection
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={loadProfiles}
            className="px-3"
          >
            <RefreshCw className="h-4 w-4" strokeWidth={1.5} />
          </Button>
        </div>

        {/* Search Bar */}
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="Search connections..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-9"
          />
        </div>

        {/* Category Tabs */}
        <Tabs value={category} onValueChange={(v) => setCategory(v as CategoryFilter)}>
          <TabsList className="grid w-full grid-cols-5 h-9">
            <TabsTrigger value="all" className="text-xs">
              <Database className="h-3 w-3 mr-1" />
              All
            </TabsTrigger>
            <TabsTrigger value="favorites" className="text-xs">
              <Star className="h-3 w-3 mr-1" />
              Favorites
            </TabsTrigger>
            <TabsTrigger value="recent" className="text-xs">
              <Clock className="h-3 w-3 mr-1" />
              Recent
            </TabsTrigger>
            <TabsTrigger value="local" className="text-xs">
              <HardDrive className="h-3 w-3 mr-1" />
              Local
            </TabsTrigger>
            <TabsTrigger value="cloud" className="text-xs">
              <Cloud className="h-3 w-3 mr-1" />
              Cloud
            </TabsTrigger>
          </TabsList>
        </Tabs>

        {/* View Mode Toggle */}
        <div className="flex items-center justify-between">
          <div className="text-xs text-muted-foreground">
            {filteredProfiles.length} connection{filteredProfiles.length !== 1 ? "s" : ""}
          </div>
          <div className="flex items-center gap-1 border border-border rounded-lg p-1">
            <Button
              variant={viewMode === "list" ? "secondary" : "ghost"}
              size="icon"
              className="h-6 w-6"
              onClick={() => setViewMode("list")}
              title="List view"
            >
              <List className="h-3 w-3" />
            </Button>
            <Button
              variant={viewMode === "grid" ? "secondary" : "ghost"}
              size="icon"
              className="h-6 w-6"
              onClick={() => setViewMode("grid")}
              title="Grid view"
            >
              <Grid3x3 className="h-3 w-3" />
            </Button>
          </div>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 rounded-xl bg-destructive/10 border border-destructive/30 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      {/* Connection List/Grid */}
      <div className="flex-1 overflow-y-auto">
        {filteredProfiles.length === 0 ? (
          searchQuery || category !== "all" ? (
            <NoSearchResultsEmpty
              searchQuery={searchQuery}
              onClearSearch={() => {
                setSearchQuery("");
                setCategory("all");
              }}
            />
          ) : (
            <NoConnectionsEmpty
              onAddConnection={() => onEdit?.(null)}
            />
          )
        ) : viewMode === "grid" ? (
          <div className="p-4 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {filteredProfiles.map((profile) => (
              <ConnectionCard
                key={profile.id}
                profile={profile}
                isActive={connectingId === profile.id}
                onConnect={handleConnectClick}
                onEdit={onEdit || (() => {})}
                onDelete={handleDeleteClick}
                onToggleFavorite={handleToggleFavorite}
              />
            ))}
          </div>
        ) : (
          <div className="p-4 space-y-2">
            {filteredProfiles.map((profile) => renderListItem(profile))}
          </div>
        )}
      </div>

      {/* Password Prompt Dialog */}
      <Dialog open={!!passwordPrompt} onOpenChange={() => setPasswordPrompt(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Enter Password</DialogTitle>
            <DialogDescription>
              Enter the password for <strong>{passwordPrompt?.profileName}</strong>
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            <div>
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleConnect()}
                autoFocus
              />
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setPasswordPrompt(null)}>
              Cancel
            </Button>
            <Button onClick={handleConnect} disabled={!password}>
              Connect
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog open={!!deletePrompt} onOpenChange={() => setDeletePrompt(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Connection</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete <strong>{deletePrompt?.profileName}</strong>?
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
