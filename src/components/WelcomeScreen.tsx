import { FC } from "react";
import { Button } from "./ui/button";
import {
  Database,
  Clock,
  BookOpen,
  Github,
  Command,
  Terminal,
  LayoutPanelLeft,
  Plug2,
} from "lucide-react";

interface WelcomeScreenProps {
  onNewConnection: () => void;
  onRecentConnections?: () => void;
  onViewSample?: () => void;
  onOpenDocs?: () => void;
}

export const HiveLogo: FC<{ className?: string }> = ({ className = "" }) => (
  <div className={`relative ${className}`}>
    {/* Geometric honey DB Hive logo */}
    <div className="relative h-24 w-24 sm:h-28 sm:w-28 md:h-32 md:w-32 rounded-2xl border border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30 shadow-[0_0_0_1px_rgba(15,23,42,0.8),0_18px_45px_rgba(0,0,0,0.85)] overflow-hidden">
      {/* Inner hex / DB symbol */}
      <div className="absolute inset-[18%] rounded-[1.15rem] bg-slate-950/90 dark:bg-slate-950/90 border border-amber-200/30 shadow-[0_0_0_1px_rgba(15,23,42,0.9),0_0_40px_rgba(251,191,36,0.45)] flex items-center justify-center">
        {/* Layered hex stack */}
        <div className="relative h-full w-full flex items-center justify-center">
          <div className="absolute h-[78%] w-[78%] border border-amber-300/40 rounded-[1rem] rotate-6"></div>
          <div className="absolute h-[78%] w-[78%] border border-amber-200/50 rounded-[1rem] -rotate-6"></div>
          <div className="relative h-[64%] w-[64%] rounded-[0.85rem] bg-gradient-to-br from-amber-300 via-amber-400 to-amber-500 shadow-[0_12px_30px_rgba(251,191,36,0.65)] flex flex-col justify-center items-center gap-0.5">
            {/* DB stripes hinting a hive */}
            <div className="w-[72%] h-[0.2rem] rounded-full bg-slate-950/85"></div>
            <div className="w-[64%] h-[0.2rem] rounded-full bg-slate-950/85"></div>
            <div className="w-[56%] h-[0.2rem] rounded-full bg-slate-950/85"></div>
          </div>
        </div>

        {/* Subtle bee wing arcs */}
        <div className="absolute -left-3 top-1/2 -translate-y-1/2 h-10 w-6 rounded-full border border-amber-200/35 transform translate-x-[-0.15rem] translate-y-[-0.25rem] rotate-[-18deg]"></div>
        <div className="absolute -right-3 top-1/2 -translate-y-1/2 h-10 w-6 rounded-full border border-amber-200/35 transform translate-x-[0.15rem] translate-y-[-0.25rem] rotate-[18deg]"></div>
      </div>

      {/* Glow ring */}
      <div className="pointer-events-none absolute -inset-6 rounded-[1.75rem] bg-[radial-gradient(circle_at_30%_0%,rgba(255,255,255,0.3),transparent_55%),radial-gradient(circle_at_80%_120%,rgba(251,191,36,0.55),transparent_60%)] opacity-80 mix-blend-screen"></div>
    </div>

    {/* Mini hex clusters */}
    <div className="pointer-events-none absolute -right-6 -top-2 opacity-80">
      <div className="h-4 w-4 rounded-md border border-amber-200/60 bg-amber-300/25"></div>
      <div className="h-3 w-3 rounded-md border border-amber-200/50 bg-amber-300/15 translate-x-3 -translate-y-1"></div>
    </div>
    <div className="pointer-events-none absolute -left-4 -bottom-3 opacity-70">
      <div className="h-3 w-3 rounded-md border border-amber-200/40 bg-amber-300/15"></div>
      <div className="h-2.5 w-2.5 rounded-md border border-amber-200/30 bg-amber-300/10 translate-x-2 -translate-y-1"></div>
    </div>
  </div>
);

