/**
 * AboutContent — shared body of the About screen.
 *
 * Rendered by both the `/about` route (variant="page") and the About modal
 * in GlobalModals (variant="modal"). Each surface keeps its own chrome
 * (route header/back button vs dialog frame); this component owns the
 * header, features list, credits, links, license, and update-check logic.
 *
 * Third-party credit versions are derived from package.json at build time
 * so they can never drift from the actual dependencies.
 */

import { FC, useState } from "react";
import { APP_VERSION } from "@/version";
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
  Database,
  Github,
  ExternalLink,
  CheckCircle2,
  Sparkles,
  Download,
  Loader2,
} from "lucide-react";
import { toast } from "sonner";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { HiveLogo } from "@/components/HiveLogo";
import pkg from "../../package.json";

const deps: Record<string, string> = pkg.dependencies;

/** Dependency version from package.json with the ^/~ range prefix stripped. */
const depVersion = (name: string) => deps[name]?.replace(/^[\^~]/, "") ?? "-";

const FEATURES = [
  "Multi-database support (PostgreSQL, MySQL, SQLite, MongoDB, SQL Server)",
  "Advanced SQL Editor with syntax highlighting",
  "Visual Query Builder for complex queries",
  "Real-time Schema Browser with ERD visualization",
  "Query History with saved queries",
  "Connection management with SSH tunneling",
  "Data export to multiple formats (CSV, JSON, SQL)",
  "Cross-platform support (Windows, macOS, Linux)",
];

const THIRD_PARTY_CREDITS = [
  {
    name: "Tauri",
    version: depVersion("@tauri-apps/api"),
    description: "Desktop application framework",
  },
  { name: "React", version: depVersion("react"), description: "UI library" },
  {
    name: "Monaco Editor",
    version: depVersion("monaco-editor"),
    description: "Code editor",
  },
  {
    name: "TanStack Table",
    version: depVersion("@tanstack/react-table"),
    description: "Data table virtualization",
  },
  {
    name: "TanStack Router",
    version: depVersion("@tanstack/react-router"),
    description: "Type-safe routing",
  },
  { name: "shadcn/ui", version: "-", description: "UI component library" },
  {
    name: "Lucide React",
    version: depVersion("lucide-react"),
    description: "Icon library",
  },
  {
    name: "Tailwind CSS",
    version: depVersion("tailwindcss"),
    description: "Utility-first CSS framework",
  },
  {
    name: "React Flow",
    version: depVersion("reactflow"),
    description: "ERD visualization",
  },
  {
    name: "Sonner",
    version: depVersion("sonner"),
    description: "Toast notifications",
  },
];

interface AboutContentProps {
  /** "page" = /about route (larger header, roomier spacing); "modal" = About dialog. */
  variant?: "page" | "modal";
}

export const AboutContent: FC<AboutContentProps> = ({ variant = "page" }) => {
  const isPage = variant === "page";
  const [isChecking, setIsChecking] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);

  const cardClass = isPage ? "mb-6" : "mb-4";

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
                console.log(
                  `Downloading update: ${event.data.chunkLength} bytes`
                );
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
            description:
              error instanceof Error
                ? error.message
                : "Failed to install update",
          });
          setIsDownloading(false);
        }
      } else {
        toast.success("You're up to date!", {
          description: `DB Hive ${APP_VERSION} is the latest version.`,
        });
        setIsChecking(false);
      }
    } catch (error) {
      console.error("Failed to check for updates:", error);
      toast.error("Update check failed", {
        description:
          error instanceof Error
            ? error.message
            : "Failed to check for updates",
      });
      setIsChecking(false);
    }
  };

  const openLink = (url: string) => {
    window.open(url, "_blank");
  };

  return (
    <>
      {/* Header Section */}
      <div
        className={`flex flex-col items-center text-center ${
          isPage ? "mb-12" : "mb-8"
        }`}
      >
        <HiveLogo
          size={isPage ? "lg" : "md"}
          className={isPage ? "mb-6" : "mb-4"}
        />

        {/* App Name & Tagline */}
        <h1 className={`font-bold mb-2 ${isPage ? "text-4xl" : "text-3xl"}`}>
          DB Hive
        </h1>
        <p
          className={`text-muted-foreground mb-4 ${
            isPage ? "text-xl" : "text-base"
          }`}
        >
          The Modern Database Desktop App
        </p>
        <div className={`flex items-center ${isPage ? "gap-4" : "gap-3"}`}>
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

      {/* Description Card */}
      <Card className={cardClass}>
        <CardHeader>
          <CardTitle>About DB Hive</CardTitle>
          <CardDescription>
            A modern, powerful, and user-friendly database client
          </CardDescription>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground leading-relaxed">
            DB Hive is a cross-platform database management tool built with
            modern technologies. It provides a seamless experience for
            developers and database administrators to connect, query, and
            manage multiple database systems from a single, intuitive
            interface. Built with Tauri and React, DB Hive combines native
            performance with a beautiful web-based UI.
          </p>
        </CardContent>
      </Card>

      {/* Features Card */}
      <Card className={cardClass}>
        <CardHeader>
          <CardTitle>Key Features</CardTitle>
          <CardDescription>
            Everything you need for database management
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ul className="space-y-2">
            {FEATURES.map((feature, index) => (
              <li key={index} className="flex items-start gap-2">
                <CheckCircle2 className="size-4 mt-0.5 text-primary shrink-0" />
                <span className="text-sm text-muted-foreground">{feature}</span>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>

      {/* Contributors & Team Card */}
      <Card className={cardClass}>
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
      <Card className={cardClass}>
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
      <Card className={cardClass}>
        <CardHeader>
          <CardTitle>Third-Party Credits</CardTitle>
          <CardDescription>Built on the shoulders of giants</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3">
            {THIRD_PARTY_CREDITS.map((credit, index) => (
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
                {index < THIRD_PARTY_CREDITS.length - 1 && (
                  <Separator className="mt-3" />
                )}
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* License Card */}
      <Card className={isPage ? "mb-6" : "mb-2"}>
        <CardHeader>
          <CardTitle>License</CardTitle>
          <CardDescription>Open source and free to use</CardDescription>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            DB Hive is released under the{" "}
            <span className="font-medium text-foreground">MIT License</span>.
          </p>
          <p className="text-sm text-muted-foreground mt-2">
            Copyright © {new Date().getFullYear()} DB Hive Contributors. All
            rights reserved.
          </p>
        </CardContent>
      </Card>

      {/* Footer */}
      <div
        className={`text-center text-sm text-muted-foreground ${
          isPage ? "pt-8 pb-4" : "pt-4"
        }`}
      >
        <p>Made with care for the developer community</p>
      </div>
    </>
  );
};
