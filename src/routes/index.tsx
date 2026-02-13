import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionForm } from "@/components/ConnectionForm";
import { HiveLogo } from "@/components/WelcomeScreen";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useRouteShortcuts } from "@/hooks/useKeyboardShortcuts";
import {
  ConnectionProfile,
  DbDriver,
  getDriverDisplayName,
} from "@/types/database";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { ConnectionLostError } from "@/components/ConnectionLostError";
import {
  ArrowLeft,
  Plus,
  Search,
  MoreVertical,
  Pencil,
  Copy,
  Files,
  Trash2,
  ChevronDown,
  RefreshCw,
} from "lucide-react";

// Database type for the selection grid
interface DatabaseTypeOption {
  id: string;
  driver: DbDriver | null;
  name: string;
  color: string;
  abbr: string;
  available: boolean;
}

const DATABASE_TYPE_OPTIONS: DatabaseTypeOption[] = [
  { id: "postgres", driver: "Postgres", name: "PostgreSQL", color: "#336791", abbr: "PG", available: true },
  { id: "mysql", driver: "MySql", name: "MySQL", color: "#F29111", abbr: "My", available: true },
  { id: "mariadb", driver: "MySql", name: "MariaDB", color: "#003545", abbr: "Ma", available: true },
  { id: "sqlserver", driver: "SqlServer", name: "SQL Server", color: "#CC2927", abbr: "SS", available: true },
  { id: "mongodb", driver: "MongoDb", name: "MongoDB", color: "#13AA52", abbr: "Mo", available: true },
  { id: "sqlite", driver: "Sqlite", name: "SQLite", color: "#003B57", abbr: "SL", available: true },
  { id: "redis", driver: null, name: "Redis", color: "#DC382D", abbr: "Re", available: false },
  { id: "supabase", driver: null, name: "Supabase", color: "#3ECF8E", abbr: "Sb", available: false },
  { id: "neon", driver: null, name: "Neon", color: "#00E699", abbr: "Ne", available: false },
  { id: "turso", driver: null, name: "LibSQL / Turso", color: "#4FF8D2", abbr: "Tu", available: false },
];

// Driver icon config for connection list
const DRIVER_ICON_CONFIG: Record<string, { color: string; abbr: string }> = {
  Postgres: { color: "#336791", abbr: "PG" },
  MySql: { color: "#F29111", abbr: "My" },
  Sqlite: { color: "#003B57", abbr: "SL" },
  MongoDb: { color: "#13AA52", abbr: "Mo" },
  SqlServer: { color: "#CC2927", abbr: "SS" },
};

// Small colored icon for database type
function DatabaseIcon({ driver, size = 36 }: { driver: string; size?: number }) {
  const config = DRIVER_ICON_CONFIG[driver] || { color: "#666", abbr: "DB" };
  return (
    <div
      className="rounded-lg flex items-center justify-center text-white font-bold shrink-0"
      style={{
        backgroundColor: config.color,
        width: size,
        height: size,
        fontSize: size * 0.35,
      }}
    >
      {config.abbr}
    </div>
  );
}

type ViewState =
  | { view: "home" }
  | { view: "new-connection" }
  | { view: "connection-form"; driver: DbDriver; editProfile?: ConnectionProfile };

export const Route = createFileRoute("/")({
  component: HomeRoute,
});

