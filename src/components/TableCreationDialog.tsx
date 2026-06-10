/**
 * TableCreationDialog Component
 *
 * Multi-step wizard for creating database tables with visual column editor,
 * constraint builder, and SQL preview.
 */

import { FC, useState, useMemo, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TableInfo, ColumnInfo, TableSchema } from "@/types/database";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogFooter,
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
import { Trash2, Plus, Database } from "lucide-react";
import { toast } from "sonner";
import { createTable, previewCreateTable } from "@/api/ddl";
import type {
  TableDefinition,
  ColumnDefinition,
  ColumnType,
  ForeignKeyConstraint,
  ForeignKeyAction,
  UniqueConstraint,
} from "@/types/ddl";
import { ColumnTypes } from "@/types/ddl";

interface TableCreationDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  connectionId: string;
  schema?: string;
  onSuccess?: () => void;
}

type Step = "basic" | "columns" | "constraints" | "preview";

// Foreign-key action options
const FK_ACTIONS: { value: ForeignKeyAction; label: string }[] = [
  { value: "NO_ACTION", label: "NO ACTION" },
  { value: "RESTRICT", label: "RESTRICT" },
  { value: "CASCADE", label: "CASCADE" },
  { value: "SET_NULL", label: "SET NULL" },
  { value: "SET_DEFAULT", label: "SET DEFAULT" },
];

// Common column types for the dropdown
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

