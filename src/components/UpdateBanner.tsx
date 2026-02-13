import { FC } from "react";
import { Download, RefreshCw, X, Sparkles, RotateCcw } from "lucide-react";
import { Button } from "./ui/button";
import type { UpdateStatus } from "@/hooks/useAutoUpdater";

interface UpdateBannerProps {
  status: UpdateStatus;
  dismissed: boolean;
  onDownload: () => void;
  onRestart: () => void;
  onDismiss: () => void;
  onRetry: () => void;
}

export const UpdateBanner: FC<UpdateBannerProps> = ({
  status,
  dismissed,
  onDownload,
  onRestart,
  onDismiss,
  onRetry,
}) => {
  // Don't show for idle/checking states or if dismissed
  if (status.state === "idle" || status.state === "checking" || dismissed) {
    return null;
  }

  const progressPercent =
    status.state === "downloading" && status.total > 0
      ? Math.round((status.progress / status.total) * 100)
      : 0;

  return (
    <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 animate-in slide-in-from-bottom-4 fade-in duration-300">
      <div className="flex items-center gap-3 rounded-xl border border-border bg-card/95 backdrop-blur-md shadow-[0_8px_32px_rgba(0,0,0,0.4)] px-4 py-3 min-w-[380px] max-w-[480px]">
        {/* Icon */}
        <div className="shrink-0">
          {status.state === "available" && (
            <div className="h-9 w-9 rounded-lg bg-primary/15 border border-primary/30 flex items-center justify-center">
              <Sparkles className="h-4 w-4 text-primary" />
            </div>
          )}
          {status.state === "downloading" && (
            <div className="h-9 w-9 rounded-lg bg-blue-500/15 border border-blue-500/30 flex items-center justify-center">
              <Download className="h-4 w-4 text-blue-400 animate-bounce" />
            </div>
          )}
          {status.state === "ready" && (
            <div className="h-9 w-9 rounded-lg bg-emerald-500/15 border border-emerald-500/30 flex items-center justify-center">
              <RefreshCw className="h-4 w-4 text-emerald-400" />
            </div>
          )}
          {status.state === "error" && (
            <div className="h-9 w-9 rounded-lg bg-destructive/15 border border-destructive/30 flex items-center justify-center">
              <X className="h-4 w-4 text-destructive" />
            </div>
          )}
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          {status.state === "available" && (
            <>
              <p className="text-sm font-medium">Update Available</p>
              <p className="text-xs text-muted-foreground truncate">
                DB-Hive {status.version} is ready to download
              </p>
            </>
          )}
          {status.state === "downloading" && (
            <>
              <p className="text-sm font-medium">Downloading Update</p>
              <div className="mt-1.5 h-1.5 w-full rounded-full bg-muted overflow-hidden">
                <div
                  className="h-full rounded-full bg-blue-500 transition-all duration-300 ease-out"
                  style={{ width: `${progressPercent}%` }}
                />
              </div>
              {status.total > 0 && (
                <p className="text-[10px] text-muted-foreground mt-1">
                  {formatBytes(status.progress)} / {formatBytes(status.total)} ({progressPercent}%)
                </p>
              )}
            </>
          )}
          {status.state === "ready" && (
            <>
              <p className="text-sm font-medium">Update Ready</p>
              <p className="text-xs text-muted-foreground">
                Restart to apply {status.version}
              </p>
            </>
          )}
          {status.state === "error" && (
            <>
              <p className="text-sm font-medium">Update Failed</p>
              <p className="text-xs text-muted-foreground truncate">
                {status.message}
              </p>
            </>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2 shrink-0">
          {status.state === "available" && (
            <>
              <Button size="sm" className="h-8 gap-1.5 text-xs" onClick={onDownload}>
                <Download className="h-3 w-3" />
                Download
              </Button>
              <button
                onClick={onDismiss}
                className="text-xs text-muted-foreground hover:text-foreground transition-colors px-1"
              >
                Later
              </button>
            </>
          )}
          {status.state === "ready" && (
            <Button size="sm" className="h-8 gap-1.5 text-xs" onClick={onRestart}>
              <RefreshCw className="h-3 w-3" />
              Restart
            </Button>
          )}
          {status.state === "error" && (
            <Button
              size="sm"
              variant="outline"
              className="h-8 gap-1.5 text-xs"
              onClick={onRetry}
            >
              <RotateCcw className="h-3 w-3" />
              Retry
            </Button>
          )}

          {/* Dismiss X for non-downloading states */}
          {status.state !== "downloading" && (
            <button
              onClick={onDismiss}
              className="h-6 w-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}
