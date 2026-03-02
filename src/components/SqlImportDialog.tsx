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
import {
  Loader2,
  Upload,
  AlertCircle,
  CheckCircle2,
  FileText,
  TriangleAlert,
  Square,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import type { SqlImportOptions, SqlImportResult } from "@/types";

interface SqlImportDialogProps {
  open: boolean;
  onClose: () => void;
  connectionId: string;
}

export const SqlImportDialog: FC<SqlImportDialogProps> = ({
  open,
  onClose,
  connectionId,
}) => {
  const [importing, setImporting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SqlImportResult | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  const [continueOnError, setContinueOnError] = useState(true);
  const [useTransaction, setUseTransaction] = useState(false);

  const handleSelectFile = async () => {
    try {
      const filePath = await openDialog({
        title: "Select SQL Dump File",
        filters: [
          { name: "SQL", extensions: ["sql"] },
          { name: "All Files", extensions: ["*"] },
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
      setStopping(false);
      setError(null);
      setResult(null);

      const options: SqlImportOptions = { continueOnError, useTransaction };

      const importResult = await invoke<SqlImportResult>("import_from_sql", {
        connectionId,
        filePath: selectedFile,
        options,
      });

      setResult(importResult);

      if (importResult.cancelled) {
        toast.info("Import stopped by user");
      } else if (importResult.errorsCount === 0) {
        toast.success("Database imported successfully!");
      } else {
        toast.warning(
          `Import completed with ${importResult.errorsCount} skipped statement(s)`
        );
      }
    } catch (err) {
      const errorMessage =
        err instanceof Error
          ? err.message
          : typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: string }).message)
            : String(err);
      setError(errorMessage);
      toast.error(`Import failed: ${errorMessage}`);
      console.error("SQL import failed:", err);
    } finally {
      setImporting(false);
      setStopping(false);
    }
  };

  const handleStop = async () => {
    setStopping(true);
    try {
      await invoke("cancel_import");
    } catch {
      // Ignore — the import will finish naturally
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

  const handleOpenLog = async (logFile: string) => {
    try {
      await openPath(logFile);
    } catch {
      toast.error("Could not open log file");
    }
  };

  const succeeded = result && result.errorsCount === 0 && !result.cancelled;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && !importing && handleClose()}>
      <DialogContent
        className="sm:max-w-[500px]"
        // Prevent dismissal while importing — user must Stop first
        onInteractOutside={(e) => importing && e.preventDefault()}
        onEscapeKeyDown={(e) => importing && e.preventDefault()}
      >
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

          {/* Import options */}
          <div className="space-y-3">
            <Label>Import Options</Label>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="useTransaction"
                checked={useTransaction}
                onCheckedChange={(checked) => setUseTransaction(checked === true)}
                disabled={importing}
              />
              <Label htmlFor="useTransaction" className="text-sm font-normal cursor-pointer">
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
              <Label htmlFor="continueOnError" className="text-sm font-normal cursor-pointer">
                Continue on error
              </Label>
            </div>
            <p className="text-xs text-muted-foreground ml-6">
              Continue importing even if some statements fail (incompatible with
              transaction mode)
            </p>
          </div>

          {/* Result */}
          {result && (
            <Alert
              className={
                succeeded
                  ? "border-green-500 text-green-700 dark:text-green-400"
                  : result.cancelled
                    ? "border-blue-500 text-blue-700 dark:text-blue-400"
                    : "border-yellow-500 text-yellow-700 dark:text-yellow-400"
              }
            >
              {succeeded ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : (
                <TriangleAlert className="h-4 w-4" />
              )}
              <AlertDescription className="space-y-2">
                <p>
                  {result.cancelled
                    ? `Stopped after ${result.executed} statements (${result.skipped} skipped).`
                    : result.errorsCount === 0
                      ? `Imported ${result.executed} statements (${result.skipped} skipped).`
                      : `Imported ${result.executed} statements — ${result.errorsCount} skipped, ${result.skipped} advisory.`}
                </p>
                {result.firstError && (
                  <p className="text-xs opacity-80 break-words">
                    {result.firstError}
                    {result.errorsCount > 1 && ` (+${result.errorsCount - 1} more)`}
                  </p>
                )}
                {result.logFile && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 gap-1.5 text-xs mt-1"
                    onClick={() => handleOpenLog(result.logFile!)}
                  >
                    <FileText className="h-3 w-3" />
                    Open error log
                  </Button>
                )}
              </AlertDescription>
            </Alert>
          )}

          {/* Fatal error */}
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          {importing ? (
            <>
              <Button
                variant="destructive"
                onClick={handleStop}
                disabled={stopping}
                className="gap-2"
              >
                {stopping ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Square className="h-3 w-3 fill-current" />
                )}
                {stopping ? "Stopping…" : "Stop"}
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={handleClose}>
                {result ? "Close" : "Cancel"}
              </Button>
              {!result && (
                <Button onClick={handleImport} disabled={!selectedFile}>
                  Import
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
