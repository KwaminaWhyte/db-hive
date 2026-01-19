import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { ConnectionList } from "@/components/ConnectionList";
import { ConnectionForm } from "@/components/ConnectionForm";
import { HiveLogo } from "@/components/WelcomeScreen";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useRouteShortcuts } from "@/hooks/useKeyboardShortcuts";
import { ConnectionProfile } from "@/types/database";
import { Button } from "@/components/ui/button";
import {
  Database,
  Command,
  Terminal,
  LayoutPanelLeft,
  Plug2,
  Github,
  ArrowLeft,
} from "lucide-react";

// Database type definition for the grid
interface DatabaseType {
  id: string;
  name: string;
  icon: string;
  bgColor: string;
  available: boolean;
}

// All supported databases - based on actual implementation status
const DATABASE_TYPES: DatabaseType[] = [
  { id: "postgres", name: "PostgreSQL", icon: "\ud83d\udc18", bgColor: "bg-[#336791]", available: true },
  { id: "mysql", name: "MySQL", icon: "\ud83d\udc2c", bgColor: "bg-[#00758F]", available: true },
  { id: "mariadb", name: "MariaDB", icon: "\ud83e\uddad", bgColor: "bg-[#003545]", available: true },
  { id: "sqlite", name: "SQLite", icon: "\ud83d\udcd8", bgColor: "bg-[#003B57]", available: true },
  { id: "mongodb", name: "MongoDB", icon: "\ud83c\udf43", bgColor: "bg-[#13AA52]", available: true },
  { id: "sqlserver", name: "SQL Server", icon: "\ud83d\uddc4\ufe0f", bgColor: "bg-[#CC2927]", available: true },
  { id: "supabase", name: "Supabase", icon: "\u26a1", bgColor: "bg-[#3ECF8E]", available: false },
  { id: "turso", name: "Turso", icon: "\ud83d\udc02", bgColor: "bg-[#4FF8D2]", available: false },
  { id: "neon", name: "Neon", icon: "\ud83d\udc9a", bgColor: "bg-[#00E699]", available: false },
  { id: "redis", name: "Redis", icon: "\ud83d\udd34", bgColor: "bg-[#DC382D]", available: false },
];

export const Route = createFileRoute("/")({
  component: HomeRoute,
});

