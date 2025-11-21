import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ConnectionForm } from "@/components/ConnectionForm";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { ArrowLeft, Settings } from "lucide-react";

export const Route = createFileRoute("/connections/new")({
  component: NewConnectionRoute,
});

function NewConnectionRoute() {
  const navigate = useNavigate({ from: "/connections/new" });

  return (
    <>
      {/* Top Controls */}
      <div className="fixed top-4 left-4 z-50">
        <Button
          variant="outline"
          size="sm"
          onClick={() => navigate({ to: "/connections" })}
        >
          <ArrowLeft className="size-4 mr-2" />
          Back
        </Button>
      </div>

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

      {/* Connection Form */}
      <div className="h-full overflow-y-auto pt-16 px-4">
        <ConnectionForm
          profile={undefined}
          onSuccess={() => navigate({ to: "/connections" })}
        />
      </div>
    </>
  );
}
