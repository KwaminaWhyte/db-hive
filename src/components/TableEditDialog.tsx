/**
 * TableEditDialog
 *
 * Modal editor for an existing table. Loads the current schema via
 * `get_table_schema`, lets the user add/drop/rename columns and toggle
 * nullability, previews the generated ALTER TABLE SQL and commits through
 * the existing DDL command pipeline.
 */

import { FC, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Pencil, Plus, Trash2, RotateCcw, Database } from "lucide-react";
import { alterTable, previewAlterTable } from "@/api/ddl";
import { ColumnTypes } from "@/types/ddl";
import type {
  AlterColumnOperation,
  AlterTableDefinition,
  ColumnDefinition,
  ColumnType,
} from "@/types/ddl";
import type { TableSchema, ColumnInfo } from "@/types";
import { notifyMetadataChanged } from "@/hooks/useMetadataCache";

interface TableEditDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  connectionId: string;
  schema: string;
  tableName: string;
  onSuccess?: () => void;
}

type Step = "columns" | "preview";

const COMMON_COLUMN_TYPES: { value: string; label: string; factory: () => ColumnType }[] = [
  { value: "integer", label: "Integer", factory: ColumnTypes.integer },
  { value: "bigint", label: "Big Integer", factory: ColumnTypes.bigInt },
  { value: "varchar", label: "VARCHAR(255)", factory: () => ColumnTypes.varchar(255) },
  { value: "text", label: "Text", factory: ColumnTypes.text },
  { value: "boolean", label: "Boolean", factory: ColumnTypes.boolean },
  { value: "timestamp", label: "Timestamp", factory: ColumnTypes.timestamp },
  { value: "date", label: "Date", factory: ColumnTypes.date },
  { value: "json", label: "JSON", factory: ColumnTypes.json },
  { value: "uuid", label: "UUID", factory: ColumnTypes.uuid },
];

interface ExistingColumnDraft {
  kind: "existing";
  original: ColumnInfo;
  name: string; // may be renamed
  nullable: boolean;
  dropped: boolean;
}

interface NewColumnDraft {
  kind: "new";
  tempId: number;
  name: string;
  columnType: ColumnType;
  nullable: boolean;
}

type ColumnDraft = ExistingColumnDraft | NewColumnDraft;

