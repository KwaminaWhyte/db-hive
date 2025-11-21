import { createFileRoute } from "@tanstack/react-router";
import { WelcomeScreen } from "@/components/WelcomeScreen";

export const Route = createFileRoute("/")({
  component: WelcomeScreenRoute,
});

function WelcomeScreenRoute() {

  return (
    <WelcomeScreen
      onNewConnection={() => {
        // TODO: Navigate to connection form when route exists
        console.log("New connection clicked");
      }}
      onRecentConnections={() => {
        // TODO: Navigate to connections when route exists
        console.log("Recent connections clicked");
      }}
      onViewSample={() => {
        console.log("View sample clicked");
      }}
      onOpenDocs={() => {
        window.open("https://github.com/anthropics/db-hive/wiki", "_blank");
      }}
    />
  );
}
