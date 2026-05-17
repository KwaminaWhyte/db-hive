import { createFileRoute } from "@tanstack/react-router";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { RedisKeyBrowser } from "@/components/RedisKeyBrowser";
import { Database } from "lucide-react";

export const Route = createFileRoute("/_connected/redis-keys")({
  component: RedisKeysRoute,
});

function RedisKeysRoute() {
  const { connectionId, connectionProfile } = useConnectionContext();

  if (connectionProfile?.driver !== "Redis") {
    return (
      <div className="h-full flex items-center justify-center p-4">
        <div className="text-center space-y-2">
          <Database className="h-12 w-12 mx-auto text-muted-foreground opacity-50" />
          <p className="text-muted-foreground">
            This view is only available for Redis connections
          </p>
          <p className="text-sm text-muted-foreground">
            Connect to a Redis database to browse keys.
          </p>
        </div>
      </div>
    );
  }

  return <RedisKeyBrowser connectionId={connectionId!} />;
}
