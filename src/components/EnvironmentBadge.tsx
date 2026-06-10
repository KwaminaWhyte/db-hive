import { FC } from "react";
import { cn } from "@/lib/utils";
import type { Environment } from "@/types/database";

interface EnvironmentBadgeProps {
  environment: Environment;
  className?: string;
}

const environmentStyles: Record<Environment, string> = {
  Production: "bg-destructive/20 border-destructive/40 text-destructive",
  Staging: "bg-warning/20 border-warning/40 text-warning",
  Local: "bg-success/20 border-success/40 text-success",
};

/**
 * EnvironmentBadge - Shared badge for connection environments.
 *
 * Maps environments onto semantic theme tokens:
 * Production → destructive, Staging → warning, Local → success.
 */
export const EnvironmentBadge: FC<EnvironmentBadgeProps> = ({
  environment,
  className,
}) => {
  return (
    <span
      className={cn(
        "text-[10px] px-1.5 py-0.5 rounded-full border font-medium shrink-0",
        environmentStyles[environment],
        className
      )}
    >
      {environment}
    </span>
  );
};
