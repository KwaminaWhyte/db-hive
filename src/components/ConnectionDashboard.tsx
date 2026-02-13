import { FC, useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ConnectionProfile,
  DbDriver,
  getDriverDisplayName,
} from "../types/database";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Database,
  Plus,
  RefreshCw,
  Grid3x3,
  List,
  FolderTree,
  Search,
  Star,
  Clock,
  HardDrive,
  Cloud,
  Filter,
  Server,
  Pencil,
  Trash,
} from "lucide-react";
import { Skeleton } from "@/components/ui/skeleton";
import { ConnectionCard } from "./ConnectionCard";
import { ConnectionTreeView } from "./ConnectionTreeView";
import { NoConnectionsEmpty, NoSearchResultsEmpty } from "@/components/empty-states";
import { ConnectionLostError } from "@/components/ConnectionLostError";

interface ConnectionDashboardProps {
  /** Callback when successfully connected to a database */
  onConnected?: (connectionId: string, profile: ConnectionProfile) => void;
  /** Callback to open connection form */
  onNewConnection?: () => void;
  /** Callback to edit connection */
  onEditConnection?: (profile: ConnectionProfile) => void;
}

type ViewMode = "grid" | "list" | "tree";
type CategoryFilter = "all" | "favorites" | "recent" | "local" | "cloud";

