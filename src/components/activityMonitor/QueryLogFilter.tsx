import { FC, useState, ChangeEvent } from "react";
import { Filter, X, ChevronDown, ChevronRight, Check, ChevronsUpDown } from "lucide-react";
import { QueryLogFilter as QueryLogFilterType, QueryType, QueryStatus } from "@/types/activity";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

interface QueryLogFilterProps {
  filter: QueryLogFilterType;
  onFilterChange: (filter: QueryLogFilterType) => void;
  availableConnections: Array<{ id: string; name: string }>;
  availableDatabases: string[];
}

export const QueryLogFilter: FC<QueryLogFilterProps> = ({
  filter,
  onFilterChange,
  availableConnections,
  availableDatabases,
}) => {
  // Local state for filter form (only applied on "Apply" click)
  const [localFilter, setLocalFilter] = useState<QueryLogFilterType>(filter);

  // UI state
  const [queryTypesOpen, setQueryTypesOpen] = useState(false);
  const [statusesOpen, setStatusesOpen] = useState(false);
  const [connectionOpen, setConnectionOpen] = useState(false);
  const [databaseOpen, setDatabaseOpen] = useState(false);

  // Available options
  const queryTypes: QueryType[] = [
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "ALTER",
    "DROP",
    "TRANSACTION",
    "OTHER",
  ];

  const statuses: QueryStatus[] = ["completed", "failed", "running", "cancelled"];

  // Count active filters
  const getActiveFilterCount = (): number => {
    let count = 0;
    if (filter.connectionId) count++;
    if (filter.database) count++;
    if (filter.queryType) count++;
    if (filter.status) count++;
    if (filter.minDuration !== undefined) count++;
    if (filter.maxDuration !== undefined) count++;
    if (filter.startDate) count++;
    if (filter.endDate) count++;
    if (filter.searchText) count++;
    if (filter.tags && filter.tags.length > 0) count++;
    return count;
  };

  // Handle field changes
  const handleChange = (field: keyof QueryLogFilterType, value: any) => {
    setLocalFilter((prev) => ({
      ...prev,
      [field]: value,
    }));
  };

  // Handle number input changes
  const handleNumberChange = (
    field: "minDuration" | "maxDuration",
    e: ChangeEvent<HTMLInputElement>
  ) => {
    const value = e.target.value;
    setLocalFilter((prev) => ({
      ...prev,
      [field]: value === "" ? undefined : parseInt(value, 10),
    }));
  };

  // Apply filters
  const handleApply = () => {
    onFilterChange(localFilter);
  };

  // Reset filters
  const handleReset = () => {
    const emptyFilter: QueryLogFilterType = {};
    setLocalFilter(emptyFilter);
    onFilterChange(emptyFilter);
  };

  // Get connection name by ID
  const getConnectionName = (id: string): string => {
    return availableConnections.find((c) => c.id === id)?.name || id;
  };

  const activeFilterCount = getActiveFilterCount();

  return (
    <div className="w-full border-b bg-muted/30 p-4 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Filter className="h-4 w-4 text-muted-foreground" />
          <h3 className="font-semibold text-sm">Filters</h3>
          {activeFilterCount > 0 && (
            <Badge variant="secondary" className="h-5">
              {activeFilterCount} active
            </Badge>
          )}
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={handleReset}
            disabled={activeFilterCount === 0}
          >
            <X className="h-4 w-4 mr-1" />
            Reset
          </Button>
          <Button size="sm" onClick={handleApply}>
            Apply Filters
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {/* Connection Filter */}
        <div className="space-y-2">
          <Label htmlFor="connection-filter">Connection</Label>
          <Popover open={connectionOpen} onOpenChange={setConnectionOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="outline"
                role="combobox"
                aria-expanded={connectionOpen}
                className="w-full justify-between"
              >
                {localFilter.connectionId ? (
                  <span className="truncate">
                    {getConnectionName(localFilter.connectionId)}
                  </span>
                ) : (
                  <span className="text-muted-foreground">All connections</span>
                )}
                <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[250px] p-0">
              <div className="p-2 space-y-1">
                <Button
                  variant="ghost"
                  className="w-full justify-start text-sm"
                  onClick={() => {
                    handleChange("connectionId", undefined);
                    setConnectionOpen(false);
                  }}
                >
                  <Check
                    className={`mr-2 h-4 w-4 ${
                      !localFilter.connectionId ? "opacity-100" : "opacity-0"
                    }`}
                  />
                  All connections
                </Button>
                {availableConnections.map((conn) => (
                  <Button
                    key={conn.id}
                    variant="ghost"
                    className="w-full justify-start text-sm"
                    onClick={() => {
                      handleChange("connectionId", conn.id);
                      setConnectionOpen(false);
                    }}
                  >
                    <Check
                      className={`mr-2 h-4 w-4 ${
                        localFilter.connectionId === conn.id
                          ? "opacity-100"
                          : "opacity-0"
                      }`}
                    />
                    {conn.name}
                  </Button>
                ))}
              </div>
            </PopoverContent>
          </Popover>
        </div>

        {/* Database Filter */}
        <div className="space-y-2">
          <Label htmlFor="database-filter">Database</Label>
          <Popover open={databaseOpen} onOpenChange={setDatabaseOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="outline"
                role="combobox"
                aria-expanded={databaseOpen}
                className="w-full justify-between"
              >
                {localFilter.database ? (
                  <span className="truncate">{localFilter.database}</span>
                ) : (
                  <span className="text-muted-foreground">All databases</span>
                )}
                <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[250px] p-0">
              <div className="p-2 space-y-1">
                <Button
                  variant="ghost"
                  className="w-full justify-start text-sm"
                  onClick={() => {
                    handleChange("database", undefined);
                    setDatabaseOpen(false);
                  }}
                >
                  <Check
                    className={`mr-2 h-4 w-4 ${
                      !localFilter.database ? "opacity-100" : "opacity-0"
                    }`}
                  />
                  All databases
                </Button>
                {availableDatabases.map((db) => (
                  <Button
                    key={db}
                    variant="ghost"
                    className="w-full justify-start text-sm"
                    onClick={() => {
                      handleChange("database", db);
                      setDatabaseOpen(false);
                    }}
                  >
                    <Check
                      className={`mr-2 h-4 w-4 ${
                        localFilter.database === db ? "opacity-100" : "opacity-0"
                      }`}
                    />
                    {db}
                  </Button>
                ))}
              </div>
            </PopoverContent>
          </Popover>
        </div>

        {/* Search Text */}
        <div className="space-y-2">
          <Label htmlFor="search-text">Search Query Text</Label>
          <Input
            id="search-text"
            type="text"
            placeholder="Search in SQL..."
            value={localFilter.searchText || ""}
            onChange={(e) => handleChange("searchText", e.target.value || undefined)}
          />
        </div>
      </div>

      {/* Query Type Filter - Collapsible */}
      <Collapsible open={queryTypesOpen} onOpenChange={setQueryTypesOpen}>
        <CollapsibleTrigger className="flex items-center gap-2 text-sm font-medium hover:underline">
          {queryTypesOpen ? (
            <ChevronDown className="h-4 w-4" />
          ) : (
            <ChevronRight className="h-4 w-4" />
          )}
          Query Type
          {localFilter.queryType && (
            <Badge variant="secondary" className="h-5">
              {localFilter.queryType}
            </Badge>
          )}
        </CollapsibleTrigger>
        <CollapsibleContent className="pt-3">
          <div className="grid grid-cols-3 md:grid-cols-5 gap-3">
            {queryTypes.map((type) => (
              <label
                key={type}
                className="flex items-center gap-2 cursor-pointer hover:bg-muted/50 p-2 rounded"
              >
                <Checkbox
                  checked={localFilter.queryType === type}
                  onCheckedChange={(checked) => {
                    handleChange("queryType", checked ? type : undefined);
                  }}
                />
                <span className="text-sm">{type}</span>
              </label>
            ))}
          </div>
        </CollapsibleContent>
      </Collapsible>

      {/* Status Filter - Collapsible */}
      <Collapsible open={statusesOpen} onOpenChange={setStatusesOpen}>
        <CollapsibleTrigger className="flex items-center gap-2 text-sm font-medium hover:underline">
          {statusesOpen ? (
            <ChevronDown className="h-4 w-4" />
          ) : (
            <ChevronRight className="h-4 w-4" />
          )}
          Status
          {localFilter.status && (
            <Badge variant="secondary" className="h-5">
              {localFilter.status}
            </Badge>
          )}
        </CollapsibleTrigger>
        <CollapsibleContent className="pt-3">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            {statuses.map((status) => (
              <label
                key={status}
                className="flex items-center gap-2 cursor-pointer hover:bg-muted/50 p-2 rounded"
              >
                <Checkbox
                  checked={localFilter.status === status}
                  onCheckedChange={(checked) => {
                    handleChange("status", checked ? status : undefined);
                  }}
                />
                <span className="text-sm capitalize">{status}</span>
              </label>
            ))}
          </div>
        </CollapsibleContent>
      </Collapsible>

      {/* Duration Range */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="min-duration">Min Duration (ms)</Label>
          <Input
            id="min-duration"
            type="number"
            placeholder="0"
            min="0"
            value={localFilter.minDuration ?? ""}
            onChange={(e) => handleNumberChange("minDuration", e)}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="max-duration">Max Duration (ms)</Label>
          <Input
            id="max-duration"
            type="number"
            placeholder="No limit"
            min="0"
            value={localFilter.maxDuration ?? ""}
            onChange={(e) => handleNumberChange("maxDuration", e)}
          />
        </div>
      </div>

      {/* Date Range */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="start-date">Start Date</Label>
          <Input
            id="start-date"
            type="datetime-local"
            value={
              localFilter.startDate
                ? new Date(localFilter.startDate)
                    .toISOString()
                    .slice(0, 16)
                : ""
            }
            onChange={(e) =>
              handleChange(
                "startDate",
                e.target.value ? new Date(e.target.value).toISOString() : undefined
              )
            }
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="end-date">End Date</Label>
          <Input
            id="end-date"
            type="datetime-local"
            value={
              localFilter.endDate
                ? new Date(localFilter.endDate)
                    .toISOString()
                    .slice(0, 16)
                : ""
            }
            onChange={(e) =>
              handleChange(
                "endDate",
                e.target.value ? new Date(e.target.value).toISOString() : undefined
              )
            }
          />
        </div>
      </div>

      {/* Tags - Placeholder for future implementation */}
      <div className="space-y-2">
        <Label htmlFor="tags">Tags</Label>
        <Input
          id="tags"
          type="text"
          placeholder="Coming soon..."
          disabled
          className="opacity-50"
        />
        <p className="text-xs text-muted-foreground">
          Tag filtering will be available in a future update
        </p>
      </div>
    </div>
  );
};
