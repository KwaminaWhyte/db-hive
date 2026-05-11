/**
 * ConfirmDestructiveDialog
 *
 * Generic AlertDialog-based confirmation for destructive operations
 * (DROP, TRUNCATE, etc.). Supports rich description content so callers can
 * include lists of dependent objects (e.g. FK-referencing tables).
 */

import { FC, ReactNode } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { AlertTriangle } from "lucide-react";

interface ConfirmDestructiveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  loading?: boolean;
  onConfirm: () => void | Promise<void>;
}

export const ConfirmDestructiveDialog: FC<ConfirmDestructiveDialogProps> = ({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  loading = false,
  onConfirm,
}) => {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="flex items-center gap-2">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            {title}
          </AlertDialogTitle>
          {description && (
            <AlertDialogDescription asChild>
              <div className="text-muted-foreground text-sm space-y-2">
                {description}
              </div>
            </AlertDialogDescription>
          )}
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={loading}>{cancelLabel}</AlertDialogCancel>
          <AlertDialogAction
            onClick={(e) => {
              e.preventDefault();
              void onConfirm();
            }}
            disabled={loading}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {loading ? "Working..." : confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
};
