import { FC, useState, useMemo } from "react";
import {
  ConnectionProfile,
  getDriverDisplayName,
  DbDriver,
} from "../types/database";
import { Button } from "@/components/ui/button";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Server,
  Star,
  Pencil,
  Trash,
  MoreHorizontal,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface ConnectionTreeViewProps {
  profiles: ConnectionProfile[];
  connectingId: string | null;
  onConnect: (profile: ConnectionProfile) => void;
  onEdit: (profile: ConnectionProfile) => void;
  onDelete: (profileId: string) => void;
  onToggleFavorite: (profileId: string) => void;
}

interface FolderGroup {
  name: string;
  connections: ConnectionProfile[];
  isExpanded: boolean;
}

// Helper to get badge color
const getDriverColor = (driver: DbDriver) => {
  const driverName = getDriverDisplayName(driver).toLowerCase();
  if (driverName.includes("postgresql")) {
    return "bg-blue-500/20 border-blue-500/40 text-blue-600 dark:text-blue-300";
  } else if (driverName.includes("mysql")) {
    return "bg-orange-500/20 border-orange-500/40 text-orange-600 dark:text-orange-300";
  } else if (driverName.includes("sqlite")) {
    return "bg-green-500/20 border-green-500/40 text-green-600 dark:text-green-300";
  } else if (driverName.includes("mongodb")) {
    return "bg-emerald-500/20 border-emerald-500/40 text-emerald-600 dark:text-emerald-300";
  } else if (driverName.includes("sql server")) {
    return "bg-red-500/20 border-red-500/40 text-red-600 dark:text-red-300";
  }
  return "bg-amber-300/10 border-amber-300/30 text-amber-700 dark:text-amber-200";
};

