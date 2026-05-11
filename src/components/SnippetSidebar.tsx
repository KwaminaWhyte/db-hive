/**
 * Snippet Sidebar Component
 *
 * Displays saved query snippets with search, filtering by tags,
 * and actions to insert snippets into the editor, edit, or delete them.
 * Supports nested folders for organization.
 */

import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { QuerySnippet, SnippetFolder } from "../types/history";
import { useSnippetFolders } from "../hooks/useSnippetFolders";
import { SnippetFolderTree } from "./SnippetFolderTree";
import { SnippetFolderPicker } from "./SnippetFolderPicker";
import { Button } from "./ui/button";
import { ScrollArea } from "./ui/scroll-area";
import { Badge } from "./ui/badge";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Textarea } from "./ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "./ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";

interface SnippetSidebarProps {
  onInsertSnippet?: (query: string) => void;
}

const EXPANDED_KEY = "snippet-folder-expanded";

export function SnippetSidebar({ onInsertSnippet }: SnippetSidebarProps) {
  const [snippets, setSnippets] = useState<QuerySnippet[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedTag, setSelectedTag] = useState<string | null>(null);

  const {
    folders,
    createFolder,
    renameFolder,
    deleteFolder,
    assignSnippet,
    folderOf,
    isDescendant,
  } = useSnippetFolders();

  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem(EXPANDED_KEY) || "{}");
    } catch {
      return {};
    }
  });

  useEffect(() => {
    localStorage.setItem(EXPANDED_KEY, JSON.stringify(expanded));
  }, [expanded]);

  const toggleExpanded = useCallback((id: string) => {
    setExpanded((prev) => ({ ...prev, [id]: !(prev[id] ?? false) }));
  }, []);

  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [editingSnippet, setEditingSnippet] = useState<QuerySnippet | null>(
    null
  );
  const [deletingSnippet, setDeletingSnippet] = useState<QuerySnippet | null>(
    null
  );
  const [createParentId, setCreateParentId] = useState<string | null>(null);

  const [formName, setFormName] = useState("");
  const [formQuery, setFormQuery] = useState("");
  const [formDescription, setFormDescription] = useState("");
  const [formTags, setFormTags] = useState("");

  const [folderDialogMode, setFolderDialogMode] = useState<
    "create" | "rename" | null
  >(null);
  const [folderDialogParentId, setFolderDialogParentId] = useState<
    string | null
  >(null);
  const [folderDialogTarget, setFolderDialogTarget] =
    useState<SnippetFolder | null>(null);
  const [folderDialogName, setFolderDialogName] = useState("");

  const [deletingFolder, setDeletingFolder] = useState<SnippetFolder | null>(
    null
  );

  const [movingSnippet, setMovingSnippet] = useState<QuerySnippet | null>(null);

  const loadSnippets = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<QuerySnippet[]>("list_snippets", {
        tag: selectedTag,
      });
      setSnippets(result);
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    } finally {
      setLoading(false);
    }
  }, [selectedTag]);

  useEffect(() => {
    loadSnippets();
  }, [loadSnippets]);

  const filteredSnippets = useMemo(() => {
    let filtered = snippets;
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (s) =>
          s.name.toLowerCase().includes(q) ||
          s.query.toLowerCase().includes(q) ||
          s.description?.toLowerCase().includes(q)
      );
    }
    if (selectedTag) {
      filtered = filtered.filter((s) => s.tags?.includes(selectedTag));
    }
    return filtered;
  }, [snippets, searchQuery, selectedTag]);

  const matchedFolderIds = useMemo(() => {
    if (!searchQuery) return new Set<string>();
    const q = searchQuery.toLowerCase();
    const matched = new Set<string>();
    for (const f of folders) {
      if (f.name.toLowerCase().includes(q)) matched.add(f.id);
    }
    for (const s of filteredSnippets) {
      const fid = folderOf(s.id);
      if (fid) matched.add(fid);
    }
    const folderById = new Map(folders.map((f) => [f.id, f]));
    const withAncestors = new Set(matched);
    for (const id of matched) {
      let cur = folderById.get(id)?.parentId ?? null;
      while (cur) {
        withAncestors.add(cur);
        cur = folderById.get(cur)?.parentId ?? null;
      }
    }
    return withAncestors;
  }, [searchQuery, folders, filteredSnippets, folderOf]);

  const allTags = useMemo((): string[] => {
    const tagSet = new Set<string>();
    snippets.forEach((s) => s.tags?.forEach((t) => tagSet.add(t)));
    return Array.from(tagSet).sort();
  }, [snippets]);

  const resetForm = useCallback(() => {
    setFormName("");
    setFormQuery("");
    setFormDescription("");
    setFormTags("");
  }, []);

  const handleCreateSnippet = useCallback(async () => {
    try {
      const tags = formTags
        .split(",")
        .map((t) => t.trim())
        .filter((t) => t.length > 0);

      const newId = await invoke<string>("save_snippet", {
        snippet: {
          id: "",
          name: formName,
          query: formQuery,
          description: formDescription || undefined,
          tags: tags.length > 0 ? tags : undefined,
          createdAt: "",
          updatedAt: "",
        },
      });

      if (createParentId && newId) {
        await assignSnippet(newId, createParentId);
      }

      setIsCreateDialogOpen(false);
      setCreateParentId(null);
      resetForm();
      await loadSnippets();
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  }, [
    formTags,
    formName,
    formQuery,
    formDescription,
    createParentId,
    assignSnippet,
    resetForm,
    loadSnippets,
  ]);

  const handleEditSnippet = useCallback(async () => {
    if (!editingSnippet) return;
    try {
      const tags = formTags
        .split(",")
        .map((t) => t.trim())
        .filter((t) => t.length > 0);

      await invoke("save_snippet", {
        snippet: {
          ...editingSnippet,
          name: formName,
          query: formQuery,
          description: formDescription || undefined,
          tags: tags.length > 0 ? tags : undefined,
        },
      });

      setIsEditDialogOpen(false);
      setEditingSnippet(null);
      resetForm();
      await loadSnippets();
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  }, [
    editingSnippet,
    formTags,
    formName,
    formQuery,
    formDescription,
    resetForm,
    loadSnippets,
  ]);

  const handleDeleteSnippet = useCallback(async () => {
    if (!deletingSnippet) return;
    try {
      await invoke("delete_snippet", { snippetId: deletingSnippet.id });
      await assignSnippet(deletingSnippet.id, null);
      setIsDeleteDialogOpen(false);
      setDeletingSnippet(null);
      await loadSnippets();
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  }, [deletingSnippet, assignSnippet, loadSnippets]);

  const openEditDialog = useCallback((snippet: QuerySnippet) => {
    setEditingSnippet(snippet);
    setFormName(snippet.name);
    setFormQuery(snippet.query);
    setFormDescription(snippet.description || "");
    setFormTags(snippet.tags?.join(", ") || "");
    setIsEditDialogOpen(true);
  }, []);

  const openDeleteDialog = useCallback((snippet: QuerySnippet) => {
    setDeletingSnippet(snippet);
    setIsDeleteDialogOpen(true);
  }, []);

  const openCreateSnippetAt = useCallback(
    (parentId: string | null) => {
      setCreateParentId(parentId);
      resetForm();
      setIsCreateDialogOpen(true);
    },
    [resetForm]
  );

  const openCreateFolderDialog = useCallback((parentId: string | null) => {
    setFolderDialogMode("create");
    setFolderDialogParentId(parentId);
    setFolderDialogTarget(null);
    setFolderDialogName("");
  }, []);

  const openRenameFolderDialog = useCallback((folder: SnippetFolder) => {
    setFolderDialogMode("rename");
    setFolderDialogTarget(folder);
    setFolderDialogParentId(folder.parentId);
    setFolderDialogName(folder.name);
  }, []);

  const submitFolderDialog = useCallback(async () => {
    const name = folderDialogName.trim();
    if (!name) return;
    if (folderDialogMode === "create") {
      const f = await createFolder(name, folderDialogParentId);
      if (folderDialogParentId) {
        setExpanded((prev) => ({ ...prev, [folderDialogParentId]: true }));
      }
      setExpanded((prev) => ({ ...prev, [f.id]: true }));
    } else if (folderDialogMode === "rename" && folderDialogTarget) {
      await renameFolder(folderDialogTarget.id, name);
    }
    setFolderDialogMode(null);
    setFolderDialogTarget(null);
    setFolderDialogName("");
  }, [
    folderDialogMode,
    folderDialogName,
    folderDialogParentId,
    folderDialogTarget,
    createFolder,
    renameFolder,
  ]);

  const confirmDeleteFolder = useCallback(async () => {
    if (!deletingFolder) return;
    await deleteFolder(deletingFolder.id);
    setDeletingFolder(null);
  }, [deletingFolder, deleteFolder]);

  const handleSnippetClick = useCallback((s: QuerySnippet) => {
    openEditDialog(s);
  }, [openEditDialog]);

  const handleSnippetInsert = useCallback(
    (s: QuerySnippet) => {
      onInsertSnippet?.(s.query);
    },
    [onInsertSnippet]
  );

  const handleMoveSnippetPick = useCallback(
    async (folderId: string | null) => {
      if (!movingSnippet) return;
      await assignSnippet(movingSnippet.id, folderId);
      if (folderId) {
        setExpanded((prev) => ({ ...prev, [folderId]: true }));
      }
      setMovingSnippet(null);
    },
    [movingSnippet, assignSnippet]
  );

  const movingSnippetDisabledSet = useMemo(() => new Set<string>(), []);
  void isDescendant;

  return (
    <div className="flex flex-col h-full border-l bg-background">
      <div className="p-4 border-b shrink-0">
        <div className="flex items-center justify-between mb-3 gap-2">
          <h2 className="text-lg font-semibold">Snippets</h2>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => openCreateFolderDialog(null)}
            >
              New Folder
            </Button>
            <Dialog
              open={isCreateDialogOpen}
              onOpenChange={(open) => {
                setIsCreateDialogOpen(open);
                if (!open) setCreateParentId(null);
              }}
            >
              <DialogTrigger asChild>
                <Button size="sm" onClick={() => setCreateParentId(null)}>
                  New Snippet
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>Create Snippet</DialogTitle>
                  <DialogDescription>
                    Save a query snippet for quick access later
                  </DialogDescription>
                </DialogHeader>
                <div className="space-y-4 py-4">
                  <div className="space-y-2">
                    <Label htmlFor="name">Name</Label>
                    <Input
                      id="name"
                      placeholder="My Query"
                      value={formName}
                      onChange={(e) => setFormName(e.target.value)}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="query">Query</Label>
                    <Textarea
                      id="query"
                      placeholder="SELECT * FROM users"
                      rows={5}
                      value={formQuery}
                      onChange={(e) => setFormQuery(e.target.value)}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="description">Description (optional)</Label>
                    <Input
                      id="description"
                      placeholder="What this query does"
                      value={formDescription}
                      onChange={(e) => setFormDescription(e.target.value)}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="tags">
                      Tags (comma-separated, optional)
                    </Label>
                    <Input
                      id="tags"
                      placeholder="backup, users, admin"
                      value={formTags}
                      onChange={(e) => setFormTags(e.target.value)}
                    />
                  </div>
                </div>
                <DialogFooter>
                  <Button
                    variant="outline"
                    onClick={() => {
                      setIsCreateDialogOpen(false);
                      setCreateParentId(null);
                      resetForm();
                    }}
                  >
                    Cancel
                  </Button>
                  <Button
                    onClick={handleCreateSnippet}
                    disabled={!formName || !formQuery}
                  >
                    Create
                  </Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          </div>
        </div>

        <Input
          type="search"
          placeholder="Search snippets..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="mb-3"
        />

        {allTags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            <Badge
              variant={selectedTag === null ? "default" : "outline"}
              className="cursor-pointer"
              onClick={() => setSelectedTag(null)}
            >
              All
            </Badge>
            {allTags.map((tag) => (
              <Badge
                key={tag}
                variant={selectedTag === tag ? "default" : "outline"}
                className="cursor-pointer"
                onClick={() => setSelectedTag(tag === selectedTag ? null : tag)}
              >
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </div>

      {error && (
        <div className="px-4 py-2 bg-destructive/10 text-destructive text-sm shrink-0">
          {error}
        </div>
      )}

      <ScrollArea className="flex-1">
        {loading ? (
          <div className="flex items-center justify-center p-8">
            <div className="text-muted-foreground text-sm">
              Loading snippets...
            </div>
          </div>
        ) : filteredSnippets.length === 0 && folders.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-8 text-center">
            <div className="text-muted-foreground mb-2">
              {snippets.length === 0
                ? "No snippets yet"
                : "No matching snippets"}
            </div>
            <div className="text-sm text-muted-foreground">
              {snippets.length === 0
                ? "Create your first snippet to get started"
                : "Try a different search or filter"}
            </div>
          </div>
        ) : (
          <SnippetFolderTree
            folders={folders}
            snippets={filteredSnippets}
            folderOf={folderOf}
            expanded={expanded}
            onToggleExpanded={toggleExpanded}
            onSnippetClick={handleSnippetClick}
            onSnippetInsert={onInsertSnippet ? handleSnippetInsert : undefined}
            onSnippetEdit={openEditDialog}
            onSnippetDelete={openDeleteDialog}
            onSnippetMove={(s) => setMovingSnippet(s)}
            onFolderNewFolder={(parentId) => openCreateFolderDialog(parentId)}
            onFolderNewSnippet={(parentId) => openCreateSnippetAt(parentId)}
            onFolderRename={openRenameFolderDialog}
            onFolderDelete={(f) => setDeletingFolder(f)}
            matchedFolderIds={matchedFolderIds}
            searchActive={searchQuery.length > 0}
          />
        )}
      </ScrollArea>

      {/* Edit Dialog */}
      <Dialog open={isEditDialogOpen} onOpenChange={setIsEditDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Snippet</DialogTitle>
            <DialogDescription>
              Update your saved query snippet
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="edit-name">Name</Label>
              <Input
                id="edit-name"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-query">Query</Label>
              <Textarea
                id="edit-query"
                rows={5}
                value={formQuery}
                onChange={(e) => setFormQuery(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-description">Description (optional)</Label>
              <Input
                id="edit-description"
                value={formDescription}
                onChange={(e) => setFormDescription(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-tags">
                Tags (comma-separated, optional)
              </Label>
              <Input
                id="edit-tags"
                value={formTags}
                onChange={(e) => setFormTags(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setIsEditDialogOpen(false);
                setEditingSnippet(null);
                resetForm();
              }}
            >
              Cancel
            </Button>
            <Button
              onClick={handleEditSnippet}
              disabled={!formName || !formQuery}
            >
              Save Changes
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Snippet Dialog */}
      <AlertDialog
        open={isDeleteDialogOpen}
        onOpenChange={setIsDeleteDialogOpen}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete snippet?</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete "{deletingSnippet?.name}"? This
              action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setDeletingSnippet(null)}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction onClick={handleDeleteSnippet}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Folder Create/Rename Dialog */}
      <Dialog
        open={folderDialogMode !== null}
        onOpenChange={(open) => {
          if (!open) {
            setFolderDialogMode(null);
            setFolderDialogTarget(null);
            setFolderDialogName("");
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {folderDialogMode === "rename" ? "Rename Folder" : "New Folder"}
            </DialogTitle>
            <DialogDescription>
              {folderDialogMode === "rename"
                ? "Enter a new name for this folder"
                : "Enter a name for the new folder"}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-4">
            <Label htmlFor="folder-name">Name</Label>
            <Input
              id="folder-name"
              autoFocus
              value={folderDialogName}
              onChange={(e) => setFolderDialogName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submitFolderDialog();
              }}
            />
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setFolderDialogMode(null);
                setFolderDialogTarget(null);
                setFolderDialogName("");
              }}
            >
              Cancel
            </Button>
            <Button
              onClick={submitFolderDialog}
              disabled={!folderDialogName.trim()}
            >
              {folderDialogMode === "rename" ? "Rename" : "Create"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Folder Dialog */}
      <AlertDialog
        open={deletingFolder !== null}
        onOpenChange={(open) => {
          if (!open) setDeletingFolder(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete folder?</AlertDialogTitle>
            <AlertDialogDescription>
              Delete "{deletingFolder?.name}"? Its snippets and subfolders will
              be moved to the root.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDeleteFolder}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Move Snippet Picker */}
      <SnippetFolderPicker
        open={movingSnippet !== null}
        onOpenChange={(open) => {
          if (!open) setMovingSnippet(null);
        }}
        folders={folders}
        disabledFolderIds={movingSnippetDisabledSet}
        title="Move snippet to folder"
        description={`Pick a destination for "${movingSnippet?.name ?? ""}"`}
        onPick={handleMoveSnippetPick}
      />
    </div>
  );
}
