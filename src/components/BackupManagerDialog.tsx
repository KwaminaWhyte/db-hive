/**
 * BackupManagerDialog
 *
 * Manage database backups for the currently connected connection:
 *   - View the backup directory and open it in the OS file manager
 *   - Create a new backup (schema/data toggles, optional note)
 *   - List existing backups with size / date / status
 *   - Restore a backup (destructive, with confirm + optional drop&recreate)
 *   - Delete a backup (with confirm)
 *
 * Backend commands already exist; this is UI only.
 */

import { FC, useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogFooter,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogCancel,
  AlertDialogAction,
} from "@/components/ui/alert-dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Archive,
  FolderOpen,
  RefreshCw,
  Loader2,
  Plus,
  Undo2,
  Trash2,
  CheckCircle2,
  AlertCircle,
} from "lucide-react";
import { toast } from "sonner";
import { useConnectionContext } from "@/contexts/ConnectionContext";

/* -------------------------------------------------------------------------- */
/*  Types (serde camelCase)                                                    */
/* -------------------------------------------------------------------------- */

type BackupStatus = "completed" | { failed: string };

interface BackupEntry {
  id: string;
  connectionId: string;
  connectionName: string;
  driver: string;
  filePath: string;
  fileName: string;
  sizeBytes: number;
  createdAt: number; // unix seconds
  status: BackupStatus;
  note: string | null;
}

interface BackupOptions {
  includeData: boolean;
  includeSchema: boolean;
  tables: string[];
  note: string | null;
  outputDir: string | null;
}

interface RestoreOptions {
  filePath: string;
  dropExisting: boolean;
}

interface BackupManagerDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/* -------------------------------------------------------------------------- */
/*  Helpers                                                                    */
/* -------------------------------------------------------------------------- */

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1
  );
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function errMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    const m = (err as { message?: unknown }).message;
    if (typeof m === "string" && m.length > 0) return m;
  }
  return String(err);
}

function isFailed(
  status: BackupStatus
): status is { failed: string } {
  return typeof status === "object" && status !== null && "failed" in status;
}

/* -------------------------------------------------------------------------- */
/*  Component                                                                  */
/* -------------------------------------------------------------------------- */

