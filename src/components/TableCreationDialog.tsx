/**
 * TableCreationDialog Component
 *
 * Multi-step wizard for creating database tables with visual column editor,
 * constraint builder, and SQL preview.
 */

import { FC, useState, useMemo } from "react";
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

  // Validate current step
  const canProceed = useMemo(() => {
    switch (step) {
      case "basic":
        return tableName.trim().length > 0;
      case "columns":
        return columns.length > 0 && columns.every((col) => col.name.trim().length > 0);
      case "constraints":
        return true; // Constraints are optional
      case "preview":
        return previewSql.length > 0;
      default:
        return false;
    }
  }, [step, tableName, columns, previewSql]);

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
        description: error.message || "Failed to generate SQL preview",
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
      <DialogContent className="max-w-4xl max-h-[90vh] p-0">
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
            <TabsContent value="constraints" className="mt-4 space-y-4">
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">Foreign Keys & Unique Constraints</CardTitle>
                </CardHeader>
                <CardContent>
                  <p className="text-sm text-muted-foreground">
                    Advanced constraint management coming soon. You can add foreign keys and unique
                    constraints after creating the table using ALTER TABLE commands.
                  </p>
                </CardContent>
              </Card>
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
