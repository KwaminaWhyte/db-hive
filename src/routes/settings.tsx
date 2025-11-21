import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { SettingsPage } from "@/components/SettingsPage";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { useEffect, useState } from "react";

export const Route = createFileRoute("/settings")({
  component: SettingsRoute,
});

function SettingsRoute() {
  const navigate = useNavigate({ from: "/settings" });
  const [previousRoute, setPreviousRoute] = useState<string>("/");

  // Store the previous route before navigating to settings
  useEffect(() => {
    const saved = sessionStorage.getItem("db-hive-previous-route");
    if (saved) {
      setPreviousRoute(saved);
      // Clear after reading
      sessionStorage.removeItem("db-hive-previous-route");
    }
  }, []);

  const handleBack = () => {
    // Navigate back to the previous route
    if (previousRoute === "/query") {
      navigate({ to: "/query" as any });
    } else if (previousRoute === "/connections") {
      navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } });
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
