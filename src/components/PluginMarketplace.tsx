import { useState, useEffect } from "react";
import {
  Search,
  Download,
  Star,
  Package,
  Palette,
  Wrench,
  FileOutput,
  FileInput,
  Code,
  BarChart3,
  Eye,
  Puzzle,
  Check,
  Loader2,
  Filter,
  X,
} from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Badge } from "./ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui/tabs";
import { ScrollArea } from "./ui/scroll-area";
import { Separator } from "./ui/separator";
import { toast } from "sonner";
import type { MarketplacePlugin, PluginCategory, PluginSortOption, PluginFilters } from "@/types/plugins";
import { getMarketplacePlugins, installPlugin } from "@/api/plugins";

const categoryIcons: Record<PluginCategory, React.ReactNode> = {
  driver: <Package className="size-4" />,
  theme: <Palette className="size-4" />,
  tool: <Wrench className="size-4" />,
  export: <FileOutput className="size-4" />,
  import: <FileInput className="size-4" />,
  formatter: <Code className="size-4" />,
  analyzer: <BarChart3 className="size-4" />,
  visualizer: <Eye className="size-4" />,
  extension: <Puzzle className="size-4" />,
};

const categoryLabels: Record<PluginCategory, string> = {
  driver: "Database Drivers",
  theme: "Themes",
  tool: "Tools",
  export: "Export Formats",
  import: "Import Formats",
  formatter: "Formatters",
  analyzer: "Analyzers",
  visualizer: "Visualizers",
  extension: "Extensions",
};

