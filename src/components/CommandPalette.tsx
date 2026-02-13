/**
 * Command Palette Component
 *
 * A VS Code / Raycast-style command palette that provides quick access to
 * navigation, theme switching, window controls, and common actions.
 *
 * Triggered via Cmd+K (macOS) / Ctrl+K (Windows/Linux) or by clicking the
 * search trigger in the toolbar.
 */

import { useState, useEffect, useRef, useCallback, useMemo, FC } from "react";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Home,
  Code2,
  Workflow,
  Activity,
  Puzzle,
  Settings,
  Info,
  Sun,
  Moon,
  Monitor,
  Minus,
  Maximize2,
  X,
  Plus,
  Keyboard,
  Search,
  type LucideIcon,
} from "lucide-react";
import { useTheme } from "@/components/theme-provider";
import { Dialog } from "@/components/ui/dialog";
import * as DialogPrimitive from "@radix-ui/react-dialog";

// ── Types ──

interface CommandItem {
  id: string;
  label: string;
  description?: string;
  icon: LucideIcon;
  shortcut?: string;
  group: string;
  action: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onShowShortcuts?: () => void;
}

// ── Constants ──

const isMacOS = navigator.userAgent.includes("Mac");

// ── Component ──

export const CommandPalette: FC<CommandPaletteProps> = ({
  open,
  onOpenChange,
  onShowShortcuts,
}) => {
  const [search, setSearch] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const navigate = useNavigate();
  const router = useRouter();
  const { setTheme } = useTheme();

  // Build the list of commands
  const commands = useMemo<CommandItem[]>(() => {
    const appWindow = getCurrentWindow();

    const navigateToSettings = () => {
      const currentPath = router.state.location.pathname;
      sessionStorage.setItem("db-hive-previous-route", currentPath);
      navigate({ to: "/settings" });
    };

    const navigateToAbout = () => {
      const currentPath = router.state.location.pathname;
      sessionStorage.setItem("db-hive-previous-route", currentPath);
      navigate({ to: "/about" });
    };

    return [
      // Navigation
      {
        id: "nav-home",
        label: "Go to Home",
        description: "Connection manager",
        icon: Home,
        group: "Navigation",
        action: () => navigate({ to: "/" }),
      },
      {
        id: "nav-editor",
        label: "Go to SQL Editor",
        description: "Write and execute queries",
        icon: Code2,
        group: "Navigation",
        action: () =>
          navigate({
            to: "/query",
            search: { tabs: "query-0", active: 0 },
          }),
      },
      {
        id: "nav-visual-query",
        label: "Go to Visual Query Builder",
        description: "Build queries visually",
        icon: Workflow,
        group: "Navigation",
        action: () => navigate({ to: "/visual-query" }),
      },
      {
        id: "nav-activity",
        label: "Go to Activity Monitor",
        description: "View running processes",
        icon: Activity,
        group: "Navigation",
        action: () => navigate({ to: "/activity" }),
      },
      {
        id: "nav-plugins",
        label: "Go to Plugin Manager",
        description: "Manage extensions",
        icon: Puzzle,
        group: "Navigation",
        action: () => navigate({ to: "/plugins" }),
      },
      {
        id: "nav-settings",
        label: "Go to Settings",
        description: "Configure application",
        icon: Settings,
        shortcut: isMacOS ? "\u2318," : "Ctrl+,",
        group: "Navigation",
        action: navigateToSettings,
      },
      {
        id: "nav-about",
        label: "Go to About",
        description: "Version and license info",
        icon: Info,
        group: "Navigation",
        action: navigateToAbout,
      },

      // Theme
      {
        id: "theme-light",
        label: "Switch to Light Theme",
        icon: Sun,
        group: "Theme",
        action: () => setTheme("light"),
      },
      {
        id: "theme-dark",
        label: "Switch to Dark Theme",
        icon: Moon,
        group: "Theme",
        action: () => setTheme("dark"),
      },
      {
        id: "theme-system",
        label: "Switch to System Theme",
        icon: Monitor,
        group: "Theme",
        action: () => setTheme("system"),
      },

      // Window
      {
        id: "window-minimize",
        label: "Minimize Window",
        icon: Minus,
        group: "Window",
        action: () => appWindow.minimize(),
      },
      {
        id: "window-maximize",
        label: "Toggle Maximize Window",
        icon: Maximize2,
        group: "Window",
        action: () => appWindow.toggleMaximize(),
      },
      {
        id: "window-close",
        label: "Close Window",
        icon: X,
        group: "Window",
        action: () => appWindow.close(),
      },

      // Actions
      {
        id: "action-new-connection",
        label: "New Connection",
        description: "Create a new database connection",
        icon: Plus,
        group: "Actions",
        action: () => navigate({ to: "/" }),
      },
      {
        id: "action-shortcuts",
        label: "Keyboard Shortcuts",
        description: "View all keyboard shortcuts",
        icon: Keyboard,
        shortcut: "?",
        group: "Actions",
        action: () => onShowShortcuts?.(),
      },
    ];
  }, [navigate, router, setTheme, onShowShortcuts]);

  // Filter commands by search query
  const filteredCommands = useMemo(() => {
    if (!search.trim()) return commands;
    const query = search.toLowerCase().trim();
    return commands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(query) ||
        cmd.group.toLowerCase().includes(query) ||
        (cmd.description && cmd.description.toLowerCase().includes(query))
    );
  }, [commands, search]);

  // Group filtered commands
  const groupedCommands = useMemo(() => {
    const groups: { name: string; items: CommandItem[] }[] = [];
    const groupOrder = ["Navigation", "Actions", "Theme", "Window"];

    for (const groupName of groupOrder) {
      const items = filteredCommands.filter((cmd) => cmd.group === groupName);
      if (items.length > 0) {
        groups.push({ name: groupName, items });
      }
    }

    return groups;
  }, [filteredCommands]);

  // Flatten for keyboard navigation indexing
  const flatItems = useMemo(
    () => groupedCommands.flatMap((g) => g.items),
    [groupedCommands]
  );

  // Reset state when opening
  useEffect(() => {
    if (open) {
      setSearch("");
      setSelectedIndex(0);
      // Small delay to ensure the dialog is mounted before focusing
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    }
  }, [open]);

  // Reset selectedIndex when filtered results change
  useEffect(() => {
    setSelectedIndex(0);
  }, [search]);

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const selectedEl = listRef.current.querySelector(
      `[data-command-index="${selectedIndex}"]`
    );
    if (selectedEl) {
      selectedEl.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  // Execute a command and close the palette
  const executeCommand = useCallback(
    (command: CommandItem) => {
      onOpenChange(false);
      // Small delay to let the dialog close animation start
      requestAnimationFrame(() => {
        command.action();
      });
    },
    [onOpenChange]
  );

  // Handle keyboard navigation within the palette
  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setSelectedIndex((prev) =>
          prev < flatItems.length - 1 ? prev + 1 : 0
        );
        break;
      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((prev) =>
          prev > 0 ? prev - 1 : flatItems.length - 1
        );
        break;
      case "Enter":
        e.preventDefault();
        if (flatItems[selectedIndex]) {
          executeCommand(flatItems[selectedIndex]);
        }
        break;
      case "Escape":
        e.preventDefault();
        onOpenChange(false);
        break;
    }
  };

  // Track flat index across groups for rendering
  let flatIndex = 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/50 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" />
        <DialogPrimitive.Content
          className="fixed left-[50%] top-[35%] z-50 w-full max-w-[520px] translate-x-[-50%] translate-y-[-50%] rounded-xl border border-border bg-background shadow-2xl data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 duration-200 overflow-hidden"
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            inputRef.current?.focus();
          }}
        >
          {/* Visually hidden title for accessibility */}
          <DialogPrimitive.Title className="sr-only">
            Command Palette
          </DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">
            Search for commands, navigation, and actions
          </DialogPrimitive.Description>

          {/* Search Input */}
          <div className="flex items-center gap-3 px-4 border-b border-border">
            <Search className="h-4 w-4 text-muted-foreground shrink-0" />
            <input
              ref={inputRef}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={handleInputKeyDown}
              placeholder="Type a command or search..."
              className="flex-1 h-12 bg-transparent text-sm text-foreground placeholder:text-muted-foreground outline-none"
              autoComplete="off"
              spellCheck={false}
            />
            <kbd className="hidden sm:inline-flex items-center gap-0.5 rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
              ESC
            </kbd>
          </div>

          {/* Command List */}
          <div
            ref={listRef}
            className="max-h-[min(400px,60vh)] overflow-y-auto overscroll-contain py-2"
          >
            {flatItems.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                No commands found for &ldquo;{search}&rdquo;
              </div>
            ) : (
              groupedCommands.map((group) => (
                <div key={group.name}>
                  {/* Group Header */}
                  <div className="px-4 py-1.5 text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                    {group.name}
                  </div>

                  {/* Group Items */}
                  {group.items.map((cmd) => {
                    const currentIndex = flatIndex++;
                    const isSelected = currentIndex === selectedIndex;
                    const Icon = cmd.icon;

                    return (
                      <button
                        key={cmd.id}
                        data-command-index={currentIndex}
                        onClick={() => executeCommand(cmd)}
                        onMouseEnter={() => setSelectedIndex(currentIndex)}
                        className={`flex items-center gap-3 w-full px-4 py-2 text-left text-sm transition-colors ${
                          isSelected
                            ? "bg-accent text-accent-foreground"
                            : "text-foreground hover:bg-accent/50"
                        }`}
                      >
                        <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                        <div className="flex-1 min-w-0">
                          <span className="truncate">{cmd.label}</span>
                          {cmd.description && (
                            <span className="ml-2 text-xs text-muted-foreground">
                              {cmd.description}
                            </span>
                          )}
                        </div>
                        {cmd.shortcut && (
                          <kbd className="shrink-0 rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                            {cmd.shortcut}
                          </kbd>
                        )}
                      </button>
                    );
                  })}
                </div>
              ))
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-between px-4 py-2 border-t border-border text-[11px] text-muted-foreground">
            <div className="flex items-center gap-3">
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border bg-muted px-1 py-0.5 text-[10px]">
                  &uarr;&darr;
                </kbd>
                Navigate
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border bg-muted px-1 py-0.5 text-[10px]">
                  &crarr;
                </kbd>
                Select
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border bg-muted px-1 py-0.5 text-[10px]">
                  esc
                </kbd>
                Close
              </span>
            </div>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </Dialog>
  );
};
