import { Outlet, createRootRoute } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import { ConnectionProvider, useConnectionContext } from "@/contexts/ConnectionContext";
import { TabProvider } from "@/contexts/TabContext";
import { CustomTitlebar } from "@/components/CustomTitlebar";
import { Toaster } from "sonner";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { setupWindowStatePersistence } from "@/utils/windowState";
import { openDatabaseWindow } from "@/utils/multiWindow";
import { KeyboardShortcutsModal } from "@/components/KeyboardShortcutsModal";
import { useSettings } from "@/hooks/useSettings";
import { useAutoUpdater } from "@/hooks/useAutoUpdater";
import { UpdateBanner } from "@/components/UpdateBanner";
import { CommandPalette } from "@/components/CommandPalette";
import { GlobalModals } from "@/components/GlobalModals";
import { openAppModal } from "@/store/useAppModal";
import { useEffect, useState } from "react";

function RootComponent() {
  const { theme } = useTheme();
  const [showShortcutsModal, setShowShortcutsModal] = useState(false);
  const [showCommandPalette, setShowCommandPalette] = useState(false);

  const isMacOS = navigator.userAgent.includes("Mac");

  // Global Cmd+K / Ctrl+K shortcut to toggle the command palette
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isModK =
        e.key.toLowerCase() === "k" && (isMacOS ? e.metaKey : e.ctrlKey);
      if (isModK) {
        e.preventDefault();
        e.stopPropagation();
        setShowCommandPalette((prev) => !prev);
      }
    };
    // Use capture phase to intercept before route-level handlers
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [isMacOS]);

  // Load application settings
  const { settings } = useSettings();

  // Setup automatic update checking with in-app banner + system notifications
  const updater = useAutoUpdater({
    enabled: settings?.general.autoCheckUpdates ?? true,
    autoDownload: settings?.general.autoDownloadUpdates ?? false,
    autoInstall: settings?.general.autoInstallUpdates ?? false,
    checkIntervalHours: settings?.general.updateCheckIntervalHours ?? 24,
  });

  // Setup window state persistence
  useEffect(() => {
    const cleanup = setupWindowStatePersistence();
    return cleanup;
  }, []);

  // Global keyboard shortcuts (sourced from user settings; re-bind on change)
  const openSettingsKey = settings?.shortcuts?.openSettings ?? "Ctrl+,";
  const showShortcutsKey = settings?.shortcuts?.showShortcuts ?? "?";
  useKeyboardShortcuts([
    {
      key: openSettingsKey,
      handler: () => {
        openAppModal("settings");
      },
      description: "Open settings",
    },
    {
      key: showShortcutsKey,
      handler: () => {
        setShowShortcutsModal(true);
      },
      description: "Show keyboard shortcuts",
    },
    {
      key: "⌘+⇧+N",
      handler: () => {
        openDatabaseWindow();
      },
      description: "Open new window",
    },
    {
      key: "Ctrl+⇧+N",
      handler: () => {
        openDatabaseWindow();
      },
      description: "Open new window",
    },
  ]);

  return (
    <div className="flex flex-col h-screen w-full bg-background">
      {/* Custom Titlebar */}
      <CustomTitlebar
        onShowShortcuts={() => setShowShortcutsModal(true)}
        onOpenCommandPalette={() => setShowCommandPalette(true)}
      />

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        <Outlet />
      </div>

      {/* Keyboard Shortcuts Modal */}
      <KeyboardShortcutsModal
        open={showShortcutsModal}
        onOpenChange={setShowShortcutsModal}
      />

      {/* Global overlay modals (Settings / About / Plugins) */}
      <GlobalModals />

      {/* Command Palette */}
      <CommandPalette
        open={showCommandPalette}
        onOpenChange={setShowCommandPalette}
        onShowShortcuts={() => {
          setShowCommandPalette(false);
          setShowShortcutsModal(true);
        }}
      />

      {/* In-app Update Banner */}
      <UpdateBanner
        status={updater.status}
        dismissed={updater.dismissed}
        onDownload={updater.downloadAndInstall}
        onRestart={updater.restartApp}
        onDismiss={updater.dismiss}
        onRetry={updater.checkForUpdates}
      />

      <Toaster
        richColors
        position="bottom-right"
        theme={theme === "system" ? undefined : (theme as "light" | "dark")}
      />
      {import.meta.env.DEV && <TanStackRouterDevtools />}
    </div>
  );
}

function WithTabProvider() {
  const { connectionId, currentDatabase } = useConnectionContext();

  return (
    <TabProvider connectionId={connectionId} currentDatabase={currentDatabase}>
      <RootComponent />
    </TabProvider>
  );
}

export const Route = createRootRoute({
  component: () => (
    <ThemeProvider defaultTheme="dark" storageKey="db-hive-theme">
      <ConnectionProvider>
        <WithTabProvider />
      </ConnectionProvider>
    </ThemeProvider>
  ),
});
