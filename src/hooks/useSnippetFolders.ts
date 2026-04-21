import { useCallback, useEffect, useState } from "react";
import { Store } from "@tauri-apps/plugin-store";
import { SnippetFolder } from "../types/history";

const STORE_FILE = "snippet-folders.json";
const FOLDERS_KEY = "snippet-folders";
const ASSIGNMENTS_KEY = "snippet-folder-assignments";

export function useSnippetFolders() {
  const [store, setStore] = useState<Store | null>(null);
  const [folders, setFolders] = useState<SnippetFolder[]>([]);
  const [assignments, setAssignments] = useState<Record<string, string>>({});
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let mounted = true;
    Store.load(STORE_FILE).then(async (s) => {
      if (!mounted) return;
      const f = (await s.get<SnippetFolder[]>(FOLDERS_KEY)) || [];
      const a = (await s.get<Record<string, string>>(ASSIGNMENTS_KEY)) || {};
      setStore(s);
      setFolders(f);
      setAssignments(a);
      setReady(true);
    });
    return () => {
      mounted = false;
    };
  }, []);

  const persistFolders = useCallback(
    async (next: SnippetFolder[]) => {
      setFolders(next);
      if (store) {
        await store.set(FOLDERS_KEY, next);
        await store.save();
      }
    },
    [store]
  );

  const persistAssignments = useCallback(
    async (next: Record<string, string>) => {
      setAssignments(next);
      if (store) {
        await store.set(ASSIGNMENTS_KEY, next);
        await store.save();
      }
    },
    [store]
  );

  const createFolder = useCallback(
    async (name: string, parentId: string | null) => {
      const folder: SnippetFolder = {
        id: `fld_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
        name,
        parentId,
        createdAt: Date.now(),
      };
      await persistFolders([...folders, folder]);
      return folder;
    },
    [folders, persistFolders]
  );

  const renameFolder = useCallback(
    async (id: string, name: string) => {
      await persistFolders(
        folders.map((f) => (f.id === id ? { ...f, name } : f))
      );
    },
    [folders, persistFolders]
  );

  // Cascade delete behavior: children (subfolders + snippets) are moved to root
  // rather than deleted. This is the least destructive option.
  const deleteFolder = useCallback(
    async (id: string) => {
      const nextFolders = folders
        .filter((f) => f.id !== id)
        .map((f) => (f.parentId === id ? { ...f, parentId: null } : f));
      const nextAssignments: Record<string, string> = {};
      for (const [snippetId, folderId] of Object.entries(assignments)) {
        if (folderId !== id) nextAssignments[snippetId] = folderId;
      }
      await persistFolders(nextFolders);
      await persistAssignments(nextAssignments);
    },
    [folders, assignments, persistFolders, persistAssignments]
  );

  const isDescendant = useCallback(
    (folderId: string, possibleAncestorId: string): boolean => {
      let current: string | null = folderId;
      while (current) {
        if (current === possibleAncestorId) return true;
        const parent: SnippetFolder | undefined = folders.find(
          (f) => f.id === current
        );
        current = parent?.parentId ?? null;
      }
      return false;
    },
    [folders]
  );

  const moveFolder = useCallback(
    async (id: string, newParentId: string | null) => {
      if (id === newParentId) return;
      if (newParentId && isDescendant(newParentId, id)) return;
      await persistFolders(
        folders.map((f) => (f.id === id ? { ...f, parentId: newParentId } : f))
      );
    },
    [folders, isDescendant, persistFolders]
  );

  const assignSnippet = useCallback(
    async (snippetId: string, folderId: string | null) => {
      const next = { ...assignments };
      if (folderId === null) {
        delete next[snippetId];
      } else {
        next[snippetId] = folderId;
      }
      await persistAssignments(next);
    },
    [assignments, persistAssignments]
  );

  const folderOf = useCallback(
    (snippetId: string): string | null => assignments[snippetId] ?? null,
    [assignments]
  );

  return {
    ready,
    folders,
    assignments,
    createFolder,
    renameFolder,
    deleteFolder,
    moveFolder,
    assignSnippet,
    folderOf,
    isDescendant,
  };
}
