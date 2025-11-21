import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { QueryPanel } from "@/components/QueryPanel";
import { TableInspector } from "@/components/TableInspector";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { invoke } from "@tauri-apps/api/core";
import { QueryExecutionResult } from "@/types/database";
import { Button } from "@/components/ui/button";
import { X, Plus } from "lucide-react";

/**
 * Query Panel Route with Multi-Tab Support
 *
 * SQL editor interface with support for multiple tabs (queries and table inspectors).
 *
 * URL: /_connected/query?tabs=query,public.users,public.orders&active=0
 *
 * Search Params:
 * - tabs: Comma-separated list of tab identifiers
 *   - "query" = SQL editor tab
 *   - "schema.tableName" = Table inspector tab
 * - active: Index of active tab (0-based, default 0)
 *
 * Features:
 * - Multiple tabs support
 * - Mix of query editors and table inspectors
 * - URL-based state (preserves tabs on refresh/back/forward)
 * - Tab close with automatic switching
 */
export const Route = createFileRoute("/_connected/query")({
  validateSearch: (search: Record<string, unknown>): {
    tabs: string;
    active: number;
  } => {
    return {
      tabs: (search.tabs as string) || "query",
      active: Number(search.active) || 0,
    };
  },
  component: QueryPanelRoute,
});

function QueryPanelRoute() {
  const navigate = useNavigate();
  const { tabs: tabsParam, active: activeIndex } = Route.useSearch();
  const { connectionId, connectionProfile, currentDatabase } = useConnectionContext();

  // Parse tabs from URL
  const tabs = tabsParam.split(",").filter(Boolean);
  const activeTab = tabs[activeIndex] || tabs[0];

  const handleExecuteQuery = async (sql: string): Promise<QueryExecutionResult> => {
    try {
      const result = await invoke<QueryExecutionResult>("execute_query", {
        connectionId,
        sql,
      });
      return result;
    } catch (error) {
      throw error;
    }
  };

  const handleCloseTab = (index: number) => {
    const newTabs = tabs.filter((_, i) => i !== index);

    if (newTabs.length === 0) {
      // If no tabs left, create a default query tab
      navigate({
        to: "/query",
        search: { tabs: "query", active: 0 },
      });
      return;
    }

    // Adjust active index if needed
    let newActive = activeIndex;
    if (index === activeIndex) {
      // Closing active tab - switch to previous or next
      newActive = Math.max(0, index - 1);
    } else if (index < activeIndex) {
      // Closing tab before active - adjust index
      newActive = activeIndex - 1;
    }

    navigate({
      to: "/query",
      search: {
        tabs: newTabs.join(","),
        active: newActive,
      },
    });
  };

  const handleSwitchTab = (index: number) => {
    navigate({
      to: "/query",
      search: {
        tabs: tabsParam,
        active: index,
      },
    });
  };

  const handleAddQueryTab = () => {
    const newTabs = [...tabs, "query"];
    navigate({
      to: "/query",
      search: {
        tabs: newTabs.join(","),
        active: newTabs.length - 1,
      },
    });
  };

  const getTabLabel = (tab: string) => {
    if (tab === "query") return "Query";
    const [schema, tableName] = tab.split(".");
    return `${schema}.${tableName}`;
  };

  const handleTableClose = () => {
    // When table inspector's close button is clicked
    handleCloseTab(activeIndex);
  };

  return (
    <div className="flex-1 h-full flex flex-col">
      {/* Tab Bar */}
      <div className="border-b border-border bg-background">
        <div className="flex items-center gap-1 px-2 py-1">
          {tabs.map((tab, index) => (
            <div
              key={`${tab}-${index}`}
              className={`
                group relative flex items-center gap-2 px-3 py-1.5 rounded-t-lg cursor-pointer
                transition-colors
                ${
                  index === activeIndex
                    ? "bg-accent text-foreground border-b-2 border-primary"
                    : "hover:bg-accent/50 text-muted-foreground"
                }
              `}
              onClick={() => handleSwitchTab(index)}
            >
              <span className="text-sm font-medium">{getTabLabel(tab)}</span>
              {tabs.length > 1 && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleCloseTab(index);
                  }}
                  className="opacity-0 group-hover:opacity-100 hover:bg-muted rounded p-0.5 transition-opacity"
                  title="Close tab"
                >
                  <X className="h-3 w-3" />
                </button>
              )}
            </div>
          ))}

          {/* Add Query Tab Button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleAddQueryTab}
            className="h-7 px-2"
            title="New query tab"
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Active Tab Content */}
      <div className="flex-1 overflow-hidden">
        {activeTab === "query" ? (
          <QueryPanel
            connectionId={connectionId}
            connectionProfile={connectionProfile}
            currentDatabase={currentDatabase}
            onExecuteQuery={handleExecuteQuery}
          />
        ) : (
          (() => {
            const [schema, tableName] = activeTab.split(".");
            return (
              <TableInspector
                connectionId={connectionId!}
                schema={schema}
                tableName={tableName}
                onClose={handleTableClose}
                driverType={connectionProfile?.driver}
              />
            );
          })()
        )}
      </div>
    </div>
  );
}
