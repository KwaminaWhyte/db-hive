import { FC } from "react";
import { Activity, AlertCircle, Clock, Database } from "lucide-react";
import { ActivityStats as ActivityStatsType, QueryType, QueryStatus } from "../../types/activity";
import { Skeleton } from "@/components/ui/skeleton";

interface ActivityStatsProps {
  stats: ActivityStatsType | null;
  loading: boolean;
}

/**
 * Format large numbers with K, M, B suffixes
 */
const formatNumber = (num: number): string => {
  if (num >= 1_000_000_000) {
    return (num / 1_000_000_000).toFixed(1) + "B";
  }
  if (num >= 1_000_000) {
    return (num / 1_000_000).toFixed(1) + "M";
  }
  if (num >= 1_000) {
    return (num / 1_000).toFixed(1) + "K";
  }
  return num.toString();
};

/**
 * Format duration in ms to human-readable format
 */
const formatDuration = (ms: number): string => {
  if (ms >= 1000) {
    return (ms / 1000).toFixed(2) + "s";
  }
  return ms.toFixed(0) + "ms";
};

/**
 * Get color class for query type
 */
const getQueryTypeColor = (type: QueryType): string => {
  switch (type) {
    case "SELECT":
      return "bg-blue-500";
    case "INSERT":
      return "bg-green-500";
    case "UPDATE":
      return "bg-yellow-500";
    case "DELETE":
      return "bg-red-500";
    case "CREATE":
      return "bg-purple-500";
    case "ALTER":
      return "bg-indigo-500";
    case "DROP":
      return "bg-pink-500";
    case "TRANSACTION":
      return "bg-cyan-500";
    case "OTHER":
      return "bg-gray-500";
    default:
      return "bg-gray-500";
  }
};

/**
 * Get color class for query status
 */
const getStatusColor = (status: QueryStatus): string => {
  switch (status) {
    case "completed":
      return "text-green-600 dark:text-green-400";
    case "failed":
      return "text-red-600 dark:text-red-400";
    case "running":
      return "text-blue-600 dark:text-blue-400";
    case "cancelled":
      return "text-yellow-600 dark:text-yellow-400";
    default:
      return "text-gray-600 dark:text-gray-400";
  }
};

/**
 * Summary Card Component
 */
interface SummaryCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  accent?: "red" | "default";
  loading?: boolean;
}

