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
    // First escape HTML to prevent XSS
    const escapeHtml = (str: string) => {
      return str
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
    };

    const escaped = escapeHtml(json);

    // Replace with spans for different token types
    return escaped
      // Numbers (must come before strings to avoid matching inside classes)
      .replace(/:\s*(-?\d+\.?\d*)/g, (match, p1) => {
        return `: <span style="color: #fb923c">${p1}</span>`;
      })
      // Property keys (followed by colon)
      .replace(/"([^"]+)"(\s*):/g, (_match, p1, p2) => {
        return `<span style="color: #60a5fa">"${p1}"</span>${p2}:`;
      })
      // String values
      .replace(/:\s*"([^"]*)"/g, (_match, p1) => {
        return `: <span style="color: #4ade80">"${p1}"</span>`;
      })
      // Booleans
      .replace(/:\s*(true|false)/g, (_match, p1) => {
        return `: <span style="color: #c084fc">${p1}</span>`;
      })
      // Null
      .replace(/:\s*(null)/g, (_match, p1) => {
        return `: <span style="color: #f87171">${p1}</span>`;
      });
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
