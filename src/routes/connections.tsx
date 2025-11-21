import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionList } from "@/components/ConnectionList";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { Settings } from "lucide-react";
import { useConnectionContext } from "@/contexts/ConnectionContext";

export const Route = createFileRoute("/connections")({
  component: ConnectionsRoute,
});

function ConnectionsRoute() {
  const navigate = useNavigate({ from: "/connections" });
  const { setConnection } = useConnectionContext();

  return (
    <>
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

      <div className="w-80 border-r overflow-y-auto">
        <ConnectionList
          onEdit={(profile) => {
            if (profile) {
              navigate({
                to: "/connections/$profileId/edit",
                params: { profileId: profile.id },
              });
            } else {
              navigate({ to: "/connections/new" });
            }
          }}
          onProfilesChange={() => {
            // Router will handle refresh
          }}
          onConnected={(connectionId, profile) => {
            // Store connection in context
            setConnection(connectionId, profile);
            // Navigate to query panel (will create in Phase 4)
            console.log("Connected:", connectionId);
            // TODO: navigate({ to: "/query" })
          }}
        />
      </div>

      {/* Right side - show welcome message or instructions */}
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center text-muted-foreground">
          <p className="text-lg">Select a connection to get started</p>
          <p className="text-sm mt-2">or create a new connection</p>
        </div>
      </div>
    </>
  );
}
