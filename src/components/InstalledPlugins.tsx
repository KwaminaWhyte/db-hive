import { useState, useEffect } from "react";
import {
  Settings,
  Power,
  Trash2,
  Info,
  Package,
  Calendar,
  Activity,
  AlertCircle,
  Check,
  X,
  Loader2,
  Play,
} from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Button } from "./ui/button";
import { Badge } from "./ui/badge";
import { Switch } from "./ui/switch";
import { ScrollArea } from "./ui/scroll-area";
import { Separator } from "./ui/separator";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
} from "./ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";
import { toast } from "sonner";
import { format } from "date-fns";
import type { Plugin } from "@/types/plugins";
import {
  getInstalledPlugins,
  uninstallPlugin,
  enablePlugin,
  disablePlugin,
  updatePluginConfig,
  loadPlugin,
  unloadPluginRuntime,
  executePluginFunction,
} from "@/api/plugins";

const categoryColors: Record<string, string> = {
  driver: "bg-blue-500/10 text-blue-500",
  theme: "bg-purple-500/10 text-purple-500",
  tool: "bg-green-500/10 text-green-500",
  export: "bg-orange-500/10 text-orange-500",
  import: "bg-cyan-500/10 text-cyan-500",
  formatter: "bg-pink-500/10 text-pink-500",
  analyzer: "bg-yellow-500/10 text-yellow-500",
  visualizer: "bg-indigo-500/10 text-indigo-500",
  extension: "bg-gray-500/10 text-gray-500",
};

