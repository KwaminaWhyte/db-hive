import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { SettingsPage } from "@/components/SettingsPage";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";

export const Route = createFileRoute("/settings")({
  component: SettingsRoute,
});

function SettingsRoute() {
  const navigate = useNavigate({ from: "/settings" });

  return (
    <>
      {/* Back Button */}
      <div className="fixed top-4 left-4 z-50">
        <Button
          variant="outline"
          size="sm"
          onClick={() => navigate({ to: "/" })}
        >
          <ArrowLeft className="size-4 mr-2" />
          Back
        </Button>
      </div>

      {/* Theme Toggle (keep existing position) */}
      <div className="fixed top-4 right-4 z-50">
        <ModeToggle />
      </div>

      <SettingsPage />
    </>
  );
}
