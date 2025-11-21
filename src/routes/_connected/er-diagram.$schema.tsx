import { createFileRoute } from "@tanstack/react-router";
import { ERDiagram } from "@/components/ERDiagram";
import { useConnectionContext } from "@/contexts/ConnectionContext";

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

function ERDiagramRoute() {
  const { schema } = Route.useParams();
  const { connectionId } = useConnectionContext();

  return (
    <div className="flex-1 h-full">
      <ERDiagram connectionId={connectionId!} schema={schema} />
    </div>
  );
}