export const TableEditDialog: FC<TableEditDialogProps> = ({
  open,
  onOpenChange,
  connectionId,
  schema,
  tableName,
  onSuccess,
}) => {
  const [loading, setLoading] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [tableSchema, setTableSchema] = useState<TableSchema | null>(null);
  const [drafts, setDrafts] = useState<ColumnDraft[]>([]);
  const [step, setStep] = useState<Step>("columns");
  const [previewSql, setPreviewSql] = useState<string[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [nextTempId, setNextTempId] = useState(-1);

  // Load the table schema when the dialog opens
  useEffect(() => {
    if (!open) return;

    let cancelled = false;
    const load = async () => {
      setLoading(true);
      setLoadError(null);
      try {
        const ts = await invoke<TableSchema>("get_table_schema", {
          connectionId,
          schema,
          table: tableName,
        });
        if (cancelled) return;
        setTableSchema(ts);
        setDrafts(
          ts.columns.map((col) => ({
            kind: "existing" as const,
            original: col,
            name: col.name,
            nullable: col.nullable,
            dropped: false,
          })),
        );
        setStep("columns");
        setPreviewSql([]);
      } catch (err: any) {
        if (!cancelled) {
          setLoadError(
            typeof err === "string" ? err : err?.message || "Failed to load table schema",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [open, connectionId, schema, tableName]);

  // Build the list of operations from the drafts
  const operations = useMemo<AlterColumnOperation[]>(() => {
    const ops: AlterColumnOperation[] = [];
    for (const draft of drafts) {
      if (draft.kind === "existing") {
        if (draft.dropped) {
          ops.push({ type: "dropColumn", columnName: draft.original.name, cascade: false });
          continue;
        }
        if (draft.name.trim() && draft.name !== draft.original.name) {
          ops.push({
            type: "renameColumn",
            oldName: draft.original.name,
            newName: draft.name.trim(),
          });
        }
        if (draft.nullable !== draft.original.nullable) {
          ops.push({
            type: "setNotNull",
            columnName: draft.name.trim() || draft.original.name,
            notNull: !draft.nullable,
          });
        }
      } else if (draft.name.trim()) {
        const column: ColumnDefinition = {
          name: draft.name.trim(),
          columnType: draft.columnType,
          nullable: draft.nullable,
          primaryKey: false,
          autoIncrement: false,
        };
        ops.push({ type: "addColumn", column });
      }
    }
    return ops;
  }, [drafts]);

  const alterDefinition = useMemo<AlterTableDefinition>(
    () => ({ schema, name: tableName, operations }),
    [schema, tableName, operations],
  );

  const hasChanges = operations.length > 0;

  // Handlers
  const updateExisting = (index: number, patch: Partial<ExistingColumnDraft>) => {
    setDrafts((prev) =>
      prev.map((d, i) => (i === index && d.kind === "existing" ? { ...d, ...patch } : d)),
    );
  };

  const updateNew = (index: number, patch: Partial<NewColumnDraft>) => {
    setDrafts((prev) =>
      prev.map((d, i) => (i === index && d.kind === "new" ? { ...d, ...patch } : d)),
    );
  };

  const addNewColumn = () => {
    const tempId = nextTempId;
    setNextTempId(tempId - 1);
    setDrafts((prev) => [
      ...prev,
      {
        kind: "new",
        tempId,
        name: "",
        columnType: ColumnTypes.varchar(255),
        nullable: true,
      },
    ]);
  };

  const removeDraft = (index: number) => {
    setDrafts((prev) => {
      const d = prev[index];
      if (d.kind === "new") {
        return prev.filter((_, i) => i !== index);
      }
      // Existing column: mark dropped
      return prev.map((item, i) =>
        i === index && item.kind === "existing" ? { ...item, dropped: true } : item,
      );
    });
  };

  const restoreDraft = (index: number) => {
    setDrafts((prev) =>
      prev.map((d, i) =>
        i === index && d.kind === "existing"
          ? { ...d, dropped: false, name: d.original.name, nullable: d.original.nullable }
          : d,
      ),
    );
  };

  const getColumnTypeKey = (columnType: ColumnType): string => {
    if (columnType.type === "varchar" && columnType.length === 255) return "varchar";
    const match = COMMON_COLUMN_TYPES.find((t) => {
      const generated = t.factory();
      return JSON.stringify(generated) === JSON.stringify(columnType);
    });
    return match?.value || "varchar";
  };

  const updateNewType = (index: number, typeKey: string) => {
    const typeConfig = COMMON_COLUMN_TYPES.find((t) => t.value === typeKey);
    if (typeConfig) updateNew(index, { columnType: typeConfig.factory() });
  };

  const loadPreview = async () => {
    if (!hasChanges) {
      toast.info("No changes to preview");
      return;
    }
    try {
      setLoading(true);
      const result = await previewAlterTable(connectionId, alterDefinition);
      setPreviewSql(result.sql);
      setStep("preview");
    } catch (err: any) {
      toast.error("Preview Failed", {
        description: err?.message || err?.toString() || "Failed to generate SQL preview",
      });
    } finally {
      setLoading(false);
    }
  };

  const handleApply = async () => {
    try {
      setCommitting(true);
      const result = await alterTable(connectionId, alterDefinition);
      toast.success("Table Updated", { description: result.message });
      notifyMetadataChanged({ schema, reason: "alter-table" });
      onOpenChange(false);
      onSuccess?.();
    } catch (err: any) {
      toast.error("Update Failed", {
        description: err?.message || err?.toString() || "Failed to alter table",
      });
    } finally {
      setCommitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl max-h-[90vh] p-0">
        <DialogHeader className="px-6 pt-6 pb-4">
          <DialogTitle className="flex items-center gap-2">
            <Database className="h-5 w-5" />
            Edit Table: {schema}.{tableName}
          </DialogTitle>
          <DialogDescription>
            Add, drop, rename, or toggle nullability on existing columns. Generated
            ALTER TABLE statements will be shown before execution.
          </DialogDescription>
        </DialogHeader>

        {loadError ? (
          <div className="px-6 pb-6">
            <div className="bg-destructive/10 text-destructive rounded-md p-3 text-sm">
              {loadError}
            </div>
          </div>
        ) : (
          <Tabs value={step} onValueChange={(v) => setStep(v as Step)} className="flex-1">
            <div className="px-6">
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="columns">1. Columns</TabsTrigger>
                <TabsTrigger value="preview" disabled={previewSql.length === 0}>
                  2. Preview
                </TabsTrigger>
              </TabsList>
            </div>

            <ScrollArea className="flex-1 px-6 pb-4" style={{ maxHeight: "55vh" }}>
              <TabsContent value="columns" className="mt-4 space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold">
                    Columns {tableSchema ? `(${tableSchema.columns.length})` : ""}
                  </h3>
                  <Button onClick={addNewColumn} size="sm" variant="outline" disabled={loading}>
                    <Plus className="h-4 w-4 mr-1" />
                    Add Column
                  </Button>
                </div>

                {loading && !tableSchema ? (
                  <p className="text-sm text-muted-foreground py-6 text-center">
                    Loading table schema...
                  </p>
                ) : (
                  <div className="space-y-3">
                    {drafts.map((draft, index) =>
                      draft.kind === "existing" ? (
                        <Card
                          key={`exist-${draft.original.name}`}
                          className={draft.dropped ? "opacity-60" : ""}
                        >
                          <CardContent className="pt-4 space-y-3">
                            <div className="flex items-start gap-2">
                              <div className="flex-1 grid grid-cols-2 gap-3">
                                <div className="space-y-1">
                                  <Label className="text-xs flex items-center gap-2">
                                    Name
                                    {draft.name !== draft.original.name && !draft.dropped && (
                                      <Badge variant="secondary" className="text-[10px]">
                                        renamed
                                      </Badge>
                                    )}
                                    {draft.dropped && (
                                      <Badge variant="destructive" className="text-[10px]">
                                        will drop
                                      </Badge>
                                    )}
                                  </Label>
                                  <Input
                                    value={draft.name}
                                    disabled={draft.dropped}
                                    onChange={(e) => updateExisting(index, { name: e.target.value })}
                                    className="h-9"
                                  />
                                </div>
                                <div className="space-y-1">
                                  <Label className="text-xs">Type</Label>
                                  <Input
                                    value={draft.original.dataType}
                                    disabled
                                    className="h-9"
                                  />
                                </div>
                              </div>

                              {draft.dropped ? (
                                <Button
                                  onClick={() => restoreDraft(index)}
                                  size="icon"
                                  variant="ghost"
                                  className="h-9 w-9 mt-5"
                                  title="Restore column"
                                  aria-label="Restore column"
                                >
                                  <RotateCcw className="h-4 w-4" />
                                </Button>
                              ) : (
                                <Button
                                  onClick={() => removeDraft(index)}
                                  size="icon"
                                  variant="ghost"
                                  className="h-9 w-9 mt-5"
                                  disabled={draft.original.isPrimaryKey}
                                  title={
                                    draft.original.isPrimaryKey
                                      ? "Cannot drop primary key column"
                                      : "Drop column"
                                  }
                                  aria-label="Drop column"
                                >
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              )}
                            </div>

                            {!draft.dropped && (
                              <div className="flex items-center space-x-2">
                                <Checkbox
                                  id={`nullable-${index}`}
                                  checked={draft.nullable}
                                  onCheckedChange={(checked) =>
                                    updateExisting(index, { nullable: checked as boolean })
                                  }
                                />
                                <Label
                                  htmlFor={`nullable-${index}`}
                                  className="text-xs cursor-pointer"
                                >
                                  Nullable
                                </Label>
                                {draft.nullable !== draft.original.nullable && (
                                  <Badge variant="secondary" className="text-[10px]">
                                    modified
                                  </Badge>
                                )}
                              </div>
                            )}
                          </CardContent>
                        </Card>
                      ) : (
                        <Card key={`new-${draft.tempId}`} className="border-green-500/40">
                          <CardContent className="pt-4 space-y-3">
                            <div className="flex items-start gap-2">
                              <div className="flex-1 grid grid-cols-2 gap-3">
                                <div className="space-y-1">
                                  <Label className="text-xs flex items-center gap-2">
                                    Name
                                    <Badge variant="default" className="text-[10px]">
                                      new
                                    </Badge>
                                  </Label>
                                  <Input
                                    value={draft.name}
                                    onChange={(e) => updateNew(index, { name: e.target.value })}
                                    placeholder="column_name"
                                    className="h-9"
                                  />
                                </div>
                                <div className="space-y-1">
                                  <Label className="text-xs">Type</Label>
                                  <Select
                                    value={getColumnTypeKey(draft.columnType)}
                                    onValueChange={(v) => updateNewType(index, v)}
                                  >
                                    <SelectTrigger className="h-9">
                                      <SelectValue />
                                    </SelectTrigger>
                                    <SelectContent>
                                      {COMMON_COLUMN_TYPES.map((type) => (
                                        <SelectItem key={type.value} value={type.value}>
                                          {type.label}
                                        </SelectItem>
                                      ))}
                                    </SelectContent>
                                  </Select>
                                </div>
                              </div>

                              <Button
                                onClick={() => removeDraft(index)}
                                size="icon"
                                variant="ghost"
                                className="h-9 w-9 mt-5"
                                aria-label="Remove column"
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </div>

                            <div className="flex items-center space-x-2">
                              <Checkbox
                                id={`new-nullable-${index}`}
                                checked={draft.nullable}
                                onCheckedChange={(checked) =>
                                  updateNew(index, { nullable: checked as boolean })
                                }
                              />
                              <Label
                                htmlFor={`new-nullable-${index}`}
                                className="text-xs cursor-pointer"
                              >
                                Nullable
                              </Label>
                            </div>
                          </CardContent>
                        </Card>
                      ),
                    )}
                  </div>
                )}
              </TabsContent>

              <TabsContent value="preview" className="mt-4 space-y-4">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-sm">Generated ALTER SQL</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <pre className="text-xs bg-muted p-4 rounded-md overflow-x-auto whitespace-pre-wrap">
                      {previewSql.join("\n\n")}
                    </pre>
                  </CardContent>
                </Card>
              </TabsContent>
            </ScrollArea>
          </Tabs>
        )}

        <DialogFooter className="px-6 pb-6 pt-4 border-t">
          <div className="flex items-center justify-between w-full">
            <div className="text-xs text-muted-foreground flex items-center gap-2">
              <Pencil className="h-3 w-3" />
              {operations.length} pending operation{operations.length === 1 ? "" : "s"}
            </div>
            <div className="flex gap-2">
              <Button
                onClick={() => onOpenChange(false)}
                variant="outline"
                disabled={committing}
              >
                Cancel
              </Button>
              {step === "preview" ? (
                <>
                  <Button
                    onClick={() => setStep("columns")}
                    variant="outline"
                    disabled={committing}
                  >
                    Back
                  </Button>
                  <Button onClick={handleApply} disabled={committing || !hasChanges}>
                    {committing ? "Applying..." : "Apply Changes"}
                  </Button>
                </>
              ) : (
                <Button onClick={loadPreview} disabled={loading || !hasChanges}>
                  {loading ? "Loading..." : "Preview SQL"}
                </Button>
              )}
            </div>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