function HomeRoute() {
  const navigate = useNavigate();
  const { setConnection } = useConnectionContext();

  // View state
  const [viewState, setViewState] = useState<ViewState>({ view: "home" });

  // Profiles state
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");

  // Connection state
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<{
    profileId: string;
    profileName: string;
    message: string;
  } | null>(null);

  // Dialogs
  const [deletePrompt, setDeletePrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);

  // Connection string for new connection view
  const [connectionString, setConnectionString] = useState("");

  // Keyboard shortcuts
  useRouteShortcuts([
    {
      key: "\u2318+K",
      handler: () => setViewState({ view: "new-connection" }),
      description: "New connection",
    },
    {
      key: "Ctrl+K",
      handler: () => setViewState({ view: "new-connection" }),
      description: "New connection",
    },
    {
      key: "Escape",
      handler: () => {
        if (viewState.view !== "home") {
          setViewState({ view: "home" });
        }
      },
      description: "Back",
    },
  ]);

  // Load profiles
  useEffect(() => {
    loadProfiles();
  }, []);

  const loadProfiles = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<ConnectionProfile[]>("list_connection_profiles");
      setProfiles(result);
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to load profiles: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Filter profiles by search
  const filteredProfiles = profiles.filter((p) => {
    if (!searchQuery) return true;
    const query = searchQuery.toLowerCase();
    return (
      p.name.toLowerCase().includes(query) ||
      getDriverDisplayName(p.driver).toLowerCase().includes(query) ||
      p.host.toLowerCase().includes(query) ||
      (p.folder && p.folder.toLowerCase().includes(query))
    );
  });

  // Handle connected
  const handleConnected = (connectionId: string, profile: ConnectionProfile) => {
    setConnection(connectionId, profile);
    const defaultTabId = `query-${Date.now()}`;
    navigate({ to: "/query", search: { tabs: defaultTabId, active: 0 } });
  };

  // Handle connection error
  const handleConnectionError = (err: unknown, profile: ConnectionProfile) => {
    const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
    const isConnectionError =
      (err as any)?.kind === "connection" || errorMessage.toLowerCase().includes("connection");
    if (isConnectionError) {
      setConnectionError({
        profileId: profile.id,
        profileName: profile.name,
        message: errorMessage,
      });
    } else {
      setError(`Failed to connect: ${errorMessage}`);
    }
  };

  // Handle connect click
  const handleConnectClick = async (profile: ConnectionProfile) => {
    setConnectingId(profile.id);
    setError(null);

    try {
      // Retrieve saved password (keyring or in-memory fallback)
      let savedPassword: string | null = null;
      try {
        savedPassword = await invoke<string | null>("get_saved_password", {
          profileId: profile.id,
        });
      } catch (err) {
        console.error("Failed to retrieve saved password:", err);
      }

      // Retrieve SSH password if needed
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

      // Connect using saved password or empty string
      const connectionId = await invoke<string>("connect_to_database", {
        profileId: profile.id,
        password: savedPassword || "",
        sshPassword,
      });
      handleConnected(connectionId, profile);
    } catch (err) {
      handleConnectionError(err, profile);
    } finally {
      setConnectingId(null);
    }
  };

  // Handle delete
  const handleDeleteConfirm = async () => {
    if (!deletePrompt) return;
    try {
      await invoke("delete_connection_profile", { profileId: deletePrompt.profileId });
      await loadProfiles();
      setDeletePrompt(null);
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to delete: ${errorMessage}`);
    }
  };

  // Handle duplicate
  const handleDuplicate = async (profile: ConnectionProfile) => {
    try {
      const duplicated: ConnectionProfile = {
        ...profile,
        id: "",
        name: `${profile.name} (copy)`,
        createdAt: Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
      };
      await invoke<string>("create_connection_profile", { profile: duplicated });
      await loadProfiles();
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to duplicate: ${errorMessage}`);
    }
  };

  // Handle copy details
  const handleCopyDetails = (profile: ConnectionProfile) => {
    const details = `${getDriverDisplayName(profile.driver)} - ${profile.host}:${profile.port}${profile.database ? `/${profile.database}` : ""}`;
    navigator.clipboard.writeText(details);
  };

  // Form success
  const handleFormSuccess = () => {
    setViewState({ view: "home" });
    loadProfiles();
  };

  // Detect driver from connection string
  const detectDriverFromConnectionString = (connStr: string): DbDriver | null => {
    const s = connStr.toLowerCase().trim();
    if (s.startsWith("postgres://") || s.startsWith("postgresql://")) return "Postgres";
    if (s.startsWith("mysql://") || s.startsWith("mariadb://")) return "MySql";
    if (s.startsWith("mongodb://") || s.startsWith("mongodb+srv://")) return "MongoDb";
    if (s.startsWith("sqlite://") || s.startsWith("file:") || s.endsWith(".db") || s.endsWith(".sqlite")) return "Sqlite";
    if (s.startsWith("mssql://") || s.startsWith("sqlserver://")) return "SqlServer";
    return null;
  };

  // ── New Connection View ──
  if (viewState.view === "new-connection") {
    return (
      <div className="flex-1 flex flex-col h-full bg-background">
        {/* Header */}
        <div className="flex items-center gap-3 px-6 py-4 border-b border-border">
          <button
            onClick={() => setViewState({ view: "home" })}
            className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowLeft className="h-4 w-4" />
            Back
          </button>
          <span className="font-semibold">New Connection</span>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-[680px] mx-auto py-10 px-6">
            {/* Connection String */}
            <div className="space-y-2">
              <Label className="text-sm font-medium">Connection String</Label>
              <Input
                value={connectionString}
                onChange={(e) => {
                  setConnectionString(e.target.value);
                  const detected = detectDriverFromConnectionString(e.target.value);
                  if (detected) {
                    setViewState({ view: "connection-form", driver: detected });
                  }
                }}
                placeholder="protocol://user:password@host:port/database"
                className="h-11"
              />
              <p className="text-xs text-muted-foreground">
                Paste your connection string to auto-detect database type
              </p>
            </div>

            {/* Divider */}
            <div className="flex items-center gap-4 my-10">
              <div className="flex-1 border-t border-border" />
              <span className="text-sm text-muted-foreground whitespace-nowrap">or select database</span>
              <div className="flex-1 border-t border-border" />
            </div>

            {/* Database Grid */}
            <div className="grid grid-cols-2 gap-3">
              {DATABASE_TYPE_OPTIONS.map((db) => (
                <button
                  key={db.id}
                  disabled={!db.available}
                  onClick={() => {
                    if (db.driver) {
                      setViewState({ view: "connection-form", driver: db.driver });
                    }
                  }}
                  className={`flex items-center gap-3.5 px-4 py-4 rounded-xl border transition-all text-left ${
                    db.available
                      ? "border-border hover:border-foreground/20 hover:bg-accent/50 cursor-pointer"
                      : "border-border/50 opacity-40 cursor-not-allowed"
                  }`}
                >
                  <div
                    className="w-10 h-10 rounded-lg flex items-center justify-center text-white font-bold text-xs shrink-0"
                    style={{ backgroundColor: db.color }}
                  >
                    {db.abbr}
                  </div>
                  <span className="text-sm font-medium">{db.name}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    );
  }

  // ── Connection Form View ──
  if (viewState.view === "connection-form") {
    return (
      <div className="flex-1 flex flex-col h-full bg-background">
        {/* Header */}
        <div className="flex items-center gap-3 px-6 py-4 border-b border-border">
          <button
            onClick={() =>
              viewState.editProfile
                ? setViewState({ view: "home" })
                : setViewState({ view: "new-connection" })
            }
            className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowLeft className="h-4 w-4" />
            Back
          </button>
          <DatabaseIcon driver={viewState.driver} size={24} />
          <span className="font-semibold">
            {viewState.editProfile
              ? viewState.editProfile.name
              : getDriverDisplayName(viewState.driver)}
          </span>
        </div>

        {/* Form */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-[680px] mx-auto py-8 px-6">
            <ConnectionForm
              driver={viewState.driver}
              profile={viewState.editProfile}
              onSuccess={handleFormSuccess}
            />
          </div>
        </div>
      </div>
    );
  }

  // ── Home View ──
  return (
    <div className="flex-1 flex h-full relative">
      {/* Left Panel: Branding */}
      <div className="w-1/2 bg-muted/30 flex flex-col items-center justify-center border-r border-border">
        <HiveLogo />
        <div className="mt-8 text-center">
          <div className="flex items-center justify-center gap-2">
            <span className="text-3xl font-semibold tracking-tight">DB</span>
            <span className="bg-foreground text-background px-2 py-0.5 rounded-md text-lg font-bold">
              Hive
            </span>
          </div>
          <p className="text-muted-foreground mt-3 text-base">
            The Modern Database Desktop App
          </p>
        </div>
      </div>

      {/* Right Panel: Connections */}
      <div className="w-1/2 flex flex-col bg-background">
        {/* Header */}
        <div className="px-6 pt-6 pb-4 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-2xl font-semibold tracking-tight">Saved Connections</h2>
            <div className="flex items-center gap-2">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline" size="sm" className="gap-1.5 h-9">
                    Manage
                    <ChevronDown className="h-3.5 w-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onClick={loadProfiles}>
                    <RefreshCw className="h-4 w-4 mr-2" />
                    Refresh
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <Button
                size="sm"
                onClick={() => setViewState({ view: "new-connection" })}
                className="gap-1.5 h-9"
              >
                New
                <Plus className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* Search */}
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search by tag..."
              className="pl-10 h-10"
            />
          </div>
        </div>

        {/* Errors */}
        {error && (
          <div className="mx-6 mb-3 rounded-lg bg-destructive/10 border border-destructive/20 p-3 text-sm text-destructive">
            {error}
          </div>
        )}

        {connectionError && (
          <div className="mx-6 mb-3">
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
              onGoToDashboard={() => setConnectionError(null)}
            />
          </div>
        )}

        {/* Connection List */}
        <div className="flex-1 overflow-y-auto px-6">
          {loading ? (
            <div className="space-y-3">
              {[1, 2, 3].map((i) => (
                <Skeleton key={i} className="h-16 rounded-xl" />
              ))}
            </div>
          ) : filteredProfiles.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-16 text-center">
              <p className="text-muted-foreground text-sm">
                {profiles.length === 0
                  ? "No connections yet. Click 'New +' to get started."
                  : "No connections match your search."}
              </p>
            </div>
          ) : (
            <div className="space-y-0.5">
              {filteredProfiles.map((profile) => {
                const isConnecting = connectingId === profile.id;
                return (
                  <div
                    key={profile.id}
                    className="group flex items-center gap-3 px-3 py-3 rounded-xl hover:bg-accent/50 transition-colors cursor-pointer"
                    onDoubleClick={() => handleConnectClick(profile)}
                    title="Double-click to connect"
                  >
                    <DatabaseIcon driver={profile.driver} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-sm truncate">
                          {profile.name}
                        </span>
                        {isConnecting && (
                          <span className="text-xs text-primary animate-pulse">
                            Connecting...
                          </span>
                        )}
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {getDriverDisplayName(profile.driver).toLowerCase()} &bull; {profile.host}
                      </span>
                    </div>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <MoreVertical className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-44">
                        <DropdownMenuItem
                          onClick={() =>
                            setViewState({
                              view: "connection-form",
                              driver: profile.driver,
                              editProfile: profile,
                            })
                          }
                        >
                          <Pencil className="h-4 w-4 mr-2" />
                          Edit
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => handleCopyDetails(profile)}>
                          <Copy className="h-4 w-4 mr-2" />
                          Copy Details
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => handleDuplicate(profile)}>
                          <Files className="h-4 w-4 mr-2" />
                          Duplicate
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          className="text-destructive focus:text-destructive"
                          onClick={() =>
                            setDeletePrompt({
                              profileId: profile.id,
                              profileName: profile.name,
                            })
                          }
                        >
                          <Trash2 className="h-4 w-4 mr-2" />
                          Delete
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Footer */}
        <footer className="border-t border-border px-6 py-3">
          <div className="flex items-center justify-center text-xs text-muted-foreground">
            <span>Version 0.16.0-beta</span>
          </div>
          <div className="flex items-center justify-center gap-3 mt-1 text-xs text-muted-foreground">
            <a
              href="https://github.com/KwaminaWhyte/db-hive"
              target="_blank"
              rel="noopener noreferrer"
              className="hover:text-foreground transition-colors"
            >
              GitHub
            </a>
            <span>&bull;</span>
            <span>DB-Hive.app</span>
          </div>
        </footer>
      </div>

      {/* Delete Confirmation Dialog */}
      <Dialog open={!!deletePrompt} onOpenChange={(open) => !open && setDeletePrompt(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Connection</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete &ldquo;{deletePrompt?.profileName}&rdquo;? This action cannot be undone.
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
}
