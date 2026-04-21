import { FC } from "react";
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
import {
  DestructiveWarning,
  describeWarning,
} from "@/utils/sqlGuards";

interface DestructiveQueryGuardProps {
  open: boolean;
  warnings: DestructiveWarning[];
  onConfirm: () => void;
  onCancel: () => void;
}

export const DestructiveQueryGuard: FC<DestructiveQueryGuardProps> = ({
  open,
  warnings,
  onConfirm,
  onCancel,
}) => {
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <AlertDialogContent className="max-w-2xl">
        <AlertDialogHeader>
          <AlertDialogTitle className="flex items-center gap-2">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            Destructive query detected
          </AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="text-muted-foreground text-sm">
              Review the {warnings.length === 1 ? "statement" : "statements"} below before running.
              This action may be irreversible.
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="max-h-[50vh] overflow-auto space-y-4">
          {warnings.map((w, idx) => (
            <div key={idx} className="space-y-2">
              <div className="text-sm font-medium text-foreground">
                {describeWarning(w.kind)}
              </div>
              <pre className="text-xs bg-muted rounded p-3 overflow-x-auto whitespace-pre-wrap break-words border border-border">
                {w.preview}
              </pre>
            </div>
          ))}
        </div>

        <AlertDialogFooter>
          <AlertDialogCancel
            autoFocus
            onClick={(e) => {
              e.preventDefault();
              onCancel();
            }}
          >
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={(e) => {
              e.preventDefault();
              onConfirm();
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            Run anyway
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
};