export const ConnectionDashboard: FC<ConnectionDashboardProps> = ({
  onConnected,
  onNewConnection,
  onEditConnection,
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
  const [deletePrompt, setDeletePrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);

  // Dashboard state
  const [viewMode, setViewMode] = useState<ViewMode>("grid");
  const [category, setCategory] = useState<CategoryFilter>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedDrivers, setSelectedDrivers] = useState<Set<DbDriver>>(
    new Set()
  );
  const [selectedEnvironments, setSelectedEnvironments] = useState<Set<string>>(
    new Set()
  );

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

  // Get unique drivers and environments from profiles
  const availableDrivers = useMemo(() => {
    const drivers = new Set<DbDriver>();
    profiles.forEach((p) => drivers.add(p.driver));
    return Array.from(drivers);
  }, [profiles]);

  const availableEnvironments = useMemo(() => {
    const envs = new Set<string>();
    profiles.forEach((p) => {
      if (p.environment) envs.add(p.environment);
    });
    return Array.from(envs);
  }, [profiles]);

  // Toggle favorite status
  const handleToggleFavorite = async (profileId: string) => {
    const profile = profiles.find((p) => p.id === profileId);
    if (!profile) return;

    try {
      await invoke("update_connection_profile", {
        profileId,
        profile: { ...profile, isFavorite: !profile.isFavorite },
      });
      await loadProfiles();
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
        (p) =>
          p.host === "localhost" ||
          p.host === "127.0.0.1" ||
          p.driver === "Sqlite"
      );
    } else if (category === "cloud") {
      filtered = filtered.filter(
        (p) =>
          p.host !== "localhost" &&
          p.host !== "127.0.0.1" &&
          p.driver !== "Sqlite"
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
          (p.database && p.database.toLowerCase().includes(query)) ||
          getDriverDisplayName(p.driver).toLowerCase().includes(query)
      );
    }

    // Apply driver filter
    if (selectedDrivers.size > 0) {
      filtered = filtered.filter((p) => selectedDrivers.has(p.driver));
    }

    // Apply environment filter
    if (selectedEnvironments.size > 0) {
      filtered = filtered.filter(
        (p) => p.environment && selectedEnvironments.has(p.environment)
      );
    }

    return filtered;
  }, [profiles, category, searchQuery, selectedDrivers, selectedEnvironments]);

  // Handle connect
  const handleConnectClick = async (profile: ConnectionProfile) => {
    setConnectingId(profile.id);
    setError(null);

    try {
      let savedPassword: string | null = null;
      try {
        savedPassword = await invoke<string | null>("get_saved_password", { profileId: profile.id });
      } catch (err) {
        console.error("Failed to retrieve saved password:", err);
      }

      let sshPassword: string | null = null;
      if (profile.sshTunnel && profile.sshTunnel.authMethod === "Password") {
        try {
          sshPassword = await invoke<string | null>("get_ssh_password", { profileId: profile.id });
        } catch (err) {
          console.error("Failed to get SSH password:", err);
        }
      }

      const connectionId = await invoke<string>("connect_to_database", {
        profileId: profile.id,
        password: savedPassword || "",
        sshPassword,
      });
      onConnected?.(connectionId, profile);
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);

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
  };

  // Handle delete
  const handleDeleteClick = (profileId: string) => {
    const profile = profiles.find((p) => p.id === profileId);
    if (!profile) return;
    setDeletePrompt({ profileId: profile.id, profileName: profile.name });
  };

  const handleDeleteConfirm = async () => {
    if (!deletePrompt) return;

    setError(null);

    try {
      await invoke("delete_connection_profile", {
        profileId: deletePrompt.profileId,
      });
      await loadProfiles();
      setDeletePrompt(null);
    } catch (err) {
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to delete profile: ${errorMessage}`);
    }
  };

  // Helper to get badge color
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
        <div className="p-4">
          <div className="flex items-start justify-between">
            <div className="flex items-start gap-3 flex-1">
              <div className="mt-0.5 h-10 w-10 rounded-lg border border-primary/40 bg-primary/10 flex items-center justify-center">
                <Server className="h-5 w-5 text-primary" strokeWidth={1.5} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  {profile.isFavorite && (
                    <Star className="h-4 w-4 text-yellow-500 fill-current" />
                  )}
                  <span className="font-semibold text-base text-foreground truncate">
                    {profile.name}
                  </span>
                  <span
                    className={`text-xs px-2 py-0.5 rounded-full border ${colorClass} font-medium`}
                  >
                    {driverName}
                  </span>
                  {isConnecting && (
                    <span className="text-xs text-primary animate-pulse">
                      Connecting...
                    </span>
                  )}
                </div>
                <p className="text-sm text-muted-foreground truncate">
                  {profile.host}:{profile.port}
                  {profile.database && ` • ${profile.database}`}
                  {profile.folder && ` • ${profile.folder}`}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8"
                onClick={(e) => {
                  e.stopPropagation();
                  handleToggleFavorite(profile.id);
                }}
              >
                <Star
                  className={`h-4 w-4 ${profile.isFavorite ? "text-yellow-500 fill-current" : ""}`}
                  strokeWidth={1.5}
                />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8"
                onClick={(e) => {
                  e.stopPropagation();
                  onEditConnection?.(profile);
                }}
              >
                <Pencil className="h-4 w-4" strokeWidth={1.5} />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 text-destructive hover:text-destructive"
                onClick={(e) => {
                  e.stopPropagation();
                  handleDeleteClick(profile.id);
                }}
              >
                <Trash className="h-4 w-4" strokeWidth={1.5} />
              </Button>
            </div>
          </div>
        </div>
      </div>
    );
  };

  if (loading) {
    return (
      <div className="h-full flex flex-col bg-background p-8">
        <div className="mb-8">
          <Skeleton className="h-12 w-64 mb-2" />
          <Skeleton className="h-6 w-96" />
        </div>
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-6">
          {[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].map((i) => (
            <Skeleton key={i} className="h-48 rounded-xl" />
          ))}
        </div>
      </div>
    );
  }

  const activeFilterCount =
    (selectedDrivers.size > 0 ? 1 : 0) +
    (selectedEnvironments.size > 0 ? 1 : 0);

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header */}
      <div className="border-b border-border bg-card/50">
        <div className="w-full px-8 py-6">
          <div className="flex items-center justify-between mb-6">
            <div>
              <h1 className="text-3xl font-bold text-foreground">
                Database Connections
              </h1>
              <p className="text-muted-foreground mt-1">
                Manage and connect to your databases
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={loadProfiles}>
                <RefreshCw className="h-4 w-4 mr-2" />
                Refresh
              </Button>
              <Button size="sm" onClick={onNewConnection}>
                <Plus className="h-4 w-4 mr-2" />
                New Connection
              </Button>
            </div>
          </div>

          {/* Search and Filters */}
          <div className="flex items-center gap-4 mb-4">
            <div className="relative flex-1 max-w-md">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search connections by name, host, folder, or database..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9"
              />
            </div>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm">
                  <Filter className="h-4 w-4 mr-2" />
                  Filters
                  {activeFilterCount > 0 && (
                    <span className="ml-2 px-1.5 py-0.5 rounded-full bg-primary text-primary-foreground text-xs">
                      {activeFilterCount}
                    </span>
                  )}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>Database Driver</DropdownMenuLabel>
                {availableDrivers.map((driver) => (
                  <DropdownMenuCheckboxItem
                    key={driver}
                    checked={selectedDrivers.has(driver)}
                    onCheckedChange={(checked) => {
                      const newSet = new Set(selectedDrivers);
                      if (checked) {
                        newSet.add(driver);
                      } else {
                        newSet.delete(driver);
                      }
                      setSelectedDrivers(newSet);
                    }}
                  >
                    {getDriverDisplayName(driver)}
                  </DropdownMenuCheckboxItem>
                ))}

                {availableEnvironments.length > 0 && (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuLabel>Environment</DropdownMenuLabel>
                    {availableEnvironments.map((env) => (
                      <DropdownMenuCheckboxItem
                        key={env}
                        checked={selectedEnvironments.has(env)}
                        onCheckedChange={(checked) => {
                          const newSet = new Set(selectedEnvironments);
                          if (checked) {
                            newSet.add(env);
                          } else {
                            newSet.delete(env);
                          }
                          setSelectedEnvironments(newSet);
                        }}
                      >
                        {env}
                      </DropdownMenuCheckboxItem>
                    ))}
                  </>
                )}

                {activeFilterCount > 0 && (
                  <>
                    <DropdownMenuSeparator />
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-full"
                      onClick={() => {
                        setSelectedDrivers(new Set());
                        setSelectedEnvironments(new Set());
                      }}
                    >
                      Clear filters
                    </Button>
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>

            <div className="flex items-center gap-1 border border-border rounded-lg p-1">
              <Button
                variant={viewMode === "grid" ? "secondary" : "ghost"}
                size="icon"
                className="h-8 w-8"
                onClick={() => setViewMode("grid")}
                title="Grid view"
              >
                <Grid3x3 className="h-4 w-4" />
              </Button>
              <Button
                variant={viewMode === "list" ? "secondary" : "ghost"}
                size="icon"
                className="h-8 w-8"
                onClick={() => setViewMode("list")}
                title="List view"
              >
                <List className="h-4 w-4" />
              </Button>
              <Button
                variant={viewMode === "tree" ? "secondary" : "ghost"}
                size="icon"
                className="h-8 w-8"
                onClick={() => setViewMode("tree")}
                title="Tree view (by folder)"
              >
                <FolderTree className="h-4 w-4" />
              </Button>
            </div>
          </div>

          {/* Category Tabs */}
          <Tabs
            value={category}
            onValueChange={(v) => setCategory(v as CategoryFilter)}
          >
            <TabsList className="h-10">
              <TabsTrigger value="all" className="text-sm">
                <Database className="h-4 w-4 mr-2" />
                All ({profiles.length})
              </TabsTrigger>
              <TabsTrigger value="favorites" className="text-sm">
                <Star className="h-4 w-4 mr-2" />
                Favorites ({profiles.filter((p) => p.isFavorite).length})
              </TabsTrigger>
              <TabsTrigger value="recent" className="text-sm">
                <Clock className="h-4 w-4 mr-2" />
                Recent
              </TabsTrigger>
              <TabsTrigger value="local" className="text-sm">
                <HardDrive className="h-4 w-4 mr-2" />
                Local
              </TabsTrigger>
              <TabsTrigger value="cloud" className="text-sm">
                <Cloud className="h-4 w-4 mr-2" />
                Cloud
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
      </div>

      {error && (
        <div className="w-full px-8 pt-4">
          <div className="rounded-xl bg-destructive/10 border border-destructive/30 p-4 text-sm text-destructive">
            {error}
          </div>
        </div>
      )}

      {connectionError && (
        <div className="w-full px-8 pt-4">
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

      {/* Connection Grid/List */}
      <div className="flex-1 overflow-y-auto">
        <div className="w-full px-8 py-6">
          {filteredProfiles.length === 0 ? (
            searchQuery || activeFilterCount > 0 || category !== "all" ? (
              <NoSearchResultsEmpty
                searchQuery={searchQuery}
                onClearSearch={() => {
                  setSearchQuery("");
                  setSelectedDrivers(new Set());
                  setSelectedEnvironments(new Set());
                  setCategory("all");
                }}
              />
            ) : (
              <NoConnectionsEmpty
                onAddConnection={onNewConnection || (() => {})}
              />
            )
          ) : viewMode === "grid" ? (
            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-6">
              {filteredProfiles.map((profile) => (
                <ConnectionCard
                  key={profile.id}
                  profile={profile}
                  isActive={connectingId === profile.id}
                  onConnect={handleConnectClick}
                  onEdit={onEditConnection || (() => {})}
                  onDelete={handleDeleteClick}
                  onToggleFavorite={handleToggleFavorite}
                />
              ))}
            </div>
          ) : viewMode === "tree" ? (
            <div className="max-w-4xl">
              <ConnectionTreeView
                profiles={filteredProfiles}
                connectingId={connectingId}
                onConnect={handleConnectClick}
                onEdit={onEditConnection || (() => {})}
                onDelete={handleDeleteClick}
                onToggleFavorite={handleToggleFavorite}
              />
            </div>
          ) : (
            <div className="space-y-3 max-w-5xl">
              {filteredProfiles.map((profile) => renderListItem(profile))}
            </div>
          )}
        </div>
      </div>

      {/* Delete Confirmation Dialog */}
      <Dialog open={!!deletePrompt} onOpenChange={() => setDeletePrompt(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Connection</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <strong>{deletePrompt?.profileName}</strong>? This action cannot
              be undone.
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
