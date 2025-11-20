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
import { Checkbox } from "./ui/checkbox";
import { Alert, AlertDescription } from "./ui/alert";
import { Loader2, Upload, AlertCircle, CheckCircle2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { SqlImportOptions } from "@/types";

interface SqlImportDialogProps {
  /** Whether the dialog is open */
  open: boolean;

  /** Callback to close the dialog */
  onClose: () => void;

  /** Active connection ID */
  connectionId: string;
}

export const SqlImportDialog: FC<SqlImportDialogProps> = ({
  open,
  onClose,
  connectionId,
}) => {
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  // Import options state
  const [continueOnError, setContinueOnError] = useState(false);
  const [useTransaction, setUseTransaction] = useState(true);

  const handleSelectFile = async () => {
    try {
      const filePath = await openDialog({
        title: "Select SQL Dump File",
        filters: [
          {
            name: "SQL",
            extensions: ["sql"],
          },
          {
            name: "All Files",
            extensions: ["*"],
          },
        ],
        multiple: false,
      });

      if (filePath && typeof filePath === "string") {
        setSelectedFile(filePath);
        setError(null);
        setResult(null);
      }
    } catch (err) {
      toast.error("Failed to select file");
      console.error("File selection failed:", err);
    }
  };

  const handleImport = async () => {
    if (!selectedFile) {
      setError("Please select a SQL file to import");
      return;
    }

    try {
      setImporting(true);
      setError(null);
      setResult(null);

      // Build import options
      const options: SqlImportOptions = {
        continueOnError,
        useTransaction,
      };

      // Call backend command
      const importResult = await invoke<string>("import_from_sql", {
        connectionId,
        filePath: selectedFile,
        options,
      });

      setResult(importResult);
      toast.success("Database imported successfully!");
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      toast.error(`Import failed: ${errorMessage}`);
      console.error("SQL import failed:", err);
    } finally {
      setImporting(false);
    }
  };

  const handleClose = () => {
    if (!importing) {
      setSelectedFile(null);
      setError(null);
      setResult(null);
      onClose();
    }
  };

  return (
    <Dialog open={open} onOpenChange={(open) => !open && handleClose()}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Upload className="h-5 w-5" />
            Import SQL Dump
          </DialogTitle>
          <DialogDescription>
            Import database structure and data from a SQL dump file
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* File selection */}
          <div className="space-y-2">
            <Label>SQL File</Label>
            <div className="flex gap-2">
              <Button
                variant="outline"
                onClick={handleSelectFile}
                disabled={importing}
                className="flex-1"
              >
                {selectedFile ? "Change File" : "Select File"}
              </Button>
            </div>
            {selectedFile && (
              <p className="text-sm text-muted-foreground truncate">
                {selectedFile}
              </p>
            )}
          </div>

          {/* Import options checkboxes */}
          <div className="space-y-3">
            <Label>Import Options</Label>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="useTransaction"
                checked={useTransaction}
                onCheckedChange={(checked) => setUseTransaction(checked === true)}
                disabled={importing}
              />
              <Label
                htmlFor="useTransaction"
                className="text-sm font-normal cursor-pointer"
              >
                Use transaction (rollback all on error)
              </Label>
            </div>
            <p className="text-xs text-muted-foreground ml-6">
              If enabled, all changes will be rolled back if any statement fails
            </p>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="continueOnError"
                checked={continueOnError}
                onCheckedChange={(checked) => setContinueOnError(checked === true)}
                disabled={importing || useTransaction}
              />
              <Label
                htmlFor="continueOnError"
                className="text-sm font-normal cursor-pointer"
              >
                Continue on error
              </Label>
            </div>
            <p className="text-xs text-muted-foreground ml-6">
              Continue importing even if some statements fail (incompatible with
              transaction mode)
            </p>
          </div>

          {/* Success result */}
          {result && !error && (
            <Alert className="border-green-500 text-green-700 dark:text-green-400">
              <CheckCircle2 className="h-4 w-4" />
              <AlertDescription>{result}</AlertDescription>
            </Alert>
          )}

          {/* Error display */}
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={handleClose} disabled={importing}>
            {result ? "Close" : "Cancel"}
          </Button>
          {!result && (
            <Button onClick={handleImport} disabled={importing || !selectedFile}>
              {importing && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              Import
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
