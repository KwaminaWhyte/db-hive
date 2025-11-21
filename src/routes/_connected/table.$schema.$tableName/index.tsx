import { createFileRoute, redirect } from "@tanstack/react-router";

/**
 * Table Inspector Route (Legacy - Redirects to Query with Tabs)
 *
 * This route now redirects to the query route with multi-tab support.
 * Direct navigation to /table/{schema}/{tableName} will open the table in a new tab.
 *
 * URL: /_connected/table/{schema}/{tableName}
 *
 * Params:
 * - schema: string (database schema name)
 * - tableName: string (table name)
 *
 * Redirects to: /query?tabs=query,{schema}.{tableName}&active=1
 */
export const Route = createFileRoute("/_connected/table/$schema/$tableName/")({
  beforeLoad: ({ params }) => {
    // Redirect to query route with this table in a tab
    const tableId = `${params.schema}.${params.tableName}`;

    throw redirect({
      to: "/query",
      search: {
        tabs: `query,${tableId}`,
        active: 1, // Make the table tab active
      },
    });
  },
});
