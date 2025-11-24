import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface GeneralSettings {
  language: string;
  defaultDatabase?: string;
  startupBehavior: string;
  autoSaveConnections: boolean;
  enableTelemetry: boolean;
  autoCheckUpdates: boolean;
  autoDownloadUpdates: boolean;
  autoInstallUpdates: boolean;
  updateCheckIntervalHours: number;
}

interface AppSettings {
  general: GeneralSettings;
  theme: any;
  query: any;
  shortcuts: any;
}

/**
 * Hook to load and manage application settings
 */
export function useSettings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);

  const loadSettings = useCallback(async () => {
    try {
      const loadedSettings = await invoke<AppSettings>('get_settings');
      setSettings(loadedSettings);
    } catch (error) {
      console.error('Failed to load settings:', error);
      // Use defaults if loading fails
      setSettings({
        general: {
          language: 'en',
          startupBehavior: 'showConnectionList',
          autoSaveConnections: true,
          enableTelemetry: false,
          autoCheckUpdates: true,
          autoDownloadUpdates: false,
          autoInstallUpdates: false,
          updateCheckIntervalHours: 24,
        },
        theme: {},
        query: {},
        shortcuts: {},
      });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  return { settings, loading, reload: loadSettings };
}
