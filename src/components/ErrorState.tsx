import { FC, ReactNode } from "react";
import { LucideIcon } from "lucide-react";
import { Button } from "./ui/button";
import { Card, CardContent } from "./ui/card";
import { cn } from "@/lib/utils";

export interface ErrorAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost" | "destructive";
  icon?: LucideIcon;
}

export interface ErrorStateProps {
  title: string;
  message: string;
  icon?: LucideIcon;
  actions?: ErrorAction[];
  className?: string;
  children?: ReactNode;
  variant?: "error" | "warning" | "info";
}

/**
 * ErrorState - A reusable error state component
 *
 * Displays a centered error message with an icon, title, description,
 * and optional action buttons. Supports different variants for different
 * types of messages (error, warning, info).
 *
 * @example
 * ```tsx
 * <ErrorState
 *   title="Connection Failed"
 *   message="Unable to connect to the database."
 *   icon={WifiOff}
 *   actions={[
 *     { label: "Retry", onClick: handleRetry },
 *     { label: "Cancel", onClick: handleCancel, variant: "outline" }
 *   ]}
 * />
 * ```
 */
export const ErrorState: FC<ErrorStateProps> = ({
  title,
  message,
  icon: Icon,
  actions = [],
  className,
  children,
  variant = "error",
}) => {
  const variantStyles = {
    error: "text-destructive",
    warning: "text-orange-500 dark:text-orange-400",
    info: "text-blue-500 dark:text-blue-400",
  };

  const iconBgStyles = {
    error: "bg-destructive/10 dark:bg-destructive/20",
    warning: "bg-orange-500/10 dark:bg-orange-500/20",
    info: "bg-blue-500/10 dark:bg-blue-500/20",
  };

  return (
    <div
      className={cn(
        "flex items-center justify-center min-h-[400px] p-6 animate-in fade-in duration-500",
        className
      )}
    >
      <Card className="max-w-md w-full border-border/50 shadow-lg">
        <CardContent className="flex flex-col items-center text-center space-y-6 py-12">
          {/* Icon */}
          {Icon && (
            <div
              className={cn(
                "rounded-full p-4 animate-in zoom-in duration-300 delay-100",
                iconBgStyles[variant]
              )}
            >
              <Icon
                className={cn("h-12 w-12", variantStyles[variant])}
                strokeWidth={1.5}
              />
            </div>
          )}

          {/* Title */}
          <div className="space-y-3 animate-in slide-in-from-bottom-4 duration-500 delay-200">
            <h2 className="text-2xl font-semibold tracking-tight">{title}</h2>

            {/* Message */}
            <p className="text-muted-foreground text-sm leading-relaxed max-w-sm">
              {message}
            </p>
          </div>

          {/* Custom content */}
          {children && (
            <div className="w-full animate-in slide-in-from-bottom-4 duration-500 delay-300">
              {children}
            </div>
          )}

          {/* Action buttons */}
          {actions.length > 0 && (
            <div className="flex flex-wrap items-center justify-center gap-3 pt-2 animate-in slide-in-from-bottom-4 duration-500 delay-400">
              {actions.map((action, index) => {
                const ActionIcon = action.icon;
                return (
                  <Button
                    key={index}
                    onClick={action.onClick}
                    variant={action.variant || "default"}
                    className="min-w-[120px]"
                  >
                    {ActionIcon && <ActionIcon className="h-4 w-4" />}
                    {action.label}
                  </Button>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
};
