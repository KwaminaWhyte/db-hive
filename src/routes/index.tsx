import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { WelcomeScreen } from "@/components/WelcomeScreen";
import { useRouteShortcuts } from "@/hooks/useKeyboardShortcuts";

export const Route = createFileRoute("/")({
  component: WelcomeScreenRoute,
});

function WelcomeScreenRoute() {
  const navigate = useNavigate();

  // Wire up keyboard shortcuts for welcome page
  useRouteShortcuts([
    {
      key: "⌘+K",
      handler: () => navigate({ to: "/connections", search: { mode: "new", profileId: undefined } }),
      description: "New connection",
    },
    {
      key: "Ctrl+K",
      handler: () => navigate({ to: "/connections", search: { mode: "new", profileId: undefined } }),
      description: "New connection",
    },
    {
      key: "⌘+R",
      handler: () => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } }),
      description: "Recent connections",
    },
    {
      key: "Ctrl+R",
      handler: () => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } }),
      description: "Recent connections",
    },
    {
      key: "?",
      handler: () => window.open("https://github.com/KwaminaWhyte/db-hive/wiki", "_blank"),
      description: "Documentation",
    },
  ]);

  return (
    <div className="flex-1 flex h-full relative">
      <WelcomeScreen
        onNewConnection={() => navigate({ to: "/connections", search: { mode: "new", profileId: undefined } })}
        onRecentConnections={() => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } })}
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
