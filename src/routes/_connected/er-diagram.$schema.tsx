import { lazy, Suspense } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { Spinner } from "@/components/ui/spinner";

// reactflow + dagre are only used by this route and the schema designer.
const ERDiagram = lazy(() =>
  import("@/components/ERDiagram").then((m) => ({ default: m.ERDiagram }))
);

/**
 * ER Diagram Route
 *
 * Displays entity-relationship diagram for the specified database schema.
 *
 * URL: /_connected/er-diagram/{schema}
 *
 * Params:
 * - schema: string (database schema name, e.g., "public")
 *
 * Features:
 * - Visual representation of tables and relationships
 * - Interactive diagram with zoom/pan
 * - Table details on click
 */
export const Route = createFileRoute("/_connected/er-diagram/$schema")({
  component: ERDiagramRoute,
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

function ERDiagramRoute() {
  const { schema } = Route.useParams();
  const { connectionId } = useConnectionContext();

  return (
    <div className="flex-1 h-full">
      <Suspense fallback={<RouteChunkFallback label="Loading ER diagram" />}>
        <ERDiagram connectionId={connectionId!} schema={schema} />
      </Suspense>
    </div>
  );
}
