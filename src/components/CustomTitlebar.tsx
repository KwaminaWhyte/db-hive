/**
 * Custom Titlebar Component
 *
 * Replaces native window decorations with a custom titlebar that includes:
 * - Drag region for window movement
 * - App branding
 * - Window menu system (File, View, Window, Help)
 * - Window controls (minimize, maximize, close)
 */

import { useState, useEffect } from "react";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, ChevronDown } from "lucide-react";
import { PluginToolbar } from "./PluginToolbar";
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

interface CustomTitlebarProps {
  onShowShortcuts?: () => void;
}

export function CustomTitlebar({ onShowShortcuts }: CustomTitlebarProps) {
  const navigate = useNavigate();
  const router = useRouter();
  const { setTheme } = useTheme();
  const [isMaximized, setIsMaximized] = useState(false);

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

  const handleNavigateToSettings = () => {
    // Save current route before navigating to settings
    const currentPath = router.state.location.pathname;
    sessionStorage.setItem("db-hive-previous-route", currentPath);
    navigate({ to: "/settings" });
  };

  const handleNavigateToAbout = () => {
    // Save current route before navigating to about
    const currentPath = router.state.location.pathname;
    sessionStorage.setItem("db-hive-previous-route", currentPath);
    navigate({ to: "/about" });
  };

  return (
    <div className="flex items-center justify-between h-10 bg-background border-b border-border select-none">
      {/* Left: Logo and Menu */}
      <div className="flex items-center gap-2 px-3">
        {/* App Logo - Small version with drag region */}
        <div
          className="relative h-6 w-6 rounded border border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30 cursor-move"
          onMouseDown={handleDragStart}
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

        {/* App Name - Also draggable */}
        <span
          className="text-sm font-medium text-foreground cursor-move"
          onMouseDown={handleDragStart}
        >
          DB Hive
        </span>

        {/* Menu Bar */}
        <div className="flex items-center gap-1 ml-4">
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
              <DropdownMenuItem onClick={() => navigate({ to: "/connections", search: { mode: "new", profileId: undefined } })}>
                New Connection
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => navigate({ to: "/connections", search: { mode: undefined, profileId: undefined } })}>
                Recent Connections
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleClose}>
                Exit
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

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
              <DropdownMenuItem onClick={() => navigate({ to: "/query", search: { tabs: "query-0", active: 0 } })}>
                SQL Editor
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => navigate({ to: "/visual-query" })}>
                Visual Query Builder
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => navigate({ to: "/activity" })}>
                Activity Monitor
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => navigate({ to: "/plugins" })}>
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

          {/* Window Menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs hover:bg-accent"
              >
                Window
                <ChevronDown className="ml-1 h-3 w-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuItem onClick={handleNavigateToSettings}>
                Settings
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={handleMinimize}>
                Minimize
              </DropdownMenuItem>
              <DropdownMenuItem onClick={handleMaximize}>
                {isMaximized ? "Restore" : "Maximize"}
              </DropdownMenuItem>
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
              <DropdownMenuItem onClick={handleNavigateToAbout}>
                About DB-Hive
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Plugin Toolbar */}
        <PluginToolbar />
      </div>

      {/* Right: Window Controls */}
      <div className="flex items-center">
        <button
          onClick={handleMinimize}
          className="h-10 px-4 hover:bg-accent transition-colors"
          title="Minimize"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          onClick={handleMaximize}
          className="h-10 px-4 hover:bg-accent transition-colors"
          title={isMaximized ? "Restore" : "Maximize"}
        >
          <Square className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={handleClose}
          className="h-10 px-4 hover:bg-destructive hover:text-destructive-foreground transition-colors"
          title="Close"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