export const BackupManagerDialog: FC<BackupManagerDialogProps> = ({
  open,
  onOpenChange,
}) => {
  const { connectionId, connectionProfile } = useConnectionContext();

  const [directory, setDirectory] = useState<string>("");
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [listLoading, setListLoading] = useState(false);

  // Create form state
  const [includeSchema, setIncludeSchema] = useState(true);
  const [includeData, setIncludeData] = useState(true);
  const [note, setNote] = useState("");
  const [creating, setCreating] = useState(false);

  // Restore dialog state
  const [restoreTarget, setRestoreTarget] = useState<BackupEntry | null>(null);
  const [dropExisting, setDropExisting] = useState(false);
  const [restoring, setRestoring] = useState(false);

  // Delete dialog state
  const [deleteTarget, setDeleteTarget] = useState<BackupEntry | null>(null);
  const [deleting, setDeleting] = useState(false);

  const loadBackups = useCallback(async () => {
    setListLoading(true);
    try {
      const result = await invoke<BackupEntry[]>("list_backups", {
        directory: null,
      });
      setBackups(result);
    } catch (err) {
      toast.error(`Failed to load backups: ${errMessage(err)}`);
    } finally {
      setListLoading(false);
    }
  }, []);

  const loadDirectory = useCallback(async () => {
    try {
      const dir = await invoke<string>("get_backup_directory");
      setDirectory(dir);
    } catch (err) {
      toast.error(`Failed to get backup directory: ${errMessage(err)}`);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    loadDirectory();
    loadBackups();
  }, [open, loadDirectory, loadBackups]);

  const handleOpenFolder = async () => {
    try {
      await invoke("open_backup_directory");
    } catch (err) {
      toast.error(`Failed to open folder: ${errMessage(err)}`);
    }
  };

  const handleCreate = async () => {
    if (!connectionId) return;
    setCreating(true);
    try {
      const options: BackupOptions = {
        includeData,
        includeSchema,
        tables: [],
        note: note.trim() ? note.trim() : null,
        outputDir: null,
      };
      const entry = await invoke<BackupEntry>("create_backup", {
        connectionId,
        options,
      });
      if (isFailed(entry.status)) {
        toast.error(`Backup failed: ${entry.status.failed}`);
      } else {
        toast.success(`Backup created: ${entry.fileName}`);
      }
      setNote("");
      await loadBackups();
    } catch (err) {
      toast.error(`Backup failed: ${errMessage(err)}`);
    } finally {
      setCreating(false);
    }
  };

  const handleRestore = async () => {
    if (!restoreTarget || !connectionId) return;
    setRestoring(true);
    try {
      const options: RestoreOptions = {
        filePath: restoreTarget.filePath,
        dropExisting,
      };
      await invoke("restore_backup", { connectionId, options });
      toast.success(`Restored from ${restoreTarget.fileName}`);
      setRestoreTarget(null);
      setDropExisting(false);
    } catch (err) {
      toast.error(`Restore failed: ${errMessage(err)}`);
    } finally {
      setRestoring(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await invoke("delete_backup", { filePath: deleteTarget.filePath });
      toast.success(`Deleted ${deleteTarget.fileName}`);
      setDeleteTarget(null);
      await loadBackups();
    } catch (err) {
      toast.error(`Delete failed: ${errMessage(err)}`);
    } finally {
      setDeleting(false);
    }
  };

  const hasConnection = !!connectionId;

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-3xl max-w-[calc(100%-2rem)] max-h-[85vh] flex flex-col">
          <DialogHeader>
            <div className="flex items-start justify-between gap-4">
              <div>
                <DialogTitle>Backup Manager</DialogTitle>
                <DialogDescription className="break-all">
                  {directory
                    ? `Backups stored in: ${directory}`
                    : "Loading backup directory…"}
                </DialogDescription>
              </div>
              <div className="flex shrink-0 gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleOpenFolder}
                  disabled={!directory}
                >
                  <FolderOpen className="size-4 mr-2" />
                  Open Folder
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={loadBackups}
                  disabled={listLoading}
                >
                  {listLoading ? (
                    <Loader2 className="size-4 mr-2 animate-spin" />
                  ) : (
                    <RefreshCw className="size-4 mr-2" />
                  )}
                  Refresh
                </Button>
              </div>
            </div>
          </DialogHeader>

          <div className="flex-1 overflow-hidden flex flex-col gap-4 min-h-[400px]">
            {/* Create Backup panel */}
            <div className="rounded-lg border p-4 space-y-3">
              <div className="flex items-center gap-2">
                <Plus className="size-4 text-muted-foreground" />
                <h3 className="text-sm font-semibold">New Backup</h3>
                {connectionProfile && (
                  <span className="text-xs text-muted-foreground">
                    {connectionProfile.name} ({connectionProfile.driver})
                  </span>
                )}
              </div>

              <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={includeSchema}
                    onCheckedChange={(v) => setIncludeSchema(!!v)}
                    disabled={!hasConnection || creating}
                  />
                  Include Schema
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={includeData}
                    onCheckedChange={(v) => setIncludeData(!!v)}
                    disabled={!hasConnection || creating}
                  />
                  Include Data
                </label>
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="backup-note">Note (optional)</Label>
                <Input
                  id="backup-note"
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  placeholder="e.g. before risky migration"
                  disabled={!hasConnection || creating}
                />
              </div>

              <div className="flex items-center gap-3">
                <Button
                  onClick={handleCreate}
                  disabled={
                    !hasConnection ||
                    creating ||
                    (!includeSchema && !includeData)
                  }
                >
                  {creating ? (
                    <Loader2 className="size-4 mr-2 animate-spin" />
                  ) : (
                    <Archive className="size-4 mr-2" />
                  )}
                  {creating ? "Creating backup…" : "Create Backup"}
                </Button>
                {!hasConnection && (
                  <span className="text-xs text-muted-foreground">
                    Connect to a database to create a backup.
                  </span>
                )}
                {hasConnection && !includeSchema && !includeData && (
                  <span className="text-xs text-muted-foreground">
                    Select at least schema or data.
                  </span>
                )}
              </div>
            </div>

            {/* Backups list */}
            <div className="flex-1 overflow-hidden flex flex-col border rounded-lg">
              {backups.length === 0 && !listLoading ? (
                <div className="flex-1 flex flex-col items-center justify-center gap-3 py-12 text-muted-foreground">
                  <Archive className="size-10 opacity-50" />
                  <p className="text-sm">No backups yet</p>
                </div>
              ) : (
                <ScrollArea className="flex-1">
                  <Table>
                    <TableHeader className="sticky top-0 bg-background z-10">
                      <TableRow>
                        <TableHead>File</TableHead>
                        <TableHead>Connection</TableHead>
                        <TableHead>Driver</TableHead>
                        <TableHead>Size</TableHead>
                        <TableHead>Created</TableHead>
                        <TableHead>Status</TableHead>
                        <TableHead className="text-right">Actions</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {backups.map((b) => {
                        const failed = isFailed(b.status);
                        return (
                          <TableRow key={b.id}>
                            <TableCell
                              className="font-medium max-w-[180px] truncate"
                              title={b.fileName}
                            >
                              {b.fileName}
                              {b.note && (
                                <span className="block text-xs text-muted-foreground truncate">
                                  {b.note}
                                </span>
                              )}
                            </TableCell>
                            <TableCell
                              className="max-w-[140px] truncate"
                              title={b.connectionName}
                            >
                              {b.connectionName}
                            </TableCell>
                            <TableCell>
                              <Badge variant="secondary">{b.driver}</Badge>
                            </TableCell>
                            <TableCell className="whitespace-nowrap">
                              {formatBytes(b.sizeBytes)}
                            </TableCell>
                            <TableCell className="whitespace-nowrap text-xs">
                              {new Date(
                                b.createdAt * 1000
                              ).toLocaleString()}
                            </TableCell>
                            <TableCell>
                              {failed ? (
                                <Badge
                                  variant="destructive"
                                  title={(b.status as { failed: string }).failed}
                                >
                                  <AlertCircle className="size-3" />
                                  Failed
                                </Badge>
                              ) : (
                                <Badge variant="outline">
                                  <CheckCircle2 className="size-3 text-green-600" />
                                  Completed
                                </Badge>
                              )}
                            </TableCell>
                            <TableCell className="text-right whitespace-nowrap">
                              <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => {
                                  setDropExisting(false);
                                  setRestoreTarget(b);
                                }}
                                disabled={failed || !hasConnection}
                                title={
                                  !hasConnection
                                    ? "Connect to a database to restore"
                                    : failed
                                      ? "Cannot restore a failed backup"
                                      : "Restore this backup"
                                }
                              >
                                <Undo2 className="size-4 mr-1" />
                                Restore
                              </Button>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="text-destructive hover:text-destructive"
                                onClick={() => setDeleteTarget(b)}
                              >
                                <Trash2 className="size-4 mr-1" />
                                Delete
                              </Button>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </ScrollArea>
              )}
            </div>
          </div>
        </DialogContent>
      </Dialog>

      {/* Restore confirm */}
      <AlertDialog
        open={!!restoreTarget}
        onOpenChange={(o) => {
          if (!o && !restoring) {
            setRestoreTarget(null);
            setDropExisting(false);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Restore backup?</AlertDialogTitle>
            <AlertDialogDescription>
              This will restore{" "}
              <span className="font-medium text-foreground">
                {restoreTarget?.fileName}
              </span>{" "}
              into the currently connected database
              {connectionProfile ? ` (${connectionProfile.name})` : ""}. This
              action is destructive and cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>

          <label className="flex items-start gap-2 text-sm rounded-md border p-3">
            <Checkbox
              checked={dropExisting}
              onCheckedChange={(v) => setDropExisting(!!v)}
              disabled={restoring}
            />
            <span>
              Drop &amp; recreate database first
              <span className="block text-xs text-muted-foreground">
                Removes all existing objects before restoring (Postgres / MySQL).
              </span>
            </span>
          </label>

          <AlertDialogFooter>
            <AlertDialogCancel disabled={restoring}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                handleRestore();
              }}
              disabled={restoring}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {restoring && (
                <Loader2 className="size-4 mr-2 animate-spin" />
              )}
              {restoring ? "Restoring…" : "Restore"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Delete confirm */}
      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => {
          if (!o && !deleting) setDeleteTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete backup?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes{" "}
              <span className="font-medium text-foreground">
                {deleteTarget?.fileName}
              </span>{" "}
              from disk. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                handleDelete();
              }}
              disabled={deleting}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {deleting && <Loader2 className="size-4 mr-2 animate-spin" />}
              {deleting ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
};
