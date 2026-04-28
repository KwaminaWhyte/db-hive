import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppSettings } from '@/types';
import { defaultSettings } from '@/types';

/**
 * Custom DOM event name used to broadcast settings changes app-wide.
 * Components that mutate settings dispatch this so other mounted
 * `useSettings` consumers re-fetch from the backing store.
 */
export const SETTINGS_CHANGED_EVENT = 'db-hive:settings-changed';

/**
 * Notify all `useSettings` consumers that settings have changed.
 * Call this after `update_settings` / `reset_settings` invocations.
 */
export function broadcastSettingsChanged() {
  window.dispatchEvent(new CustomEvent(SETTINGS_CHANGED_EVENT));
}

/**
 * Hook to load and manage application settings.
 *
 * Auto-refreshes when `broadcastSettingsChanged()` is called from
 * any component, so keyboard-shortcut bindings update without a
 * page reload.
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
      setSettings(defaultSettings);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  useEffect(() => {
    const handler = () => {
      loadSettings();
    };
    window.addEventListener(SETTINGS_CHANGED_EVENT, handler);
    return () => window.removeEventListener(SETTINGS_CHANGED_EVENT, handler);
  }, [loadSettings]);

  return { settings, loading, reload: loadSettings };
}
