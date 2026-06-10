import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactFlow, {
  Background,
  BackgroundVariant,
  Controls,
  Edge,
  Handle,
  MiniMap,
  Node,
  NodeTypes,
  OnConnect,
  Position,
  ReactFlowProvider,
  addEdge,
} from "reactflow";
import "reactflow/dist/style.css";
import {
  Plus,
  Trash2,
  Key,
  Link,
  Play,
  Eye,
} from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Badge } from "./ui/badge";
import { ScrollArea } from "./ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { toast } from "sonner";

// ── Types ──────────────────────────────────────────────────────────────────

type ColType =
  | "TEXT"
  | "INTEGER"
  | "BIGINT"
  | "BOOLEAN"
  | "DECIMAL"
  | "VARCHAR"
  | "UUID"
  | "TIMESTAMP"
  | "DATE"
  | "JSONB"
  | "FLOAT"
  | "BLOB";

interface DesignerColumn {
  id: string;
  name: string;
  type: ColType;
  nullable: boolean;
  primaryKey: boolean;
  unique: boolean;
  defaultValue?: string;
  referencesTable?: string;
  referencesColumn?: string;
}

interface DesignerTable {
  id: string;
  name: string;
  schema?: string;
  columns: DesignerColumn[];
}

interface TableNodeData {
  table: DesignerTable;
  selected: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

// ── Backend type mapping ───────────────────────────────────────────────────

const TYPE_MAP: Record<ColType, string> = {
  TEXT: "Text",
  INTEGER: "Integer",
  BIGINT: "BigInt",
  BOOLEAN: "Boolean",
  DECIMAL: "Decimal",
  VARCHAR: "Varchar",
  UUID: "Uuid",
  TIMESTAMP: "Timestamp",
  DATE: "Date",
  JSONB: "Jsonb",
  FLOAT: "Decimal",
  BLOB: "Text",
};

const COL_TYPES: ColType[] = [
  "TEXT", "INTEGER", "BIGINT", "BOOLEAN", "DECIMAL",
  "VARCHAR", "UUID", "TIMESTAMP", "DATE", "JSONB", "FLOAT", "BLOB",
];

// ── TableSchemaNode ────────────────────────────────────────────────────────

function TableSchemaNode({ data }: { data: TableNodeData }) {
  const { table, selected, onSelect, onDelete } = data;

  return (
    <div
      onClick={onSelect}
      className={`w-64 rounded-lg shadow-lg border-2 cursor-pointer transition-all ${
        selected
          ? "border-primary shadow-primary/20"
          : "border-border hover:border-primary/50"
      } bg-card`}
    >
      <Handle type="target" position={Position.Left} className="!bg-primary !border-primary-foreground" />
      <Handle type="source" position={Position.Right} className="!bg-primary !border-primary-foreground" />

      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 bg-primary text-primary-foreground rounded-t-md">
        <span className="font-semibold text-sm truncate">{table.name || "untitled"}</span>
        <button
          onClick={(e) => { e.stopPropagation(); onDelete(); }}
          aria-label="Delete table"
          className="ml-2 text-primary-foreground/70 hover:text-primary-foreground"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* Columns */}
      <div className="divide-y divide-border">
        {table.columns.length === 0 && (
          <div className="px-3 py-2 text-xs text-muted-foreground italic">No columns</div>
        )}
        {table.columns.map((col) => (
          <div key={col.id} className="flex items-center gap-1.5 px-3 py-1.5 text-xs">
            {col.primaryKey && <Key className="h-3 w-3 text-amber-500 shrink-0" />}
            {col.referencesTable && !col.primaryKey && (
              <Link className="h-3 w-3 text-blue-500 shrink-0" />
            )}
            {!col.primaryKey && !col.referencesTable && (
              <span className="w-3 shrink-0" />
            )}
            <span className="flex-1 truncate font-medium text-foreground">{col.name || "…"}</span>
            <span className="text-muted-foreground shrink-0">{col.type}</span>
            {col.unique && <Badge variant="outline" className="text-[10px] px-1 py-0 h-4">UNQ</Badge>}
            {!col.nullable && !col.primaryKey && (
              <Badge variant="outline" className="text-[10px] px-1 py-0 h-4 text-red-500 border-red-300">NN</Badge>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

const nodeTypes: NodeTypes = { tableSchema: TableSchemaNode };

// ── Helpers ────────────────────────────────────────────────────────────────

function newTable(position = { x: 100, y: 100 }): DesignerTable & { position: { x: number; y: number } } {
  return {
    id: crypto.randomUUID(),
    name: "",
    schema: "public",
    columns: [],
    position,
  };
}

function newColumn(): DesignerColumn {
  return {
    id: crypto.randomUUID(),
    name: "",
    type: "TEXT",
    nullable: true,
    primaryKey: false,
    unique: false,
  };
}

function tablesToBackendDef(table: DesignerTable) {
  return {
    tableName: table.name,
    schema: table.schema || null,
    columns: table.columns.map((c) => ({
      name: c.name,
      columnType: TYPE_MAP[c.type],
      nullable: c.nullable,
      primaryKey: c.primaryKey,
      unique: c.unique,
      defaultValue: c.defaultValue || null,
      references:
        c.referencesTable && c.referencesColumn
          ? { table: c.referencesTable, column: c.referencesColumn }
          : null,
    })),
  };
}

// ── Main component (inner, needs ReactFlowProvider) ────────────────────────

interface VisualSchemaDesignerInnerProps {
  connectionId: string;
}

function VisualSchemaDesignerInner({ connectionId }: VisualSchemaDesignerInnerProps) {
  const [tables, setTables] = useState<(DesignerTable & { position: { x: number; y: number } })[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sqlPreview, setSqlPreview] = useState<string | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [creating, setCreating] = useState(false);

  // ── ReactFlow state derived from tables ──────────────────────────────────

  const rfNodes: Node[] = useMemo(
    () =>
      tables.map((t) => ({
        id: t.id,
        type: "tableSchema",
        position: t.position,
        data: {
          table: t,
          selected: t.id === selectedId,
          onSelect: () => setSelectedId(t.id),
          onDelete: () => {
            setTables((prev) => prev.filter((x) => x.id !== t.id));
            if (selectedId === t.id) setSelectedId(null);
          },
        },
      })),
    [tables, selectedId]
  );

  const rfEdges: Edge[] = useMemo(() => {
    const edges: Edge[] = [];
    for (const t of tables) {
      for (const col of t.columns) {
        if (col.referencesTable) {
          const target = tables.find((x) => x.name === col.referencesTable);
          if (target) {
            edges.push({
              id: `${t.id}-${col.id}`,
              source: t.id,
              target: target.id,
              animated: true,
              label: `${col.name} → ${col.referencesColumn || "id"}`,
              style: { stroke: "hsl(var(--primary))" },
            });
          }
        }
      }
    }
    return edges;
  }, [tables]);

  // Manual edges added by drawing connections on canvas
  const [manualEdges, setManualEdges] = useState<Edge[]>([]);

  // Merge FK-derived edges with manually drawn edges
  const allEdges = useMemo(
    () => [...rfEdges, ...manualEdges],
    [rfEdges, manualEdges]
  );

  const onConnect: OnConnect = useCallback(
    (params) => setManualEdges((eds) => addEdge(params, eds)),
    []
  );

  // Handle node position changes from dragging
  const handleNodesChange = useCallback(
    (changes: any[]) => {
      for (const change of changes) {
        if (change.type === "position" && change.position) {
          setTables((prev) =>
            prev.map((t) =>
              t.id === change.id ? { ...t, position: change.position } : t
            )
          );
        }
      }
    },
    []
  );

  const handleEdgesChange = useCallback(
    (changes: any[]) => {
      setManualEdges((eds) => {
        const removed = new Set(
          changes.filter((c) => c.type === "remove").map((c) => c.id)
        );
        return eds.filter((e) => !removed.has(e.id));
      });
    },
    []
  );

  // ── Selected table helpers ───────────────────────────────────────────────

  const selectedTable = tables.find((t) => t.id === selectedId) ?? null;

  function updateTable(patch: Partial<DesignerTable>) {
    setTables((prev) =>
      prev.map((t) => (t.id === selectedId ? { ...t, ...patch } : t))
    );
  }

  function updateColumn(colId: string, patch: Partial<DesignerColumn>) {
    if (!selectedTable) return;
    setTables((prev) =>
      prev.map((t) =>
        t.id === selectedId
          ? {
              ...t,
              columns: t.columns.map((c) =>
                c.id === colId ? { ...c, ...patch } : c
              ),
            }
          : t
      )
    );
  }

  function addColumn() {
    if (!selectedTable) return;
    updateTable({ columns: [...selectedTable.columns, newColumn()] });
  }

  function removeColumn(colId: string) {
    if (!selectedTable) return;
    updateTable({ columns: selectedTable.columns.filter((c) => c.id !== colId) });
  }

  // ── Actions ──────────────────────────────────────────────────────────────

  async function handlePreviewSQL() {
    if (tables.length === 0) {
      toast.error("Add at least one table first");
      return;
    }
    try {
      const parts: string[] = [];
      for (const t of tables) {
        if (!t.name) continue;
        const sql = await invoke<string>("preview_create_table", {
          connectionId,
          definition: tablesToBackendDef(t),
        });
        parts.push(sql);
      }
      setSqlPreview(parts.join("\n\n"));
      setPreviewOpen(true);
    } catch (e: any) {
      toast.error(`Preview failed: ${e?.message ?? e}`);
    }
  }

  async function handleCreateTables() {
    const validTables = tables.filter((t) => t.name && t.columns.length > 0);
    if (validTables.length === 0) {
      toast.error("Add tables with at least one column");
      return;
    }
    setCreating(true);
    let ok = 0;
    for (const t of validTables) {
      try {
        await invoke("create_table", {
          connectionId,
          definition: tablesToBackendDef(t),
        });
        ok++;
      } catch (e: any) {
        toast.error(`Failed to create "${t.name}": ${e?.message ?? e}`);
      }
    }
    setCreating(false);
    if (ok > 0) toast.success(`Created ${ok} table${ok > 1 ? "s" : ""}`);
  }

  // ── Render ────────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b bg-background shrink-0">
        <span className="font-semibold text-sm">Visual Schema Designer</span>
        <div className="flex-1" />
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            const pos = { x: 100 + tables.length * 50, y: 100 + tables.length * 30 };
            const t = newTable(pos);
            setTables((prev) => [...prev, t]);
            setSelectedId(t.id);
          }}
        >
          <Plus className="h-4 w-4 mr-1" /> Add Table
        </Button>
        <Button size="sm" variant="outline" onClick={handlePreviewSQL}>
          <Eye className="h-4 w-4 mr-1" /> Preview SQL
        </Button>
        <Button size="sm" onClick={handleCreateTables} disabled={creating}>
          <Play className="h-4 w-4 mr-1" />
          {creating ? "Creating…" : "Create Tables"}
        </Button>
      </div>

      {/* Body */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left: table list */}
        <div className="w-48 border-r bg-background flex flex-col shrink-0">
          <div className="px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground border-b">
            Tables ({tables.length})
          </div>
          <ScrollArea className="flex-1">
            {tables.map((t) => (
              <button
                key={t.id}
                onClick={() => setSelectedId(t.id)}
                className={`w-full text-left px-3 py-2 text-sm truncate hover:bg-accent transition-colors ${
                  t.id === selectedId ? "bg-accent font-medium" : ""
                }`}
              >
                {t.name || <span className="italic text-muted-foreground">untitled</span>}
              </button>
            ))}
            {tables.length === 0 && (
              <p className="px-3 py-4 text-xs text-muted-foreground italic">
                Click "+ Add Table" to start
              </p>
            )}
          </ScrollArea>
        </div>

        {/* Center: ReactFlow canvas */}
        <div className="flex-1 bg-muted/30">
          <ReactFlow
            nodes={rfNodes}
            edges={allEdges}
            onNodesChange={handleNodesChange}
            onEdgesChange={handleEdgesChange}
            onConnect={onConnect}
            nodeTypes={nodeTypes}
            fitView
            fitViewOptions={{ padding: 0.2 }}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
            <Controls />
            <MiniMap nodeStrokeWidth={3} />
          </ReactFlow>
        </div>

        {/* Right: properties panel */}
        {selectedTable ? (
          <div className="w-80 border-l bg-background flex flex-col shrink-0">
            <div className="px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground border-b">
              Table Properties
            </div>
            <ScrollArea className="flex-1 px-3 py-3">
              <div className="space-y-4">
                {/* Table name */}
                <div className="space-y-1">
                  <Label className="text-xs">Table Name</Label>
                  <Input
                    value={selectedTable.name}
                    onChange={(e) => updateTable({ name: e.target.value })}
                    placeholder="e.g. users"
                    className="h-8 text-sm"
                  />
                </div>

                {/* Schema */}
                <div className="space-y-1">
                  <Label className="text-xs">Schema</Label>
                  <Input
                    value={selectedTable.schema ?? ""}
                    onChange={(e) => updateTable({ schema: e.target.value })}
                    placeholder="public"
                    className="h-8 text-sm"
                  />
                </div>

                {/* Columns */}
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <Label className="text-xs">Columns</Label>
                    <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={addColumn}>
                      <Plus className="h-3 w-3 mr-1" /> Add
                    </Button>
                  </div>

                  {selectedTable.columns.length === 0 && (
                    <p className="text-xs text-muted-foreground italic">No columns yet</p>
                  )}

                  {selectedTable.columns.map((col) => (
                    <div
                      key={col.id}
                      className="rounded border bg-muted/40 p-2 space-y-2"
                    >
                      <div className="flex gap-1.5">
                        <Input
                          value={col.name}
                          onChange={(e) => updateColumn(col.id, { name: e.target.value })}
                          placeholder="column_name"
                          className="h-7 text-xs flex-1"
                        />
                        <Select
                          value={col.type}
                          onValueChange={(v) => updateColumn(col.id, { type: v as ColType })}
                        >
                          <SelectTrigger className="h-7 text-xs w-28">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {COL_TYPES.map((t) => (
                              <SelectItem key={t} value={t} className="text-xs">
                                {t}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <button
                          onClick={() => removeColumn(col.id)}
                          aria-label="Remove column"
                          className="text-muted-foreground hover:text-destructive"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>

                      {/* Toggles */}
                      <div className="flex flex-wrap gap-2 text-xs">
                        {(["primaryKey", "nullable", "unique"] as const).map((flag) => (
                          <label key={flag} className="flex items-center gap-1 cursor-pointer">
                            <input
                              type="checkbox"
                              checked={!!col[flag]}
                              onChange={(e) => updateColumn(col.id, { [flag]: e.target.checked })}
                              className="h-3 w-3"
                            />
                            <span className="capitalize">{flag === "primaryKey" ? "PK" : flag === "nullable" ? "Nullable" : "Unique"}</span>
                          </label>
                        ))}
                      </div>

                      {/* FK reference */}
                      <div className="flex gap-1.5">
                        <Input
                          value={col.referencesTable ?? ""}
                          onChange={(e) => updateColumn(col.id, { referencesTable: e.target.value || undefined })}
                          placeholder="ref table"
                          className="h-7 text-xs flex-1"
                        />
                        <Input
                          value={col.referencesColumn ?? ""}
                          onChange={(e) => updateColumn(col.id, { referencesColumn: e.target.value || undefined })}
                          placeholder="ref col"
                          className="h-7 text-xs flex-1"
                        />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </ScrollArea>
          </div>
        ) : (
          <div className="w-80 border-l bg-background flex items-center justify-center text-sm text-muted-foreground shrink-0">
            Select a table to edit
          </div>
        )}
      </div>

      {/* SQL Preview dialog */}
      <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
        <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>Generated SQL</DialogTitle>
          </DialogHeader>
          <ScrollArea className="flex-1">
            <pre className="text-xs font-mono whitespace-pre-wrap p-4 bg-muted rounded">
              {sqlPreview}
            </pre>
          </ScrollArea>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ── Export (wraps in ReactFlowProvider) ───────────────────────────────────

interface VisualSchemaDesignerProps {
  connectionId: string;
}

export default function VisualSchemaDesigner({ connectionId }: VisualSchemaDesignerProps) {
  return (
    <ReactFlowProvider>
      <VisualSchemaDesignerInner connectionId={connectionId} />
    </ReactFlowProvider>
  );
}
