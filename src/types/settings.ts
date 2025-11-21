/**
 * Application settings types
 *
 * TypeScript types matching the Rust settings model.
 * All settings are stored in Tauri Store and loaded on app startup.
 */

/**
 * Complete application settings
 */
export interface AppSettings {
  general: GeneralSettings;
  theme: ThemeSettings;
  query: QuerySettings;
  shortcuts: ShortcutsSettings;
}

/**
 * General application settings
 */
export interface GeneralSettings {
  /** Language code (e.g., "en", "es", "fr") */
  language: string;

  /** Default database to connect to on startup (connection profile ID) */
  defaultDatabase: string | null;

  /** Behavior when application starts */
  startupBehavior: StartupBehavior;

  /** Enable auto-save for connection profiles */
  autoSaveConnections: boolean;

  /** Enable telemetry and crash reporting */
  enableTelemetry: boolean;
}

/**
 * Application startup behavior options
 */
export type StartupBehavior =
  | "showConnectionList"
  | "openLastConnection"
  | "openDefaultConnection"
  | "showQueryEditor";

/**
 * Theme and appearance settings
 */
export interface ThemeSettings {
  /** Theme mode (light, dark, system) */
  mode: ThemeMode;

  /** Accent color (hex color code, e.g., "#f59e0b" for amber) */
  accentColor: string;

  /** Font size for SQL editor (in pixels) */
  editorFontSize: number;

  /** Font family for SQL editor */
  editorFontFamily: string;

  /** Enable line numbers in SQL editor */
  editorLineNumbers: boolean;

  /** Enable minimap in SQL editor */
  editorMinimap: boolean;

  /** Word wrap in SQL editor */
  editorWordWrap: boolean;
}

/**
 * Theme mode options
 */
export type ThemeMode = "light" | "dark" | "system";

/**
 * Query execution settings
 */
export interface QuerySettings {
  /** Query execution timeout in seconds (0 = no timeout) */
  timeoutSeconds: number;

  /** Maximum number of rows to fetch per query */
  maxRows: number;

  /** Enable auto-commit (applies changes immediately) */
  autoCommit: boolean;

  /** Confirm before running DELETE or DROP statements */
  confirmDestructive: boolean;

  /** Save query to history automatically */
  autoSaveHistory: boolean;

  /** Maximum number of history entries to keep */
  maxHistoryEntries: number;

  /** Format SQL automatically before execution */
  autoFormatSql: boolean;
}

/**
 * Keyboard shortcuts configuration
 */
export interface ShortcutsSettings {
  /** Execute query (default: Ctrl/Cmd+Enter) */
  executeQuery: string;

  /** Clear editor (default: Ctrl/Cmd+K) */
  clearEditor: string;

  /** Open new tab (default: Ctrl/Cmd+T) */
  newTab: string;

  /** Close tab (default: Ctrl/Cmd+W) */
  closeTab: string;

  /** Save snippet (default: Ctrl/Cmd+S) */
  saveSnippet: string;

  /** Open settings (default: Ctrl/Cmd+,) */
  openSettings: string;

  /** Toggle sidebar (default: Ctrl/Cmd+B) */
  toggleSidebar: string;

  /** Search (default: Ctrl/Cmd+F) */
  search: string;

  /** Format SQL (default: Ctrl/Cmd+Shift+F) */
  formatSql: string;

  /** Show keyboard shortcuts (default: ?) */
  showShortcuts: string;
}

/**
 * Default settings values
 */
export const defaultSettings: AppSettings = {
  general: {
    language: "en",
    defaultDatabase: null,
    startupBehavior: "showConnectionList",
    autoSaveConnections: true,
    enableTelemetry: false,
  },
  theme: {
    mode: "system",
    accentColor: "#f59e0b",
    editorFontSize: 14,
    editorFontFamily: "Monaco, 'Courier New', monospace",
    editorLineNumbers: true,
    editorMinimap: false,
    editorWordWrap: false,
  },
  query: {
    timeoutSeconds: 30,
    maxRows: 1000,
    autoCommit: false,
    confirmDestructive: true,
    autoSaveHistory: true,
    maxHistoryEntries: 500,
    autoFormatSql: false,
  },
  shortcuts: {
    executeQuery: "Ctrl+Enter",
    clearEditor: "Ctrl+K",
    newTab: "Ctrl+T",
    closeTab: "Ctrl+W",
    saveSnippet: "Ctrl+S",
    openSettings: "Ctrl+,",
    toggleSidebar: "Ctrl+B",
    search: "Ctrl+F",
    formatSql: "Ctrl+Shift+F",
    showShortcuts: "?",
  },
};
