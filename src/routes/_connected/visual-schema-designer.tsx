import { lazy, Suspense } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { Spinner } from "@/components/ui/spinner";

// Shares the reactflow chunk with the ER diagram route.
const VisualSchemaDesigner = lazy(
  () => import("@/components/VisualSchemaDesigner")
);

export const Route = createFileRoute("/_connected/visual-schema-designer")({
  component: VisualSchemaDesignerRoute,
});

function RouteChunkFallback({ label }: { label: string }) {
  return (
    <div
      role="status"
      aria-label={label}
      className="flex h-full w-full items-center justify-center"
    >
      <Spinner className="size-6 text-muted-foreground" />
    </div>
  );
}

function VisualSchemaDesignerRoute() {
  const { connectionId } = useConnectionContext();
  return (
    <Suspense fallback={<RouteChunkFallback label="Loading schema designer" />}>
      <VisualSchemaDesigner connectionId={connectionId!} />
    </Suspense>
  );
}
