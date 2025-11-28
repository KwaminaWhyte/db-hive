import { usePlugins } from "@/contexts/PluginContext";
import { Button } from "./ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";
import {
  FileOutput,
  Palette,
  Code,
  Database,
  Settings,
  Box,
  Plug,
} from "lucide-react";

// Map icon names to Lucide icons
const iconMap: Record<string, React.ComponentType<{ className?: string }>> = {
  "file-export": FileOutput,
  "file-output": FileOutput,
  palette: Palette,
  code: Code,
  database: Database,
  settings: Settings,
  box: Box,
  plug: Plug,
};

export function PluginToolbar() {
  const { toolbarButtons, executeAction } = usePlugins();

  if (toolbarButtons.length === 0) {
    return null;
  }

  const getIcon = (iconName?: string) => {
    if (!iconName) return Plug;
    return iconMap[iconName.toLowerCase()] || Plug;
  };

  return (
    <TooltipProvider>
      <div className="flex items-center gap-1 px-2 border-l">
        {toolbarButtons.map((button) => {
          if (button.componentType.type !== "button") return null;
          const { data } = button.componentType;
          const Icon = getIcon(data.icon);

          return (
            <Tooltip key={button.id}>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-8 px-2"
                  onClick={() => executeAction(button.pluginId, data.action)}
                >
                  <Icon className="size-4 mr-1" />
                  <span className="text-xs">{data.label}</span>
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>{data.tooltip || data.label}</p>
                <p className="text-xs text-muted-foreground">{button.pluginName}</p>
              </TooltipContent>
            </Tooltip>
          );
        })}
      </div>
    </TooltipProvider>
  );
}
