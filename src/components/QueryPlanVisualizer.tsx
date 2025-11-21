/**
 * Query Plan Visualizer Component
 *
 * Visualizes PostgreSQL EXPLAIN query execution plans in a tree structure
 * with cost highlighting and performance insights.
 *
 * Features:
 * - Tree visualization of query plan nodes
 * - Cost highlighting (high cost nodes in red/orange)
 * - Detailed node information on hover
 * - Support for EXPLAIN and EXPLAIN ANALYZE
 * - Execution time metrics
 */

import { useState } from "react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { AlertCircle, ChevronDown, ChevronRight, Info } from "lucide-react";
import { QueryPlanNode, QueryPlanResult } from "@/types/database";

interface QueryPlanVisualizerProps {
  /** The query plan result from EXPLAIN */
  planResult: QueryPlanResult | null;

  /** Loading state */
  loading?: boolean;

  /** Error message if plan parsing failed */
  error?: string;
}

export function QueryPlanVisualizer({
  planResult,
  loading,
  error,
}: QueryPlanVisualizerProps) {
  if (loading) {
    return (
      <Card className="p-6">
        <div className="flex items-center justify-center text-muted-foreground">
          <div className="animate-spin mr-2">⏳</div>
          Loading query plan...
        </div>
      </Card>
    );
  }

  if (error) {
    return (
      <Card className="p-6 border-destructive">
        <div className="flex items-start gap-3">
          <AlertCircle className="h-5 w-5 text-destructive mt-0.5" />
          <div>
            <h3 className="font-medium text-destructive">
              Failed to parse query plan
            </h3>
            <p className="text-sm text-muted-foreground mt-1">{error}</p>
          </div>
        </div>
      </Card>
    );
  }

  if (!planResult) {
    return (
      <Card className="p-6">
        <div className="text-center text-muted-foreground">
          <Info className="h-8 w-8 mx-auto mb-2 opacity-50" />
          <p className="text-sm">
            Run a query with <code className="bg-muted px-1 py-0.5 rounded">EXPLAIN</code> or{" "}
            <code className="bg-muted px-1 py-0.5 rounded">EXPLAIN ANALYZE</code> to view the
            query plan
          </p>
          <p className="text-xs mt-2 text-muted-foreground/70">
            Example: <code className="bg-muted px-1 py-0.5 rounded">EXPLAIN ANALYZE SELECT *
            FROM users WHERE id = 1</code>
          </p>
        </div>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      {/* Metrics Header */}
      {(planResult.planningTime || planResult.executionTime) && (
        <Card className="p-4">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-medium">Execution Metrics</h3>
            <div className="flex items-center gap-4 text-sm">
              {planResult.planningTime !== undefined && (
                <div>
                  <span className="text-muted-foreground">Planning:</span>{" "}
                  <span className="font-mono font-medium">
                    {planResult.planningTime.toFixed(3)}ms
                  </span>
                </div>
              )}
              {planResult.executionTime !== undefined && (
                <div>
                  <span className="text-muted-foreground">Execution:</span>{" "}
                  <span className="font-mono font-medium">
                    {planResult.executionTime.toFixed(3)}ms
                  </span>
                </div>
              )}
              {planResult.totalTime !== undefined && (
                <div>
                  <span className="text-muted-foreground">Total:</span>{" "}
                  <span className="font-mono font-medium text-primary">
                    {planResult.totalTime.toFixed(3)}ms
                  </span>
                </div>
              )}
            </div>
          </div>
        </Card>
      )}

      {/* Plan Tree */}
      <Card className="p-4">
        <h3 className="text-sm font-medium mb-4">Query Plan</h3>
        <PlanNode node={planResult.plan} depth={0} />
      </Card>
    </div>
  );
}

interface PlanNodeProps {
  node: QueryPlanNode;
  depth: number;
}

function PlanNode({ node, depth }: PlanNodeProps) {
  const [expanded, setExpanded] = useState(true);
  const hasChildren = node.plans && node.plans.length > 0;

  // Calculate cost level for color coding
  const getCostLevel = (cost: number | undefined): "low" | "medium" | "high" => {
    if (!cost) return "low";
    if (cost < 10) return "low";
    if (cost < 100) return "medium";
    return "high";
  };

  const costLevel = getCostLevel(node.totalCost);
  const costColors = {
    low: "text-green-600 dark:text-green-400",
    medium: "text-yellow-600 dark:text-yellow-400",
    high: "text-red-600 dark:text-red-400",
  };

  return (
    <div className="space-y-2">
      <div
        className={`flex items-start gap-2 p-3 rounded-lg border bg-card hover:bg-accent/50 transition-colors ${
          depth === 0 ? "border-primary" : ""
        }`}
        style={{ marginLeft: `${depth * 24}px` }}
      >
        {/* Expand/Collapse Button */}
        {hasChildren && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="mt-0.5 hover:bg-accent rounded p-0.5"
          >
            {expanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
          </button>
        )}

        <div className="flex-1 min-w-0">
          {/* Node Type and Relation */}
          <div className="flex items-center gap-2 flex-wrap">
            <Badge variant="secondary" className="font-mono text-xs">
              {node.nodeType}
            </Badge>

            {node.relationName && (
              <span className="text-sm font-medium">
                {node.schema && `${node.schema}.`}
                {node.relationName}
                {node.alias && ` (${node.alias})`}
              </span>
            )}

            {node.indexName && (
              <Badge variant="outline" className="text-xs">
                Index: {node.indexName}
              </Badge>
            )}
          </div>

          {/* Metrics */}
          <div className="flex items-center gap-4 mt-2 text-xs">
            {node.totalCost !== undefined && (
              <div className="flex items-center gap-1">
                <span className="text-muted-foreground">Cost:</span>
                <span className={`font-mono font-medium ${costColors[costLevel]}`}>
                  {node.startupCost?.toFixed(2)}..{node.totalCost.toFixed(2)}
                </span>
              </div>
            )}

            {node.planRows !== undefined && (
              <div className="flex items-center gap-1">
                <span className="text-muted-foreground">Est. Rows:</span>
                <span className="font-mono">{node.planRows.toLocaleString()}</span>
              </div>
            )}

            {node.actualRows !== undefined && (
              <div className="flex items-center gap-1">
                <span className="text-muted-foreground">Actual Rows:</span>
                <span className="font-mono font-medium text-primary">
                  {node.actualRows.toLocaleString()}
                </span>
              </div>
            )}

            {node.actualTotalTime !== undefined && (
              <div className="flex items-center gap-1">
                <span className="text-muted-foreground">Time:</span>
                <span className="font-mono font-medium">
                  {node.actualTotalTime.toFixed(3)}ms
                </span>
              </div>
            )}
          </div>

          {/* Conditions and Filters */}
          {(node.indexCond || node.filter || node.hashCond || node.joinType) && (
            <div className="mt-2 space-y-1 text-xs">
              {node.joinType && (
                <div className="text-muted-foreground">
                  Join Type: <span className="font-mono">{node.joinType}</span>
                </div>
              )}
              {node.indexCond && (
                <div className="text-muted-foreground">
                  Index Cond: <code className="text-foreground">{node.indexCond}</code>
                </div>
              )}
              {node.filter && (
                <div className="text-muted-foreground">
                  Filter: <code className="text-foreground">{node.filter}</code>
                  {node.rowsRemovedByFilter !== undefined && (
                    <span className="ml-2">
                      (removed {node.rowsRemovedByFilter.toLocaleString()} rows)
                    </span>
                  )}
                </div>
              )}
              {node.hashCond && (
                <div className="text-muted-foreground">
                  Hash Cond: <code className="text-foreground">{node.hashCond}</code>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Child Nodes */}
      {hasChildren && expanded && (
        <div className="space-y-2">
          {node.plans!.map((childNode, index) => (
            <PlanNode key={index} node={childNode} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Parse EXPLAIN JSON output into QueryPlanResult
 *
 * PostgreSQL EXPLAIN (FORMAT JSON) returns an array with a single element
 * containing the plan and metrics.
 */
export function parseExplainJson(explainOutput: any): QueryPlanResult {
  // PostgreSQL returns: [{ "Plan": {...}, "Planning Time": 0.123, "Execution Time": 1.234 }]
  const result = Array.isArray(explainOutput) ? explainOutput[0] : explainOutput;

  // Convert PostgreSQL format to our QueryPlanNode format
  function convertNode(pgNode: any): QueryPlanNode {
    return {
      nodeType: pgNode["Node Type"],
      relationName: pgNode["Relation Name"],
      schema: pgNode["Schema"],
      alias: pgNode["Alias"],
      startupCost: pgNode["Startup Cost"],
      totalCost: pgNode["Total Cost"],
      planRows: pgNode["Plan Rows"],
      planWidth: pgNode["Plan Width"],
      actualStartupTime: pgNode["Actual Startup Time"],
      actualTotalTime: pgNode["Actual Total Time"],
      actualRows: pgNode["Actual Rows"],
      actualLoops: pgNode["Actual Loops"],
      indexName: pgNode["Index Name"],
      indexCond: pgNode["Index Cond"],
      filter: pgNode["Filter"],
      rowsRemovedByFilter: pgNode["Rows Removed by Filter"],
      joinType: pgNode["Join Type"],
      hashCond: pgNode["Hash Cond"],
      plans: pgNode["Plans"]?.map(convertNode),
    };
  }

  const plan = convertNode(result["Plan"] || result);

  return {
    plan,
    planningTime: result["Planning Time"],
    executionTime: result["Execution Time"],
    totalTime:
      result["Planning Time"] && result["Execution Time"]
        ? result["Planning Time"] + result["Execution Time"]
        : undefined,
    triggers: result["Triggers"]?.map((t: any) => ({
      triggerName: t["Trigger Name"],
      relationName: t["Relation"],
      time: t["Time"],
      calls: t["Calls"],
    })),
  };
}
