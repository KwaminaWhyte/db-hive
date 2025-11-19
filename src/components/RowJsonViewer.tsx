import { FC } from "react";
import { Button } from "./ui/button";
import { ScrollArea } from "./ui/scroll-area";
import { Code, X } from "lucide-react";

interface RowJsonViewerProps {
  columns: string[];
  row: any[];
  onClose: () => void;
}

export const RowJsonViewer: FC<RowJsonViewerProps> = ({
  columns,
  row,
  onClose,
}) => {
  // Convert row to JSON object
  const rowToJSON = () => {
    const obj: Record<string, any> = {};
    columns.forEach((col, idx) => {
      obj[col] = row[idx];
    });
    return JSON.stringify(obj, null, 2);
  };

  return (
    <div className="h-full flex flex-col border-l">
      <div className="flex items-center justify-between p-3 border-b bg-muted/30">
        <div className="flex items-center gap-2">
          <Code className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-semibold">JSON Row Viewer</span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={onClose}
          className="h-7 w-7 p-0"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>
      <ScrollArea className="flex-1 bg-muted/30">
        <pre className="p-4 text-xs font-mono leading-relaxed">
          <code className="language-json">{rowToJSON()}</code>
        </pre>
      </ScrollArea>
    </div>
  );
};
