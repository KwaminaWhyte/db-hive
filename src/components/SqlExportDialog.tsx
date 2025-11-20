import { FC, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { Button } from "./ui/button";
import { Label } from "./ui/label";
import { Input } from "./ui/input";
import { Checkbox } from "./ui/checkbox";
import { Alert, AlertDescription } from "./ui/alert";
import { Loader2, Database, AlertCircle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { SqlExportOptions } from "@/types";

interface SqlExportDialogProps {
  /** Whether the dialog is open */
  open: boolean;

  /** Callback to close the dialog */
  onClose: () => void;

  /** Active connection ID */
  connectionId: string;

  /** Current schema/database name */
  currentSchema?: string;
}

export const SqlExportDialog: FC<SqlExportDialogProps> = ({
  open,
  onClose,
  connectionId,
  currentSchema,
}) => {
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Export options state
  const [includeDrop, setIncludeDrop] = useState(false);
  const [includeCreate, setIncludeCreate] = useState(true);
  const [includeData, setIncludeData] = useState(true);
  const [schema, setSchema] = useState(currentSchema || "public");
  const [tables, setTables] = useState("");

  const handleExport = async () => {
    try {
      setExporting(true);
      setError(null);

      // Show save dialog
      const filePath = await save({
        defaultPath: `database_dump_${new Date().toISOString().split("T")[0]}.sql`,
        filters: [
          {
            name: "SQL",
            extensions: ["sql"],
          },
        ],
      });

      if (!filePath) {
        setExporting(false);
        return;
      }

      // Parse table filter (comma-separated list)
      const tableList = tables
        .split(",")
        .map((t) => t.trim())
        .filter((t) => t.length > 0);

      // Build export options
      const options: SqlExportOptions = {
        includeDrop,
        includeCreate,
        includeData,
        tables: tableList,
        schema: schema || null,
      };

      // Call backend command
      await invoke("export_to_sql", {
        connectionId,
        filePath,
        options,
      });

      toast.success("Database exported successfully!");
      onClose();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      toast.error(`Export failed: ${errorMessage}`);
      console.error("SQL export failed:", err);
    } finally {
      setExporting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Database className="h-5 w-5" />
            Export Database to SQL
          </DialogTitle>
          <DialogDescription>
            Export database structure and data to a SQL dump file
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Schema input */}
          <div className="space-y-2">
            <Label htmlFor="schema">Schema/Database</Label>
            <Input
              id="schema"
              value={schema}
              onChange={(e) => setSchema(e.target.value)}
              placeholder="public"
              disabled={exporting}
            />
            <p className="text-xs text-muted-foreground">
              Schema to export from (PostgreSQL/MySQL)
            </p>
          </div>

          {/* Table filter input */}
          <div className="space-y-2">
            <Label htmlFor="tables">Tables (optional)</Label>
            <Input
              id="tables"
              value={tables}
              onChange={(e) => setTables(e.target.value)}
              placeholder="table1, table2, table3"
              disabled={exporting}
            />
            <p className="text-xs text-muted-foreground">
              Leave empty to export all tables, or specify comma-separated list
            </p>
          </div>

          {/* Export options checkboxes */}
          <div className="space-y-3">
            <Label>Export Options</Label>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="includeDrop"
                checked={includeDrop}
                onCheckedChange={(checked) => setIncludeDrop(checked === true)}
                disabled={exporting}
              />
              <Label
                htmlFor="includeDrop"
                className="text-sm font-normal cursor-pointer"
              >
                Include DROP TABLE statements
              </Label>
            </div>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="includeCreate"
                checked={includeCreate}
                onCheckedChange={(checked) => setIncludeCreate(checked === true)}
                disabled={exporting}
              />
              <Label
                htmlFor="includeCreate"
                className="text-sm font-normal cursor-pointer"
              >
                Include CREATE TABLE statements
              </Label>
            </div>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="includeData"
                checked={includeData}
                onCheckedChange={(checked) => setIncludeData(checked === true)}
                disabled={exporting}
              />
              <Label
                htmlFor="includeData"
                className="text-sm font-normal cursor-pointer"
              >
                Include INSERT statements (data)
              </Label>
            </div>
          </div>

          {/* Error display */}
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={exporting}>
            Cancel
          </Button>
          <Button onClick={handleExport} disabled={exporting}>
            {exporting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            Export
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