export const ConnectionTreeView: FC<ConnectionTreeViewProps> = ({
  profiles,
  connectingId,
  onConnect,
  onEdit,
  onDelete,
  onToggleFavorite,
}) => {
  // Track expanded folders
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(
    new Set(["__unfiled__"]) // Start with unfiled expanded
  );

  // Group connections by folder
  const folderGroups = useMemo(() => {
    const groups: Map<string, ConnectionProfile[]> = new Map();

    // Initialize unfiled group
    groups.set("__unfiled__", []);

    profiles.forEach((profile) => {
      const folderName = profile.folder || "__unfiled__";
      if (!groups.has(folderName)) {
        groups.set(folderName, []);
      }
      groups.get(folderName)!.push(profile);
    });

    // Sort folders alphabetically, but keep unfiled at the end
    const sortedGroups: FolderGroup[] = [];
    const sortedKeys = Array.from(groups.keys()).sort((a, b) => {
      if (a === "__unfiled__") return 1;
      if (b === "__unfiled__") return -1;
      return a.localeCompare(b);
    });

    sortedKeys.forEach((key) => {
      const connections = groups.get(key)!;
      if (connections.length > 0) {
        sortedGroups.push({
          name: key,
          connections: connections.sort((a, b) => a.name.localeCompare(b.name)),
          isExpanded: expandedFolders.has(key),
        });
      }
    });

    return sortedGroups;
  }, [profiles, expandedFolders]);

  const toggleFolder = (folderName: string) => {
    setExpandedFolders((prev) => {
      const newSet = new Set(prev);
      if (newSet.has(folderName)) {
        newSet.delete(folderName);
      } else {
        newSet.add(folderName);
      }
      return newSet;
    });
  };

  const renderConnectionItem = (profile: ConnectionProfile, indented = false) => {
    const colorClass = getDriverColor(profile.driver);
    const driverName = getDriverDisplayName(profile.driver);
    const isConnecting = connectingId === profile.id;

    return (
      <div
        key={profile.id}
        className={`group flex items-center gap-2 py-2 px-3 rounded-lg hover:bg-accent cursor-pointer transition-colors ${
          indented ? "ml-6" : ""
        }`}
        onDoubleClick={() => onConnect(profile)}
        title="Double-click to connect"
      >
        {/* Server icon */}
        <div className="h-8 w-8 rounded-md border border-primary/30 bg-primary/10 flex items-center justify-center flex-shrink-0">
          <Server className="h-4 w-4 text-primary" strokeWidth={1.5} />
        </div>

        {/* Connection info */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            {profile.isFavorite && (
              <Star className="h-3.5 w-3.5 text-yellow-500 fill-current flex-shrink-0" />
            )}
            <span className="font-medium text-sm text-foreground truncate">
              {profile.name}
            </span>
            <span
              className={`text-[0.65rem] px-1.5 py-0.5 rounded-full border ${colorClass} font-medium flex-shrink-0`}
            >
              {driverName}
            </span>
            {isConnecting && (
              <span className="text-xs text-primary animate-pulse flex-shrink-0">
                Connecting...
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground truncate">
            {profile.host}:{profile.port}
            {profile.database && ` / ${profile.database}`}
          </p>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={(e) => {
              e.stopPropagation();
              onConnect(profile);
            }}
          >
            <Server className="h-3.5 w-3.5" strokeWidth={1.5} />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={(e) => e.stopPropagation()}
              >
                <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => onToggleFavorite(profile.id)}>
                <Star
                  className={`h-4 w-4 mr-2 ${profile.isFavorite ? "text-yellow-500 fill-current" : ""}`}
                />
                {profile.isFavorite ? "Remove from Favorites" : "Add to Favorites"}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => onEdit(profile)}>
                <Pencil className="h-4 w-4 mr-2" />
                Edit Connection
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={() => onDelete(profile.id)}
                className="text-destructive focus:text-destructive"
              >
                <Trash className="h-4 w-4 mr-2" />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
    );
  };

  const renderFolderGroup = (group: FolderGroup) => {
    const isUnfiled = group.name === "__unfiled__";
    const displayName = isUnfiled ? "Unfiled Connections" : group.name;
    const isExpanded = expandedFolders.has(group.name);

    return (
      <div key={group.name} className="mb-2">
        {/* Folder header */}
        <button
          className="w-full flex items-center gap-2 py-2 px-3 rounded-lg hover:bg-accent transition-colors text-left"
          onClick={() => toggleFolder(group.name)}
        >
          {/* Expand/collapse chevron */}
          {isExpanded ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground flex-shrink-0" />
          ) : (
            <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
          )}

          {/* Folder icon */}
          {isExpanded ? (
            <FolderOpen
              className={`h-5 w-5 flex-shrink-0 ${
                isUnfiled ? "text-muted-foreground" : "text-amber-500"
              }`}
              strokeWidth={1.5}
            />
          ) : (
            <Folder
              className={`h-5 w-5 flex-shrink-0 ${
                isUnfiled ? "text-muted-foreground" : "text-amber-500"
              }`}
              strokeWidth={1.5}
            />
          )}

          {/* Folder name */}
          <span
            className={`font-medium text-sm flex-1 ${
              isUnfiled ? "text-muted-foreground italic" : "text-foreground"
            }`}
          >
            {displayName}
          </span>

          {/* Connection count badge */}
          <span className="text-xs px-2 py-0.5 rounded-full bg-muted text-muted-foreground">
            {group.connections.length}
          </span>
        </button>

        {/* Folder contents */}
        {isExpanded && (
          <div className="ml-2 pl-4 border-l border-border">
            {group.connections.map((profile) =>
              renderConnectionItem(profile, true)
            )}
          </div>
        )}
      </div>
    );
  };

  // Calculate totals
  const totalFolders = folderGroups.filter((g) => g.name !== "__unfiled__").length;
  const totalConnections = profiles.length;

  return (
    <div className="space-y-1">
      {/* Summary header */}
      <div className="flex items-center justify-between px-3 py-2 text-xs text-muted-foreground mb-2">
        <span>
          {totalFolders} folder{totalFolders !== 1 ? "s" : ""}, {totalConnections} connection
          {totalConnections !== 1 ? "s" : ""}
        </span>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="h-6 text-xs"
            onClick={() => {
              // Expand all
              const allFolders = new Set(folderGroups.map((g) => g.name));
              setExpandedFolders(allFolders);
            }}
          >
            Expand All
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 text-xs"
            onClick={() => {
              // Collapse all
              setExpandedFolders(new Set());
            }}
          >
            Collapse All
          </Button>
        </div>
      </div>

      {/* Folder groups */}
      {folderGroups.map((group) => renderFolderGroup(group))}
    </div>
  );
};
