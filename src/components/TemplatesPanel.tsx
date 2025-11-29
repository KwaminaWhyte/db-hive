import { useState, useMemo } from "react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { ScrollArea } from "./ui/scroll-area";
import { Badge } from "./ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
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
import {
  Search,
  Plus,
  MoreVertical,
  Play,
  Edit,
  Copy,
  Trash2,
  Download,
  Upload,
  FileCode,
  FolderOpen,
  X,
} from "lucide-react";
import { useQueryTemplates } from "../hooks/useQueryTemplates";
import { QueryTemplateDialog } from "./QueryTemplateDialog";
import { QueryTemplate } from "../types/templates";
import { toast } from "sonner";

interface TemplatesPanelProps {
  onExecuteQuery: (sql: string) => void;
}

export function TemplatesPanel({ onExecuteQuery }: TemplatesPanelProps) {
  const {
    templates,
    loading,
    addTemplate,
    updateTemplate,
    deleteTemplate,
    duplicateTemplate,
    exportTemplates,
    importTemplates,
    getCategories,
  } = useQueryTemplates();

  const [searchQuery, setSearchQuery] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const [dialogMode, setDialogMode] = useState<"create" | "edit" | "execute">("create");
  const [dialogOpen, setDialogOpen] = useState(false);
  const [selectedTemplate, setSelectedTemplate] = useState<QueryTemplate | null>(null);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [templateToDelete, setTemplateToDelete] = useState<QueryTemplate | null>(null);

  const categories = useMemo(() => getCategories(), [getCategories]);

  const filteredTemplates = useMemo(() => {
    let filtered = templates;

    // Filter by category
    if (selectedCategory) {
      filtered = filtered.filter((t) => t.category === selectedCategory);
    }

    // Filter by search
    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (t) =>
          t.name.toLowerCase().includes(query) ||
          t.description?.toLowerCase().includes(query) ||
          t.tags?.some((tag) => tag.toLowerCase().includes(query))
      );
    }

    return filtered;
  }, [templates, selectedCategory, searchQuery]);

  const handleCreate = () => {
    setSelectedTemplate(null);
    setDialogMode("create");
    setDialogOpen(true);
  };

  const handleEdit = (template: QueryTemplate) => {
    setSelectedTemplate(template);
    setDialogMode("edit");
    setDialogOpen(true);
  };

  const handleExecute = (template: QueryTemplate) => {
    if (template.parameters.length === 0) {
      // No parameters, execute directly
      onExecuteQuery(template.sql);
    } else {
      // Open dialog for parameter input
      setSelectedTemplate(template);
      setDialogMode("execute");
      setDialogOpen(true);
    }
  };

  const handleDuplicate = (template: QueryTemplate) => {
    const newTemplate = duplicateTemplate(template.id);
    if (newTemplate) {
      toast.success(`Template duplicated as "${newTemplate.name}"`);
    }
  };

  const handleDelete = (template: QueryTemplate) => {
    setTemplateToDelete(template);
    setDeleteDialogOpen(true);
  };

  const confirmDelete = () => {
    if (templateToDelete) {
      deleteTemplate(templateToDelete.id);
      toast.success("Template deleted");
      setTemplateToDelete(null);
    }
    setDeleteDialogOpen(false);
  };

  const handleImport = async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
        try {
          const count = await importTemplates(file);
          toast.success(`Imported ${count} template(s)`);
        } catch (err) {
          toast.error("Failed to import templates");
        }
      }
    };
    input.click();
  };

  const handleSaveTemplate = (
    templateData: Omit<QueryTemplate, "id" | "createdAt" | "updatedAt">
  ) => {
    if (dialogMode === "edit" && selectedTemplate) {
      updateTemplate(selectedTemplate.id, templateData);
      toast.success("Template updated");
    } else {
      addTemplate(templateData);
      toast.success("Template created");
    }
  };

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center text-muted-foreground">
        Loading templates...
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-4 border-b space-y-3">
        {/* Search */}
        <div className="relative">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="Search templates..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-9 pr-8"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery("")}
              className="absolute right-3 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={handleCreate} className="flex-1">
            <Plus className="h-4 w-4 mr-1" />
            New Template
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm">
                <MoreVertical className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={handleImport}>
                <Upload className="h-4 w-4 mr-2" />
                Import Templates
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={exportTemplates}
                disabled={templates.length === 0}
              >
                <Download className="h-4 w-4 mr-2" />
                Export Templates
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Category Filter */}
        {categories.length > 0 && (
          <div className="flex flex-wrap gap-1">
            <Badge
              variant={selectedCategory === null ? "default" : "outline"}
              className="cursor-pointer"
              onClick={() => setSelectedCategory(null)}
            >
              All
            </Badge>
            {categories.filter(Boolean).map((cat, idx) => (
              <Badge
                key={cat || `cat-${idx}`}
                variant={selectedCategory === cat ? "default" : "outline"}
                className="cursor-pointer"
                onClick={() => setSelectedCategory(cat)}
              >
                {cat}
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Template List */}
      <ScrollArea className="flex-1">
        <div className="p-2 space-y-1">
          {filteredTemplates.length > 0 ? (
            filteredTemplates.map((template) => (
              <div
                key={template.id}
                className="group p-3 rounded-lg border bg-card hover:bg-accent/50 transition-colors"
              >
                <div className="flex items-start justify-between gap-2">
                  <div
                    className="flex-1 min-w-0 cursor-pointer"
                    onClick={() => handleExecute(template)}
                  >
                    <div className="flex items-center gap-2">
                      <FileCode className="h-4 w-4 text-blue-500 shrink-0" />
                      <span className="font-medium text-sm truncate">
                        {template.name}
                      </span>
                    </div>
                    {template.description && (
                      <p className="text-xs text-muted-foreground mt-1 line-clamp-2">
                        {template.description}
                      </p>
                    )}
                    <div className="flex items-center gap-2 mt-2">
                      {template.category && (
                        <Badge variant="secondary" className="text-xs">
                          <FolderOpen className="h-3 w-3 mr-1" />
                          {template.category}
                        </Badge>
                      )}
                      {template.parameters.length > 0 && (
                        <Badge variant="outline" className="text-xs">
                          {template.parameters.length} param
                          {template.parameters.length !== 1 ? "s" : ""}
                        </Badge>
                      )}
                    </div>
                  </div>

                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 w-7 p-0 opacity-0 group-hover:opacity-100"
                      >
                        <MoreVertical className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={() => handleExecute(template)}>
                        <Play className="h-4 w-4 mr-2" />
                        Execute
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={() => handleEdit(template)}>
                        <Edit className="h-4 w-4 mr-2" />
                        Edit
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={() => handleDuplicate(template)}>
                        <Copy className="h-4 w-4 mr-2" />
                        Duplicate
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onClick={() => handleDelete(template)}
                        className="text-destructive"
                      >
                        <Trash2 className="h-4 w-4 mr-2" />
                        Delete
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
            ))
          ) : (
            <div className="text-center py-8 text-muted-foreground">
              {searchQuery || selectedCategory ? (
                <p className="text-sm">No templates found</p>
              ) : (
                <div className="space-y-2">
                  <FileCode className="h-12 w-12 mx-auto opacity-50" />
                  <p className="text-sm">No templates yet</p>
                  <p className="text-xs">
                    Create a template to save reusable queries with parameters
                  </p>
                </div>
              )}
            </div>
          )}
        </div>
      </ScrollArea>

      {/* Template Dialog */}
      <QueryTemplateDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        template={selectedTemplate}
        mode={dialogMode}
        onSave={handleSaveTemplate}
        onExecute={onExecuteQuery}
      />

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Template</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete "{templateToDelete?.name}"? This
              action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
