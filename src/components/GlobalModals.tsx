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
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Database,
  Github,
  ExternalLink,
  CheckCircle2,
  Sparkles,
  Download,
  Loader2,
  Package,
  Store,
} from "lucide-react";
import { toast } from "sonner";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { SettingsPage } from "@/components/SettingsPage";
import { InstalledPlugins } from "@/components/InstalledPlugins";
import { PluginMarketplace } from "@/components/PluginMarketplace";
import { MigrationsDialog } from "@/components/MigrationsDialog";
import { BackupManagerDialog } from "@/components/BackupManagerDialog";
import { useAppModal, setAppModal } from "@/store/useAppModal";

const APP_VERSION = "0.19.5-beta";

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
            <DialogDescription>Extend DB-Hive with plugins</DialogDescription>
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
            <DialogTitle>About DB-Hive</DialogTitle>
            <DialogDescription>Application information</DialogDescription>
          </DialogHeader>
          <div className="max-h-[85vh] overflow-y-auto">
            <AboutModalBody />
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

function AboutModalBody() {
  const [isChecking, setIsChecking] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);

  const handleCheckForUpdates = async () => {
    if (isChecking || isDownloading) return;

    setIsChecking(true);
    try {
      const update = await check();

      if (update?.available) {
        toast.info(`Update available: ${update.version}`, {
          description: "Downloading update...",
          duration: 5000,
        });

        setIsChecking(false);
        setIsDownloading(true);

        try {
          await update.downloadAndInstall(() => {});
          toast.success("Update installed!", {
            description: "Restarting application...",
            duration: 3000,
          });
          setTimeout(async () => {
            await relaunch();
          }, 2000);
        } catch (error) {
          toast.error("Update failed", {
            description:
              error instanceof Error
                ? error.message
                : "Failed to install update",
          });
          setIsDownloading(false);
        }
      } else {
        toast.success("You're up to date!", {
          description: `DB-Hive ${APP_VERSION} is the latest version.`,
        });
        setIsChecking(false);
      }
    } catch (error) {
      toast.error("Update check failed", {
        description:
          error instanceof Error
            ? error.message
            : "Failed to check for updates",
      });
      setIsChecking(false);
    }
  };

  const openLink = (url: string) => window.open(url, "_blank");

  const features = [
    "Multi-database support (PostgreSQL, MySQL, SQLite, MongoDB, SQL Server)",
    "Advanced SQL Editor with syntax highlighting",
    "Visual Query Builder for complex queries",
    "Real-time Schema Browser with ERD visualization",
    "Query History with saved queries",
    "Connection management with SSH tunneling",
    "Data export to multiple formats (CSV, JSON, SQL)",
    "Cross-platform support (Windows, macOS, Linux)",
  ];

  const thirdPartyCredits = [
    { name: "Tauri", version: "2.0", description: "Desktop application framework" },
    { name: "React", version: "19", description: "UI library" },
    { name: "Monaco Editor", version: "0.54.0", description: "Code editor" },
    { name: "TanStack Table", version: "8.21.3", description: "Data table virtualization" },
    { name: "TanStack Router", version: "1.139.0", description: "Type-safe routing" },
    { name: "shadcn/ui", version: "-", description: "UI component library" },
    { name: "Lucide React", version: "0.554.0", description: "Icon library" },
    { name: "Tailwind CSS", version: "4.1.17", description: "Utility-first CSS framework" },
    { name: "React Flow", version: "11.11.4", description: "ERD visualization" },
    { name: "Sonner", version: "2.0.7", description: "Toast notifications" },
  ];

  return (
    <div className="px-8 py-8">
      {/* Header */}
      <div className="flex flex-col items-center text-center mb-8">
        <div className="relative h-20 w-20 mb-4 rounded-xl border-2 border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30">
          <div className="absolute inset-[12%] rounded-lg bg-slate-950/90 border-2 border-amber-200/30 flex items-center justify-center">
            <div className="relative h-full w-full flex items-center justify-center">
              <div className="absolute h-[65%] w-[65%] border-2 border-amber-300/40 rounded-md rotate-6"></div>
              <div className="absolute h-[65%] w-[65%] border-2 border-amber-200/50 rounded-md -rotate-6"></div>
              <div className="relative h-[50%] w-[50%] rounded-md bg-gradient-to-br from-amber-300 via-amber-400 to-amber-500 flex flex-col justify-center items-center gap-[3px]">
                <div className="w-[65%] h-[2.5px] rounded-full bg-slate-950/85"></div>
                <div className="w-[55%] h-[2.5px] rounded-full bg-slate-950/85"></div>
                <div className="w-[45%] h-[2.5px] rounded-full bg-slate-950/85"></div>
              </div>
            </div>
          </div>
        </div>

        <h1 className="text-3xl font-bold mb-2">DB-Hive</h1>
        <p className="text-base text-muted-foreground mb-4">
          A Professional Cross-Platform Database Client
        </p>
        <div className="flex items-center gap-3">
          <span className="text-sm font-medium px-3 py-1 rounded-full bg-primary/10 text-primary">
            Version {APP_VERSION}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={handleCheckForUpdates}
            disabled={isChecking || isDownloading}
          >
            {isChecking ? (
              <>
                <Loader2 className="size-4 mr-2 animate-spin" />
                Checking...
              </>
            ) : isDownloading ? (
              <>
                <Download className="size-4 mr-2" />
                Downloading...
              </>
            ) : (
              <>
                <CheckCircle2 className="size-4 mr-2" />
                Check for Updates
              </>
            )}
          </Button>
        </div>
      </div>

      <Card className="mb-4">
        <CardHeader>
          <CardTitle>About DB-Hive</CardTitle>
          <CardDescription>
            A modern, powerful, and user-friendly database client
          </CardDescription>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground leading-relaxed text-sm">
            DB-Hive is a cross-platform database management tool built with
            modern technologies. It provides a seamless experience for
            developers and database administrators to connect, query, and
            manage multiple database systems from a single, intuitive
            interface.
          </p>
        </CardContent>
      </Card>

      <Card className="mb-4">
        <CardHeader>
          <CardTitle>Key Features</CardTitle>
        </CardHeader>
        <CardContent>
          <ul className="space-y-2">
            {features.map((feature, index) => (
              <li key={index} className="flex items-start gap-2">
                <CheckCircle2 className="size-4 mt-0.5 text-primary shrink-0" />
                <span className="text-sm text-muted-foreground">{feature}</span>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>

      <Card className="mb-4">
        <CardHeader>
          <CardTitle>Contributors</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/50 border border-border">
              <div className="flex items-center justify-center size-10 rounded-lg bg-gradient-to-br from-orange-500 to-pink-500">
                <Sparkles className="size-5 text-white" />
              </div>
              <div className="flex-1">
                <p className="font-medium">Built with Claude Code</p>
                <p className="text-sm text-muted-foreground">
                  AI-powered development by Anthropic's Claude
                </p>
              </div>
            </div>
            <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/50 border border-border">
              <div className="flex items-center justify-center size-10 rounded-lg bg-muted">
                <Github className="size-5" />
              </div>
              <div className="flex-1">
                <p className="font-medium">Open Source on GitHub</p>
                <p className="text-sm text-muted-foreground">
                  Community-driven development
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  openLink("https://github.com/KwaminaWhyte/db-hive")
                }
              >
                <ExternalLink className="size-4" />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card className="mb-4">
        <CardHeader>
          <CardTitle>Resources & Links</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            <Button
              variant="ghost"
              className="w-full justify-start"
              onClick={() => openLink("https://github.com/KwaminaWhyte/db-hive")}
            >
              <Github className="size-4 mr-3" />
              GitHub Repository
              <ExternalLink className="size-3 ml-auto" />
            </Button>
            <Button
              variant="ghost"
              className="w-full justify-start"
              onClick={() =>
                openLink("https://github.com/KwaminaWhyte/db-hive/wiki")
              }
            >
              <Database className="size-4 mr-3" />
              Documentation
              <ExternalLink className="size-3 ml-auto" />
            </Button>
            <Button
              variant="ghost"
              className="w-full justify-start"
              onClick={() =>
                openLink("https://github.com/KwaminaWhyte/db-hive/issues")
              }
            >
              <Github className="size-4 mr-3" />
              Report Issues
              <ExternalLink className="size-3 ml-auto" />
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card className="mb-4">
        <CardHeader>
          <CardTitle>Third-Party Credits</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3">
            {thirdPartyCredits.map((credit, index) => (
              <div key={index}>
                <div className="flex items-center justify-between">
                  <div>
                    <p className="font-medium text-sm">{credit.name}</p>
                    <p className="text-xs text-muted-foreground">
                      {credit.description}
                    </p>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {credit.version}
                  </span>
                </div>
                {index < thirdPartyCredits.length - 1 && (
                  <Separator className="mt-3" />
                )}
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card className="mb-2">
        <CardHeader>
          <CardTitle>License</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            DB-Hive is released under the{" "}
            <span className="font-medium text-foreground">MIT License</span>.
          </p>
          <p className="text-sm text-muted-foreground mt-2">
            Copyright © {new Date().getFullYear()} DB-Hive Contributors.
          </p>
        </CardContent>
      </Card>

      <div className="text-center text-sm text-muted-foreground pt-4">
        <p>Made with care for the developer community</p>
      </div>
    </div>
  );
}
