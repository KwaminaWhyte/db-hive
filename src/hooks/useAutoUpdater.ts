import { useEffect, useRef, useCallback } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';

interface UseAutoUpdaterOptions {
  enabled: boolean;
  autoDownload: boolean;
  autoInstall: boolean;
  checkIntervalHours: number;
}

/**
 * Custom hook for automatic update checking with system notifications
 *
 * Features:
 * - Automatic periodic update checking
 * - System notifications for update events
 * - Optional auto-download and auto-install
 * - Configurable check interval
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

  /**
   * Request notification permission if not already granted
   */
  const ensureNotificationPermission = useCallback(async () => {
    let permissionGranted = await isPermissionGranted();

    if (!permissionGranted) {
      const permission = await requestPermission();
      permissionGranted = permission === 'granted';
    }

    return permissionGranted;
  }, []);

  /**
   * Send a system notification
   */
  const notify = useCallback(async (title: string, body: string) => {
    const hasPermission = await ensureNotificationPermission();
    if (hasPermission) {
      await sendNotification({ title, body });
    }
  }, [ensureNotificationPermission]);

  /**
   * Check for updates
   */
  const checkForUpdates = useCallback(async () => {
    // Prevent multiple simultaneous checks
    if (isCheckingRef.current) {
      console.log('[AutoUpdater] Update check already in progress, skipping');
      return;
    }

    isCheckingRef.current = true;
    console.log('[AutoUpdater] Checking for updates...');

    try {
      const update = await check();

      if (update?.available) {
        console.log(`[AutoUpdater] Update available: ${update.version}`);

        // Notify user about available update
        await notify(
          'Update Available',
          `DB-Hive ${update.version} is available. ${autoDownload ? 'Downloading...' : 'Click to update.'}`
        );

        if (autoDownload) {
          console.log('[AutoUpdater] Auto-download enabled, downloading update...');

          try {
            await update.downloadAndInstall((event) => {
              switch (event.event) {
                case 'Started':
                  console.log('[AutoUpdater] Download started');
                  break;
                case 'Progress':
                  console.log(`[AutoUpdater] Downloaded ${event.data.chunkLength} bytes`);
                  break;
                case 'Finished':
                  console.log('[AutoUpdater] Download finished');
                  break;
              }
            });

            console.log('[AutoUpdater] Update downloaded and installed successfully');

            if (autoInstall) {
              // Notify about automatic restart
              await notify(
                'Update Installed',
                'DB-Hive has been updated. Restarting in 3 seconds...'
              );

              console.log('[AutoUpdater] Auto-install enabled, restarting app in 3 seconds...');
              setTimeout(async () => {
                await relaunch();
              }, 3000);
            } else {
              // Notify that restart is needed
              await notify(
                'Update Ready',
                'DB-Hive has been updated. Please restart the application to apply changes.'
              );
            }
          } catch (error) {
            console.error('[AutoUpdater] Failed to download/install update:', error);
            await notify(
              'Update Failed',
              error instanceof Error ? error.message : 'Failed to install update'
            );
          }
        }
      } else {
        console.log('[AutoUpdater] No updates available');
      }
    } catch (error) {
      console.error('[AutoUpdater] Update check failed:', error);
      // Don't notify about check failures in background
    } finally {
      isCheckingRef.current = false;
    }
  }, [autoDownload, autoInstall, notify]);

  /**
   * Start automatic update checking
   */
  useEffect(() => {
    if (!enabled) {
      console.log('[AutoUpdater] Automatic updates disabled');

      // Clear existing interval if any
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current);
        intervalRef.current = null;
      }

      return;
    }

    console.log(`[AutoUpdater] Enabled with ${checkIntervalHours}h interval`);

    // Check immediately on mount
    checkForUpdates();

    // Set up periodic checking
    const intervalMs = checkIntervalHours * 60 * 60 * 1000; // Convert hours to ms
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
    checkForUpdates,
  };
}
