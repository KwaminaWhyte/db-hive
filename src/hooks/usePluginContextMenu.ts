import { usePlugins } from "@/contexts/PluginContext";
import type { MenuItemComponent } from "@/types/plugins";

export interface PluginMenuItem {
  id: string;
  pluginId: string;
  label: string;
  icon?: string;
  shortcut?: string;
  action: string;
  onClick: () => void;
}

/**
 * Hook to get plugin-registered context menu items
 */
export function usePluginContextMenu(): PluginMenuItem[] {
  const { contextMenuItems, executeAction } = usePlugins();

  return contextMenuItems
    .filter((item) => item.componentType.type === "menuItem")
    .map((item) => {
      const data = item.componentType.data as MenuItemComponent;
      return {
        id: item.id,
        pluginId: item.pluginId,
        label: data.label,
        icon: data.icon,
        shortcut: data.shortcut,
        action: data.action,
        onClick: () => executeAction(item.pluginId, data.action),
      };
    });
}
