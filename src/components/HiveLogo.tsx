/**
 * HiveLogo — the DB Hive brand mark, built in CSS.
 *
 * Single source of truth for the logo previously hand-built in four places
 * (CustomTitlebar, WelcomeScreen, the /about route, and the About modal).
 * The amber/slate palette is brand artwork — intentionally hardcoded rather
 * than themed.
 *
 * Sizes:
 * - `sm` — titlebar (24px)
 * - `md` — About modal (80px)
 * - `lg` — About page (96px)
 * - `xl` — Welcome/landing hero (responsive, supports `animated`)
 *
 * `animated` enables the WelcomeScreen treatment: opposing hex-ring
 * rotation, shimmer sweep, bee-wing arcs, and pulsing glow (CSS classes
 * defined in the global stylesheet).
 */

import { FC } from "react";
import { cn } from "@/lib/utils";

export type HiveLogoSize = "sm" | "md" | "lg" | "xl";

interface HiveLogoProps {
  size?: HiveLogoSize;
  /** Enable the animated hero treatment (designed for `xl`). */
  animated?: boolean;
  className?: string;
}

interface SizeConfig {
  outer: string;
  inner: string;
  hex: string;
  core: string;
  stripes: [string, string, string];
}

const SIZES: Record<HiveLogoSize, SizeConfig> = {
  sm: {
    outer: "h-6 w-6 rounded border",
    inner: "inset-[15%] rounded border",
    hex: "h-[70%] w-[70%] rounded border",
    core: "h-[55%] w-[55%] rounded gap-[1px]",
    stripes: ["w-[70%] h-[1.5px]", "w-[60%] h-[1.5px]", "w-[50%] h-[1.5px]"],
  },
  md: {
    outer: "h-20 w-20 rounded-xl border-2",
    inner: "inset-[12%] rounded-lg border-2",
    hex: "h-[65%] w-[65%] rounded-md border-2",
    core: "h-[50%] w-[50%] rounded-md gap-[3px]",
    stripes: ["w-[65%] h-[2.5px]", "w-[55%] h-[2.5px]", "w-[45%] h-[2.5px]"],
  },
  lg: {
    outer: "h-24 w-24 rounded-xl border-2",
    inner: "inset-[12%] rounded-lg border-2",
    hex: "h-[65%] w-[65%] rounded-md border-2",
    core: "h-[50%] w-[50%] rounded-md gap-[3px]",
    stripes: ["w-[65%] h-[2.5px]", "w-[55%] h-[2.5px]", "w-[45%] h-[2.5px]"],
  },
  xl: {
    outer:
      "h-24 w-24 sm:h-28 sm:w-28 md:h-32 md:w-32 rounded-2xl border shadow-[0_0_0_1px_rgba(15,23,42,0.8),0_18px_45px_rgba(0,0,0,0.85)] overflow-hidden",
    inner:
      "inset-[18%] rounded-[1.15rem] border shadow-[0_0_0_1px_rgba(15,23,42,0.9),0_0_40px_rgba(251,191,36,0.45)]",
    hex: "h-[78%] w-[78%] rounded-[1rem] border",
    core: "h-[64%] w-[64%] rounded-[0.85rem] gap-0.5 shadow-[0_12px_30px_rgba(251,191,36,0.65)] overflow-hidden",
    stripes: [
      "w-[72%] h-[0.2rem]",
      "w-[64%] h-[0.2rem]",
      "w-[56%] h-[0.2rem]",
    ],
  },
};

export const HiveLogo: FC<HiveLogoProps> = ({
  size = "lg",
  animated = false,
  className,
}) => {
  const s = SIZES[size];

  return (
    <div
      className={cn(
        "relative border-amber-300/40 bg-gradient-to-br from-amber-300/40 via-amber-400/25 to-amber-500/30",
        s.outer,
        className
      )}
    >
      {/* Inner hex / DB symbol */}
      <div
        className={cn(
          "absolute bg-slate-950/90 dark:bg-slate-950/90 border-amber-200/30 flex items-center justify-center",
          s.inner
        )}
      >
        {/* Layered hex stack */}
        <div className="relative h-full w-full flex items-center justify-center">
          <div
            className={cn(
              "absolute border-amber-300/40",
              s.hex,
              animated ? "hive-hex-cw" : "rotate-6"
            )}
          ></div>
          <div
            className={cn(
              "absolute border-amber-200/50",
              s.hex,
              animated ? "hive-hex-ccw" : "-rotate-6"
            )}
          ></div>
          <div
            className={cn(
              "relative bg-gradient-to-br from-amber-300 via-amber-400 to-amber-500 flex flex-col justify-center items-center",
              s.core
            )}
          >
            {/* DB stripes hinting a hive */}
            {s.stripes.map((stripe, i) => (
              <div
                key={i}
                className={cn("rounded-full bg-slate-950/85", stripe)}
              ></div>
            ))}
            {animated && (
              <div className="absolute inset-0 hive-shimmer pointer-events-none"></div>
            )}
          </div>
        </div>

        {/* Subtle bee wing arcs */}
        {animated && (
          <>
            <div className="absolute -left-3 top-1/2 -translate-y-1/2 h-10 w-6 rounded-full border border-amber-200/35 transform translate-x-[-0.15rem] translate-y-[-0.25rem] rotate-[-18deg]"></div>
            <div className="absolute -right-3 top-1/2 -translate-y-1/2 h-10 w-6 rounded-full border border-amber-200/35 transform translate-x-[0.15rem] translate-y-[-0.25rem] rotate-[18deg]"></div>
          </>
        )}
      </div>

      {/* Glow ring — pulsing */}
      {animated && (
        <div className="pointer-events-none absolute -inset-6 rounded-[1.75rem] bg-[radial-gradient(circle_at_30%_0%,rgba(255,255,255,0.3),transparent_55%),radial-gradient(circle_at_80%_120%,rgba(251,191,36,0.55),transparent_60%)] mix-blend-screen hive-glow-animate"></div>
      )}
    </div>
  );
};
