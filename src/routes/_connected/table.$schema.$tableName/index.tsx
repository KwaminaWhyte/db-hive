import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { TableInspector } from "@/components/TableInspector";
import { useConnectionContext } from "@/contexts/ConnectionContext";

/**
 * Table Inspector Route
 *
 * Displays table data with pagination, filtering, and sorting.
 *
 * URL: /_connected/table/{schema}/{tableName}
 *
 * Params:
 * - schema: string (database schema name)
 * - tableName: string (table name)
 *
 * Note: Pagination and filtering are handled internally by TableInspector component
 */
export const Route = createFileRoute("/_connected/table/$schema/$tableName/")({
  component: TableInspectorRoute,
});

function TableInspectorRoute() {
  const { schema, tableName } = Route.useParams();
  const { connectionId, connectionProfile } = useConnectionContext();
  const navigate = useNavigate();

  const handleClose = () => {
    navigate({ to: "/query" });
  };

  return (
    <div className="flex-1 h-full">
      <TableInspector
        connectionId={connectionId!}
        schema={schema}
        tableName={tableName}
        onClose={handleClose}
        driverType={connectionProfile?.driver}
      />
    </div>
  );
}
