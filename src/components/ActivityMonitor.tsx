/**
 * Activity Monitor Component
 *
 * Comprehensive activity monitoring dashboard for tracking query execution,
 * performance metrics, and activity statistics.
 */

import { FC, useState, useEffect, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  RefreshCw,
  Trash2,
  Download,
  Play,
  Pause,
  Filter,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from './ui/resizable';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Button } from './ui/button';
import { Badge } from './ui/badge';
import { Separator } from './ui/separator';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from './ui/collapsible';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select';
import { Checkbox } from './ui/checkbox';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { Skeleton } from './ui/skeleton';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from './ui/alert-dialog';
import type {
  QueryLog,
  QueryLogFilter,
  QueryLogSort,
  QueryLogResponse,
  ActivityStats,
  QueryType,
  QueryStatus,
} from '@/types/activity';

interface ActivityMonitorProps {
  /** Optional initial connection ID filter */
  connectionId?: string;
}

/**
 * Auto-refresh interval options (in milliseconds)
 */
type RefreshInterval = 5000 | 10000 | 30000 | 0;

const REFRESH_INTERVALS: { label: string; value: RefreshInterval }[] = [
  { label: '5 seconds', value: 5000 },
  { label: '10 seconds', value: 10000 },
  { label: '30 seconds', value: 30000 },
  { label: 'Off', value: 0 },
];