export const WelcomeScreen: FC<WelcomeScreenProps> = ({
  onNewConnection,
  onRecentConnections,
  onViewSample,
  onOpenDocs,
}) => {
  return (
    <div className="min-h-screen bg-background text-foreground antialiased flex items-stretch justify-center relative overflow-hidden">
      {/* Main shell */}
      <div className="relative z-10 flex w-full items-stretch justify-center">
        <section className="w-full md:w-3/5 flex flex-col justify-between border-b md:border-b-0 md:border-r border-border">
          {/* Top content */}
          <div className="flex-1 flex flex-col items-center justify-center py-10">
            {/* Logo stack */}
            <div className="flex flex-col items-center gap-4 md:gap-6">
              <HiveLogo />

              {/* Title + subtitle */}
              <div className="text-center space-y-1.5 md:space-y-2">
                <div className="flex items-center justify-center gap-2">
                  <span className="uppercase tracking-[0.16em] text-[0.72rem] text-amber-600 dark:text-amber-300/80">
                    DB HIVE
                  </span>
                  <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/20 bg-amber-500/5 px-2 py-0.5">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 shadow-[0_0_0_4px_rgba(16,185,129,0.3)]"></span>
                    <span className="text-[0.7rem] font-medium text-emerald-600 dark:text-emerald-300/90">
                      open source
                    </span>
                  </span>
                </div>
                <h1 className="text-2xl md:text-3xl lg:text-4xl tracking-tight font-semibold text-foreground">
                  A focused workspace for your data.
                </h1>
                <p className="max-w-md mx-auto text-sm md:text-base text-muted-foreground">
                  Inspect, query, and shape your databases with a
                  developer-first client that stays out of your way.
                </p>
              </div>
            </div>

            {/* Quick actions */}
            <div className="mt-8 md:mt-10 w-full max-w-md mx-auto grid grid-cols-1 gap-2.5 px-4">
              {/* Primary: Connect */}
              <Button
                onClick={onNewConnection}
                className="group relative inline-flex items-center justify-between gap-3 rounded-xl border border-amber-300/70 bg-gradient-to-r from-amber-400 via-amber-300 to-amber-400 text-slate-950 px-4 py-6 text-sm md:text-base font-medium shadow-[0_15px_40px_rgba(251,191,36,0.5)] hover:-translate-y-[1px] active:translate-y-[1px] transition-transform duration-150"
              >
                <div className="flex items-center gap-2.5">
                  <Plug2 className="h-4 w-4" strokeWidth={1.5} />
                  <div className="flex flex-col items-start">
                    <span>Connect to database</span>
                    <span className="text-[0.7rem] text-slate-900/80 font-normal">
                      Discover and save new connections
                    </span>
                  </div>
                </div>
                <div className="flex items-center gap-1.5 rounded-full bg-slate-950/15 px-2 py-0.5 text-[0.66rem] font-medium tracking-[0.12em] uppercase">
                  <span>⌘</span>
                  <span>K</span>
                </div>
              </Button>

              {onRecentConnections && (
                <Button
                  variant="outline"
                  onClick={onRecentConnections}
                  className="group flex items-center justify-between gap-3 rounded-xl px-4 py-5 text-sm md:text-base"
                >
                  <div className="flex items-center gap-2.5">
                    <Clock
                      className="h-4 w-4 text-foreground/85"
                      strokeWidth={1.5}
                    />
                    <div className="flex flex-col items-start">
                      <span className="font-medium">
                        Open recent connections
                      </span>
                      <span className="text-[0.7rem] text-muted-foreground font-normal">
                        Jump back into your last sessions
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 rounded-full bg-secondary px-2 py-0.5 text-[0.66rem] font-medium tracking-[0.12em] uppercase text-secondary-foreground">
                    <span>⌘</span>
                    <span>R</span>
                  </div>
                </Button>
              )}

              {onViewSample && (
                <Button
                  variant="outline"
                  onClick={onViewSample}
                  className="group flex items-center justify-between gap-3 rounded-xl px-4 py-5 text-sm md:text-base"
                >
                  <div className="flex items-center gap-2.5">
                    <Database
                      className="h-4 w-4 text-amber-600 dark:text-amber-300/90"
                      strokeWidth={1.5}
                    />
                    <div className="flex flex-col items-start">
                      <span className="font-medium">View sample workspace</span>
                      <span className="text-[0.7rem] text-muted-foreground font-normal">
                        Explore a curated sample database
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 rounded-full bg-secondary px-2 py-0.5 text-[0.66rem] font-medium tracking-[0.12em] uppercase text-secondary-foreground">
                    <span>⌘</span>
                    <span>O</span>
                  </div>
                </Button>
              )}

              {onOpenDocs && (
                <Button
                  variant="outline"
                  onClick={onOpenDocs}
                  className="group flex items-center justify-between gap-3 rounded-xl px-4 py-5 text-sm md:text-base"
                >
                  <div className="flex items-center gap-2.5">
                    <BookOpen
                      className="h-4 w-4 text-foreground/85"
                      strokeWidth={1.5}
                    />
                    <div className="flex flex-col items-start">
                      <span className="font-medium">Documentation</span>
                      <span className="text-[0.7rem] text-muted-foreground font-normal">
                        Learn keyboard flows, drivers, and workflows
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 rounded-full bg-secondary px-2 py-0.5 text-[0.66rem] font-medium tracking-[0.12em] uppercase text-secondary-foreground">
                    <span>?</span>
                    <span>/</span>
                  </div>
                </Button>
              )}
            </div>
          </div>

          {/* Footer: version + GitHub */}
          <footer className="w-full border-t border-border bg-card/50">
            <div className="flex items-center justify-between gap-4 px-4 md:px-8 py-2.5 md:py-3 text-[0.7rem] md:text-xs text-muted-foreground">
              <div className="flex items-center gap-3">
                <span className="inline-flex items-center gap-1.5">
                  <span className="inline-flex h-1.5 w-1.5 rounded-full bg-emerald-400 shadow-[0_0_0_4px_rgba(16,185,129,0.16)]"></span>
                  <span>DB Hive</span>
                </span>
                <span className="text-muted-foreground/70">v0.4.0-beta</span>
                <span className="hidden sm:inline text-border">•</span>
                <span className="hidden sm:inline">
                  Dark mode · Keyboard-first
                </span>
              </div>

              <div className="flex items-center gap-3">
                <Button
                  variant="ghost"
                  size="sm"
                  className="hidden sm:inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-2.5 py-1 text-[0.68rem] hover:border-amber-500/60 hover:text-amber-600 dark:hover:text-amber-200 h-auto"
                >
                  <Terminal className="h-3.5 w-3.5" strokeWidth={1.5} />
                  <span className="uppercase tracking-[0.14em]">⌘ /</span>
                  <span className="text-muted-foreground">shortcuts</span>
                </Button>

                <a
                  href="https://github.com/KwaminaWhyte/db-hive"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1.5 hover:text-amber-600 dark:hover:text-amber-200 transition-colors"
                >
                  <Github className="h-3.5 w-3.5" strokeWidth={1.5} />
                  <span>GitHub</span>
                </a>
              </div>
            </div>
          </footer>
        </section>

        {/* Right pane: shortcuts / empty state */}
        <aside className="w-full md:w-2/5 bg-background flex flex-col justify-between">
          {/* Keyboard cheatsheet */}
          <div className="px-4 md:px-6 pt-5 md:pt-6 pb-4 border-b border-border">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <span className="inline-flex h-6 w-6 items-center justify-center rounded-lg bg-amber-500/10 border border-amber-500/40">
                  <Command
                    className="h-3.5 w-3.5 text-amber-600 dark:text-amber-300"
                    strokeWidth={1.5}
                  />
                </span>
                <div className="flex flex-col">
                  <span className="text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
                    keyboard flow
                  </span>
                  <span className="text-sm text-foreground">
                    Stay on the home row
                  </span>
                </div>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5 text-xs">
              {[
                { icon: Plug2, label: "New connection", keys: ["⌘", "K"] },
                { icon: Terminal, label: "Run query", keys: ["⇧", "⏎"] },
                { icon: Database, label: "Switch table", keys: ["⌥", "⇥"] },
                {
                  icon: LayoutPanelLeft,
                  label: "Toggle sidebar",
                  keys: ["⌘", "B"],
                },
              ].map((shortcut, i) => (
                <div
                  key={i}
                  className="flex items-center justify-between rounded-lg border border-border bg-card px-2.5 py-2"
                >
                  <div className="flex items-center gap-1.5">
                    <shortcut.icon
                      className="h-3.5 w-3.5 text-foreground"
                      strokeWidth={1.5}
                    />
                    <span className="text-foreground">{shortcut.label}</span>
                  </div>
                  <div className="inline-flex items-center gap-1 rounded-md border border-border bg-secondary px-1.5 py-0.5 text-[0.64rem] text-secondary-foreground">
                    {shortcut.keys.map((key, j) => (
                      <span key={j}>{key}</span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* Empty state / dockable hint */}
          <div className="flex-1 flex items-center justify-center px-4 md:px-6">
            <div className="w-full max-w-xs space-y-3 text-xs">
              <div className="flex items-center gap-2">
                <div className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-amber-500/10 border border-amber-500/40">
                  <div className="h-3 w-3 rounded-[0.4rem] bg-amber-600 dark:bg-amber-300/80"></div>
                </div>
                <div className="flex flex-col">
                  <span className="text-[0.74rem] font-medium uppercase tracking-[0.18em] text-amber-600 dark:text-amber-200/90">
                    workspace layout
                  </span>
                  <span className="text-foreground">Dock panels anywhere</span>
                </div>
              </div>

              <p className="text-muted-foreground text-[0.8rem] leading-relaxed">
                DB Hive starts with a clean canvas. Once connected, you can dock
                the schema browser, query editor, and result grid into a layout
                that matches how you think.
              </p>

              <ul className="mt-1 space-y-1 text-[0.78rem] text-muted-foreground leading-relaxed">
                <li className="flex items-center gap-2">
                  <span className="h-1.5 w-1.5 rounded-sm bg-amber-600 dark:bg-amber-300/80"></span>
                  <span>Drag panels by their header to rearrange.</span>
                </li>
                <li className="flex items-center gap-2">
                  <span className="h-1.5 w-1.5 rounded-sm bg-amber-600/80 dark:bg-amber-300/60"></span>
                  <span>Collapse sidebars to focus on schemas or results.</span>
                </li>
                <li className="flex items-center gap-2">
                  <span className="h-1.5 w-1.5 rounded-sm bg-amber-600/60 dark:bg-amber-300/40"></span>
                  <span>Save layouts per project for instant recall.</span>
                </li>
              </ul>
            </div>
          </div>

          {/* Subtle bee/network motif */}
          <div className="border-t border-border bg-card/50 px-4 md:px-6 py-2.5">
            <div className="flex items-center justify-between text-[0.7rem]">
              <div className="flex items-center gap-1.5 text-muted-foreground">
                <span className="inline-flex h-4 w-4 items-center justify-center rounded-md border border-amber-500/50 bg-amber-500/10">
                  <span className="h-1.5 w-1.5 rounded-sm bg-amber-600 dark:bg-amber-300/90"></span>
                </span>
                <span>Nodes ready for your first hive.</span>
              </div>
              <div className="flex items-center gap-1.5 text-muted-foreground">
                <span className="h-0.5 w-6 rounded-full bg-amber-600 dark:bg-amber-300/50"></span>
                <span className="h-0.5 w-6 rounded-full bg-amber-600/60 dark:bg-amber-300/30"></span>
                <span className="h-0.5 w-6 rounded-full bg-amber-600/30 dark:bg-amber-300/20"></span>
              </div>
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
};