const SummaryCard: FC<SummaryCardProps> = ({ icon, label, value, accent = "default", loading }) => {
  if (loading) {
    return (
      <div className="flex flex-col gap-2 rounded-lg border bg-card p-4 text-card-foreground">
        <div className="flex items-center gap-2">
          <Skeleton className="h-5 w-5" />
          <Skeleton className="h-4 w-24" />
        </div>
        <Skeleton className="h-8 w-20" />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2 rounded-lg border bg-card p-4 text-card-foreground">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <div className={accent === "red" ? "text-red-500" : ""}>{icon}</div>
        <span>{label}</span>
      </div>
      <div className={`text-2xl font-bold ${accent === "red" ? "text-red-600 dark:text-red-400" : ""}`}>
        {value}
      </div>
    </div>
  );
};

/**
 * Query Type Bar Component
 */
interface QueryTypeBarProps {
  type: QueryType;
  count: number;
  total: number;
}

const QueryTypeBar: FC<QueryTypeBarProps> = ({ type, count, total }) => {
  const percentage = total > 0 ? (count / total) * 100 : 0;

  return (
    <div className="flex items-center gap-2">
      <div className="w-20 text-sm font-medium">{type}</div>
      <div className="flex-1">
        <div className="h-6 w-full overflow-hidden rounded-md bg-muted">
          <div
            className={`h-full ${getQueryTypeColor(type)} transition-all duration-300`}
            style={{ width: `${percentage}%` }}
          />
        </div>
      </div>
      <div className="w-16 text-right text-sm text-muted-foreground">
        {count} ({percentage.toFixed(0)}%)
      </div>
    </div>
  );
};

/**
 * Status Item Component
 */
interface StatusItemProps {
  status: QueryStatus;
  count: number;
  total: number;
}

const StatusItem: FC<StatusItemProps> = ({ status, count, total }) => {
  const percentage = total > 0 ? (count / total) * 100 : 0;
  const capitalizedStatus = status.charAt(0).toUpperCase() + status.slice(1);

  return (
    <div className="flex items-center justify-between py-2">
      <div className="flex items-center gap-2">
        <div className={`h-3 w-3 rounded-full ${getStatusColor(status).replace("text-", "bg-")}`} />
        <span className="text-sm font-medium">{capitalizedStatus}</span>
      </div>
      <div className="flex items-center gap-4">
        <span className="text-sm text-muted-foreground">{percentage.toFixed(0)}%</span>
        <span className={`text-sm font-bold ${getStatusColor(status)}`}>{count}</span>
      </div>
    </div>
  );
};

/**
 * Activity Statistics Component
 */
export const ActivityStats: FC<ActivityStatsProps> = ({ stats, loading }) => {
  // Calculate query types sorted by count
  const queryTypeEntries = stats
    ? (Object.entries(stats.queriesByType) as [QueryType, number][])
        .filter(([_, count]) => count > 0)
        .sort(([, a], [, b]) => b - a)
    : [];

  // Calculate status breakdown
  const statusEntries = stats
    ? (Object.entries(stats.queriesByStatus) as [QueryStatus, number][])
    : [];

  return (
    <div className="flex flex-col gap-6">
      {/* Summary Cards */}
      <div className="grid grid-cols-2 gap-4">
        <SummaryCard
          icon={<Activity className="h-5 w-5" />}
          label="Total Queries"
          value={stats ? formatNumber(stats.totalQueries) : "0"}
          loading={loading}
        />
        <SummaryCard
          icon={<AlertCircle className="h-5 w-5" />}
          label="Failed Queries"
          value={stats ? formatNumber(stats.failedQueries) : "0"}
          accent="red"
          loading={loading}
        />
        <SummaryCard
          icon={<Clock className="h-5 w-5" />}
          label="Avg Duration"
          value={stats ? formatDuration(stats.avgDuration) : "0ms"}
          loading={loading}
        />
        <SummaryCard
          icon={<Database className="h-5 w-5" />}
          label="Total Rows"
          value={stats ? formatNumber(stats.totalRows) : "0"}
          loading={loading}
        />
      </div>

      {/* Query Type Breakdown & Status Breakdown */}
      <div className="grid gap-4 grid-cols-1">
        {/* Query Type Breakdown */}
        <div className="flex flex-col gap-4 rounded-lg border bg-card p-4 text-card-foreground">
          <h3 className="text-lg font-semibold">Query Types</h3>
          {loading ? (
            <div className="flex flex-col gap-3">
              {[1, 2, 3].map((i) => (
                <div key={i} className="flex items-center gap-2">
                  <Skeleton className="h-4 w-20" />
                  <Skeleton className="h-6 flex-1" />
                  <Skeleton className="h-4 w-16" />
                </div>
              ))}
            </div>
          ) : queryTypeEntries.length > 0 ? (
            <div className="flex flex-col gap-3">
              {queryTypeEntries.map(([type, count]) => (
                <QueryTypeBar
                  key={type}
                  type={type}
                  count={count}
                  total={stats?.totalQueries || 0}
                />
              ))}
            </div>
          ) : (
            <div className="py-8 text-center text-sm text-muted-foreground">
              No queries executed yet
            </div>
          )}
        </div>

        {/* Status Breakdown */}
        <div className="flex flex-col gap-4 rounded-lg border bg-card p-4 text-card-foreground">
          <h3 className="text-lg font-semibold">Status Breakdown</h3>
          {loading ? (
            <div className="flex flex-col gap-2">
              {[1, 2, 3, 4].map((i) => (
                <div key={i} className="flex items-center justify-between py-2">
                  <div className="flex items-center gap-2">
                    <Skeleton className="h-3 w-3 rounded-full" />
                    <Skeleton className="h-4 w-20" />
                  </div>
                  <div className="flex items-center gap-4">
                    <Skeleton className="h-4 w-12" />
                    <Skeleton className="h-4 w-8" />
                  </div>
                </div>
              ))}
            </div>
          ) : statusEntries.length > 0 ? (
            <div className="flex flex-col divide-y divide-border">
              {statusEntries.map(([status, count]) => (
                <StatusItem
                  key={status}
                  status={status}
                  count={count}
                  total={stats?.totalQueries || 0}
                />
              ))}
            </div>
          ) : (
            <div className="py-8 text-center text-sm text-muted-foreground">
              No status data available
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
