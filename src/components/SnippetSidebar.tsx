/**
 * Snippet Sidebar Component
 *
 * Displays saved query snippets with search, filtering by tags,
 * and actions to insert snippets into the editor, edit, or delete them.
 */

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { QuerySnippet } from "../types/history";
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
  /** Callback to insert snippet query into editor */
  onInsertSnippet?: (query: string) => void;
}

export function SnippetSidebar({ onInsertSnippet }: SnippetSidebarProps) {
  const [snippets, setSnippets] = useState<QuerySnippet[]>([]);
  const [filteredSnippets, setFilteredSnippets] = useState<QuerySnippet[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedTag, setSelectedTag] = useState<string | null>(null);

  // Dialog states
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [editingSnippet, setEditingSnippet] = useState<QuerySnippet | null>(
    null
  );
  const [deletingSnippet, setDeletingSnippet] = useState<QuerySnippet | null>(
    null
  );

  // Form states
  const [formName, setFormName] = useState("");
  const [formQuery, setFormQuery] = useState("");
  const [formDescription, setFormDescription] = useState("");
  const [formTags, setFormTags] = useState("");

  useEffect(() => {
    loadSnippets();
  }, []);

  useEffect(() => {
    filterSnippets();
  }, [snippets, searchQuery, selectedTag]);

  const loadSnippets = async () => {
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
  };

  const filterSnippets = () => {
    let filtered = snippets;

    // Filter by search query
    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (s) =>
          s.name.toLowerCase().includes(query) ||
          s.query.toLowerCase().includes(query) ||
          s.description?.toLowerCase().includes(query)
      );
    }

    // Filter by selected tag
    if (selectedTag) {
      filtered = filtered.filter((s) => s.tags?.includes(selectedTag));
    }

    setFilteredSnippets(filtered);
  };

  const getAllTags = (): string[] => {
    const tagSet = new Set<string>();
    snippets.forEach((s) => {
      s.tags?.forEach((tag) => tagSet.add(tag));
    });
    return Array.from(tagSet).sort();
  };

  const handleCreateSnippet = async () => {
    try {
      const tags = formTags
        .split(",")
        .map((t) => t.trim())
        .filter((t) => t.length > 0);

      await invoke("save_snippet", {
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

      setIsCreateDialogOpen(false);
      resetForm();
      await loadSnippets();
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  };

  const handleEditSnippet = async () => {
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
  };

  const handleDeleteSnippet = async () => {
    if (!deletingSnippet) return;

    try {
      await invoke("delete_snippet", {
        snippetId: deletingSnippet.id,
      });

      setIsDeleteDialogOpen(false);
      setDeletingSnippet(null);
      await loadSnippets();
    } catch (err) {
      setError(
        typeof err === "string" ? err : (err as any)?.message || String(err)
      );
    }
  };

  const openEditDialog = (snippet: QuerySnippet) => {
    setEditingSnippet(snippet);
    setFormName(snippet.name);
    setFormQuery(snippet.query);
    setFormDescription(snippet.description || "");
    setFormTags(snippet.tags?.join(", ") || "");
    setIsEditDialogOpen(true);
  };

  const openDeleteDialog = (snippet: QuerySnippet) => {
    setDeletingSnippet(snippet);
    setIsDeleteDialogOpen(true);
  };

  const resetForm = () => {
    setFormName("");
    setFormQuery("");
    setFormDescription("");
    setFormTags("");
  };

  return (
    <div className="flex flex-col h-full border-l bg-background">
      {/* Header */}
      <div className="p-4 border-b shrink-0">
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-lg font-semibold">Snippets</h2>
          <Dialog
            open={isCreateDialogOpen}
            onOpenChange={setIsCreateDialogOpen}
          >
            <DialogTrigger asChild>
              <Button size="sm">New Snippet</Button>
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
                  <Label htmlFor="tags">Tags (comma-separated, optional)</Label>
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

        {/* Search */}
        <Input
          type="search"
          placeholder="Search snippets..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="mb-3"
        />

        {/* Tag Filter */}
        {getAllTags().length > 0 && (
          <div className="flex flex-wrap gap-1">
            <Badge
              variant={selectedTag === null ? "default" : "outline"}
              className="cursor-pointer"
              onClick={() => setSelectedTag(null)}
            >
              All
            </Badge>
            {getAllTags().map((tag) => (
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

      {/* Error */}
      {error && (
        <div className="px-4 py-2 bg-destructive/10 text-destructive text-sm shrink-0">
          {error}
        </div>
      )}

      {/* Snippet List */}
      <ScrollArea className="flex-1">
        {loading ? (
          <div className="flex items-center justify-center p-8">
            <div className="text-muted-foreground text-sm">
              Loading snippets...
            </div>
          </div>
        ) : filteredSnippets.length === 0 ? (
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
          <div className="p-4 space-y-2">
            {filteredSnippets.map((snippet) => (
              <div
                key={snippet.id}
                className="p-3 border rounded-lg hover:bg-accent/50 transition-colors"
              >
                <div className="flex items-start justify-between gap-2 mb-2">
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate">{snippet.name}</div>
                    {snippet.description && (
                      <div className="text-xs text-muted-foreground mt-1">
                        {snippet.description}
                      </div>
                    )}
                  </div>
                </div>

                {snippet.tags && snippet.tags.length > 0 && (
                  <div className="flex flex-wrap gap-1 mb-2">
                    {snippet.tags.map((tag) => (
                      <Badge key={tag} variant="secondary" className="text-xs">
                        {tag}
                      </Badge>
                    ))}
                  </div>
                )}

                <div className="bg-muted rounded p-2 mb-2 text-xs font-mono overflow-x-auto">
                  <pre className="whitespace-pre-wrap">{snippet.query}</pre>
                </div>

                <div className="flex items-center gap-2">
                  {onInsertSnippet && (
                    <Button
                      variant="outline"
                      size="sm"
                      className="flex-1"
                      onClick={() => onInsertSnippet(snippet.query)}
                    >
                      Insert
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => openEditDialog(snippet)}
                  >
                    Edit
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => openDeleteDialog(snippet)}
                  >
                    Delete
                  </Button>
                </div>
              </div>
            ))}
          </div>
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

      {/* Delete Dialog */}
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
    </div>
  );
}
