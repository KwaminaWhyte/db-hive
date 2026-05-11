import { FC, useMemo } from "react";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FileCode,
} from "lucide-react";
import { QuerySnippet, SnippetFolder } from "../types/history";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "./ui/context-menu";

export interface SnippetFolderTreeProps {
  folders: SnippetFolder[];
  snippets: QuerySnippet[];
  folderOf: (snippetId: string) => string | null;
  expanded: Record<string, boolean>;
  onToggleExpanded: (id: string) => void;
  onSnippetClick: (snippet: QuerySnippet) => void;
  onSnippetInsert?: (snippet: QuerySnippet) => void;
  onSnippetEdit: (snippet: QuerySnippet) => void;
  onSnippetDelete: (snippet: QuerySnippet) => void;
  onSnippetMove: (snippet: QuerySnippet) => void;
  onFolderNewFolder: (parentId: string | null) => void;
  onFolderNewSnippet: (parentId: string | null) => void;
  onFolderRename: (folder: SnippetFolder) => void;
  onFolderDelete: (folder: SnippetFolder) => void;
  matchedFolderIds?: Set<string>;
  searchActive?: boolean;
}

interface TreeNode {
  folder: SnippetFolder;
  children: TreeNode[];
  snippets: QuerySnippet[];
}

function buildTree(
  folders: SnippetFolder[],
  snippets: QuerySnippet[],
  folderOf: (id: string) => string | null
): { roots: TreeNode[]; rootSnippets: QuerySnippet[] } {
  const byParent = new Map<string | null, SnippetFolder[]>();
  for (const f of folders) {
    const arr = byParent.get(f.parentId) || [];
    arr.push(f);
    byParent.set(f.parentId, arr);
  }
  const snippetsByFolder = new Map<string | null, QuerySnippet[]>();
  for (const s of snippets) {
    const fid = folderOf(s.id);
    const arr = snippetsByFolder.get(fid) || [];
    arr.push(s);
    snippetsByFolder.set(fid, arr);
  }

  const validFolderIds = new Set(folders.map((f) => f.id));

  const build = (parentId: string | null): TreeNode[] => {
    const children = (byParent.get(parentId) || [])
      .slice()
      .sort((a, b) => a.name.localeCompare(b.name));
    return children.map((folder) => ({
      folder,
      children: build(folder.id),
      snippets: (snippetsByFolder.get(folder.id) || [])
        .slice()
        .sort((a, b) => a.name.localeCompare(b.name)),
    }));
  };

  const rootSnippets: QuerySnippet[] = [];
  for (const s of snippets) {
    const fid = folderOf(s.id);
    if (fid === null || !validFolderIds.has(fid)) rootSnippets.push(s);
  }

  return {
    roots: build(null),
    rootSnippets: rootSnippets.sort((a, b) => a.name.localeCompare(b.name)),
  };
}

export const SnippetFolderTree: FC<SnippetFolderTreeProps> = (props) => {
  const {
    folders,
    snippets,
    folderOf,
    expanded,
    onToggleExpanded,
    onSnippetClick,
    onSnippetInsert,
    onSnippetEdit,
    onSnippetDelete,
    onSnippetMove,
    onFolderNewFolder,
    onFolderNewSnippet,
    onFolderRename,
    onFolderDelete,
    matchedFolderIds,
    searchActive,
  } = props;

  const { roots, rootSnippets } = useMemo(
    () => buildTree(folders, snippets, folderOf),
    [folders, snippets, folderOf]
  );

  const renderFolder = (node: TreeNode, depth: number) => {
    const isExpanded = searchActive
      ? matchedFolderIds?.has(node.folder.id) ?? false
      : expanded[node.folder.id] ?? false;
    return (
      <div key={node.folder.id}>
        <ContextMenu>
          <ContextMenuTrigger asChild>
            <div
              className="flex items-center gap-1 px-2 py-1 hover:bg-accent/50 rounded cursor-pointer text-sm select-none"
              style={{ paddingLeft: 8 + depth * 16 }}
              onClick={() => onToggleExpanded(node.folder.id)}
            >
              {isExpanded ? (
                <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              )}
              {isExpanded ? (
                <FolderOpen className="h-4 w-4 shrink-0 text-amber-500" />
              ) : (
                <Folder className="h-4 w-4 shrink-0 text-amber-500" />
              )}
              <span className="truncate">{node.folder.name}</span>
            </div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem onClick={() => onFolderNewFolder(node.folder.id)}>
              New Folder here
            </ContextMenuItem>
            <ContextMenuItem onClick={() => onFolderNewSnippet(node.folder.id)}>
              New Snippet here
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem onClick={() => onFolderRename(node.folder)}>
              Rename
            </ContextMenuItem>
            <ContextMenuItem
              onClick={() => onFolderDelete(node.folder)}
              className="text-destructive focus:text-destructive"
            >
              Delete
            </ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
        {isExpanded && (
          <div>
            {node.children.map((c) => renderFolder(c, depth + 1))}
            {node.snippets.map((s) => renderSnippet(s, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  const renderSnippet = (snippet: QuerySnippet, depth: number) => (
    <ContextMenu key={snippet.id}>
      <ContextMenuTrigger asChild>
        <div
          className="flex items-center gap-1 px-2 py-1 hover:bg-accent/50 rounded cursor-pointer text-sm select-none"
          style={{ paddingLeft: 8 + depth * 16 + 14 }}
          onClick={() => onSnippetClick(snippet)}
          onDoubleClick={() => onSnippetInsert?.(snippet)}
        >
          <FileCode className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="truncate">{snippet.name}</span>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {onSnippetInsert && (
          <ContextMenuItem onClick={() => onSnippetInsert(snippet)}>
            Open
          </ContextMenuItem>
        )}
        <ContextMenuItem onClick={() => onSnippetEdit(snippet)}>
          Rename
        </ContextMenuItem>
        <ContextMenuItem onClick={() => onSnippetMove(snippet)}>
          Move to folder…
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          onClick={() => onSnippetDelete(snippet)}
          className="text-destructive focus:text-destructive"
        >
          Delete
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );

  return (
    <div className="py-1">
      {roots.map((n) => renderFolder(n, 0))}
      {rootSnippets.map((s) => renderSnippet(s, 0))}
    </div>
  );
};
