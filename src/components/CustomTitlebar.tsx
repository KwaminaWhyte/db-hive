/**
 * Custom Titlebar Component
 *
 * Replaces native window decorations with a custom titlebar that includes:
 * - Drag region for window movement
 * - App branding
 * - Window menu system (File, View, Help)
 * - Window controls (minimize, maximize, close)
 */

import { useState, useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";
import { openAppModal } from "@/store/useAppModal";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openDatabaseWindow } from "@/utils/multiWindow";
import { Minus, Square, X, ChevronDown, Search } from "lucide-react";
import { PluginToolbar } from "./PluginToolbar";
import { TableCreationDialog } from "./TableCreationDialog";
import { notifyMetadataChanged } from "@/hooks/useMetadataCache";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { useTheme } from "./theme-provider";
import { useSettings } from "@/hooks/useSettings";
import { formatShortcutKeys } from "@/lib/shortcutRegistry";

interface CustomTitlebarProps {
  onShowShortcuts?: () => void;
  onOpenCommandPalette?: () => void;
}

const isMacOS = navigator.userAgent.includes("Mac");

export function CustomTitlebar({ onShowShortcuts, onOpenCommandPalette }: CustomTitlebarProps) {
  const navigate = useNavigate();
  const { setTheme } = useTheme();
  const { connectionId } = useConnectionContext();
  const isConnected = !!connectionId;
  const [isMaximized, setIsMaximized] = useState(false);
  const [showCreateTableDialog, setShowCreateTableDialog] = useState(false);

  const appWindow = getCurrentWindow();

  // Check if window is maximized on mount
  useEffect(() => {
    const checkMaximized = async () => {
      try {
        const maximized = await appWindow.isMaximized();
        setIsMaximized(maximized);
      } catch (error) {
        console.error("Failed to check maximized state:", error);
      }
    };
    checkMaximized();
  }, [appWindow]);

  const handleMinimize = async () => {
    try {
      await appWindow.minimize();
    } catch (error) {
      console.error("Failed to minimize window:", error);
    }
  };

  const handleMaximize = async () => {
    try {
      await appWindow.toggleMaximize();
      const maximized = await appWindow.isMaximized();
      setIsMaximized(maximized);
    } catch (error) {
      console.error("Failed to toggle maximize:", error);
    }
  };

  const handleClose = async () => {
    try {
      await appWindow.close();
    } catch (error) {
      console.error("Failed to close window:", error);
    }
  };

  // Handle window dragging
  const handleDragStart = async (e: React.MouseEvent) => {
    // Only start dragging on left mouse button
    if (e.button === 0) {
      try {
        await appWindow.startDragging();
      } catch (error) {
        console.error("Failed to start dragging:", error);
      }
    }
  };

  // Live settings shortcut binding (user-rebindable) for the menu hint
  const { settings } = useSettings();
  const openSettingsKey = settings?.shortcuts?.openSettings ?? "Ctrl+,";

  // Open overlay modals instead of navigating to full-page routes so the
  // underlying route (e.g. SQL query editor) keeps its in-progress state.
  const handleOpenSettings = () => openAppModal("settings");
  const handleOpenAbout = () => openAppModal("about");
  const handleOpenPlugins = () => openAppModal("plugins");

  return (
    <div
      data-tauri-drag-region
      className="relative flex items-center justify-between h-10 bg-background border-b border-border select-none"
      onMouseDown={handleDragStart}
    >
      {/* Left: Logo and Menu — extra left padding on macOS for native traffic lights */}
      <div className={`flex items-center gap-2 px-3 ${isMacOS ? "pl-[78px]" : ""}`}>
        {/* App Logo - Small version */}
        <div
          className="relative h-6 w-6 rounded border border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30"
        >
          <div className="absolute inset-[15%] rounded bg-slate-950/90 dark:bg-slate-950/90 border border-amber-200/30 flex items-center justify-center">
            <div className="relative h-full w-full flex items-center justify-center">
              <div className="absolute h-[70%] w-[70%] border border-amber-300/40 rounded rotate-6"></div>
              <div className="absolute h-[70%] w-[70%] border border-amber-200/50 rounded -rotate-6"></div>
              <div className="relative h-[55%] w-[55%] rounded bg-gradient-to-br from-amber-300 via-amber-400 to-amber-500 flex flex-col justify-center items-center gap-[1px]">
                <div className="w-[70%] h-[1.5px] rounded-full bg-slate-950/85"></div>
                <div className="w-[60%] h-[1.5px] rounded-full bg-slate-950/85"></div>
                <div className="w-[50%] h-[1.5px] rounded-full bg-slate-950/85"></div>
              </div>
            </div>
          </div>
        </div>

        {/* App Name */}
        <span className="text-sm font-medium text-foreground">
          DB Hive
        </span>

        {/* Menu Bar */}
        <div className="flex items-center gap-1 ml-4" onMouseDown={(e) => e.stopPropagation()}>
          {/* File Menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs hover:bg-accent"
              >
                File
                <ChevronDown className="ml-1 h-3 w-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuItem onClick={() => navigate({ to: "/" })}>
                New Connection
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => openDatabaseWindow()}>
                New Window
                <span className="ml-auto pl-4 text-xs text-muted-foreground">
                  {formatShortcutKeys("Ctrl+Shift+N")}
                </span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleOpenSettings}>
                Settings
                <span className="ml-auto pl-4 text-xs text-muted-foreground">
                  {formatShortcutKeys(openSettingsKey)}
                </span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleClose}>
                Close Window
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          {/* Schema Menu (only when connected) */}
          {isConnected && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs hover:bg-accent"
                >
                  Schema
                  <ChevronDown className="ml-1 h-3 w-3" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onClick={() => setShowCreateTableDialog(true)}>
                  New Table...
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => {
                    notifyMetadataChanged({ reason: "manual-refresh" });
                    window.dispatchEvent(new Event("metadata:refresh"));
                  }}
                >
                  Refresh Metadata
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          {/* View Menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs hover:bg-accent"
              >
                View
                <ChevronDown className="ml-1 h-3 w-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {isConnected && (
                <>
                  <DropdownMenuItem onClick={() => navigate({ to: "/query", search: { tabs: "query-0", active: 0 } })}>
                    SQL Editor
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => navigate({ to: "/visual-query" })}>
                    Visual Query Builder
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => navigate({ to: "/visual-schema-designer" })}>
                    Schema Designer
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => navigate({ to: "/activity" })}>
                    Activity Monitor
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => openAppModal("backup")}>
                    Backup Manager
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                </>
              )}
              <DropdownMenuItem onClick={handleOpenPlugins}>
                Plugin Manager
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuSub>
                <DropdownMenuSubTrigger>Theme</DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  <DropdownMenuItem onClick={() => setTheme("light")}>
                    Light
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setTheme("dark")}>
                    Dark
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setTheme("system")}>
                    System
                  </DropdownMenuItem>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            </DropdownMenuContent>
          </DropdownMenu>

          {/* Help Menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs hover:bg-accent"
              >
                Help
                <ChevronDown className="ml-1 h-3 w-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuItem onClick={onShowShortcuts}>
                Keyboard Shortcuts
                <span className="ml-auto pl-4 text-xs text-muted-foreground">?</span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={() =>
                  window.open("https://github.com/KwaminaWhyte/db-hive/wiki", "_blank")
                }
              >
                Documentation
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() =>
                  window.open("https://github.com/KwaminaWhyte/db-hive", "_blank")
                }
              >
                GitHub Repository
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleOpenAbout}>
                About DB Hive
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Plugin Toolbar */}
        <div onMouseDown={(e) => e.stopPropagation()}>
          <PluginToolbar />
        </div>
      </div>

      {/* Center: Command Palette Trigger — absolute-centered on the titlebar so side groups don't skew it */}
      <div
        className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <button
          onClick={onOpenCommandPalette}
          className="pointer-events-auto flex items-center gap-2 px-3 h-7 rounded-md border border-border bg-muted/50 text-xs text-muted-foreground hover:bg-accent transition-colors w-[360px]"
        >
          <Search className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">Search or run commands...</span>
          <kbd className="ml-auto shrink-0 text-[10px] bg-background px-1 rounded border border-border">
            {isMacOS ? "\u2318" : "Ctrl+"}K
          </kbd>
        </button>
      </div>

      {/* Right: Window Controls (hidden on macOS where native traffic lights are used) */}
      {isMacOS ? (
        <div className="w-4 shrink-0" />
      ) : (
        <div className="flex items-center" onMouseDown={(e) => e.stopPropagation()}>
          <button
            onClick={handleMinimize}
            className="h-10 px-4 hover:bg-accent transition-colors"
            title="Minimize"
            aria-label="Minimize window"
          >
            <Minus className="h-4 w-4" />
          </button>
          <button
            onClick={handleMaximize}
            className="h-10 px-4 hover:bg-accent transition-colors"
            title={isMaximized ? "Restore" : "Maximize"}
            aria-label={isMaximized ? "Restore window" : "Maximize window"}
          >
            <Square className="h-3.5 w-3.5" />
          </button>
          <button
            onClick={handleClose}
            className="h-10 px-4 hover:bg-destructive hover:text-destructive-foreground transition-colors"
            title="Close"
            aria-label="Close window"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      )}

      {/* Schema menu: New Table dialog */}
      {connectionId && (
        <TableCreationDialog
          open={showCreateTableDialog}
          onOpenChange={setShowCreateTableDialog}
          connectionId={connectionId}
          onSuccess={() => notifyMetadataChanged({ reason: "create-table" })}
        />
      )}
    </div>
  );
}
