/**
 * Plugin System Types
 */

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  author: PluginAuthor;
  category: PluginCategory;
  main: string;
  pluginType: PluginType;
  permissions: PluginPermission[];
  minVersion: string;
  maxVersion?: string;
  icon?: string;
  homepage?: string;
  repository?: string;
  license: string;
  keywords: string[];
  dependencies?: Record<string, string>;
  configSchema?: any;
}

export interface PluginAuthor {
  name: string;
  email?: string;
  url?: string;
}

export type PluginCategory =
  | 'driver'
  | 'theme'
  | 'tool'
  | 'export'
  | 'import'
  | 'formatter'
  | 'analyzer'
  | 'visualizer'
  | 'extension';

export type PluginType = 'javascript' | 'webassembly';

export type PluginPermission =
  | 'readFiles'
  | 'writeFiles'
  | 'executeQuery'
  | 'modifySchema'
  | 'readMetadata'
  | 'createTab'
  | 'modifyUI'
  | 'showNotification'
  | 'networkAccess'
  | 'runCommand'
  | 'accessClipboard'
  | 'accessOtherPlugins';

export interface Plugin {
  manifest: PluginManifest;
  path: string;
  enabled: boolean;
  loaded: boolean;
  config?: any;
  stats: PluginStats;
}

export interface PluginStats {
  installDate: string;
  lastUsed?: string;
  executionCount: number;
  errorCount: number;
  rating?: number;
  downloads: number;
}

export interface MarketplacePlugin {
  manifest: PluginManifest;
  stats: MarketplaceStats;
  screenshots: string[];
  changelog?: string;
  readme?: string;
  downloadUrl: string;
  size: number;
  hash: string;
  signature?: string;
  verified: boolean;
}

export interface MarketplaceStats {
  downloads: number;
  stars: number;
  rating: number;
  reviews: number;
  updatedAt: string;
  createdAt: string;
}

export type PluginEvent =
  | { type: 'installed'; pluginId: string; version: string }
  | { type: 'uninstalled'; pluginId: string }
  | { type: 'enabled'; pluginId: string }
  | { type: 'disabled'; pluginId: string }
  | { type: 'updated'; pluginId: string; oldVersion: string; newVersion: string }
  | { type: 'error'; pluginId: string; error: string };

// Plugin UI Components
export interface TabConfig {
  title: string;
  icon?: string;
  contentType: TabContentType;
  closable: boolean;
}

export type TabContentType =
  | { type: 'html'; content: string }
  | { type: 'iframe'; url: string }
  | { type: 'custom'; data: any };

export interface Notification {
  title: string;
  message: string;
  notificationType: 'info' | 'success' | 'warning' | 'error';
  duration?: number;
}

export interface UiComponent {
  id: string;
  location: UiLocation;
  componentType: UiComponentType;
}

export type UiLocation =
  | 'toolbar'
  | 'sidebar'
  | 'statusBar'
  | 'contextMenu'
  | 'panel';

export type UiComponentType =
  | { type: 'button'; data: ButtonComponent }
  | { type: 'menuItem'; data: MenuItemComponent }
  | { type: 'panel'; data: PanelComponent };

export interface ButtonComponent {
  label: string;
  icon?: string;
  tooltip?: string;
  action: string;
}

export interface MenuItemComponent {
  label: string;
  icon?: string;
  shortcut?: string;
  action: string;
}

export interface PanelComponent {
  title: string;
  icon?: string;
  resizable: boolean;
  content: string;
}

// Plugin sort options
export type PluginSortOption = 'popular' | 'recent' | 'rating' | 'name' | 'downloads';

// Plugin filter state
export interface PluginFilters {
  category?: PluginCategory;
  search?: string;
  sort: PluginSortOption;
  verified?: boolean;
}