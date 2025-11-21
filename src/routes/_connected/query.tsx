import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { QueryPanel } from "@/components/QueryPanel";
import { TableInspector } from "@/components/TableInspector";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { useTabContext } from "@/contexts/TabContext";
import { invoke } from "@tauri-apps/api/core";
import { QueryExecutionResult } from "@/types/database";
import { Button } from "@/components/ui/button";
import { X, Plus } from "lucide-react";
import { useEffect } from "react";

/**
 * Query Panel Route with Multi-Tab Support
 *
 * SQL editor interface with support for multiple tabs (queries and table inspectors).
 *
 * URL: /_connected/query?tabs=query-1,table-public.users&active=0
 *
 * Search Params:
 * - tabs: Comma-separated list of tab IDs
 * - active: Index of active tab (0-based, default 0)
 *
 * Features:
 * - Multiple tabs with unique IDs
 * - Mix of query editors and table inspectors
 * - Per-tab state preservation (SQL content, filters, etc.)
 * - LocalStorage persistence per connection
 * - All tabs stay mounted (no content loss on switch)
 */
export const Route = createFileRoute("/_connected/query")({
  validateSearch: (search: Record<string, unknown>): {
    tabs: string;
    active: number;
  } => {
    return {
      tabs: (search.tabs as string) || "query-0",
      active: Number(search.active) || 0,
    };
  },
  component: QueryPanelRoute,
});

function QueryPanelRoute() {
  const navigate = useNavigate();
  const { tabs: tabsParam, active: activeIndex } = Route.useSearch();
  const { connectionId, connectionProfile, currentDatabase } = useConnectionContext();
  const { getTabState, createTabState, removeTabState } = useTabContext();

  // Parse tab IDs from URL
  const tabIds = tabsParam.split(",").filter(Boolean);

  // Initialize tab states if they don't exist
  useEffect(() => {
    tabIds.forEach((tabId) => {
      const existing = getTabState(tabId);
      if (!existing) {
        // Create initial state for this tab
        if (tabId.startsWith("query-")) {
          createTabState({
            id: tabId,
            type: "query",
            label: "Query",
            sql: "",
          });
        } else if (tabId.startsWith("table-")) {
          const [schema, tableName] = tabId.replace("table-", "").split(".");
          createTabState({
            id: tabId,
            type: "table",
            label: `${schema}.${tableName}`,
            schema,
            tableName,
          });
        }
      }
    });
  }, [tabIds, getTabState, createTabState]);

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
    const tabId = tabIds[index];
    const newTabIds = tabIds.filter((_, i) => i !== index);

    if (newTabIds.length === 0) {
      // If no tabs left, create a default query tab
      const newTabId = `query-${Date.now()}`;
      createTabState({
        id: newTabId,
        type: "query",
        label: "Query",
        sql: "",
      });

      navigate({
        to: "/query",
        search: { tabs: newTabId, active: 0 },
      });
      return;
    }

    // Remove tab state
    removeTabState(tabId);

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
        tabs: newTabIds.join(","),
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
    const newTabId = `query-${Date.now()}`;
    createTabState({
      id: newTabId,
      type: "query",
      label: "Query",
      sql: "",
    });

    const newTabIds = [...tabIds, newTabId];
    navigate({
      to: "/query",
      search: {
        tabs: newTabIds.join(","),
        active: newTabIds.length - 1,
      },
    });
  };

  const getTabLabel = (tabId: string) => {
    const state = getTabState(tabId);
    return state?.label || tabId;
  };

  return (
    <div className="flex-1 h-full flex flex-col">
      {/* Tab Bar */}
      <div className="border-b border-border bg-background">
        <div className="flex items-center gap-1 px-2 py-1">
          {tabIds.map((tabId, index) => (
            <div
              key={tabId}
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
              <span className="text-sm font-medium">{getTabLabel(tabId)}</span>
              {tabIds.length > 1 && (
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

      {/* Tab Content - Render ALL tabs but hide inactive ones */}
      <div className="flex-1 overflow-hidden relative">
        {tabIds.map((tabId, index) => {
          const tabState = getTabState(tabId);
          if (!tabState) return null;

          const isActive = index === activeIndex;

          return (
            <div
              key={tabId}
              className={`absolute inset-0 ${isActive ? "block" : "hidden"}`}
            >
              {tabState.type === "query" ? (
                <QueryPanel
                  connectionId={connectionId}
                  connectionProfile={connectionProfile}
                  currentDatabase={currentDatabase}
                  onExecuteQuery={handleExecuteQuery}
                  pendingQuery={tabState.sql || null}
                />
              ) : (
                <TableInspector
                  connectionId={connectionId!}
                  schema={tabState.schema!}
                  tableName={tabState.tableName!}
                  onClose={() => handleCloseTab(index)}
                  driverType={connectionProfile?.driver}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