export function InstalledPlugins() {
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedPlugin, setSelectedPlugin] = useState<Plugin | null>(null);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const [pluginToDelete, setPluginToDelete] = useState<Plugin | null>(null);
  const [togglingPlugins, setTogglingPlugins] = useState<Set<string>>(new Set());
  const [runningPlugins, setRunningPlugins] = useState<Set<string>>(new Set());

  useEffect(() => {
    loadInstalledPlugins();

    // Listen for plugin events
    const unlisten = setupPluginEventListeners();

    return () => {
      unlisten?.();
    };
  }, []);

  const loadInstalledPlugins = async () => {
    try {
      setLoading(true);
      const installed = await getInstalledPlugins();
      setPlugins(installed);
    } catch (error) {
      console.error("Failed to load installed plugins:", error);
      toast.error("Failed to load installed plugins");
    } finally {
      setLoading(false);
    }
  };

  const setupPluginEventListeners = () => {
    // TODO: Setup event listeners for plugin events
    // This would listen for install/uninstall/enable/disable events
    // and refresh the plugin list accordingly
    return undefined;
  };

  const handleTogglePlugin = async (plugin: Plugin) => {
    try {
      setTogglingPlugins(prev => new Set(prev).add(plugin.manifest.id));

      if (plugin.enabled) {
        // First unload the plugin runtime, then disable it
        try {
          await unloadPluginRuntime(plugin.manifest.id);
        } catch (e) {
          // Plugin might not be loaded, that's okay
          console.log("Plugin runtime not loaded:", e);
        }
        await disablePlugin(plugin.manifest.id);
        toast.success(`${plugin.manifest.name} disabled`);
      } else {
        // First enable the plugin, then load its runtime
        await enablePlugin(plugin.manifest.id);
        try {
          await loadPlugin(plugin.manifest.id);
          toast.success(`${plugin.manifest.name} enabled and loaded`);
        } catch (loadError: any) {
          // If loading fails, still report enable success but warn about load
          console.error("Failed to load plugin runtime:", loadError);
          toast.warning(`${plugin.manifest.name} enabled but failed to load: ${loadError.message || loadError}`);
        }
      }

      // Reload plugins to get updated state
      await loadInstalledPlugins();
    } catch (error: any) {
      console.error("Failed to toggle plugin:", error);
      toast.error(`Failed to toggle plugin: ${error.message || error}`);
    } finally {
      setTogglingPlugins(prev => {
        const next = new Set(prev);
        next.delete(plugin.manifest.id);
        return next;
      });
    }
  };

  const handleUninstall = async () => {
    if (!pluginToDelete) return;

    try {
      await uninstallPlugin(pluginToDelete.manifest.id);
      toast.success(`${pluginToDelete.manifest.name} uninstalled`);
      setShowDeleteDialog(false);
      setPluginToDelete(null);
      await loadInstalledPlugins();
    } catch (error: any) {
      console.error("Failed to uninstall plugin:", error);
      toast.error(`Failed to uninstall plugin: ${error.message || error}`);
    }
  };

  const handleRunPlugin = async (plugin: Plugin, functionName: string = "exportToCsv") => {
    try {
      setRunningPlugins(prev => new Set(prev).add(plugin.manifest.id));
      toast.info(`Running ${plugin.manifest.name}...`);

      const result = await executePluginFunction(plugin.manifest.id, functionName, null);
      console.log("Plugin function result:", result);

      if (result && typeof result === "object" && "success" in result) {
        if (result.success) {
          toast.success(`${plugin.manifest.name}: ${result.message || result.filename || "Success"}`);
        } else {
          toast.error(`${plugin.manifest.name}: ${result.error || result.message || "Failed"}`);
        }
      } else {
        toast.success(`${plugin.manifest.name} executed`);
      }
    } catch (error: any) {
      console.error("Failed to run plugin:", error);
      toast.error(`Failed to run plugin: ${error.message || error}`);
    } finally {
      setRunningPlugins(prev => {
        const next = new Set(prev);
        next.delete(plugin.manifest.id);
        return next;
      });
    }
  };

  const formatPermissions = (permissions: string[]): string => {
    const permissionLabels: Record<string, string> = {
      readFiles: "Read Files",
      writeFiles: "Write Files",
      executeQuery: "Execute Queries",
      modifySchema: "Modify Schema",
      readMetadata: "Read Metadata",
      createTab: "Create Tabs",
      modifyUI: "Modify UI",
      showNotification: "Show Notifications",
      networkAccess: "Network Access",
      runCommand: "Run Commands",
      accessClipboard: "Access Clipboard",
      accessOtherPlugins: "Access Other Plugins",
    };

    return permissions.map(p => permissionLabels[p] || p).join(", ");
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="size-8 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {/* Plugin List */}
      <div className="w-80 border-r bg-muted/10">
        <div className="p-4 border-b">
          <h2 className="text-lg font-semibold">Installed Plugins</h2>
          <p className="text-sm text-muted-foreground mt-1">
            {plugins.length} plugin{plugins.length !== 1 ? "s" : ""} installed
          </p>
        </div>

        <ScrollArea className="flex-1">
          {plugins.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-64 text-muted-foreground">
              <Package className="size-12 mb-4" />
              <p className="text-sm font-medium">No plugins installed</p>
              <p className="text-xs">Visit the marketplace to get started</p>
            </div>
          ) : (
            <div className="p-4 space-y-2">
              {plugins.map((plugin) => (
                <Card
                  key={plugin.manifest.id}
                  className={`cursor-pointer transition-colors ${
                    selectedPlugin?.manifest.id === plugin.manifest.id
                      ? "border-primary"
                      : "hover:bg-accent"
                  }`}
                  onClick={() => setSelectedPlugin(plugin)}
                >
                  <CardHeader className="p-4">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <CardTitle className="text-sm flex items-center gap-2">
                          {plugin.manifest.name}
                          {plugin.enabled && (
                            <Badge variant="secondary" className="text-xs">
                              <Check className="size-3 mr-1" />
                              Active
                            </Badge>
                          )}
                        </CardTitle>
                        <CardDescription className="text-xs mt-1">
                          v{plugin.manifest.version} • {plugin.manifest.author.name}
                        </CardDescription>
                      </div>
                      <Switch
                        checked={plugin.enabled}
                        disabled={togglingPlugins.has(plugin.manifest.id)}
                        onCheckedChange={() => handleTogglePlugin(plugin)}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </div>
                    <div className="mt-2">
                      <Badge
                        variant="outline"
                        className={`text-xs ${categoryColors[plugin.manifest.category] || ""}`}
                      >
                        {plugin.manifest.category}
                      </Badge>
                    </div>
                  </CardHeader>
                </Card>
              ))}
            </div>
          )}
        </ScrollArea>
      </div>

      {/* Plugin Details */}
      <div className="flex-1">
        {selectedPlugin ? (
          <ScrollArea className="h-full">
            <div className="p-6 space-y-6">
              {/* Header */}
              <div>
                <div className="flex items-start justify-between">
                  <div>
                    <h1 className="text-2xl font-semibold flex items-center gap-3">
                      {selectedPlugin.manifest.name}
                      <Badge variant="outline">v{selectedPlugin.manifest.version}</Badge>
                      {selectedPlugin.enabled && (
                        <Badge variant="secondary">
                          <Power className="size-3 mr-1" />
                          Enabled
                        </Badge>
                      )}
                    </h1>
                    <p className="text-muted-foreground mt-1">
                      {selectedPlugin.manifest.description}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    {selectedPlugin.enabled && (
                      <Button
                        variant="default"
                        size="sm"
                        onClick={() => handleRunPlugin(selectedPlugin)}
                        disabled={runningPlugins.has(selectedPlugin.manifest.id)}
                      >
                        {runningPlugins.has(selectedPlugin.manifest.id) ? (
                          <>
                            <Loader2 className="size-4 mr-2 animate-spin" />
                            Running...
                          </>
                        ) : (
                          <>
                            <Play className="size-4 mr-2" />
                            Run
                          </>
                        )}
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleTogglePlugin(selectedPlugin)}
                      disabled={togglingPlugins.has(selectedPlugin.manifest.id)}
                    >
                      {selectedPlugin.enabled ? (
                        <>
                          <X className="size-4 mr-2" />
                          Disable
                        </>
                      ) : (
                        <>
                          <Check className="size-4 mr-2" />
                          Enable
                        </>
                      )}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setPluginToDelete(selectedPlugin);
                        setShowDeleteDialog(true);
                      }}
                    >
                      <Trash2 className="size-4 mr-2" />
                      Uninstall
                    </Button>
                  </div>
                </div>
              </div>

              <Separator />

              {/* Plugin Info */}
              <div className="grid grid-cols-2 gap-6">
                <Card>
                  <CardHeader className="pb-3">
                    <CardTitle className="text-sm">Information</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-2">
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Author</span>
                      <span>{selectedPlugin.manifest.author.name}</span>
                    </div>
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">License</span>
                      <span>{selectedPlugin.manifest.license}</span>
                    </div>
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Category</span>
                      <Badge variant="outline" className="text-xs">
                        {selectedPlugin.manifest.category}
                      </Badge>
                    </div>
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Type</span>
                      <span>{selectedPlugin.manifest.pluginType}</span>
                    </div>
                  </CardContent>
                </Card>

                <Card>
                  <CardHeader className="pb-3">
                    <CardTitle className="text-sm">Statistics</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-2">
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Installed</span>
                      <span>{format(new Date(selectedPlugin.stats.installDate), "MMM d, yyyy")}</span>
                    </div>
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Executions</span>
                      <span>{selectedPlugin.stats.executionCount}</span>
                    </div>
                    <div className="flex justify-between text-sm">
                      <span className="text-muted-foreground">Errors</span>
                      <span className={selectedPlugin.stats.errorCount > 0 ? "text-red-500" : ""}>
                        {selectedPlugin.stats.errorCount}
                      </span>
                    </div>
                    {selectedPlugin.stats.lastUsed && (
                      <div className="flex justify-between text-sm">
                        <span className="text-muted-foreground">Last Used</span>
                        <span>{format(new Date(selectedPlugin.stats.lastUsed), "MMM d, yyyy")}</span>
                      </div>
                    )}
                  </CardContent>
                </Card>
              </div>

              {/* Permissions */}
              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="text-sm flex items-center gap-2">
                    <AlertCircle className="size-4" />
                    Permissions
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <p className="text-sm text-muted-foreground">
                    {selectedPlugin.manifest.permissions.length > 0
                      ? formatPermissions(selectedPlugin.manifest.permissions)
                      : "No special permissions required"}
                  </p>
                </CardContent>
              </Card>

              {/* Keywords */}
              {selectedPlugin.manifest.keywords.length > 0 && (
                <Card>
                  <CardHeader className="pb-3">
                    <CardTitle className="text-sm">Keywords</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="flex flex-wrap gap-2">
                      {selectedPlugin.manifest.keywords.map((keyword) => (
                        <Badge key={keyword} variant="secondary">
                          {keyword}
                        </Badge>
                      ))}
                    </div>
                  </CardContent>
                </Card>
              )}

              {/* Links */}
              <div className="flex items-center gap-2">
                {selectedPlugin.manifest.homepage && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => window.open(selectedPlugin.manifest.homepage, "_blank")}
                  >
                    Homepage
                  </Button>
                )}
                {selectedPlugin.manifest.repository && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => window.open(selectedPlugin.manifest.repository, "_blank")}
                  >
                    Repository
                  </Button>
                )}
              </div>
            </div>
          </ScrollArea>
        ) : (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground">
            <Info className="size-12 mb-4" />
            <p className="text-lg font-medium">Select a plugin</p>
            <p className="text-sm">Choose a plugin from the list to view details</p>
          </div>
        )}
      </div>

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Uninstall Plugin</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to uninstall {pluginToDelete?.manifest.name}? This action
              cannot be undone and will remove all plugin data.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleUninstall}>Uninstall</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}