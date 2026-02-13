import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import {
  ArrowLeft,
  Database,
  Github,
  ExternalLink,
  CheckCircle2,
  Sparkles,
  Download,
  Loader2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export const Route = createFileRoute("/about")({
  component: AboutRoute,
});

function AboutRoute() {
  const navigate = useNavigate({ from: "/about" });
  const [previousRoute, setPreviousRoute] = useState<string | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const version = "0.18.0-beta";

  // Store the previous route before navigating to about
  useEffect(() => {
    const saved = sessionStorage.getItem("db-hive-previous-route");
    if (saved) {
      setPreviousRoute(saved);
      // Clear after reading
      sessionStorage.removeItem("db-hive-previous-route");
    }
  }, []);

  const handleBack = () => {
    // If we have a saved previous route, navigate there
    if (previousRoute) {
      // Handle routes with dynamic segments or search params
      if (previousRoute.startsWith("/query")) {
        navigate({ to: "/query" as any });
      } else if (previousRoute.startsWith("/settings")) {
        navigate({ to: "/settings" });
      } else if (previousRoute.startsWith("/er-diagram")) {
        navigate({ to: previousRoute as any });
      } else if (previousRoute.startsWith("/activity")) {
        navigate({ to: "/activity" as any });
      } else if (previousRoute.startsWith("/visual-query")) {
        navigate({ to: "/visual-query" as any });
      } else if (previousRoute.startsWith("/plugins")) {
        navigate({ to: "/plugins" });
      } else {
        navigate({ to: "/" });
      }
    } else if (window.history.length > 1) {
      // Try browser history if no saved route
      window.history.back();
    } else {
      // Fallback to home
      navigate({ to: "/" });
    }
  };

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
          await update.downloadAndInstall((event) => {
            switch (event.event) {
              case "Started":
                console.log("Update download started");
                break;
              case "Progress":
                console.log(`Downloading update: ${event.data.chunkLength} bytes`);
                break;
              case "Finished":
                console.log("Download finished");
                break;
            }
          });

          toast.success("Update installed!", {
            description: "Restarting application...",
            duration: 3000,
          });

          // Relaunch the app after a short delay
          setTimeout(async () => {
            await relaunch();
          }, 2000);
        } catch (error) {
          console.error("Failed to download and install update:", error);
          toast.error("Update failed", {
            description: error instanceof Error ? error.message : "Failed to install update",
          });
          setIsDownloading(false);
        }
      } else {
        toast.success("You're up to date!", {
          description: `DB-Hive ${version} is the latest version.`,
        });
        setIsChecking(false);
      }
    } catch (error) {
      console.error("Failed to check for updates:", error);
      toast.error("Update check failed", {
        description: error instanceof Error ? error.message : "Failed to check for updates",
      });
      setIsChecking(false);
    }
  };

  const openLink = (url: string) => {
    window.open(url, "_blank");
  };

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
    <div className="flex-1 h-full relative bg-background">
      {/* Back Button */}
      <div className="absolute top-4 left-4 z-50">
        <Button variant="outline" size="sm" onClick={handleBack}>
          <ArrowLeft className="size-4 mr-2" />
          Back
        </Button>
      </div>

      {/* Main Content */}
      <div className="h-full overflow-y-auto">
        <div className="max-w-4xl mx-auto px-8 py-12">
          {/* Header Section */}
          <div className="flex flex-col items-center text-center mb-12">
            {/* Logo */}
            <div className="relative h-24 w-24 mb-6 rounded-xl border-2 border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30">
              <div className="absolute inset-[12%] rounded-lg bg-slate-950/90 dark:bg-slate-950/90 border-2 border-amber-200/30 flex items-center justify-center">
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

            {/* App Name & Tagline */}
            <h1 className="text-4xl font-bold mb-2">DB-Hive</h1>
            <p className="text-xl text-muted-foreground mb-4">
              A Professional Cross-Platform Database Client
            </p>
            <div className="flex items-center gap-4">
              <span className="text-sm font-medium px-3 py-1 rounded-full bg-primary/10 text-primary">
                Version {version}
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

          {/* Description Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>About DB-Hive</CardTitle>
              <CardDescription>
                A modern, powerful, and user-friendly database client
              </CardDescription>
            </CardHeader>
            <CardContent>
              <p className="text-muted-foreground leading-relaxed">
                DB-Hive is a cross-platform database management tool built with
                modern technologies. It provides a seamless experience for
                developers and database administrators to connect, query, and
                manage multiple database systems from a single, intuitive
                interface. Built with Tauri and React, DB-Hive combines native
                performance with a beautiful web-based UI.
              </p>
            </CardContent>
          </Card>

          {/* Features Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>Key Features</CardTitle>
              <CardDescription>
                Everything you need for database management
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ul className="space-y-2">
                {features.map((feature, index) => (
                  <li key={index} className="flex items-start gap-2">
                    <CheckCircle2 className="size-4 mt-0.5 text-primary shrink-0" />
                    <span className="text-sm text-muted-foreground">
                      {feature}
                    </span>
                  </li>
                ))}
              </ul>
            </CardContent>
          </Card>

          {/* Contributors & Team Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>Contributors & Core Team</CardTitle>
              <CardDescription>Built with passion and AI</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                {/* Claude Code Badge */}
                <div className="flex items-center gap-3 p-4 rounded-lg bg-muted/50 border border-border">
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

                {/* GitHub Repository */}
                <div className="flex items-center gap-3 p-4 rounded-lg bg-muted/50 border border-border">
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

          {/* Links Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>Resources & Links</CardTitle>
              <CardDescription>Get help and stay connected</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-2">
                <Button
                  variant="ghost"
                  className="w-full justify-start"
                  onClick={() =>
                    openLink("https://github.com/KwaminaWhyte/db-hive")
                  }
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

          {/* Third-Party Credits Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>Third-Party Credits</CardTitle>
              <CardDescription>
                Built on the shoulders of giants
              </CardDescription>
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

          {/* License Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>License</CardTitle>
              <CardDescription>Open source and free to use</CardDescription>
            </CardHeader>
            <CardContent>
              <p className="text-sm text-muted-foreground">
                DB-Hive is released under the{" "}
                <span className="font-medium text-foreground">MIT License</span>
                .
              </p>
              <p className="text-sm text-muted-foreground mt-2">
                Copyright © {new Date().getFullYear()} DB-Hive Contributors. All
                rights reserved.
              </p>
            </CardContent>
          </Card>

          {/* Footer */}
          <div className="text-center text-sm text-muted-foreground pt-8 pb-4">
            <p>Made with care for the developer community</p>
          </div>
        </div>
      </div>
    </div>
  );
}
