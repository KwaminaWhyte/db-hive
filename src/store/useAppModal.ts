/**
 * Lightweight global modal store.
 *
 * Tracks which global overlay modal (Settings / About / Plugins) is open
 * so that titlebar menu items can open them without navigating away from
 * the current route (preserving in-progress query editor state, etc.).
 *
 * Implemented with useSyncExternalStore to avoid adding a new dependency
 * (zustand) to the project.
 */

import { useSyncExternalStore } from "react";

export type AppModal = "settings" | "about" | "plugins" | "migrations" | null;

type Listener = () => void;

let currentModal: AppModal = null;
const listeners = new Set<Listener>();

function emit() {
  for (const l of listeners) l();
}

function subscribe(listener: Listener) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): AppModal {
  return currentModal;
}

export function openAppModal(modal: Exclude<AppModal, null>) {
  if (currentModal === modal) return;
  currentModal = modal;
  emit();
}

export function closeAppModal() {
  if (currentModal === null) return;
  currentModal = null;
  emit();
}

export function setAppModal(modal: AppModal) {
  if (currentModal === modal) return;
  currentModal = modal;
  emit();
}

export function useAppModal(): AppModal {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
