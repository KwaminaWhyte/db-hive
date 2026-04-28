import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Settings,
  Globe,
  Palette,
  Zap,
  Keyboard,
  Save,
  RotateCcw,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";
import { Separator } from "./ui/separator";
import { ScrollArea } from "./ui/scroll-area";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";
import type { AppSettings, ShortcutsSettings } from "@/types";
import { defaultSettings } from "@/types";
import { useTheme } from "./theme-provider";
import { broadcastSettingsChanged } from "@/hooks/useSettings";

const IS_MAC =
  typeof navigator !== "undefined" &&
  navigator.platform.toUpperCase().indexOf("MAC") >= 0;

/**
 * Convert a KeyboardEvent into the shortcut string format used by
 * `parseShortcut` in useKeyboardShortcuts (e.g. "Ctrl+Shift+F", "Cmd+K").
 *
 * Returns null if `event.key` is itself a modifier (user still holding mods).
 */
function shortcutFromEvent(event: KeyboardEvent): string | null {
  const key = event.key;
  // Ignore pure modifier keystrokes — wait for a real key
  if (
    key === "Control" ||
    key === "Shift" ||
    key === "Alt" ||
    key === "Meta" ||
    key === "OS" ||
    key === "Dead"
  ) {
    return null;
  }

  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  // Use Cmd on macOS, Meta elsewhere (parser accepts both)
  if (event.metaKey) parts.push(IS_MAC ? "Cmd" : "Meta");
  if (event.altKey) parts.push(IS_MAC ? "Option" : "Alt");
  if (event.shiftKey) parts.push("Shift");

  // Normalize key label
  let label = key;
  if (key === " ") label = "Space";
  else if (key.length === 1) label = key.toUpperCase();
  // Arrow keys / Enter / Escape / Tab / Backspace come through as-is
  parts.push(label);
  return parts.join("+");
}

/**
 * A binding is valid if it has at least one non-Shift modifier OR is a
 * single special key like "?" / "Escape" / function keys.
 */
function isValidShortcut(combo: string): boolean {
  if (!combo) return false;
  const parts = combo.split("+");
  const last = parts[parts.length - 1];
  const mods = parts.slice(0, -1).map((p) => p.toLowerCase());
  const hasNonShiftModifier = mods.some(
    (m) => m === "ctrl" || m === "cmd" || m === "meta" || m === "alt" || m === "option",
  );
  // Single-character keys without a real modifier are not allowed —
  // would block ordinary typing. Allow special keys and "?".
  if (!hasNonShiftModifier) {
    if (last.length === 1 && last !== "?") return false;
  }
  return true;
}

type SettingsSection = "general" | "theme" | "query" | "shortcuts";

