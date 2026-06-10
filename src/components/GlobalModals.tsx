/**
 * Global modal overlays mounted at the app root.
 *
 * Renders Settings / About / Plugins inside shadcn Dialogs instead of
 * navigating to full-page routes, so the underlying route (e.g. the SQL
 * query editor) keeps its state while the user views these screens.
 *
 * Driven by the `useAppModal` store. Existing routes (`/settings`, `/about`,
 * `/plugins`) are intentionally left intact for URL deep-linking.
 */

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Package, Store } from "lucide-react";
import { AboutContent } from "@/components/AboutContent";
import { SettingsPage } from "@/components/SettingsPage";
import { InstalledPlugins } from "@/components/InstalledPlugins";
import { PluginMarketplace } from "@/components/PluginMarketplace";
import { MigrationsDialog } from "@/components/MigrationsDialog";
import { BackupManagerDialog } from "@/components/BackupManagerDialog";
import { useAppModal, setAppModal } from "@/store/useAppModal";

export function GlobalModals() {
  const modal = useAppModal();

  const handleOpenChange = (open: boolean) => {
    if (!open) setAppModal(null);
  };

  return (
    <>
      {/* Settings Modal */}
      <Dialog open={modal === "settings"} onOpenChange={handleOpenChange}>
        <DialogContent
          className="sm:max-w-5xl max-w-[calc(100%-2rem)] p-0 gap-0 overflow-hidden"
          // Settings page manages its own header/padding
        >
          <DialogHeader className="sr-only">
            <DialogTitle>Settings</DialogTitle>
            <DialogDescription>Application settings</DialogDescription>
          </DialogHeader>
          <div className="h-[85vh] overflow-y-auto">
            <SettingsPage />
          </div>
        </DialogContent>
      </Dialog>

      {/* Plugins Modal */}
      <Dialog open={modal === "plugins"} onOpenChange={handleOpenChange}>
        <DialogContent className="sm:max-w-5xl max-w-[calc(100%-2rem)] p-0 gap-0 overflow-hidden">
          <DialogHeader className="px-6 pt-6 pb-4 border-b">
            <DialogTitle>Plugin Manager</DialogTitle>
            <DialogDescription>Extend DB Hive with plugins</DialogDescription>
          </DialogHeader>
          <PluginsModalBody />
        </DialogContent>
      </Dialog>

      {/* Schema Migrations Modal */}
      <MigrationsDialog
        open={modal === "migrations"}
        onOpenChange={handleOpenChange}
      />

      {/* Backup Manager Modal */}
      <BackupManagerDialog
        open={modal === "backup"}
        onOpenChange={handleOpenChange}
      />

      {/* About Modal */}
      <Dialog open={modal === "about"} onOpenChange={handleOpenChange}>
        <DialogContent className="sm:max-w-3xl max-w-[calc(100%-2rem)] p-0 gap-0 overflow-hidden">
          <DialogHeader className="sr-only">
            <DialogTitle>About DB Hive</DialogTitle>
            <DialogDescription>Application information</DialogDescription>
          </DialogHeader>
          <div className="max-h-[85vh] overflow-y-auto">
            <div className="px-8 py-8">
              <AboutContent variant="modal" />
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

function PluginsModalBody() {
  const [activeTab, setActiveTab] = useState<"marketplace" | "installed">(
    "marketplace"
  );

  return (
    <Tabs
      value={activeTab}
      onValueChange={(v) => setActiveTab(v as "marketplace" | "installed")}
      className="flex flex-col h-[80vh]"
    >
      <TabsList className="px-6 py-2 justify-start mx-6 mt-4">
        <TabsTrigger value="marketplace" className="gap-2">
          <Store className="size-4" />
          Marketplace
        </TabsTrigger>
        <TabsTrigger value="installed" className="gap-2">
          <Package className="size-4" />
          Installed
        </TabsTrigger>
      </TabsList>

      <TabsContent value="marketplace" className="flex-1 m-0 overflow-auto">
        <PluginMarketplace />
      </TabsContent>

      <TabsContent value="installed" className="flex-1 m-0 overflow-auto">
        <InstalledPlugins />
      </TabsContent>
    </Tabs>
  );
}
