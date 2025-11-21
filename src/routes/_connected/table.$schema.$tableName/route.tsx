import { createFileRoute } from "@tanstack/react-router";

/**
 * Table Layout Route
 *
 * Parent route for table-related views (data, structure, indexes, etc.)
 *
 * URL: /_connected/table/{schema}/{tableName}
 *
 * Params:
 * - schema: string (database schema name, e.g., "public")
 * - tableName: string (table name, e.g., "users")
 */
export const Route = createFileRoute("/_connected/table/$schema/$tableName")({
  component: () => {
    // This is just a pass-through layout
    // The actual content is in the index route
    return null;
  },
});
