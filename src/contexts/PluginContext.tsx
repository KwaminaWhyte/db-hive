import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import type { UiComponent, Notification } from "@/types/plugins";
import { executePluginFunction, getInstalledPlugins } from "@/api/plugins";

interface RegisteredComponent extends UiComponent {
  pluginId: string;
  pluginName: string;
}

interface PluginContextValue {
  // Registered UI components by location
  toolbarButtons: RegisteredComponent[];
  contextMenuItems: RegisteredComponent[];
  sidebarPanels: RegisteredComponent[];
  statusBarItems: RegisteredComponent[];

  // Execute a plugin action
  executeAction: (pluginId: string, action: string) => Promise<void>;

  // Refresh registered components
  refreshComponents: () => Promise<void>;
}

const PluginContext = createContext<PluginContextValue | null>(null);

export function PluginProvider({ children }: { children: ReactNode }) {
  const [toolbarButtons, setToolbarButtons] = useState<RegisteredComponent[]>([]);
  const [contextMenuItems, setContextMenuItems] = useState<RegisteredComponent[]>([]);
  const [sidebarPanels, setSidebarPanels] = useState<RegisteredComponent[]>([]);
  const [statusBarItems, setStatusBarItems] = useState<RegisteredComponent[]>([]);

  // Handle plugin notifications
  const handleNotification = useCallback((notification: Notification & { pluginId: string }) => {
    const { title, message, notificationType } = notification;

    switch (notificationType) {
      case "success":
        toast.success(title, { description: message });
        break;
      case "error":
        toast.error(title, { description: message });
        break;
      case "warning":
        toast.warning(title, { description: message });
        break;
      default:
        toast.info(title, { description: message });
    }
  }, []);

  // Handle UI component registration
  const handleRegisterUi = useCallback(
    (event: { pluginId: string; component: UiComponent; pluginName?: string }) => {
      const { pluginId, component, pluginName } = event;
      const registeredComponent: RegisteredComponent = {
        ...component,
        pluginId,
        pluginName: pluginName || pluginId,
      };

      console.log("[PluginContext] Registering UI component:", registeredComponent);

      switch (component.location) {
        case "toolbar":
          setToolbarButtons((prev) => {
            // Avoid duplicates
            if (prev.some((c) => c.id === component.id)) return prev;
            return [...prev, registeredComponent];
          });
          break;
        case "contextMenu":
          setContextMenuItems((prev) => {
            if (prev.some((c) => c.id === component.id)) return prev;
            return [...prev, registeredComponent];
          });
          break;
        case "sidebar":
          setSidebarPanels((prev) => {
            if (prev.some((c) => c.id === component.id)) return prev;
            return [...prev, registeredComponent];
          });
          break;
        case "statusBar":
          setStatusBarItems((prev) => {
            if (prev.some((c) => c.id === component.id)) return prev;
            return [...prev, registeredComponent];
          });
          break;
      }
    },
    []
  );

  // Execute a plugin action
  const executeAction = useCallback(async (pluginId: string, action: string) => {
    try {
      console.log(`[PluginContext] Executing action: ${action} for plugin: ${pluginId}`);
      const result = await executePluginFunction(pluginId, action, null);

      if (result && typeof result === "object" && "success" in result) {
        if (result.success) {
          toast.success(`Action completed: ${result.message || action}`);
        } else {
          toast.error(`Action failed: ${result.error || result.message || "Unknown error"}`);
        }
      }
    } catch (error: any) {
      console.error(`[PluginContext] Action failed:`, error);
      toast.error(`Failed to execute ${action}: ${error.message || error}`);
    }
  }, []);

  // Refresh registered components from enabled plugins
  const refreshComponents = useCallback(async () => {
    try {
      // Clear existing registrations
      setToolbarButtons([]);
      setContextMenuItems([]);
      setSidebarPanels([]);
      setStatusBarItems([]);

      // Get installed and enabled plugins
      const plugins = await getInstalledPlugins();
      const enabledPlugins = plugins.filter((p) => p.enabled);

      console.log(`[PluginContext] Found ${enabledPlugins.length} enabled plugins`);

      // Note: Plugins register their UI components when loaded via onLoad()
      // The backend will emit plugin-register-ui events which we handle above
    } catch (error) {
      console.error("[PluginContext] Failed to refresh components:", error);
    }
  }, []);

  // Setup event listeners
  useEffect(() => {
    let unlistenNotification: (() => void) | null = null;
    let unlistenRegisterUi: (() => void) | null = null;

    const setupListeners = async () => {
      // Listen for plugin notifications
      unlistenNotification = await listen<{
        plugin_id: string;
        notification: Notification;
      }>("plugin-notification", (event) => {
        const { plugin_id, notification } = event.payload;
        handleNotification({ ...notification, pluginId: plugin_id });
      });

      // Listen for UI component registrations
      unlistenRegisterUi = await listen<{
        plugin_id: string;
        component: UiComponent;
      }>("plugin-register-ui", (event) => {
        const { plugin_id, component } = event.payload;
        handleRegisterUi({ pluginId: plugin_id, component });
      });
    };

    setupListeners();

    // Initial refresh
    refreshComponents();

    return () => {
      unlistenNotification?.();
      unlistenRegisterUi?.();
    };
  }, [handleNotification, handleRegisterUi, refreshComponents]);

  const value: PluginContextValue = {
    toolbarButtons,
    contextMenuItems,
    sidebarPanels,
    statusBarItems,
    executeAction,
    refreshComponents,
  };

  return <PluginContext.Provider value={value}>{children}</PluginContext.Provider>;
}

export function usePlugins() {
  const context = useContext(PluginContext);
  if (!context) {
    throw new Error("usePlugins must be used within a PluginProvider");
  }
  return context;
}
