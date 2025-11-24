import { FC, ReactNode } from "react";
import { LucideIcon } from "lucide-react";
import { Button } from "./ui/button";
import { cn } from "@/lib/utils";

export interface EmptyStateAction {
  label: string;
  onClick: () => void;
  variant?: "default" | "outline" | "secondary" | "ghost";
  icon?: LucideIcon;
}

export interface EmptyStateProps {
  title: string;
  message: string;
  icon?: LucideIcon;
  illustration?: ReactNode;
  actions?: EmptyStateAction[];
  className?: string;
  iconClassName?: string;
  size?: "sm" | "md" | "lg";
}

export const EmptyState: FC<EmptyStateProps> = ({
  title,
  message,
  icon: Icon,
  illustration,
  actions = [],
  className,
  iconClassName,
  size = "md",
}) => {
  // Size-based styling
  const sizeConfig = {
    sm: {
      container: "gap-3 py-8",
      iconSize: "size-12",
      title: "text-base",
      message: "text-sm",
    },
    md: {
      container: "gap-4 py-12",
      iconSize: "size-16",
      title: "text-lg",
      message: "text-base",
    },
    lg: {
      container: "gap-6 py-16",
      iconSize: "size-20",
      title: "text-xl",
      message: "text-lg",
    },
  };

  const config = sizeConfig[size];

  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center text-center max-w-md mx-auto px-6",
        "animate-in fade-in-0 slide-in-from-bottom-4 duration-500",
        config.container,
        className
      )}
    >
      {/* Icon or Illustration */}
      <div className="relative">
        {illustration ? (
          <div className="animate-in fade-in-0 zoom-in-95 duration-700 delay-100">
            {illustration}
          </div>
        ) : Icon ? (
          <div
            className={cn(
              "rounded-full bg-muted/50 p-4 text-muted-foreground/70",
              "transition-all duration-300 hover:bg-muted/70 hover:scale-105",
              "animate-in fade-in-0 zoom-in-95 duration-700 delay-100",
              iconClassName
            )}
          >
            <Icon className={cn(config.iconSize, "stroke-[1.5]")} />
          </div>
        ) : null}
      </div>

      {/* Content */}
      <div className="space-y-2 animate-in fade-in-0 slide-in-from-bottom-2 duration-500 delay-200">
        <h3
          className={cn(
            "font-semibold text-foreground tracking-tight",
            config.title
          )}
        >
          {title}
        </h3>
        <p className={cn("text-muted-foreground leading-relaxed", config.message)}>
          {message}
        </p>
      </div>

      {/* Actions */}
      {actions.length > 0 && (
        <div className="flex flex-wrap items-center justify-center gap-2 mt-2 animate-in fade-in-0 slide-in-from-bottom-2 duration-500 delay-300">
          {actions.map((action, index) => {
            const ActionIcon = action.icon;
            return (
              <Button
                key={index}
                onClick={action.onClick}
                variant={action.variant || (index === 0 ? "default" : "outline")}
                className="shadow-sm hover:shadow transition-shadow"
              >
                {ActionIcon && <ActionIcon className="size-4" />}
                {action.label}
              </Button>
            );
          })}
        </div>
      )}
    </div>
  );
};
