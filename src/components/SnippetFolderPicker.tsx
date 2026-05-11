import { FC, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Folder } from "lucide-react";
import { SnippetFolder } from "../types/history";
import { Button } from "./ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";

interface SnippetFolderPickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  folders: SnippetFolder[];
  disabledFolderIds?: Set<string>;
  title?: string;
  description?: string;
  onPick: (folderId: string | null) => void;
}

export const SnippetFolderPicker: FC<SnippetFolderPickerProps> = ({
  open,
  onOpenChange,
  folders,
  disabledFolderIds,
  title = "Move to folder",
  description = "Pick a destination folder",
  onPick,
}) => {
  const [selected, setSelected] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const byParent = useMemo(() => {
    const m = new Map<string | null, SnippetFolder[]>();
    for (const f of folders) {
      const arr = m.get(f.parentId) || [];
      arr.push(f);
      m.set(f.parentId, arr);
    }
    for (const arr of m.values()) arr.sort((a, b) => a.name.localeCompare(b.name));
    return m;
  }, [folders]);

  const renderNode = (folder: SnippetFolder, depth: number) => {
    const children = byParent.get(folder.id) || [];
    const isExpanded = expanded[folder.id] ?? true;
    const disabled = disabledFolderIds?.has(folder.id) ?? false;
    const isSelected = selected === folder.id;
    return (
      <div key={folder.id}>
        <div
          className={`flex items-center gap-1 px-2 py-1 rounded text-sm select-none ${
            disabled
              ? "opacity-40 cursor-not-allowed"
              : isSelected
              ? "bg-accent cursor-pointer"
              : "hover:bg-accent/50 cursor-pointer"
          }`}
          style={{ paddingLeft: 8 + depth * 16 }}
          onClick={() => {
            if (disabled) return;
            setSelected(folder.id);
          }}
        >
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded((prev) => ({ ...prev, [folder.id]: !isExpanded }));
            }}
            className="shrink-0"
          >
            {children.length > 0 ? (
              isExpanded ? (
                <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
              )
            ) : (
              <span className="inline-block w-3.5" />
            )}
          </button>
          <Folder className="h-4 w-4 shrink-0 text-amber-500" />
          <span className="truncate">{folder.name}</span>
        </div>
        {isExpanded && children.map((c) => renderNode(c, depth + 1))}
      </div>
    );
  };

  const roots = byParent.get(null) || [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="max-h-[360px] overflow-auto border rounded p-1">
          <div
            className={`flex items-center gap-1 px-2 py-1 rounded text-sm select-none cursor-pointer ${
              selected === null ? "bg-accent" : "hover:bg-accent/50"
            }`}
            onClick={() => setSelected(null)}
          >
            <span className="inline-block w-3.5" />
            <Folder className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="truncate">(Root)</span>
          </div>
          {roots.map((r) => renderNode(r, 0))}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            onClick={() => {
              onPick(selected);
              onOpenChange(false);
            }}
          >
            Move here
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
