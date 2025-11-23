/**
 * Keyboard Shortcuts Cheat Sheet Modal
 *
 * Interactive modal displaying all keyboard shortcuts grouped by category.
 * Features:
 * - Search/filter functionality
 * - Platform-specific shortcuts (Cmd vs Ctrl)
 * - Organized by category (Editor, Navigation, Query, Tables)
 * - Triggered by "?" hotkey
 */

import { FC, useState, useMemo } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Search, X } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export interface ShortcutDefinition {
  label: string;
  windows: string;
  mac: string;
  description?: string;
}

export interface ShortcutCategory {
  category: string;
  shortcuts: ShortcutDefinition[];
}

interface KeyboardShortcutsModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export const KeyboardShortcutsModal: FC<KeyboardShortcutsModalProps> = ({
  open,
  onOpenChange,
}) => {
  const [searchQuery, setSearchQuery] = useState("");

  // Detect platform
  const isMac = navigator.platform.toUpperCase().indexOf("MAC") >= 0;

  // Define all shortcuts organized by category
  const allShortcuts: ShortcutCategory[] = [
    {
      category: "Editor",
      shortcuts: [
        {
          label: "Execute Query",
          windows: "Ctrl+Enter",
          mac: "⌘+Enter",
          description: "Run the current SQL query",
        },
        {
          label: "Clear Editor",
          windows: "Ctrl+K",
          mac: "⌘+K",
          description: "Clear all text from the editor",
        },
        {
          label: "Format SQL",
          windows: "Ctrl+Shift+F",
          mac: "⌘+Shift+F",
          description: "Auto-format SQL query",
        },
        {
          label: "Save Snippet",
          windows: "Ctrl+S",
          mac: "⌘+S",
          description: "Save current query as a snippet",
        },
      ],
    },
    {
      category: "Navigation",
      shortcuts: [
        {
          label: "New Tab",
          windows: "Ctrl+T",
          mac: "⌘+T",
          description: "Open a new query tab",
        },
        {
          label: "Close Tab",
          windows: "Ctrl+W",
          mac: "⌘+W",
          description: "Close the current tab",
        },
        {
          label: "Toggle Sidebar",
          windows: "Ctrl+B",
          mac: "⌘+B",
          description: "Show/hide the schema explorer",
        },
        {
          label: "Search",
          windows: "Ctrl+F",
          mac: "⌘+F",
          description: "Search within the current view",
        },
        {
          label: "Open Settings",
          windows: "Ctrl+,",
          mac: "⌘+,",
          description: "Open application settings",
        },
        {
          label: "Show Shortcuts",
          windows: "?",
          mac: "?",
          description: "Show this keyboard shortcuts guide",
        },
      ],
    },
    {
      category: "Welcome Screen",
      shortcuts: [
        {
          label: "New Connection",
          windows: "Ctrl+K",
          mac: "⌘+K",
          description: "Create a new database connection",
        },
        {
          label: "Recent Connections",
          windows: "Ctrl+R",
          mac: "⌘+R",
          description: "View recent connections",
        },
        {
          label: "View Sample",
          windows: "Ctrl+O",
          mac: "⌘+O",
          description: "Open sample workspace",
        },
        {
          label: "Documentation",
          windows: "?",
          mac: "?",
          description: "Open documentation",
        },
      ],
    },
  ];

  // Filter shortcuts based on search query
  const filteredShortcuts = useMemo(() => {
    if (!searchQuery.trim()) {
      return allShortcuts;
    }

    const query = searchQuery.toLowerCase();
    return allShortcuts
      .map((category) => ({
        ...category,
        shortcuts: category.shortcuts.filter(
          (shortcut) =>
            shortcut.label.toLowerCase().includes(query) ||
            shortcut.windows.toLowerCase().includes(query) ||
            shortcut.mac.toLowerCase().includes(query) ||
            shortcut.description?.toLowerCase().includes(query)
        ),
      }))
      .filter((category) => category.shortcuts.length > 0);
  }, [searchQuery, allShortcuts]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[90vh] p-0">
        <DialogHeader className="px-6 pt-6 pb-4">
          <DialogTitle className="text-2xl">Keyboard Shortcuts</DialogTitle>
          <DialogDescription>
            Quick reference guide for all keyboard shortcuts in DB-Hive
          </DialogDescription>
        </DialogHeader>

        {/* Search Bar */}
        <div className="px-6 pb-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder="Search shortcuts..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-9 pr-9"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
              >
                <X className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>

        {/* Shortcuts List */}
        <ScrollArea className="flex-1 px-6 pb-6" style={{ maxHeight: "60vh" }}>
          {filteredShortcuts.length === 0 ? (
            <div className="text-center py-12">
              <p className="text-muted-foreground text-sm">
                No shortcuts found matching "{searchQuery}"
              </p>
            </div>
          ) : (
            <div className="space-y-6">
              {filteredShortcuts.map((category) => (
                <Card key={category.category}>
                  <CardHeader>
                    <CardTitle className="text-lg">{category.category}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="space-y-3">
                      {category.shortcuts.map((shortcut, index) => (
                        <div
                          key={index}
                          className="flex items-start justify-between py-2 border-b border-border last:border-0"
                        >
                          <div className="flex-1">
                            <div className="font-medium text-sm">
                              {shortcut.label}
                            </div>
                            {shortcut.description && (
                              <div className="text-xs text-muted-foreground mt-0.5">
                                {shortcut.description}
                              </div>
                            )}
                          </div>
                          <kbd className="bg-muted border-border rounded border px-2.5 py-1.5 text-xs font-mono whitespace-nowrap ml-4">
                            {isMac ? shortcut.mac : shortcut.windows}
                          </kbd>
                        </div>
                      ))}
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </ScrollArea>

        {/* Platform Indicator */}
        <div className="px-6 py-3 border-t border-border bg-muted/30">
          <p className="text-xs text-muted-foreground text-center">
            Showing shortcuts for{" "}
            <span className="font-medium text-foreground">
              {isMac ? "macOS" : "Windows/Linux"}
            </span>
          </p>
        </div>
      </DialogContent>
    </Dialog>
  );
};