export const ActivityMonitor: FC<ActivityMonitorProps> = ({ connectionId }) => {
  // State
  const [logs, setLogs] = useState<QueryLog[]>([]);
  const [stats, setStats] = useState<ActivityStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [statsLoading, setStatsLoading] = useState(false);

  // Pagination
  const [page, setPage] = useState(0);
  const [pageSize] = useState(50);
  const [total, setTotal] = useState(0);

  // Sort
  const [sort] = useState<QueryLogSort>({
    field: 'startedAt',
    direction: 'desc',
  });

  // Filters
  const [filters, setFilters] = useState<QueryLogFilter>({
    connectionId,
  });

  // Auto-refresh
  const [autoRefreshEnabled, setAutoRefreshEnabled] = useState(false);
  const [refreshInterval, setRefreshInterval] = useState<RefreshInterval>(10000);

  // UI State
  const [filtersOpen, setFiltersOpen] = useState(true);
  const [statsOpen, setStatsOpen] = useState(true);
  const [availableConnections, setAvailableConnections] = useState<
    { id: string; name: string }[]
  >([]);
  const [availableDatabases, setAvailableDatabases] = useState<string[]>([]);

  // Query type filter state
  const [selectedQueryTypes, setSelectedQueryTypes] = useState<Set<QueryType>>(
    new Set()
  );

  // Status filter state
  const [selectedStatuses, setSelectedStatuses] = useState<Set<QueryStatus>>(
    new Set()
  );

  // Duration range
  const [minDuration, setMinDuration] = useState<string>('');
  const [maxDuration, setMaxDuration] = useState<string>('');

  // Search text
  const [searchText, setSearchText] = useState<string>('');

  // Fetch query logs
  const fetchLogs = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invoke<QueryLogResponse>('get_query_logs', {
        filter: filters,
        sort,
        page,
        pageSize,
      });
      setLogs(response.logs);
      setTotal(response.total);
    } catch (error) {
      console.error('Failed to fetch query logs:', error);
      // In a real app, show error toast
    } finally {
      setLoading(false);
    }
  }, [filters, sort, page, pageSize]);

  // Fetch activity stats
  const fetchStats = useCallback(async () => {
    setStatsLoading(true);
    try {
      const statsData = await invoke<ActivityStats>('get_activity_stats', {
        filter: filters,
      });
      setStats(statsData);
    } catch (error) {
      console.error('Failed to fetch activity stats:', error);
    } finally {
      setStatsLoading(false);
    }
  }, [filters]);

  // Fetch metadata (connections, databases)
  const fetchMetadata = useCallback(async () => {
    try {
      // Fetch available connections using correct command name
      const profiles = await invoke<{ id: string; name: string }[]>(
        'list_connection_profiles'
      );
      // Map profiles to connection list format
      setAvailableConnections(profiles.map(p => ({ id: p.id, name: p.name })));

      // Fetch available databases (if a connection is selected)
      if (filters.connectionId) {
        try {
          const databases = await invoke<string[]>('get_databases', {
            connectionId: filters.connectionId,
          });
          setAvailableDatabases(databases);
        } catch {
          // Connection might not be active, ignore database fetch error
          setAvailableDatabases([]);
        }
      }
    } catch (error) {
      console.error('Failed to fetch metadata:', error);
    }
  }, [filters.connectionId]);

  // Initial fetch
  useEffect(() => {
    fetchLogs();
    fetchStats();
    fetchMetadata();
  }, [fetchLogs, fetchStats, fetchMetadata]);

  // Auto-refresh effect
  useEffect(() => {
    if (autoRefreshEnabled && refreshInterval > 0) {
      const timer = setInterval(() => {
        fetchLogs();
        fetchStats();
      }, refreshInterval);

      return () => clearInterval(timer);
    }
  }, [autoRefreshEnabled, refreshInterval, fetchLogs, fetchStats]);

  // Handle refresh
  const handleRefresh = useCallback(() => {
    fetchLogs();
    fetchStats();
  }, [fetchLogs, fetchStats]);

  // Handle clear logs
  const handleClearLogs = useCallback(async () => {
    try {
      await invoke('clear_query_logs');
      fetchLogs();
      fetchStats();
      // Show success toast
    } catch (error) {
      console.error('Failed to clear logs:', error);
      // Show error toast
    }
  }, [fetchLogs, fetchStats]);

  // Handle export logs
  const handleExportLogs = useCallback(async () => {
    try {
      await invoke('export_query_logs', {
        filter: filters,
        format: 'csv',
      });
      // Show success toast
    } catch (error) {
      console.error('Failed to export logs:', error);
      // Show error toast
    }
  }, [filters]);

  // Handle apply filters
  const handleApplyFilters = useCallback(() => {
    const newFilters: QueryLogFilter = {
      connectionId: filters.connectionId,
      database: filters.database,
      queryType: selectedQueryTypes.size > 0 ? Array.from(selectedQueryTypes)[0] : undefined,
      status: selectedStatuses.size > 0 ? Array.from(selectedStatuses)[0] : undefined,
      minDuration: minDuration ? parseInt(minDuration, 10) : undefined,
      maxDuration: maxDuration ? parseInt(maxDuration, 10) : undefined,
      searchText: searchText || undefined,
    };
    setFilters(newFilters);
    setPage(0); // Reset to first page
  }, [
    filters.connectionId,
    filters.database,
    selectedQueryTypes,
    selectedStatuses,
    minDuration,
    maxDuration,
    searchText,
  ]);

  // Handle reset filters
  const handleResetFilters = useCallback(() => {
    setFilters({ connectionId });
    setSelectedQueryTypes(new Set());
    setSelectedStatuses(new Set());
    setMinDuration('');
    setMaxDuration('');
    setSearchText('');
    setPage(0);
  }, [connectionId]);

  // Handle query type toggle
  const handleQueryTypeToggle = useCallback((type: QueryType) => {
    setSelectedQueryTypes((prev) => {
      const next = new Set(prev);
      if (next.has(type)) {
        next.delete(type);
      } else {
        next.add(type);
      }
      return next;
    });
  }, []);

  // Handle status toggle
  const handleStatusToggle = useCallback((status: QueryStatus) => {
    setSelectedStatuses((prev) => {
      const next = new Set(prev);
      if (next.has(status)) {
        next.delete(status);
      } else {
        next.add(status);
      }
      return next;
    });
  }, []);

  // Calculate total pages
  const totalPages = useMemo(() => {
    return Math.ceil(total / pageSize);
  }, [total, pageSize]);

  // Query types for filter
  const queryTypes: QueryType[] = [
    'SELECT',
    'INSERT',
    'UPDATE',
    'DELETE',
    'CREATE',
    'ALTER',
    'DROP',
    'TRANSACTION',
    'OTHER',
  ];

  // Query statuses for filter
  const queryStatuses: QueryStatus[] = ['running', 'completed', 'failed', 'cancelled'];

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="border-b bg-card px-6 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <h2 className="text-xl font-semibold">Activity Monitor</h2>
            <Badge variant="outline" className="text-xs">
              {total} total logs
            </Badge>
          </div>

          <div className="flex items-center gap-2">
            {/* Auto-refresh toggle */}
            <div className="flex items-center gap-2 mr-2">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setAutoRefreshEnabled(!autoRefreshEnabled)}
                className="h-8 w-8"
              >
                {autoRefreshEnabled ? (
                  <Pause className="h-4 w-4" />
                ) : (
                  <Play className="h-4 w-4" />
                )}
              </Button>

              <Select
                value={refreshInterval.toString()}
                onValueChange={(value) =>
                  setRefreshInterval(parseInt(value, 10) as RefreshInterval)
                }
                disabled={!autoRefreshEnabled}
              >
                <SelectTrigger className="h-8 w-[140px] text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {REFRESH_INTERVALS.map((interval) => (
                    <SelectItem
                      key={interval.value}
                      value={interval.value.toString()}
                    >
                      {interval.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <Separator orientation="vertical" className="h-6" />

            {/* Refresh button */}
            <Button
              variant="outline"
              size="sm"
              onClick={handleRefresh}
              disabled={loading}
              className="gap-2"
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
              Refresh
            </Button>

            {/* Export button */}
            <Button
              variant="outline"
              size="sm"
              onClick={handleExportLogs}
              disabled={logs.length === 0}
              className="gap-2"
            >
              <Download className="h-4 w-4" />
              Export
            </Button>

            {/* Clear logs button */}
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="destructive"
                  size="sm"
                  disabled={logs.length === 0}
                  className="gap-2"
                >
                  <Trash2 className="h-4 w-4" />
                  Clear Logs
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Clear All Logs?</AlertDialogTitle>
                  <AlertDialogDescription>
                    This will permanently delete all query logs. This action cannot be
                    undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={handleClearLogs}>
                    Clear Logs
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </div>
      </div>

      {/* Main Content - Resizable Panels */}
      <div className="flex-1 overflow-hidden">
        <ResizablePanelGroup direction="horizontal">
          {/* Left Panel - Query Log Table */}
          <ResizablePanel defaultSize={60} minSize={40}>
            <div className="h-full flex flex-col">
              {/* Query Log Table Section */}
              <div className="flex-1 overflow-auto p-6">
                <Card className="h-full">
                  <CardHeader>
                    <CardTitle className="text-base">Query Logs</CardTitle>
                  </CardHeader>
                  <CardContent>
                    {loading ? (
                      <div className="space-y-2">
                        {Array.from({ length: 10 }).map((_, i) => (
                          <Skeleton key={i} className="h-12 w-full" />
                        ))}
                      </div>
                    ) : logs.length === 0 ? (
                      <div className="flex flex-col items-center justify-center py-12 text-center">
                        <p className="text-muted-foreground">No query logs found</p>
                        <p className="text-sm text-muted-foreground/70 mt-2">
                          Execute queries to see them appear here
                        </p>
                      </div>
                    ) : (
                      <div className="space-y-4">
                        {/* Placeholder for QueryLogTable component */}
                        <div className="text-sm text-muted-foreground">
                          QueryLogTable component will be rendered here
                          <div className="mt-4 space-y-2">
                            {logs.map((log) => (
                              <div
                                key={log.id}
                                className="p-3 border rounded-md text-xs"
                              >
                                <div className="flex items-center justify-between mb-2">
                                  <Badge variant="outline">{log.queryType}</Badge>
                                  <Badge
                                    variant={
                                      log.status === 'completed'
                                        ? 'default'
                                        : log.status === 'failed'
                                          ? 'destructive'
                                          : 'secondary'
                                    }
                                  >
                                    {log.status}
                                  </Badge>
                                </div>
                                <div className="font-mono text-muted-foreground truncate">
                                  {log.sql}
                                </div>
                                <div className="flex items-center gap-4 mt-2 text-muted-foreground">
                                  <span>{log.connectionName}</span>
                                  {log.database && <span>{log.database}</span>}
                                  {log.durationMs && (
                                    <span>{log.durationMs}ms</span>
                                  )}
                                  {log.rowCount !== undefined && (
                                    <span>{log.rowCount} rows</span>
                                  )}
                                </div>
                              </div>
                            ))}
                          </div>
                        </div>

                        {/* Pagination Controls */}
                        <div className="flex items-center justify-between pt-4 border-t">
                          <div className="text-sm text-muted-foreground">
                            Page {page + 1} of {totalPages} ({total} total)
                          </div>
                          <div className="flex items-center gap-2">
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => setPage(Math.max(0, page - 1))}
                              disabled={page === 0}
                            >
                              Previous
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() =>
                                setPage(Math.min(totalPages - 1, page + 1))
                              }
                              disabled={page >= totalPages - 1}
                            >
                              Next
                            </Button>
                          </div>
                        </div>
                      </div>
                    )}
                  </CardContent>
                </Card>
              </div>
            </div>
          </ResizablePanel>

          <ResizableHandle withHandle />

          {/* Right Panel - Statistics and Filters */}
          <ResizablePanel defaultSize={40} minSize={30}>
            <div className="h-full overflow-auto p-6 space-y-4">
              {/* Statistics Card */}
              <Collapsible open={statsOpen} onOpenChange={setStatsOpen}>
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {statsOpen ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">Statistics</CardTitle>
                        </div>
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      {statsLoading ? (
                        <div className="space-y-2">
                          <Skeleton className="h-16 w-full" />
                          <Skeleton className="h-16 w-full" />
                          <Skeleton className="h-16 w-full" />
                        </div>
                      ) : stats ? (
                        <div className="space-y-4">
                          {/* Overall Stats */}
                          <div className="grid grid-cols-2 gap-3">
                            <div className="border rounded-md p-3">
                              <div className="text-2xl font-bold">
                                {stats.totalQueries}
                              </div>
                              <div className="text-xs text-muted-foreground">
                                Total Queries
                              </div>
                            </div>
                            <div className="border rounded-md p-3">
                              <div className="text-2xl font-bold text-destructive">
                                {stats.failedQueries}
                              </div>
                              <div className="text-xs text-muted-foreground">
                                Failed Queries
                              </div>
                            </div>
                            <div className="border rounded-md p-3">
                              <div className="text-2xl font-bold">
                                {stats.avgDuration.toFixed(0)}ms
                              </div>
                              <div className="text-xs text-muted-foreground">
                                Avg Duration
                              </div>
                            </div>
                            <div className="border rounded-md p-3">
                              <div className="text-2xl font-bold">
                                {stats.totalRows.toLocaleString()}
                              </div>
                              <div className="text-xs text-muted-foreground">
                                Total Rows
                              </div>
                            </div>
                          </div>

                          {/* Queries by Type */}
                          <div>
                            <div className="text-sm font-medium mb-2">
                              Queries by Type
                            </div>
                            <div className="space-y-2">
                              {Object.entries(stats.queriesByType).map(
                                ([type, count]) =>
                                  count > 0 && (
                                    <div
                                      key={type}
                                      className="flex items-center justify-between text-xs"
                                    >
                                      <span className="text-muted-foreground">
                                        {type}
                                      </span>
                                      <Badge variant="secondary">{count}</Badge>
                                    </div>
                                  )
                              )}
                            </div>
                          </div>

                          {/* Queries by Status */}
                          <div>
                            <div className="text-sm font-medium mb-2">
                              Queries by Status
                            </div>
                            <div className="space-y-2">
                              {Object.entries(stats.queriesByStatus).map(
                                ([status, count]) =>
                                  count > 0 && (
                                    <div
                                      key={status}
                                      className="flex items-center justify-between text-xs"
                                    >
                                      <span className="text-muted-foreground">
                                        {status}
                                      </span>
                                      <Badge
                                        variant={
                                          status === 'completed'
                                            ? 'default'
                                            : status === 'failed'
                                              ? 'destructive'
                                              : 'secondary'
                                        }
                                      >
                                        {count}
                                      </Badge>
                                    </div>
                                  )
                              )}
                            </div>
                          </div>
                        </div>
                      ) : (
                        <div className="text-sm text-muted-foreground text-center py-4">
                          No statistics available
                        </div>
                      )}
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>

              {/* Filters Card */}
              <Collapsible open={filtersOpen} onOpenChange={setFiltersOpen}>
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {filtersOpen ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">Filters</CardTitle>
                        </div>
                        <Filter className="h-4 w-4 text-muted-foreground" />
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent className="space-y-4">
                      {/* Connection Filter */}
                      <div className="space-y-2">
                        <Label className="text-xs">Connection</Label>
                        <Select
                          value={filters.connectionId || '__all__'}
                          onValueChange={(value) =>
                            setFilters((prev) => ({
                              ...prev,
                              connectionId: value === '__all__' ? undefined : value,
                            }))
                          }
                        >
                          <SelectTrigger className="h-8 text-xs">
                            <SelectValue placeholder="All connections" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="__all__">All connections</SelectItem>
                            {availableConnections.map((conn) => (
                              <SelectItem key={conn.id} value={conn.id}>
                                {conn.name}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>

                      {/* Database Filter */}
                      <div className="space-y-2">
                        <Label className="text-xs">Database</Label>
                        <Select
                          value={filters.database || '__all__'}
                          onValueChange={(value) =>
                            setFilters((prev) => ({
                              ...prev,
                              database: value === '__all__' ? undefined : value,
                            }))
                          }
                          disabled={!filters.connectionId}
                        >
                          <SelectTrigger className="h-8 text-xs">
                            <SelectValue placeholder="All databases" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="__all__">All databases</SelectItem>
                            {availableDatabases.filter(Boolean).map((db) => (
                              <SelectItem key={db} value={db}>
                                {db}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>

                      {/* Query Type Filter */}
                      <div className="space-y-2">
                        <Label className="text-xs">Query Type</Label>
                        <div className="grid grid-cols-2 gap-2">
                          {queryTypes.map((type) => (
                            <div
                              key={type}
                              className="flex items-center space-x-2"
                            >
                              <Checkbox
                                id={`type-${type}`}
                                checked={selectedQueryTypes.has(type)}
                                onCheckedChange={() => handleQueryTypeToggle(type)}
                              />
                              <label
                                htmlFor={`type-${type}`}
                                className="text-xs cursor-pointer"
                              >
                                {type}
                              </label>
                            </div>
                          ))}
                        </div>
                      </div>

                      {/* Status Filter */}
                      <div className="space-y-2">
                        <Label className="text-xs">Status</Label>
                        <div className="grid grid-cols-2 gap-2">
                          {queryStatuses.map((status) => (
                            <div
                              key={status}
                              className="flex items-center space-x-2"
                            >
                              <Checkbox
                                id={`status-${status}`}
                                checked={selectedStatuses.has(status)}
                                onCheckedChange={() => handleStatusToggle(status)}
                              />
                              <label
                                htmlFor={`status-${status}`}
                                className="text-xs cursor-pointer capitalize"
                              >
                                {status}
                              </label>
                            </div>
                          ))}
                        </div>
                      </div>

                      {/* Duration Range */}
                      <div className="space-y-2">
                        <Label className="text-xs">Duration (ms)</Label>
                        <div className="grid grid-cols-2 gap-2">
                          <Input
                            type="number"
                            placeholder="Min"
                            value={minDuration}
                            onChange={(e) => setMinDuration(e.target.value)}
                            className="h-8 text-xs"
                          />
                          <Input
                            type="number"
                            placeholder="Max"
                            value={maxDuration}
                            onChange={(e) => setMaxDuration(e.target.value)}
                            className="h-8 text-xs"
                          />
                        </div>
                      </div>

                      {/* Search Text */}
                      <div className="space-y-2">
                        <Label className="text-xs">Search in SQL</Label>
                        <Input
                          type="text"
                          placeholder="Enter search term..."
                          value={searchText}
                          onChange={(e) => setSearchText(e.target.value)}
                          className="h-8 text-xs"
                        />
                      </div>

                      {/* Filter Actions */}
                      <div className="flex items-center gap-2 pt-2">
                        <Button
                          size="sm"
                          onClick={handleApplyFilters}
                          className="flex-1"
                        >
                          Apply Filters
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={handleResetFilters}
                          className="flex-1"
                        >
                          Reset
                        </Button>
                      </div>
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
    </div>
  );
};