export const TableCreationDialog: FC<TableCreationDialogProps> = ({
  open,
  onOpenChange,
  connectionId,
  schema,
  onSuccess,
}) => {
  const [step, setStep] = useState<Step>("basic");
  const [loading, setLoading] = useState(false);

  // Basic info
  const [tableName, setTableName] = useState("");
  const [tableComment, setTableComment] = useState("");
  const [ifNotExists, setIfNotExists] = useState(true);

  // Columns
  const [columns, setColumns] = useState<ColumnDefinition[]>([
    {
      name: "id",
      columnType: ColumnTypes.integer(),
      nullable: false,
      primaryKey: true,
      autoIncrement: true,
      comment: undefined,
      default: undefined,
    },
  ]);

  // Constraints
  const [foreignKeys, setForeignKeys] = useState<ForeignKeyConstraint[]>([]);
  const [uniqueConstraints, setUniqueConstraints] = useState<UniqueConstraint[]>([]);

  // SQL Preview
  const [previewSql, setPreviewSql] = useState<string[]>([]);

  // Schema lookup for foreign keys
  const effectiveSchema = schema || "public";
  const [availableTables, setAvailableTables] = useState<TableInfo[]>([]);
  const [tablesLoading, setTablesLoading] = useState(false);
  const [tablesError, setTablesError] = useState<string | null>(null);
  const [refColumnsCache, setRefColumnsCache] = useState<Record<string, ColumnInfo[]>>({});
  const [refColumnsLoading, setRefColumnsLoading] = useState<Record<string, boolean>>({});

  // Load tables when entering the constraints step (once per dialog open / connection / schema)
  useEffect(() => {
    if (step !== "constraints") return;
    if (availableTables.length > 0 || tablesLoading) return;
    let cancelled = false;
    (async () => {
      setTablesLoading(true);
      setTablesError(null);
      try {
        const tables = await invoke<TableInfo[]>("get_tables", {
          connectionId,
          schema: effectiveSchema,
        });
        if (!cancelled) setAvailableTables(tables);
      } catch (err: any) {
        if (!cancelled) {
          setTablesError(
            typeof err === "string" ? err : err?.message || String(err)
          );
        }
      } finally {
        if (!cancelled) setTablesLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [step, connectionId, effectiveSchema, availableTables.length, tablesLoading]);

  // Lazy-load referenced columns for a chosen table
  const ensureRefColumnsLoaded = async (tableName: string) => {
    if (!tableName) return;
    if (refColumnsCache[tableName] || refColumnsLoading[tableName]) return;
    setRefColumnsLoading((prev) => ({ ...prev, [tableName]: true }));
    try {
      const result = await invoke<TableSchema>("get_table_schema", {
        connectionId,
        schema: effectiveSchema,
        table: tableName,
      });
      setRefColumnsCache((prev) => ({ ...prev, [tableName]: result.columns }));
    } catch (err: any) {
      toast.error("Failed to load columns", {
        description: typeof err === "string" ? err : err?.message || String(err),
      });
    } finally {
      setRefColumnsLoading((prev) => ({ ...prev, [tableName]: false }));
    }
  };

  // Build table definition
  const tableDefinition = useMemo((): TableDefinition => {
    return {
      schema,
      name: tableName,
      columns,
      foreignKeys,
      uniqueConstraints,
      checkConstraints: [],
      comment: tableComment || undefined,
      ifNotExists,
    };
  }, [schema, tableName, columns, foreignKeys, uniqueConstraints, tableComment, ifNotExists]);

  // Validate constraints; returns null if valid, or a human-readable reason
  const constraintsError = useMemo<string | null>(() => {
    for (let i = 0; i < uniqueConstraints.length; i++) {
      const u = uniqueConstraints[i];
      if (u.columns.length === 0) {
        return `Unique constraint #${i + 1} needs at least one column.`;
      }
    }
    for (let i = 0; i < foreignKeys.length; i++) {
      const fk = foreignKeys[i];
      if (fk.columns.length === 0) {
        return `Foreign key #${i + 1} needs at least one source column.`;
      }
      if (!fk.referencedTable) {
        return `Foreign key #${i + 1} needs a referenced table.`;
      }
      if (fk.referencedColumns.length === 0) {
        return `Foreign key #${i + 1} needs at least one referenced column.`;
      }
      if (fk.columns.length !== fk.referencedColumns.length) {
        return `Foreign key #${i + 1}: source and referenced column counts must match.`;
      }
    }
    return null;
  }, [foreignKeys, uniqueConstraints]);

  // Validate current step
  const canProceed = useMemo(() => {
    switch (step) {
      case "basic":
        return tableName.trim().length > 0;
      case "columns":
        return columns.length > 0 && columns.every((col) => col.name.trim().length > 0);
      case "constraints":
        return constraintsError === null;
      case "preview":
        return previewSql.length > 0;
      default:
        return false;
    }
  }, [step, tableName, columns, previewSql, constraintsError]);

  // Add column
  const addColumn = () => {
    setColumns([
      ...columns,
      {
        name: "",
        columnType: ColumnTypes.varchar(255),
        nullable: true,
        primaryKey: false,
        autoIncrement: false,
        comment: undefined,
        default: undefined,
      },
    ]);
  };

  // Remove column
  const removeColumn = (index: number) => {
    setColumns(columns.filter((_, i) => i !== index));
  };

  // Update column
  const updateColumn = (index: number, updates: Partial<ColumnDefinition>) => {
    setColumns(columns.map((col, i) => (i === index ? { ...col, ...updates } : col)));
  };

  // Update column type from dropdown
  const updateColumnType = (index: number, typeKey: string) => {
    const typeConfig = COMMON_COLUMN_TYPES.find((t) => t.value === typeKey);
    if (typeConfig) {
      updateColumn(index, { columnType: typeConfig.factory() });
    }
  };

  // Load SQL preview
  const loadPreview = async () => {
    try {
      setLoading(true);
      const result = await previewCreateTable(connectionId, tableDefinition);
      setPreviewSql(result.sql);
      setStep("preview");
    } catch (error: any) {
      toast.error("Preview Failed", {
        description: error.message || error.toString() || "Failed to generate SQL preview",
      });
    } finally {
      setLoading(false);
    }
  };

  // Create table
  const handleCreate = async () => {
    try {
      setLoading(true);
      const result = await createTable(connectionId, tableDefinition);
      toast.success("Table Created", {
        description: result.message,
      });
      onOpenChange(false);
      onSuccess?.();
      resetForm();
    } catch (error: any) {
      toast.error("Creation Failed", {
        description: error.message || "Failed to create table",
      });
    } finally {
      setLoading(false);
    }
  };

  // Reset form
  const resetForm = () => {
    setStep("basic");
    setTableName("");
    setTableComment("");
    setIfNotExists(true);
    setColumns([
      {
        name: "id",
        columnType: ColumnTypes.integer(),
        nullable: false,
        primaryKey: true,
        autoIncrement: true,
        comment: undefined,
        default: undefined,
      },
    ]);
    setForeignKeys([]);
    setUniqueConstraints([]);
    setPreviewSql([]);
    setAvailableTables([]);
    setTablesError(null);
    setRefColumnsCache({});
    setRefColumnsLoading({});
  };

  // ---- Unique constraint helpers ----
  const addUniqueConstraint = () => {
    setUniqueConstraints([...uniqueConstraints, { name: undefined, columns: [] }]);
  };
  const removeUniqueConstraint = (index: number) => {
    setUniqueConstraints(uniqueConstraints.filter((_, i) => i !== index));
  };
  const updateUniqueConstraint = (index: number, updates: Partial<UniqueConstraint>) => {
    setUniqueConstraints(
      uniqueConstraints.map((u, i) => (i === index ? { ...u, ...updates } : u))
    );
  };
  const toggleUniqueColumn = (index: number, columnName: string) => {
    const u = uniqueConstraints[index];
    const has = u.columns.includes(columnName);
    updateUniqueConstraint(index, {
      columns: has ? u.columns.filter((c) => c !== columnName) : [...u.columns, columnName],
    });
  };

  // ---- Foreign key helpers ----
  const addForeignKey = () => {
    setForeignKeys([
      ...foreignKeys,
      {
        name: undefined,
        columns: [],
        referencedTable: "",
        referencedColumns: [],
        onDelete: "NO_ACTION",
        onUpdate: "NO_ACTION",
      },
    ]);
  };
  const removeForeignKey = (index: number) => {
    setForeignKeys(foreignKeys.filter((_, i) => i !== index));
  };
  const updateForeignKey = (index: number, updates: Partial<ForeignKeyConstraint>) => {
    setForeignKeys(foreignKeys.map((fk, i) => (i === index ? { ...fk, ...updates } : fk)));
  };
  const toggleFkSourceColumn = (index: number, columnName: string) => {
    const fk = foreignKeys[index];
    const has = fk.columns.includes(columnName);
    updateForeignKey(index, {
      columns: has ? fk.columns.filter((c) => c !== columnName) : [...fk.columns, columnName],
    });
  };
  const toggleFkRefColumn = (index: number, columnName: string) => {
    const fk = foreignKeys[index];
    const has = fk.referencedColumns.includes(columnName);
    updateForeignKey(index, {
      referencedColumns: has
        ? fk.referencedColumns.filter((c) => c !== columnName)
        : [...fk.referencedColumns, columnName],
    });
  };
  const handleFkTableChange = (index: number, tableName: string) => {
    updateForeignKey(index, { referencedTable: tableName, referencedColumns: [] });
    void ensureRefColumnsLoaded(tableName);
  };

  // Get column type key for select value
  const getColumnTypeKey = (columnType: ColumnType): string => {
    if (columnType.type === "varchar" && columnType.length === 255) return "varchar";
    const match = COMMON_COLUMN_TYPES.find((t) => {
      const generated = t.factory();
      return JSON.stringify(generated) === JSON.stringify(columnType);
    });
    return match?.value || "custom";
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[min(1280px,95vw)] w-[95vw] sm:w-[95vw] max-h-[90vh] p-0">
        <DialogHeader className="px-6 pt-6 pb-4">
          <DialogTitle className="flex items-center gap-2">
            <Database className="h-5 w-5" />
            Create New Table
          </DialogTitle>
          <DialogDescription>
            Design your table structure with columns, constraints, and relationships
          </DialogDescription>
        </DialogHeader>

        <Tabs value={step} onValueChange={(v) => setStep(v as Step)} className="flex-1">
          <div className="px-6">
            <TabsList className="grid w-full grid-cols-4">
              <TabsTrigger value="basic" disabled={false}>
                1. Basic Info
              </TabsTrigger>
              <TabsTrigger value="columns" disabled={!tableName}>
                2. Columns
              </TabsTrigger>
              <TabsTrigger value="constraints" disabled={!tableName || columns.length === 0}>
                3. Constraints
              </TabsTrigger>
              <TabsTrigger value="preview" disabled={previewSql.length === 0}>
                4. Preview
              </TabsTrigger>
            </TabsList>
          </div>

          <ScrollArea className="flex-1 px-6 pb-4" style={{ maxHeight: "50vh" }}>
            {/* Step 1: Basic Info */}
            <TabsContent value="basic" className="mt-4 space-y-4">
              <div className="space-y-2">
                <Label htmlFor="tableName">
                  Table Name <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="tableName"
                  value={tableName}
                  onChange={(e) => setTableName(e.target.value)}
                  placeholder="e.g., users, products, orders"
                />
              </div>

              {schema && (
                <div className="space-y-2">
                  <Label>Schema</Label>
                  <Input value={schema} disabled />
                </div>
              )}

              <div className="space-y-2">
                <Label htmlFor="tableComment">Description (Optional)</Label>
                <Input
                  id="tableComment"
                  value={tableComment}
                  onChange={(e) => setTableComment(e.target.value)}
                  placeholder="Table description or comment"
                />
              </div>

              <div className="flex items-center space-x-2">
                <Checkbox
                  id="ifNotExists"
                  checked={ifNotExists}
                  onCheckedChange={(checked) => setIfNotExists(checked as boolean)}
                />
                <Label htmlFor="ifNotExists" className="text-sm font-normal cursor-pointer">
                  Add IF NOT EXISTS clause
                </Label>
              </div>
            </TabsContent>

            {/* Step 2: Columns */}
            <TabsContent value="columns" className="mt-4 space-y-4">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-semibold">Column Definitions</h3>
                <Button onClick={addColumn} size="sm" variant="outline">
                  <Plus className="h-4 w-4 mr-1" />
                  Add Column
                </Button>
              </div>

              <div className="space-y-3">
                {columns.map((col, index) => (
                  <Card key={index}>
                    <CardContent className="pt-4 space-y-3">
                      <div className="flex items-start gap-2">
                        <div className="flex-1 grid grid-cols-2 gap-3">
                          <div className="space-y-1">
                            <Label className="text-xs">Name</Label>
                            <Input
                              value={col.name}
                              onChange={(e) => updateColumn(index, { name: e.target.value })}
                              placeholder="column_name"
                              className="h-9"
                            />
                          </div>

                          <div className="space-y-1">
                            <Label className="text-xs">Type</Label>
                            <Select
                              value={getColumnTypeKey(col.columnType)}
                              onValueChange={(value) => updateColumnType(index, value)}
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
                          onClick={() => removeColumn(index)}
                          size="icon"
                          variant="ghost"
                          className="h-9 w-9 mt-5"
                          disabled={columns.length === 1}
                          aria-label="Remove column"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>

                      <div className="flex flex-wrap gap-4">
                        <div className="flex items-center space-x-2">
                          <Checkbox
                            id={`pk-${index}`}
                            checked={col.primaryKey}
                            onCheckedChange={(checked) =>
                              updateColumn(index, { primaryKey: checked as boolean })
                            }
                          />
                          <Label htmlFor={`pk-${index}`} className="text-xs cursor-pointer">
                            Primary Key
                          </Label>
                        </div>

                        <div className="flex items-center space-x-2">
                          <Checkbox
                            id={`auto-${index}`}
                            checked={col.autoIncrement}
                            onCheckedChange={(checked) =>
                              updateColumn(index, { autoIncrement: checked as boolean })
                            }
                          />
                          <Label htmlFor={`auto-${index}`} className="text-xs cursor-pointer">
                            Auto Increment
                          </Label>
                        </div>

                        <div className="flex items-center space-x-2">
                          <Checkbox
                            id={`nullable-${index}`}
                            checked={col.nullable}
                            onCheckedChange={(checked) =>
                              updateColumn(index, { nullable: checked as boolean })
                            }
                          />
                          <Label htmlFor={`nullable-${index}`} className="text-xs cursor-pointer">
                            Nullable
                          </Label>
                        </div>
                      </div>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </TabsContent>

            {/* Step 3: Constraints */}
            <TabsContent value="constraints" className="mt-4 space-y-6">
              {/* Unique Constraints */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold">Unique Constraints</h3>
                  <Button onClick={addUniqueConstraint} size="sm" variant="outline">
                    <Plus className="h-4 w-4 mr-1" />
                    Add Unique
                  </Button>
                </div>

                {uniqueConstraints.length === 0 ? (
                  <p className="text-xs text-muted-foreground">
                    No unique constraints. Add one to enforce uniqueness across one or more columns.
                  </p>
                ) : (
                  <div className="space-y-3">
                    {uniqueConstraints.map((u, index) => (
                      <Card key={`uniq-${index}`}>
                        <CardContent className="pt-4 space-y-3">
                          <div className="flex items-start gap-2">
                            <div className="flex-1 space-y-1">
                              <Label className="text-xs">Constraint Name (Optional)</Label>
                              <Input
                                value={u.name || ""}
                                onChange={(e) =>
                                  updateUniqueConstraint(index, {
                                    name: e.target.value || undefined,
                                  })
                                }
                                placeholder="e.g., uq_users_email"
                                className="h-9"
                              />
                            </div>
                            <Button
                              onClick={() => removeUniqueConstraint(index)}
                              size="icon"
                              variant="ghost"
                              className="h-9 w-9 mt-5"
                              aria-label="Remove unique constraint"
                            >
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </div>

                          <div className="space-y-1">
                            <Label className="text-xs">
                              Columns <span className="text-destructive">*</span>
                            </Label>
                            {columns.filter((c) => c.name.trim()).length === 0 ? (
                              <p className="text-xs text-muted-foreground">
                                Define named columns in step 2 first.
                              </p>
                            ) : (
                              <div className="flex flex-wrap gap-3 rounded-md border p-2">
                                {columns
                                  .filter((c) => c.name.trim())
                                  .map((col) => {
                                    const id = `uniq-${index}-${col.name}`;
                                    return (
                                      <div
                                        key={id}
                                        className="flex items-center space-x-2"
                                      >
                                        <Checkbox
                                          id={id}
                                          checked={u.columns.includes(col.name)}
                                          onCheckedChange={() =>
                                            toggleUniqueColumn(index, col.name)
                                          }
                                        />
                                        <Label
                                          htmlFor={id}
                                          className="text-xs cursor-pointer"
                                        >
                                          {col.name}
                                        </Label>
                                      </div>
                                    );
                                  })}
                              </div>
                            )}
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                  </div>
                )}
              </div>

              {/* Foreign Keys */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold">Foreign Keys</h3>
                  <Button onClick={addForeignKey} size="sm" variant="outline">
                    <Plus className="h-4 w-4 mr-1" />
                    Add Foreign Key
                  </Button>
                </div>

                {tablesError && (
                  <p className="text-xs text-destructive">
                    Failed to load tables: {tablesError}
                  </p>
                )}

                {foreignKeys.length === 0 ? (
                  <p className="text-xs text-muted-foreground">
                    No foreign keys. Add one to reference rows in another table.
                  </p>
                ) : (
                  <div className="space-y-3">
                    {foreignKeys.map((fk, index) => {
                      const refCols = fk.referencedTable
                        ? refColumnsCache[fk.referencedTable] || []
                        : [];
                      const refLoading = fk.referencedTable
                        ? refColumnsLoading[fk.referencedTable]
                        : false;
                      const definedCols = columns.filter((c) => c.name.trim());

                      return (
                        <Card key={`fk-${index}`}>
                          <CardContent className="pt-4 space-y-3">
                            <div className="flex items-start gap-2">
                              <div className="flex-1 space-y-1">
                                <Label className="text-xs">Constraint Name (Optional)</Label>
                                <Input
                                  value={fk.name || ""}
                                  onChange={(e) =>
                                    updateForeignKey(index, {
                                      name: e.target.value || undefined,
                                    })
                                  }
                                  placeholder="e.g., fk_orders_user_id"
                                  className="h-9"
                                />
                              </div>
                              <Button
                                onClick={() => removeForeignKey(index)}
                                size="icon"
                                variant="ghost"
                                className="h-9 w-9 mt-5"
                                aria-label="Remove foreign key"
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </div>

                            <div className="space-y-1">
                              <Label className="text-xs">
                                Source Columns <span className="text-destructive">*</span>
                              </Label>
                              {definedCols.length === 0 ? (
                                <p className="text-xs text-muted-foreground">
                                  Define named columns in step 2 first.
                                </p>
                              ) : (
                                <div className="flex flex-wrap gap-3 rounded-md border p-2">
                                  {definedCols.map((col) => {
                                    const id = `fk-${index}-src-${col.name}`;
                                    return (
                                      <div
                                        key={id}
                                        className="flex items-center space-x-2"
                                      >
                                        <Checkbox
                                          id={id}
                                          checked={fk.columns.includes(col.name)}
                                          onCheckedChange={() =>
                                            toggleFkSourceColumn(index, col.name)
                                          }
                                        />
                                        <Label
                                          htmlFor={id}
                                          className="text-xs cursor-pointer"
                                        >
                                          {col.name}
                                        </Label>
                                      </div>
                                    );
                                  })}
                                </div>
                              )}
                            </div>

                            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                              <div className="space-y-1">
                                <Label className="text-xs">
                                  Referenced Table{" "}
                                  <span className="text-destructive">*</span>
                                </Label>
                                <Select
                                  value={fk.referencedTable || undefined}
                                  onValueChange={(value) =>
                                    handleFkTableChange(index, value)
                                  }
                                  disabled={tablesLoading}
                                >
                                  <SelectTrigger className="h-9">
                                    <SelectValue
                                      placeholder={
                                        tablesLoading
                                          ? "Loading tables..."
                                          : "Select a table"
                                      }
                                    />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {availableTables.length === 0 && !tablesLoading ? (
                                      <div className="px-2 py-1.5 text-xs text-muted-foreground">
                                        No tables in {effectiveSchema}
                                      </div>
                                    ) : (
                                      availableTables.map((t) => (
                                        <SelectItem key={t.name} value={t.name}>
                                          {t.name}
                                        </SelectItem>
                                      ))
                                    )}
                                  </SelectContent>
                                </Select>
                              </div>

                              <div className="space-y-1">
                                <Label className="text-xs">
                                  Referenced Columns{" "}
                                  <span className="text-destructive">*</span>
                                </Label>
                                {!fk.referencedTable ? (
                                  <p className="text-xs text-muted-foreground pt-2">
                                    Pick a referenced table first.
                                  </p>
                                ) : refLoading ? (
                                  <p className="text-xs text-muted-foreground pt-2">
                                    Loading columns...
                                  </p>
                                ) : refCols.length === 0 ? (
                                  <p className="text-xs text-muted-foreground pt-2">
                                    No columns found for {fk.referencedTable}.
                                  </p>
                                ) : (
                                  <div className="flex flex-wrap gap-3 rounded-md border p-2">
                                    {refCols.map((col) => {
                                      const id = `fk-${index}-ref-${col.name}`;
                                      return (
                                        <div
                                          key={id}
                                          className="flex items-center space-x-2"
                                        >
                                          <Checkbox
                                            id={id}
                                            checked={fk.referencedColumns.includes(col.name)}
                                            onCheckedChange={() =>
                                              toggleFkRefColumn(index, col.name)
                                            }
                                          />
                                          <Label
                                            htmlFor={id}
                                            className="text-xs cursor-pointer"
                                          >
                                            {col.name}
                                          </Label>
                                        </div>
                                      );
                                    })}
                                  </div>
                                )}
                              </div>
                            </div>

                            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                              <div className="space-y-1">
                                <Label className="text-xs">ON DELETE</Label>
                                <Select
                                  value={fk.onDelete}
                                  onValueChange={(value) =>
                                    updateForeignKey(index, {
                                      onDelete: value as ForeignKeyAction,
                                    })
                                  }
                                >
                                  <SelectTrigger className="h-9">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {FK_ACTIONS.map((a) => (
                                      <SelectItem key={a.value} value={a.value}>
                                        {a.label}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>

                              <div className="space-y-1">
                                <Label className="text-xs">ON UPDATE</Label>
                                <Select
                                  value={fk.onUpdate}
                                  onValueChange={(value) =>
                                    updateForeignKey(index, {
                                      onUpdate: value as ForeignKeyAction,
                                    })
                                  }
                                >
                                  <SelectTrigger className="h-9">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {FK_ACTIONS.map((a) => (
                                      <SelectItem key={a.value} value={a.value}>
                                        {a.label}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>
                            </div>
                          </CardContent>
                        </Card>
                      );
                    })}
                  </div>
                )}
              </div>

              {constraintsError && (
                <p className="text-xs text-destructive">{constraintsError}</p>
              )}
            </TabsContent>

            {/* Step 4: Preview */}
            <TabsContent value="preview" className="mt-4 space-y-4">
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">Generated SQL</CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="text-xs bg-muted p-4 rounded-md overflow-x-auto">
                    {previewSql.join("\n\n")}
                  </pre>
                </CardContent>
              </Card>
            </TabsContent>
          </ScrollArea>
        </Tabs>

        <DialogFooter className="px-6 pb-6 pt-4 border-t">
          <div className="flex items-center justify-between w-full">
            <Button
              onClick={() => onOpenChange(false)}
              variant="outline"
              disabled={loading}
            >
              Cancel
            </Button>

            <div className="flex gap-2">
              {step === "preview" ? (
                <Button onClick={handleCreate} disabled={loading || !canProceed}>
                  {loading ? "Creating..." : "Create Table"}
                </Button>
              ) : step === "constraints" ? (
                <Button onClick={loadPreview} disabled={loading || !canProceed}>
                  {loading ? "Generating..." : "Preview SQL"}
                </Button>
              ) : (
                <Button
                  onClick={() => {
                    if (step === "basic") setStep("columns");
                    else if (step === "columns") setStep("constraints");
                  }}
                  disabled={!canProceed}
                >
                  Next
                </Button>
              )}
            </div>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
