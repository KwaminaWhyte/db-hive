import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionList } from "@/components/ConnectionList";
import { ConnectionForm } from "@/components/ConnectionForm";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { Settings } from "lucide-react";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionProfile } from "@/types/database";

// Define search params validation
export const Route = createFileRoute("/connections")({
  validateSearch: (search: Record<string, unknown>): { mode?: "new" | "edit"; profileId?: string } => {
    return {
      mode: search.mode as "new" | "edit" | undefined,
      profileId: search.profileId as string | undefined,
    };
  },
  component: ConnectionsRoute,
});

function ConnectionsRoute() {
  const navigate = useNavigate({ from: "/connections" });
  const { setConnection } = useConnectionContext();
  const { mode, profileId } = Route.useSearch();
  const [editProfile, setEditProfile] = useState<ConnectionProfile | undefined>();
  const [loading, setLoading] = useState(false);

  // Load profile data when in edit mode
  useEffect(() => {
    if (mode === "edit" && profileId) {
      setLoading(true);
      invoke<ConnectionProfile[]>("list_connection_profiles")
        .then((profiles) => {
          const profile = profiles.find((p) => p.id === profileId);
          setEditProfile(profile);
        })
        .catch((err) => {
          console.error("Failed to load profile:", err);
          // Navigate back to connections if profile not found
          navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } });
        })
        .finally(() => {
          setLoading(false);
        });
    } else {
      setEditProfile(undefined);
    }
  }, [mode, profileId, navigate]);

  const showForm = mode === "new" || mode === "edit";

  return (
    <div className="flex-1 flex h-full relative">
      {/* Top Right Controls */}
      <div className="fixed top-4 right-4 z-50 flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => navigate({ to: "/settings" })}
        >
          <Settings className="size-4" />
        </Button>
        <ModeToggle />
      </div>

      {/* Left Sidebar - Always visible */}
      <div className="w-80 border-r overflow-y-auto">
        <ConnectionList
          onEdit={(profile) => {
            if (profile) {
              navigate({
                to: "/connections",
                search: { mode: "edit", profileId: profile.id },
              });
            } else {
              navigate({ to: "/connections", search: { mode: "new", profileId: undefined } });
            }
          }}
          onProfilesChange={() => {
            // Reload if we're in edit mode
            if (mode === "edit" && profileId) {
              navigate({ to: "/connections", search: { mode: "edit", profileId } });
            }
          }}
          onConnected={(connectionId, profile) => {
            // Store connection in context
            setConnection(connectionId, profile);

            // Navigate to query panel with default query tab
            // Tabs will be restored per-database in the query route
            const defaultTabId = `query-${Date.now()}`;
            navigate({ to: "/query", search: { tabs: defaultTabId, active: 0 } });
          }}
        />
      </div>

      {/* Right side - show placeholder or form */}
      {!showForm ? (
        // Show placeholder when no mode is set
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center text-muted-foreground">
            <p className="text-lg">Select a connection to get started</p>
            <p className="text-sm mt-2">or create a new connection</p>
          </div>
        </div>
      ) : (
        // Show form on the right
        <div className="flex-1 overflow-y-auto p-6">
          <div className="mb-6">
            <div className="flex items-center justify-between">
              <h2 className="text-2xl font-bold">
                {mode === "edit" ? "Edit Connection" : "New Connection"}
              </h2>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } })}
              >
                Cancel
              </Button>
            </div>
            <p className="text-sm text-muted-foreground mt-1">
              {mode === "edit"
                ? `Update connection: ${editProfile?.name || ""}`
                : "Create a new database connection"}
            </p>
          </div>

          {loading ? (
            <div className="flex items-center justify-center py-12">
              <div className="text-muted-foreground">Loading...</div>
            </div>
          ) : (
            <ConnectionForm
              profile={mode === "edit" ? editProfile : undefined}
              onSuccess={() => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } })}
            />
          )}
        </div>
      )}
    </div>
  );
}
