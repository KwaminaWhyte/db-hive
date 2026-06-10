import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { SettingsPage } from "@/components/SettingsPage";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";

export const Route = createFileRoute("/settings")({
  component: SettingsRoute,
});

function SettingsRoute() {
  const navigate = useNavigate({ from: "/settings" });

  const handleBack = () => {
    if (window.history.length > 1) {
      window.history.back();
    } else {
      navigate({ to: "/" });
    }
  };

  return (
    <div className="flex-1 h-full relative">
      {/* Back Button */}
      <div className="absolute top-4 left-4 z-50">
        <Button
          variant="outline"
          size="sm"
          onClick={handleBack}
        >
          <ArrowLeft className="size-4 mr-2" />
          Back
        </Button>
      </div>

      <SettingsPage />
    </div>
  );
}
