/**
 * Keyboard Shortcuts Cheat Sheet Modal
 *
 * Interactive modal displaying all keyboard shortcuts grouped by category.
 * Features:
 * - Search/filter functionality
 * - Platform-specific shortcuts (Cmd vs Ctrl)
 * - Rendered from the shortcut registry (src/lib/shortcutRegistry.ts) so the
 *   list always matches the real bindings, including user-rebound shortcuts
 *   from Settings
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
import { useSettings } from "@/hooks/useSettings";
import {
  formatShortcutKeys,
  isMacOS,
  resolveShortcutKeys,
  shortcutRegistry,
} from "@/lib/shortcutRegistry";

interface KeyboardShortcutsModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface DisplayShortcut {
  id: string;
  label: string;
  description?: string;
  /** Platform-formatted binding, with user rebinds applied */
  keys: string;
}

interface DisplayCategory {
  category: string;
  shortcuts: DisplayShortcut[];
}

export const KeyboardShortcutsModal: FC<KeyboardShortcutsModalProps> = ({
  open,
  onOpenChange,
}) => {
  const [searchQuery, setSearchQuery] = useState("");
  const { settings } = useSettings();

  // Build display data from the registry, substituting user-rebound keys
  // and formatting modifiers for the current platform (⌘ vs Ctrl).
  const allShortcuts = useMemo<DisplayCategory[]>(() => {
    const categories: DisplayCategory[] = [];
    for (const entry of shortcutRegistry) {
      const keys = formatShortcutKeys(
        resolveShortcutKeys(entry, settings?.shortcuts)
      );
      let category = categories.find((c) => c.category === entry.category);
      if (!category) {
        category = { category: entry.category, shortcuts: [] };
        categories.push(category);
      }
      category.shortcuts.push({
        id: entry.id,
        label: entry.label,
        description: entry.description,
        keys,
      });
    }
    return categories;
  }, [settings?.shortcuts]);

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
            shortcut.keys.toLowerCase().includes(query) ||
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
            Quick reference guide for all keyboard shortcuts in DB Hive
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
                aria-label="Clear search"
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
                      {category.shortcuts.map((shortcut) => (
                        <div
                          key={shortcut.id}
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
                            {shortcut.keys}
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
              {isMacOS ? "macOS" : "Windows/Linux"}
            </span>
          </p>
        </div>
      </DialogContent>
    </Dialog>
  );
};
