import { useState } from "react";
import { Database, Star, MoreVertical, Pencil, Trash2, PlayCircle } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Badge } from "@/components/ui/badge";
import type { ConnectionProfile } from "@/types";

interface ConnectionCardProps {
  profile: ConnectionProfile;
  isActive?: boolean;
  onConnect: (profile: ConnectionProfile) => void;
  onEdit: (profile: ConnectionProfile) => void;
  onDelete: (profileId: string) => void;
  onToggleFavorite: (profileId: string) => void;
}

export function ConnectionCard({
  profile,
  isActive = false,
  onConnect,
  onEdit,
  onDelete,
  onToggleFavorite,
}: ConnectionCardProps) {
  const [isHovered, setIsHovered] = useState(false);

  const getDriverIcon = () => {
    // You could add driver-specific icons here
    return <Database className="size-8" />;
  };

  const getDriverColor = (driver: string) => {
    const colors: Record<string, string> = {
      postgres: "bg-blue-500",
      mysql: "bg-orange-500",
      sqlite: "bg-green-500",
      mongodb: "bg-emerald-500",
      sqlserver: "bg-red-500",
    };
    return colors[driver.toLowerCase()] || "bg-gray-500";
  };

  const getEnvironmentColor = (env?: string) => {
    const colors: Record<string, string> = {
      local: "bg-green-600",
      staging: "bg-yellow-600",
      production: "bg-red-600",
    };
    return colors[env?.toLowerCase() || ""] || "bg-gray-600";
  };

  return (
    <Card
      className={`relative transition-all duration-200 cursor-pointer hover:shadow-lg hover:scale-105 ${
        isActive ? "ring-2 ring-primary" : ""
      }`}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onClick={() => onConnect(profile)}
    >
      {/* Favorite Star */}
      <button
        onClick={(e) => {
          e.stopPropagation();
          onToggleFavorite(profile.id);
        }}
        className={`absolute top-3 right-3 z-10 p-1 rounded-full transition-all ${
          profile.isFavorite
            ? "text-yellow-500 hover:text-yellow-600"
            : "text-muted-foreground hover:text-foreground"
        }`}
        title={profile.isFavorite ? "Remove from favorites" : "Add to favorites"}
      >
        <Star className={`size-4 ${profile.isFavorite ? "fill-current" : ""}`} />
      </button>

      <CardHeader className="pb-3">
        <div className="flex items-start gap-3">
          {/* Driver Icon */}
          <div
            className={`flex-shrink-0 p-3 rounded-lg ${getDriverColor(
              profile.driver
            )} text-white`}
          >
            {getDriverIcon()}
          </div>

          <div className="flex-1 min-w-0">
            <CardTitle className="text-lg truncate pr-6">{profile.name}</CardTitle>
            <CardDescription className="text-sm truncate">
              {profile.host}:{profile.port}
            </CardDescription>
          </div>
        </div>
      </CardHeader>

      <CardContent className="space-y-3">
        {/* Driver & Environment Badges */}
        <div className="flex items-center gap-2 flex-wrap">
          <Badge variant="outline" className="text-xs font-medium">
            {profile.driver.toUpperCase()}
          </Badge>
          {profile.environment && (
            <Badge
              className={`text-xs font-medium text-white ${getEnvironmentColor(
                profile.environment
              )}`}
            >
              {profile.environment}
            </Badge>
          )}
        </div>

        {/* Folder */}
        {profile.folder && (
          <div className="text-xs text-muted-foreground truncate">
            📁 {profile.folder}
          </div>
        )}

        {/* Actions */}
        <div
          className={`flex items-center gap-2 transition-opacity ${
            isHovered ? "opacity-100" : "opacity-0"
          }`}
        >
          <Button
            size="sm"
            variant="default"
            className="flex-1"
            onClick={(e) => {
              e.stopPropagation();
              onConnect(profile);
            }}
          >
            <PlayCircle className="size-4 mr-1" />
            Connect
          </Button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
              <Button size="sm" variant="outline">
                <MoreVertical className="size-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  onEdit(profile);
                }}
              >
                <Pencil className="size-4 mr-2" />
                Edit
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleFavorite(profile.id);
                }}
              >
                <Star className="size-4 mr-2" />
                {profile.isFavorite ? "Remove from favorites" : "Add to favorites"}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete(profile.id);
                }}
                className="text-destructive focus:text-destructive"
              >
                <Trash2 className="size-4 mr-2" />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardContent>
    </Card>
  );
}