export function PluginMarketplace() {
  const [plugins, setPlugins] = useState<MarketplacePlugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [installingPlugins, setInstallingPlugins] = useState<Set<string>>(new Set());
  const [filters, setFilters] = useState<PluginFilters>({
    sort: "popular",
    verified: undefined,
  });
  const [searchQuery, setSearchQuery] = useState("");

  useEffect(() => {
    loadPlugins();
  }, [filters.category]);

  const loadPlugins = async () => {
    try {
      setLoading(true);
      const marketplacePlugins = await getMarketplacePlugins(filters.category, searchQuery);

      // Apply client-side sorting
      const sorted = sortPlugins(marketplacePlugins, filters.sort);

      // Apply verified filter if set
      const filtered = filters.verified !== undefined
        ? sorted.filter(p => p.verified === filters.verified)
        : sorted;

      setPlugins(filtered);
    } catch (error) {
      console.error("Failed to load marketplace plugins:", error);
      toast.error("Failed to load plugins");
    } finally {
      setLoading(false);
    }
  };

  const sortPlugins = (plugins: MarketplacePlugin[], sort: PluginSortOption): MarketplacePlugin[] => {
    const sorted = [...plugins];
    switch (sort) {
      case "popular":
        return sorted.sort((a, b) => b.stats.stars - a.stats.stars);
      case "recent":
        return sorted.sort((a, b) =>
          new Date(b.stats.updatedAt).getTime() - new Date(a.stats.updatedAt).getTime()
        );
      case "rating":
        return sorted.sort((a, b) => b.stats.rating - a.stats.rating);
      case "downloads":
        return sorted.sort((a, b) => b.stats.downloads - a.stats.downloads);
      case "name":
        return sorted.sort((a, b) => a.manifest.name.localeCompare(b.manifest.name));
      default:
        return sorted;
    }
  };

  const handleInstall = async (plugin: MarketplacePlugin) => {
    try {
      setInstallingPlugins(prev => new Set(prev).add(plugin.manifest.id));
      await installPlugin(plugin);
      toast.success(`${plugin.manifest.name} installed successfully`);
    } catch (error: any) {
      console.error("Failed to install plugin:", error);
      toast.error(`Failed to install plugin: ${error.message || error}`);
    } finally {
      setInstallingPlugins(prev => {
        const next = new Set(prev);
        next.delete(plugin.manifest.id);
        return next;
      });
    }
  };

  const handleSearch = () => {
    loadPlugins();
  };

  const formatDownloads = (downloads: number): string => {
    if (downloads >= 1000000) {
      return `${(downloads / 1000000).toFixed(1)}M`;
    } else if (downloads >= 1000) {
      return `${(downloads / 1000).toFixed(1)}K`;
    }
    return downloads.toString();
  };

  const formatSize = (bytes: number): string => {
    if (bytes >= 1024 * 1024) {
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    } else if (bytes >= 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${bytes} B`;
  };

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="border-b px-6 py-4">
        <h1 className="text-2xl font-semibold">Plugin Marketplace</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Discover and install plugins to extend DB-Hive
        </p>
      </div>

      {/* Filters Bar */}
      <div className="border-b px-6 py-3">
        <div className="flex items-center gap-4">
          {/* Search */}
          <div className="flex-1 flex items-center gap-2">
            <Input
              type="text"
              placeholder="Search plugins..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyPress={(e) => e.key === "Enter" && handleSearch()}
              className="max-w-md"
            />
            <Button onClick={handleSearch} size="sm">
              <Search className="size-4" />
            </Button>
          </div>

          {/* Sort */}
          <Select
            value={filters.sort}
            onValueChange={(value) =>
              setFilters(prev => ({ ...prev, sort: value as PluginSortOption }))
            }
          >
            <SelectTrigger className="w-[140px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="popular">Popular</SelectItem>
              <SelectItem value="recent">Recent</SelectItem>
              <SelectItem value="rating">Top Rated</SelectItem>
              <SelectItem value="downloads">Most Downloaded</SelectItem>
              <SelectItem value="name">Name</SelectItem>
            </SelectContent>
          </Select>

          {/* Verified Filter */}
          <div className="flex items-center gap-2">
            <Button
              variant={filters.verified === true ? "default" : "outline"}
              size="sm"
              onClick={() =>
                setFilters(prev => ({
                  ...prev,
                  verified: prev.verified === true ? undefined : true
                }))
              }
            >
              <Check className="size-4 mr-1" />
              Verified Only
            </Button>
          </div>

          {/* Clear Filters */}
          {(searchQuery || filters.category || filters.verified !== undefined) && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setSearchQuery("");
                setFilters({ sort: "popular", verified: undefined });
                loadPlugins();
              }}
            >
              <X className="size-4 mr-1" />
              Clear
            </Button>
          )}
        </div>
      </div>

      {/* Categories Tabs */}
      <Tabs
        value={filters.category || "all"}
        onValueChange={(value) =>
          setFilters(prev => ({ ...prev, category: value === "all" ? undefined : value as PluginCategory }))
        }
        className="flex-1 flex flex-col"
      >
        <TabsList className="px-6 py-2 justify-start w-full h-auto flex-wrap">
          <TabsTrigger value="all" className="gap-2">
            All Plugins
          </TabsTrigger>
          {Object.entries(categoryLabels).map(([key, label]) => (
            <TabsTrigger key={key} value={key} className="gap-2">
              {categoryIcons[key as PluginCategory]}
              {label}
            </TabsTrigger>
          ))}
        </TabsList>

        <TabsContent value={filters.category || "all"} className="flex-1 px-6 py-4">
          <ScrollArea className="h-full">
            {loading ? (
              <div className="flex items-center justify-center h-64">
                <Loader2 className="size-8 animate-spin text-muted-foreground" />
              </div>
            ) : plugins.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 text-muted-foreground">
                <Package className="size-12 mb-4" />
                <p className="text-lg font-medium">No plugins found</p>
                <p className="text-sm">Try adjusting your filters or search query</p>
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {plugins.map((plugin) => (
                  <Card key={plugin.manifest.id} className="flex flex-col">
                    <CardHeader className="pb-3">
                      <div className="flex items-start justify-between">
                        <div className="flex items-center gap-3">
                          <div className="p-2 rounded-lg bg-primary/10">
                            {categoryIcons[plugin.manifest.category]}
                          </div>
                          <div>
                            <CardTitle className="text-base flex items-center gap-2">
                              {plugin.manifest.name}
                              {plugin.verified && (
                                <Badge variant="secondary" className="text-xs">
                                  <Check className="size-3 mr-1" />
                                  Verified
                                </Badge>
                              )}
                            </CardTitle>
                            <div className="flex items-center gap-2 mt-1">
                              <Badge variant="outline" className="text-xs">
                                v{plugin.manifest.version}
                              </Badge>
                              <span className="text-xs text-muted-foreground">
                                by {plugin.manifest.author.name}
                              </span>
                            </div>
                          </div>
                        </div>
                      </div>
                    </CardHeader>
                    <CardContent className="flex-1 flex flex-col gap-3">
                      <CardDescription className="text-sm line-clamp-2">
                        {plugin.manifest.description}
                      </CardDescription>

                      {/* Stats */}
                      <div className="flex items-center gap-4 text-sm text-muted-foreground">
                        <div className="flex items-center gap-1">
                          <Download className="size-3" />
                          {formatDownloads(plugin.stats.downloads)}
                        </div>
                        <div className="flex items-center gap-1">
                          <Star className="size-3 fill-amber-500 text-amber-500" />
                          {plugin.stats.rating.toFixed(1)}
                        </div>
                        <div className="text-xs">
                          {formatSize(plugin.size)}
                        </div>
                      </div>

                      {/* Keywords */}
                      {plugin.manifest.keywords.length > 0 && (
                        <div className="flex flex-wrap gap-1">
                          {plugin.manifest.keywords.slice(0, 3).map((keyword) => (
                            <Badge key={keyword} variant="secondary" className="text-xs">
                              {keyword}
                            </Badge>
                          ))}
                        </div>
                      )}

                      {/* Actions */}
                      <div className="flex items-center gap-2 mt-auto pt-2">
                        <Button
                          size="sm"
                          className="flex-1"
                          onClick={() => handleInstall(plugin)}
                          disabled={installingPlugins.has(plugin.manifest.id)}
                        >
                          {installingPlugins.has(plugin.manifest.id) ? (
                            <>
                              <Loader2 className="size-4 mr-2 animate-spin" />
                              Installing...
                            </>
                          ) : (
                            <>
                              <Download className="size-4 mr-2" />
                              Install
                            </>
                          )}
                        </Button>
                        {plugin.manifest.homepage && (
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => window.open(plugin.manifest.homepage, "_blank")}
                          >
                            View
                          </Button>
                        )}
                      </div>
                    </CardContent>
                  </Card>
                ))}
              </div>
            )}
          </ScrollArea>
        </TabsContent>
      </Tabs>
    </div>
  );
}