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
      handler: () => navigate({ to: "/connections", search: { mode: "new" } }),
      description: "New connection",
    },
    {
      key: "Ctrl+K",
      handler: () => navigate({ to: "/connections", search: { mode: "new" } }),
      description: "New connection",
    },
    {
      key: "⌘+R",
      handler: () => navigate({ to: "/connections" }),
      description: "Recent connections",
    },
    {
      key: "Ctrl+R",
      handler: () => navigate({ to: "/connections" }),
      description: "Recent connections",
    },
    {
      key: "⌘+O",
      handler: () => navigate({ to: "/connections", search: { mode: "new" } }),
      description: "View sample workspace",
    },
    {
      key: "Ctrl+O",
      handler: () => navigate({ to: "/connections", search: { mode: "new" } }),
      description: "View sample workspace",
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
        onNewConnection={() => navigate({ to: "/connections", search: { mode: "new" } })}
        onRecentConnections={() => navigate({ to: "/connections" })}
        onViewSample={() => {
          // Navigate to connections page to create a sample SQLite database connection
          // TODO: In the future, we could auto-create a sample SQLite database with demo data
          navigate({ to: "/connections", search: { mode: "new" } });
        }}
        onOpenDocs={() => {
          window.open("https://github.com/KwaminaWhyte/db-hive/wiki", "_blank");
        }}
      />
    </div>
  );
}
