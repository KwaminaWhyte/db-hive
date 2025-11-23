import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionDashboard } from "@/components/ConnectionDashboard";
import { ConnectionForm } from "@/components/ConnectionForm";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionProfile } from "@/types/database";

// Define search params validation
export const Route = createFileRoute("/connections")({
  validateSearch: (
    search: Record<string, unknown>,
  ): { mode?: "new" | "edit"; profileId?: string } => {
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
  const [editProfile, setEditProfile] = useState<
    ConnectionProfile | undefined
  >();
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
          navigate({
            to: "/connections",
            search: { mode: undefined, profileId: undefined },
          });
        })
        .finally(() => {
          setLoading(false);
        });
    } else {
      setEditProfile(undefined);
    }
  }, [mode, profileId, navigate]);

  const showModal = mode === "new" || mode === "edit";

  return (
    <div className="w-full h-full">
      {/* Full-page dashboard */}
      <ConnectionDashboard
        onConnected={(connectionId, profile) => {
          // Store connection in context
          setConnection(connectionId, profile);

          // Navigate to query panel with default query tab
          const defaultTabId = `query-${Date.now()}`;
          navigate({ to: "/query", search: { tabs: defaultTabId, active: 0 } });
        }}
        onNewConnection={() => {
          navigate({ to: "/connections", search: { mode: "new" } });
        }}
        onEditConnection={(profile) => {
          navigate({
            to: "/connections",
            search: { mode: "edit", profileId: profile.id },
          });
        }}
      />

      {/* Modal for Connection Form */}
      <Dialog
        open={showModal}
        onOpenChange={(open) => {
          if (!open) {
            navigate({
              to: "/connections",
              search: { mode: undefined, profileId: undefined },
            });
          }
        }}
      >
        <DialogContent className="min-w-4xl max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="text-2xl">
              {mode === "edit" ? "Edit Connection" : "New Connection"}
            </DialogTitle>
            <DialogDescription>
              {mode === "edit"
                ? `Update connection: ${editProfile?.name || ""}`
                : "Create a new database connection"}
            </DialogDescription>
          </DialogHeader>

          {loading ? (
            <div className="flex items-center justify-center py-12">
              <div className="text-muted-foreground">Loading...</div>
            </div>
          ) : (
            <ConnectionForm
              profile={mode === "edit" ? editProfile : undefined}
              onSuccess={() =>
                navigate({
                  to: "/connections",
                  search: { mode: undefined, profileId: undefined },
                })
              }
            />
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
