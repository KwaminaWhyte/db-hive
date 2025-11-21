import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionForm } from "@/components/ConnectionForm";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionProfile } from "@/types/database";

export const Route = createFileRoute("/connections/$profileId/edit")({
  loader: async ({ params }) => {
    // Load connection profile
    const profiles = await invoke<ConnectionProfile[]>(
      "get_connection_profiles"
    );
    const profile = profiles.find((p) => p.id === params.profileId);

    if (!profile) {
      throw new Error("Connection profile not found");
    }

    return { profile };
  },
  component: EditConnectionRoute,
});

function EditConnectionRoute() {
  const { profile } = Route.useLoaderData();
  const navigate = useNavigate({ from: "/connections/$profileId/edit" });

  return (
    <div className="h-full overflow-y-auto p-6">
      {/* Header */}
      <div className="mb-6">
        <div className="flex items-center justify-between">
          <h2 className="text-2xl font-bold">Edit Connection</h2>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => navigate({ to: "/connections" })}
          >
            <ArrowLeft className="size-4 mr-2" />
            Cancel
          </Button>
        </div>
        <p className="text-sm text-muted-foreground mt-1">
          Update connection: {profile.name}
        </p>
      </div>

      {/* Connection Form */}
      <ConnectionForm
        profile={profile}
        onSuccess={() => navigate({ to: "/connections" })}
      />
    </div>
  );
}
