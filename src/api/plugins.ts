import { invoke } from "@tauri-apps/api/core";
import type {
  Plugin,
  MarketplacePlugin,
  PluginCategory,
} from "@/types/plugins";

/**
 * Get all installed plugins
 */
export async function getInstalledPlugins(): Promise<Plugin[]> {
  return invoke<Plugin[]>("get_installed_plugins");
}

/**
 * Get a specific plugin by ID
 */
export async function getPlugin(pluginId: string): Promise<Plugin | null> {
  return invoke<Plugin | null>("get_plugin", { pluginId });
}

/**
 * Install a plugin from marketplace
 */
export async function installPlugin(marketplacePlugin: MarketplacePlugin): Promise<void> {
  return invoke("install_plugin", { marketplacePlugin });
}

/**
 * Uninstall a plugin
 */
export async function uninstallPlugin(pluginId: string): Promise<void> {
  return invoke("uninstall_plugin", { pluginId });
}

/**
 * Enable a plugin
 */
export async function enablePlugin(pluginId: string): Promise<void> {
  return invoke("enable_plugin", { pluginId });
}

/**
 * Disable a plugin
 */
export async function disablePlugin(pluginId: string): Promise<void> {
  return invoke("disable_plugin", { pluginId });
}

/**
 * Update plugin configuration
 */
export async function updatePluginConfig(pluginId: string, config: any): Promise<void> {
  return invoke("update_plugin_config", { pluginId, config });
}

/**
 * Get marketplace plugins with optional filtering
 */
export async function getMarketplacePlugins(
  category?: PluginCategory,
  search?: string
): Promise<MarketplacePlugin[]> {
  return invoke<MarketplacePlugin[]>("get_marketplace_plugins", { category, search });
}

/**
 * Load a plugin (initialize and execute its code)
 */
export async function loadPlugin(pluginId: string): Promise<void> {
  return invoke("load_plugin", { pluginId });
}

/**
 * Unload a plugin runtime
 */
export async function unloadPluginRuntime(pluginId: string): Promise<void> {
  return invoke("unload_plugin_runtime", { pluginId });
}

/**
 * Execute a plugin function
 */
export async function executePluginFunction(
  pluginId: string,
  functionName: string,
  args: any
): Promise<any> {
  return invoke("execute_plugin_function", {
    pluginId,
    functionName,
    args,
  });
}

/**
 * Get list of currently loaded plugins
 */
export async function getLoadedPlugins(): Promise<string[]> {
  return invoke<string[]>("get_loaded_plugins");
}

/**
 * Check if a plugin is loaded
 */
export async function isPluginLoaded(pluginId: string): Promise<boolean> {
  return invoke<boolean>("is_plugin_loaded", { pluginId });
}