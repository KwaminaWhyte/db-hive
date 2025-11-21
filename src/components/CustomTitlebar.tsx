/**
 * Custom Titlebar Component
 *
 * Replaces native window decorations with a custom titlebar that includes:
 * - Drag region for window movement
 * - App branding
 * - Window menu system (File, View, Window, Help)
 * - Window controls (minimize, maximize, close)
 */

import { useState } from "react";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { Window } from "@tauri-apps/api/window";
import { Minus, Square, X, ChevronDown } from "lucide-react";
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

export function CustomTitlebar() {
  const navigate = useNavigate();
  const router = useRouter();
  const { setTheme } = useTheme();
  const [isMaximized, setIsMaximized] = useState(false);

  const appWindow = Window.getCurrent();

  const handleMinimize = async () => {
    await appWindow.minimize();
  };

  const handleMaximize = async () => {
    await appWindow.toggleMaximize();
    const maximized = await appWindow.isMaximized();
    setIsMaximized(maximized);
  };

  const handleClose = async () => {
    await appWindow.close();
  };

  const handleNavigateToSettings = () => {
    // Save current route before navigating to settings
    const currentPath = router.state.location.pathname;
    sessionStorage.setItem("db-hive-previous-route", currentPath);
    navigate({ to: "/settings" });
  };

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-10 bg-background border-b border-border select-none"
    >
      {/* Left: Logo and Menu */}
      <div className="flex items-center gap-2 px-3">
        {/* App Logo - Small version */}
        <div className="relative h-6 w-6 rounded border border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30">
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
        <span className="text-sm font-medium text-foreground">DB Hive</span>

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
              <DropdownMenuItem disabled>
                About DB Hive v0.6.0-beta
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
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
