import { useEffect, useRef, useCallback, useState } from 'react';
import { check, Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';

interface UseAutoUpdaterOptions {
  enabled: boolean;
  autoDownload: boolean;
  autoInstall: boolean;
  checkIntervalHours: number;
}

export type UpdateStatus =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'available'; version: string; body?: string }
  | { state: 'downloading'; progress: number; total: number }
  | { state: 'ready'; version: string }
  | { state: 'error'; message: string };

/**
 * Custom hook for automatic update checking with in-app UI state + system notifications
 */
export function useAutoUpdater(options: UseAutoUpdaterOptions) {
  const {
    enabled,
    autoDownload,
    autoInstall,
    checkIntervalHours,
  } = options;

  const intervalRef = useRef<number | null>(null);
  const isCheckingRef = useRef(false);
  const updateRef = useRef<Update | null>(null);
  const [status, setStatus] = useState<UpdateStatus>({ state: 'idle' });
  const [dismissed, setDismissed] = useState(false);

  const ensureNotificationPermission = useCallback(async () => {
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      const permission = await requestPermission();
      permissionGranted = permission === 'granted';
    }
    return permissionGranted;
  }, []);

  const notify = useCallback(async (title: string, body: string) => {
    const hasPermission = await ensureNotificationPermission();
    if (hasPermission) {
      await sendNotification({ title, body });
    }
  }, [ensureNotificationPermission]);

  /**
   * Download and install the pending update
   */
  const downloadAndInstall = useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;

    setStatus({ state: 'downloading', progress: 0, total: 0 });

    try {
      let totalBytes = 0;
      let downloadedBytes = 0;

      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            totalBytes = event.data.contentLength ?? 0;
            setStatus({ state: 'downloading', progress: 0, total: totalBytes });
            break;
          case 'Progress':
            downloadedBytes += event.data.chunkLength;
            setStatus({ state: 'downloading', progress: downloadedBytes, total: totalBytes });
            break;
          case 'Finished':
            break;
        }
      });

      setStatus({ state: 'ready', version: update.version });

      await notify('Update Ready', 'DB-Hive has been updated. Restart to apply changes.');

      if (autoInstall) {
        setTimeout(async () => {
          await relaunch();
        }, 3000);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to download update';
      setStatus({ state: 'error', message });
      await notify('Update Failed', message);
    }
  }, [autoInstall, notify]);

  /**
   * Check for updates
   */
  const checkForUpdates = useCallback(async () => {
    if (isCheckingRef.current) return;

    isCheckingRef.current = true;
    setStatus({ state: 'checking' });
    setDismissed(false);

    try {
      const update = await check();

      if (update?.available) {
        updateRef.current = update;
        setStatus({ state: 'available', version: update.version, body: update.body ?? undefined });

        await notify(
          'Update Available',
          `DB-Hive ${update.version} is available. ${autoDownload ? 'Downloading...' : 'Open app to update.'}`
        );

        if (autoDownload) {
          await downloadAndInstall();
        }
      } else {
        setStatus({ state: 'idle' });
      }
    } catch (error) {
      console.error('[AutoUpdater] Update check failed:', error);
      setStatus({ state: 'idle' });
    } finally {
      isCheckingRef.current = false;
    }
  }, [autoDownload, downloadAndInstall, notify]);

  /**
   * Restart the app (for "ready" state)
   */
  const restartApp = useCallback(async () => {
    await relaunch();
  }, []);

  /**
   * Dismiss the update banner
   */
  const dismiss = useCallback(() => {
    setDismissed(true);
  }, []);

  useEffect(() => {
    if (!enabled) {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    // Check immediately on mount
    checkForUpdates();

    const intervalMs = checkIntervalHours * 60 * 60 * 1000;
    intervalRef.current = window.setInterval(() => {
      checkForUpdates();
    }, intervalMs);

    return () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [enabled, checkIntervalHours, checkForUpdates]);

  return {
    status,
    dismissed,
    checkForUpdates,
    downloadAndInstall,
    restartApp,
    dismiss,
  };
}