export function SettingsPage() {
  const { setTheme } = useTheme();
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [activeSection, setActiveSection] = useState<SettingsSection>("general");
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  // Load settings on mount
  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      setIsLoading(true);
      const loadedSettings = await invoke<AppSettings>("get_settings");
      setSettings(loadedSettings);
    } catch (error) {
      console.error("Failed to load settings:", error);
      toast.error("Failed to load settings");
    } finally {
      setIsLoading(false);
    }
  };

  const saveSettings = async () => {
    try {
      setIsSaving(true);
      await invoke("update_settings", { settings });
      broadcastSettingsChanged();
      toast.success("Settings saved successfully");
    } catch (error) {
      console.error("Failed to save settings:", error);
      toast.error("Failed to save settings");
    } finally {
      setIsSaving(false);
    }
  };

  const resetSettings = async () => {
    try {
      const fresh = await invoke<AppSettings>("reset_settings");
      setSettings(fresh);
      broadcastSettingsChanged();
      toast.success("Settings reset to defaults");
    } catch (error) {
      console.error("Failed to reset settings:", error);
      toast.error("Failed to reset settings");
    }
  };

  /**
   * Update a single shortcut binding and persist immediately so the
   * change applies app-wide without a manual save.
   */
  const updateShortcutBinding = useCallback(
    async (key: keyof ShortcutsSettings, value: string) => {
      const nextSettings: AppSettings = {
        ...settings,
        shortcuts: { ...settings.shortcuts, [key]: value },
      };
      setSettings(nextSettings);
      try {
        await invoke("update_settings", { settings: nextSettings });
        broadcastSettingsChanged();
      } catch (error) {
        console.error("Failed to persist shortcut:", error);
        toast.error("Failed to save shortcut");
      }
    },
    [settings],
  );

  const updateGeneralSettings = (key: keyof AppSettings["general"], value: any) => {
    setSettings((prev) => ({
      ...prev,
      general: { ...prev.general, [key]: value },
    }));
  };

  const updateThemeSettings = (key: keyof AppSettings["theme"], value: any) => {
    setSettings((prev) => ({
      ...prev,
      theme: { ...prev.theme, [key]: value },
    }));

    // Apply theme change immediately if mode is changed
    if (key === "mode") {
      setTheme(value as "light" | "dark" | "system");
    }
  };

  const updateQuerySettings = (key: keyof AppSettings["query"], value: any) => {
    setSettings((prev) => ({
      ...prev,
      query: { ...prev.query, [key]: value },
    }));
  };

  const sections = [
    { id: "general" as const, label: "General", icon: Globe },
    { id: "theme" as const, label: "Appearance", icon: Palette },
    { id: "query" as const, label: "Query Execution", icon: Zap },
    { id: "shortcuts" as const, label: "Keyboard Shortcuts", icon: Keyboard },
  ];

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-muted-foreground">Loading settings...</div>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {/* Sidebar */}
      <div className="w-64 border-r bg-muted/10 p-4">
        <div className="mb-6 flex items-center gap-2">
          <Settings className="size-5" />
          <h2 className="text-lg font-semibold">Settings</h2>
        </div>

        <nav className="space-y-1">
          {sections.map((section) => {
            const Icon = section.icon;
            return (
              <button
                key={section.id}
                onClick={() => setActiveSection(section.id)}
                className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors ${
                  activeSection === section.id
                    ? "bg-primary text-primary-foreground"
                    : "hover:bg-accent hover:text-accent-foreground"
                }`}
              >
                <Icon className="size-4" />
                {section.label}
              </button>
            );
          })}
        </nav>

        <Separator className="my-4" />

        <div className="space-y-2">
          <Button onClick={saveSettings} disabled={isSaving} className="w-full" size="sm">
            <Save className="mr-2 size-4" />
            {isSaving ? "Saving..." : "Save Changes"}
          </Button>

          <Button
            onClick={resetSettings}
            variant="outline"
            className="w-full"
            size="sm"
          >
            <RotateCcw className="mr-2 size-4" />
            Reset to Defaults
          </Button>
        </div>
      </div>

      {/* Content */}
      <ScrollArea className="flex-1">
        <div className="p-8">
          {activeSection === "general" && (
            <div className="space-y-6">
              <div>
                <h3 className="mb-1 text-2xl font-semibold">General Settings</h3>
                <p className="text-muted-foreground text-sm">
                  Configure general application behavior and preferences
                </p>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle>Language</CardTitle>
                  <CardDescription>Choose your preferred language</CardDescription>
                </CardHeader>
                <CardContent>
                  <Select
                    value={settings.general.language}
                    onValueChange={(value) => updateGeneralSettings("language", value)}
                  >
                    <SelectTrigger className="w-full max-w-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="en">English</SelectItem>
                      <SelectItem value="es">Español</SelectItem>
                      <SelectItem value="fr">Français</SelectItem>
                      <SelectItem value="de">Deutsch</SelectItem>
                    </SelectContent>
                  </Select>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Startup Behavior</CardTitle>
                  <CardDescription>What to show when the application starts</CardDescription>
                </CardHeader>
                <CardContent>
                  <Select
                    value={settings.general.startupBehavior}
                    onValueChange={(value) => updateGeneralSettings("startupBehavior", value)}
                  >
                    <SelectTrigger className="w-full max-w-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="showConnectionList">Show Connection List</SelectItem>
                      <SelectItem value="openLastConnection">Open Last Connection</SelectItem>
                      <SelectItem value="openDefaultConnection">Open Default Connection</SelectItem>
                      <SelectItem value="showQueryEditor">Show Query Editor</SelectItem>
                    </SelectContent>
                  </Select>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Auto-Save Connections</CardTitle>
                  <CardDescription>Automatically save connection profiles after testing</CardDescription>
                </CardHeader>
                <CardContent>
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.general.autoSaveConnections}
                      onChange={(e) => updateGeneralSettings("autoSaveConnections", e.target.checked)}
                      className="size-4"
                    />
                    <span className="text-sm">Enable auto-save</span>
                  </label>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Automatic Updates</CardTitle>
                  <CardDescription>Configure automatic update checking and installation</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.general.autoCheckUpdates}
                      onChange={(e) => updateGeneralSettings("autoCheckUpdates", e.target.checked)}
                      className="size-4"
                    />
                    <div>
                      <div className="text-sm font-medium">Check for updates automatically</div>
                      <div className="text-muted-foreground text-xs">
                        Periodically check for new versions and notify you via system notification
                      </div>
                    </div>
                  </label>

                  {settings.general.autoCheckUpdates && (
                    <>
                      <div className="pl-6 space-y-4">
                        <div>
                          <Label>Check interval (hours)</Label>
                          <Input
                            type="number"
                            min="1"
                            max="168"
                            value={settings.general.updateCheckIntervalHours}
                            onChange={(e) => updateGeneralSettings("updateCheckIntervalHours", parseInt(e.target.value))}
                            className="mt-2 max-w-[200px]"
                          />
                          <p className="text-muted-foreground text-xs mt-1">
                            How often to check for updates (1-168 hours)
                          </p>
                        </div>

                        <label className="flex items-center gap-2 cursor-pointer">
                          <input
                            type="checkbox"
                            checked={settings.general.autoDownloadUpdates}
                            onChange={(e) => updateGeneralSettings("autoDownloadUpdates", e.target.checked)}
                            className="size-4"
                          />
                          <div>
                            <div className="text-sm font-medium">Download updates automatically</div>
                            <div className="text-muted-foreground text-xs">
                              Automatically download updates when available
                            </div>
                          </div>
                        </label>

                        {settings.general.autoDownloadUpdates && (
                          <label className="flex items-center gap-2 cursor-pointer pl-6">
                            <input
                              type="checkbox"
                              checked={settings.general.autoInstallUpdates}
                              onChange={(e) => updateGeneralSettings("autoInstallUpdates", e.target.checked)}
                              className="size-4"
                            />
                            <div>
                              <div className="text-sm font-medium">Install updates automatically</div>
                              <div className="text-muted-foreground text-xs">
                                Automatically install downloaded updates and restart (3 second delay)
                              </div>
                            </div>
                          </label>
                        )}
                      </div>
                    </>
                  )}
                </CardContent>
              </Card>
            </div>
          )}

          {activeSection === "theme" && (
            <div className="space-y-6">
              <div>
                <h3 className="mb-1 text-2xl font-semibold">Appearance</h3>
                <p className="text-muted-foreground text-sm">
                  Customize the look and feel of the application
                </p>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle>Theme Mode</CardTitle>
                  <CardDescription>Choose between light, dark, or system theme</CardDescription>
                </CardHeader>
                <CardContent>
                  <Select
                    value={settings.theme.mode}
                    onValueChange={(value) => updateThemeSettings("mode", value)}
                  >
                    <SelectTrigger className="w-full max-w-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="light">Light</SelectItem>
                      <SelectItem value="dark">Dark</SelectItem>
                      <SelectItem value="system">System</SelectItem>
                    </SelectContent>
                  </Select>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Accent Color</CardTitle>
                  <CardDescription>Primary accent color for the UI</CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="flex items-center gap-4">
                    <Input
                      type="color"
                      value={settings.theme.accentColor}
                      onChange={(e) => updateThemeSettings("accentColor", e.target.value)}
                      className="h-10 w-20 cursor-pointer"
                    />
                    <Input
                      type="text"
                      value={settings.theme.accentColor}
                      onChange={(e) => updateThemeSettings("accentColor", e.target.value)}
                      className="max-w-[200px]"
                      placeholder="#f59e0b"
                    />
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Editor Font</CardTitle>
                  <CardDescription>Customize SQL editor font settings</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  <div>
                    <Label>Font Size (px)</Label>
                    <Input
                      type="number"
                      min="10"
                      max="24"
                      value={settings.theme.editorFontSize}
                      onChange={(e) => updateThemeSettings("editorFontSize", parseInt(e.target.value))}
                      className="mt-2 max-w-[200px]"
                    />
                  </div>

                  <div>
                    <Label>Font Family</Label>
                    <Input
                      type="text"
                      value={settings.theme.editorFontFamily}
                      onChange={(e) => updateThemeSettings("editorFontFamily", e.target.value)}
                      className="mt-2"
                      placeholder="Monaco, 'Courier New', monospace"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={settings.theme.editorLineNumbers}
                        onChange={(e) => updateThemeSettings("editorLineNumbers", e.target.checked)}
                        className="size-4"
                      />
                      <span className="text-sm">Show line numbers</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={settings.theme.editorMinimap}
                        onChange={(e) => updateThemeSettings("editorMinimap", e.target.checked)}
                        className="size-4"
                      />
                      <span className="text-sm">Show minimap</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={settings.theme.editorWordWrap}
                        onChange={(e) => updateThemeSettings("editorWordWrap", e.target.checked)}
                        className="size-4"
                      />
                      <span className="text-sm">Word wrap</span>
                    </label>
                  </div>
                </CardContent>
              </Card>
            </div>
          )}

          {activeSection === "query" && (
            <div className="space-y-6">
              <div>
                <h3 className="mb-1 text-2xl font-semibold">Query Execution</h3>
                <p className="text-muted-foreground text-sm">
                  Configure how queries are executed and displayed
                </p>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle>Query Timeout</CardTitle>
                  <CardDescription>Maximum time to wait for query execution (0 = no timeout)</CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      min="0"
                      max="300"
                      value={settings.query.timeoutSeconds}
                      onChange={(e) => updateQuerySettings("timeoutSeconds", parseInt(e.target.value))}
                      className="max-w-[200px]"
                    />
                    <span className="text-muted-foreground text-sm">seconds</span>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Maximum Rows</CardTitle>
                  <CardDescription>Maximum number of rows to fetch per query</CardDescription>
                </CardHeader>
                <CardContent>
                  <Input
                    type="number"
                    min="100"
                    max="100000"
                    step="100"
                    value={settings.query.maxRows}
                    onChange={(e) => updateQuerySettings("maxRows", parseInt(e.target.value))}
                    className="max-w-[200px]"
                  />
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Safety & History</CardTitle>
                  <CardDescription>Configure safety and history options</CardDescription>
                </CardHeader>
                <CardContent className="space-y-3">
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.query.confirmDestructive}
                      onChange={(e) => updateQuerySettings("confirmDestructive", e.target.checked)}
                      className="size-4"
                    />
                    <div>
                      <div className="text-sm font-medium">Confirm destructive queries</div>
                      <div className="text-muted-foreground text-xs">Show confirmation before DELETE or DROP</div>
                    </div>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={settings.query.autoSaveHistory}
                      onChange={(e) => updateQuerySettings("autoSaveHistory", e.target.checked)}
                      className="size-4"
                    />
                    <div>
                      <div className="text-sm font-medium">Auto-save to history</div>
                      <div className="text-muted-foreground text-xs">Automatically save queries to history</div>
                    </div>
                  </label>

                  <div className="pt-2">
                    <Label className="mb-2 block">Max History Entries</Label>
                    <Input
                      type="number"
                      min="50"
                      max="2000"
                      step="50"
                      value={settings.query.maxHistoryEntries}
                      onChange={(e) => updateQuerySettings("maxHistoryEntries", parseInt(e.target.value))}
                      className="max-w-[200px]"
                    />
                  </div>
                </CardContent>
              </Card>
            </div>
          )}

          {activeSection === "shortcuts" && (
            <ShortcutsSection
              shortcuts={settings.shortcuts}
              onChange={updateShortcutBinding}
              onResetAll={resetSettings}
            />
          )}
        </div>
      </ScrollArea>
    </div>
  );
}

interface ShortcutFieldDef {
  label: string;
  field: keyof ShortcutsSettings;
}

const EDITOR_SHORTCUT_FIELDS: ShortcutFieldDef[] = [
  { label: "Execute Query", field: "executeQuery" },
  { label: "Clear Editor", field: "clearEditor" },
  { label: "Format SQL", field: "formatSql" },
  { label: "Save Snippet", field: "saveSnippet" },
];

const NAVIGATION_SHORTCUT_FIELDS: ShortcutFieldDef[] = [
  { label: "New Tab", field: "newTab" },
  { label: "Close Tab", field: "closeTab" },
  { label: "Toggle Sidebar", field: "toggleSidebar" },
  { label: "Search", field: "search" },
  { label: "Open Settings", field: "openSettings" },
  { label: "Show Shortcuts", field: "showShortcuts" },
];

function ShortcutsSection({
  shortcuts,
  onChange,
  onResetAll,
}: {
  shortcuts: ShortcutsSettings;
  onChange: (key: keyof ShortcutsSettings, value: string) => Promise<void> | void;
  onResetAll: () => Promise<void> | void;
}) {
  const [confirmReset, setConfirmReset] = useState(false);

  // Lookup current binding -> list of field labels using it (for duplicate warning)
  const findDuplicates = useCallback(
    (newValue: string, ownField: keyof ShortcutsSettings): string[] => {
      if (!newValue) return [];
      const dupes: string[] = [];
      const allFields = [...EDITOR_SHORTCUT_FIELDS, ...NAVIGATION_SHORTCUT_FIELDS];
      for (const f of allFields) {
        if (f.field === ownField) continue;
        if (
          shortcuts[f.field] &&
          shortcuts[f.field].toLowerCase() === newValue.toLowerCase()
        ) {
          dupes.push(f.label);
        }
      }
      return dupes;
    },
    [shortcuts],
  );

  const handleChange = useCallback(
    (field: keyof ShortcutsSettings, value: string) => {
      const dupes = findDuplicates(value, field);
      if (dupes.length > 0) {
        toast.warning(
          `"${value}" is already bound to: ${dupes.join(", ")}`,
        );
      }
      void onChange(field, value);
    },
    [findDuplicates, onChange],
  );

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h3 className="mb-1 text-2xl font-semibold">Keyboard Shortcuts</h3>
          <p className="text-muted-foreground text-sm">
            View and customize keyboard shortcuts (click any shortcut to rebind)
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setConfirmReset(true)}
          className="shrink-0"
        >
          <RotateCcw className="mr-2 size-4" />
          Reset all to defaults
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Editor Shortcuts</CardTitle>
          <CardDescription>
            Bindings active inside the SQL editor and query view
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            {EDITOR_SHORTCUT_FIELDS.map((f) => (
              <ShortcutRow
                key={f.field}
                label={f.label}
                value={shortcuts[f.field]}
                onChange={(v) => handleChange(f.field, v)}
              />
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Navigation Shortcuts</CardTitle>
          <CardDescription>
            Global navigation and window-level bindings
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            {NAVIGATION_SHORTCUT_FIELDS.map((f) => (
              <ShortcutRow
                key={f.field}
                label={f.label}
                value={shortcuts[f.field]}
                onChange={(v) => handleChange(f.field, v)}
              />
            ))}
          </div>
        </CardContent>
      </Card>

      <AlertDialog open={confirmReset} onOpenChange={setConfirmReset}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Reset all settings to defaults?</AlertDialogTitle>
            <AlertDialogDescription>
              This restores every setting (not only shortcuts) to its default
              value. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmReset(false);
                void onResetAll();
              }}
            >
              Reset
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ShortcutRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // Capture the next valid keystroke while in recording mode
  useEffect(() => {
    if (!recording) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      // Always swallow keys while recording so they don't fall through
      // to other shortcut handlers.
      event.preventDefault();
      event.stopPropagation();

      // Escape cancels recording without changing the binding
      if (event.key === "Escape") {
        setRecording(false);
        return;
      }

      // Backspace clears the binding
      if (event.key === "Backspace" && !event.ctrlKey && !event.metaKey && !event.altKey) {
        onChange("");
        setRecording(false);
        return;
      }

      const combo = shortcutFromEvent(event);
      if (!combo) return; // still holding modifiers; wait for real key

      if (!isValidShortcut(combo)) {
        toast.error(
          "Invalid shortcut: needs at least one modifier (Ctrl, Cmd, Alt) for letter keys",
        );
        return; // stay in recording mode
      }

      onChange(combo);
      setRecording(false);
    };

    // Capture phase so we beat global hooks (which use bubble phase)
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [recording, onChange]);

  // Auto-blur the button so the global "input/textarea" guard in the
  // shortcut hooks does not need any change
  useEffect(() => {
    if (recording && buttonRef.current) {
      buttonRef.current.focus();
    }
  }, [recording]);

  const display = value || "(unbound)";

  return (
    <div className="flex items-center justify-between gap-3 py-2">
      <span className="text-sm">{label}</span>
      <div className="flex items-center gap-2">
        <button
          ref={buttonRef}
          type="button"
          onClick={() => setRecording((r) => !r)}
          onBlur={() => setRecording(false)}
          className={`rounded border px-2 py-1 text-xs font-mono min-w-[110px] text-center transition-colors ${
            recording
              ? "border-primary bg-primary/10 text-primary animate-pulse"
              : value
              ? "bg-muted border-border hover:border-primary/40"
              : "bg-muted/50 border-dashed border-border text-muted-foreground hover:border-primary/40"
          }`}
          aria-label={`Edit shortcut for ${label}`}
        >
          {recording ? "Press shortcut…" : display}
        </button>
        {value && !recording && (
          <button
            type="button"
            onClick={() => onChange("")}
            className="text-xs text-muted-foreground hover:text-destructive transition-colors"
            aria-label={`Clear shortcut for ${label}`}
            title="Clear binding"
          >
            clear
          </button>
        )}
      </div>
    </div>
  );
}