function HomeRoute() {
  const navigate = useNavigate();
  const { setConnection } = useConnectionContext();
  const [showForm, setShowForm] = useState(false);
  const [editProfile, setEditProfile] = useState<ConnectionProfile | undefined>();
  const [refreshKey, setRefreshKey] = useState(0);

  // Wire up keyboard shortcuts for home page
  useRouteShortcuts([
    {
      key: "\u2318+K",
      handler: () => {
        setEditProfile(undefined);
        setShowForm(true);
      },
      description: "New connection",
    },
    {
      key: "Ctrl+K",
      handler: () => {
        setEditProfile(undefined);
        setShowForm(true);
      },
      description: "New connection",
    },
    {
      key: "Escape",
      handler: () => {
        if (showForm) {
          setShowForm(false);
          setEditProfile(undefined);
        }
      },
      description: "Cancel",
    },
  ]);

  const handleEdit = (profile: ConnectionProfile | null) => {
    if (profile) {
      setEditProfile(profile);
    } else {
      setEditProfile(undefined);
    }
    setShowForm(true);
  };

  const handleConnected = (connectionId: string, profile: ConnectionProfile) => {
    setConnection(connectionId, profile);
    const defaultTabId = `query-${Date.now()}`;
    navigate({ to: "/query", search: { tabs: defaultTabId, active: 0 } });
  };

  const handleFormSuccess = () => {
    setShowForm(false);
    setEditProfile(undefined);
    setRefreshKey((k) => k + 1);
  };

  return (
    <div className="flex-1 flex h-full relative">
      {/* Left Sidebar: Connections List */}
      <aside className="w-72 border-r border-border bg-card/50 flex flex-col">
        <ConnectionList
          key={refreshKey}
          onEdit={handleEdit}
          onProfilesChange={() => setRefreshKey((k) => k + 1)}
          onConnected={handleConnected}
        />
      </aside>

      {/* Right Panel: Branding or Form */}
      <main className="flex-1 flex flex-col bg-background overflow-hidden">
        {showForm ? (
          /* Connection Form */
          <div className="flex-1 flex flex-col overflow-y-auto">
            <div className="p-6 border-b border-border flex items-center gap-4">
              <Button
                variant="ghost"
                size="icon"
                onClick={() => {
                  setShowForm(false);
                  setEditProfile(undefined);
                }}
                className="h-8 w-8"
              >
                <ArrowLeft className="h-4 w-4" />
              </Button>
              <div>
                <h1 className="text-xl font-semibold">
                  {editProfile ? "Edit Connection" : "New Connection"}
                </h1>
                <p className="text-sm text-muted-foreground">
                  {editProfile
                    ? `Update: ${editProfile.name}`
                    : "Create a new database connection"}
                </p>
              </div>
            </div>
            <div className="flex-1 p-6 max-w-2xl">
              <ConnectionForm
                profile={editProfile}
                onSuccess={handleFormSuccess}
              />
            </div>
          </div>
        ) : (
          /* Branding Panel */
          <div className="flex-1 flex flex-col overflow-y-auto">
            {/* Hero Section */}
            <div className="flex-1 flex flex-col items-center justify-center py-10 px-6">
              <HiveLogo />

              <div className="mt-6 text-center space-y-2 max-w-md">
                <div className="flex items-center justify-center gap-2">
                  <span className="uppercase tracking-[0.16em] text-[0.72rem] text-amber-600 dark:text-amber-300/80">
                    DB HIVE
                  </span>
                  <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/20 bg-amber-500/5 px-2 py-0.5">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 shadow-[0_0_0_4px_rgba(16,185,129,0.3)]"></span>
                    <span className="text-[0.7rem] font-medium text-emerald-600 dark:text-emerald-300/90">
                      open source
                    </span>
                  </span>
                </div>
                <h1 className="text-2xl md:text-3xl tracking-tight font-semibold text-foreground">
                  A focused workspace for your data.
                </h1>
                <p className="text-sm text-muted-foreground">
                  Inspect, query, and shape your databases with a developer-first client that stays out of your way.
                </p>
              </div>
            </div>

            {/* Keyboard Shortcuts */}
            <div className="px-6 py-4 border-t border-border">
              <div className="flex items-center gap-2 mb-3">
                <span className="inline-flex h-6 w-6 items-center justify-center rounded-lg bg-amber-500/10 border border-amber-500/40">
                  <Command className="h-3.5 w-3.5 text-amber-600 dark:text-amber-300" strokeWidth={1.5} />
                </span>
                <span className="text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
                  keyboard flow
                </span>
              </div>

              <div className="grid grid-cols-2 gap-2.5 text-xs">
                {[
                  { icon: Plug2, label: "New connection", keys: ["\u2318", "K"] },
                  { icon: Terminal, label: "Run query", keys: ["\u21e7", "\u23ce"] },
                  { icon: Database, label: "Switch table", keys: ["\u2325", "\u21e5"] },
                  { icon: LayoutPanelLeft, label: "Toggle sidebar", keys: ["\u2318", "B"] },
                ].map((shortcut, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between rounded-lg border border-border bg-card px-2.5 py-2"
                  >
                    <div className="flex items-center gap-1.5">
                      <shortcut.icon className="h-3.5 w-3.5 text-foreground" strokeWidth={1.5} />
                      <span className="text-foreground">{shortcut.label}</span>
                    </div>
                    <div className="inline-flex items-center gap-1 rounded-md border border-border bg-secondary px-1.5 py-0.5 text-[0.64rem] text-secondary-foreground">
                      {shortcut.keys.map((key, j) => (
                        <span key={j}>{key}</span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* Supported Databases */}
            <div className="px-6 py-4 border-t border-border">
              <div className="flex items-center gap-2 mb-4">
                <div className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-amber-500/10 border border-amber-500/40">
                  <Database className="h-3.5 w-3.5 text-amber-600 dark:text-amber-300" strokeWidth={1.5} />
                </div>
                <div className="flex flex-col">
                  <span className="text-[0.74rem] font-medium uppercase tracking-[0.18em] text-amber-600 dark:text-amber-200/90">
                    supported databases
                  </span>
                </div>
              </div>

              <div className="grid grid-cols-5 gap-3">
                {DATABASE_TYPES.map((db) => (
                  <div key={db.id} className="flex flex-col items-center gap-1.5 group cursor-default">
                    <div
                      className={`relative w-12 h-12 rounded-xl ${db.bgColor} flex items-center justify-center shadow-lg transition-transform group-hover:scale-105 ${
                        !db.available ? "opacity-60" : ""
                      }`}
                    >
                      <span className="text-xl">{db.icon}</span>
                      {db.available && (
                        <div className="absolute inset-0 rounded-xl bg-white/10 opacity-0 group-hover:opacity-100 transition-opacity" />
                      )}
                    </div>
                    <span className={`text-[0.68rem] text-center leading-tight ${
                      db.available ? "text-foreground" : "text-muted-foreground"
                    }`}>
                      {db.name}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Footer */}
            <footer className="border-t border-border bg-card/50 px-6 py-2.5">
              <div className="flex items-center justify-between text-[0.7rem] text-muted-foreground">
                <div className="flex items-center gap-3">
                  <span className="inline-flex items-center gap-1.5">
                    <span className="inline-flex h-1.5 w-1.5 rounded-full bg-emerald-400 shadow-[0_0_0_4px_rgba(16,185,129,0.16)]"></span>
                    <span>DB Hive</span>
                  </span>
                  <span className="text-muted-foreground/70">v0.4.0-beta</span>
                </div>
                <a
                  href="https://github.com/KwaminaWhyte/db-hive"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1.5 hover:text-amber-600 dark:hover:text-amber-200 transition-colors"
                >
                  <Github className="h-3.5 w-3.5" strokeWidth={1.5} />
                  <span>GitHub</span>
                </a>
              </div>
            </footer>
          </div>
        )}
      </main>
    </div>
  );
}
