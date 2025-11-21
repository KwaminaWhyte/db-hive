import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionForm } from "@/components/ConnectionForm";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";

export const Route = createFileRoute("/connections/new")({
  component: NewConnectionRoute,
});

function NewConnectionRoute() {
  const navigate = useNavigate({ from: "/connections/new" });

  return (
    <div className="h-full overflow-y-auto p-6">
      {/* Header */}
      <div className="mb-6">
        <div className="flex items-center justify-between">
          <h2 className="text-2xl font-bold">New Connection</h2>
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
          Create a new database connection
        </p>
      </div>

      {/* Connection Form */}
      <ConnectionForm
        profile={undefined}
        onSuccess={() => navigate({ to: "/connections" })}
      />
    </div>
  );
}
