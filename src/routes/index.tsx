import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { WelcomeScreen } from "@/components/WelcomeScreen";
import { ModeToggle } from "@/components/mode-toggle";
import { Button } from "@/components/ui/button";
import { Settings } from "lucide-react";

export const Route = createFileRoute("/")({
  component: WelcomeScreenRoute,
});

function WelcomeScreenRoute() {
  const navigate = useNavigate();

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

      <WelcomeScreen
        onNewConnection={() => navigate({ to: "/connections/new" })}
        onRecentConnections={() => navigate({ to: "/connections" })}
        onViewSample={() => {
          console.log("View sample clicked");
        }}
        onOpenDocs={() => {
          window.open("https://github.com/KwaminaWhyte/db-hive/wiki", "_blank");
        }}
      />
    </div>
  );
}
