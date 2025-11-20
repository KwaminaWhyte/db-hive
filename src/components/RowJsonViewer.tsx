import { FC } from "react";
import { Button } from "./ui/button";
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

  // Syntax highlight JSON
  const highlightJSON = (json: string) => {
    // Replace with spans for different token types
    return json
      .replace(/("(?:\\.|[^"\\])*")(\s*:)?/g, (_match, p1, p2) => {
        // Property keys (followed by colon) vs string values
        if (p2) {
          return `<span class="text-blue-400">${p1}</span>${p2}`;
        }
        return `<span class="text-green-400">${p1}</span>`;
      })
      .replace(/\b(true|false)\b/g, '<span class="text-purple-400">$1</span>')
      .replace(/\b(null)\b/g, '<span class="text-red-400">$1</span>')
      .replace(/\b(-?\d+\.?\d*)\b/g, '<span class="text-orange-400">$1</span>');
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
      <div className="flex-1 bg-muted/30 overflow-auto">
        <pre className="p-4 text-xs font-mono leading-relaxed">
          <code
            className="language-json"
            dangerouslySetInnerHTML={{ __html: highlightJSON(rowToJSON()) }}
          />
        </pre>
      </div>
    </div>
  );
};
