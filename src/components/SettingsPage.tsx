import { useEffect, useState } from "react";
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
import type { AppSettings } from "@/types";
import { defaultSettings } from "@/types";
import { useTheme } from "./theme-provider";

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
      const defaultSettings = await invoke<AppSettings>("reset_settings");
      setSettings(defaultSettings);
      toast.success("Settings reset to defaults");
    } catch (error) {
      console.error("Failed to reset settings:", error);
      toast.error("Failed to reset settings");
    }
  };

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
            <div className="space-y-6">
              <div>
                <h3 className="mb-1 text-2xl font-semibold">Keyboard Shortcuts</h3>
                <p className="text-muted-foreground text-sm">
                  View and customize keyboard shortcuts (customization coming soon)
                </p>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle>Editor Shortcuts</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="space-y-3">
                    <ShortcutRow label="Execute Query" value={settings.shortcuts.executeQuery} />
                    <ShortcutRow label="Clear Editor" value={settings.shortcuts.clearEditor} />
                    <ShortcutRow label="Format SQL" value={settings.shortcuts.formatSql} />
                    <ShortcutRow label="Save Snippet" value={settings.shortcuts.saveSnippet} />
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Navigation Shortcuts</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="space-y-3">
                    <ShortcutRow label="New Tab" value={settings.shortcuts.newTab} />
                    <ShortcutRow label="Close Tab" value={settings.shortcuts.closeTab} />
                    <ShortcutRow label="Toggle Sidebar" value={settings.shortcuts.toggleSidebar} />
                    <ShortcutRow label="Search" value={settings.shortcuts.search} />
                    <ShortcutRow label="Open Settings" value={settings.shortcuts.openSettings} />
                    <ShortcutRow label="Show Shortcuts" value={settings.shortcuts.showShortcuts} />
                  </div>
                </CardContent>
              </Card>
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  );
}

function ShortcutRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between py-2">
      <span className="text-sm">{label}</span>
      <kbd className="bg-muted border-border rounded border px-2 py-1 text-xs font-mono">
        {value}
      </kbd>
    </div>
  );
}
