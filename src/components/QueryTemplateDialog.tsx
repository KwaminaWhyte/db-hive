import { useState, useEffect, useCallback } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Textarea } from "./ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { Checkbox } from "./ui/checkbox";
import { Badge } from "./ui/badge";
import { ScrollArea } from "./ui/scroll-area";
import {
  Play,
  Save,
  X,
  AlertCircle,
  Info,
  Plus,
  Trash2,
  Code,
} from "lucide-react";
import {
  QueryTemplate,
  TemplateParameter,
  TemplateParameterType,
  TemplateParameterValues,
  extractParameterDefinitions,
  validateParameterValue,
  formatParameterForSql,
} from "../types/templates";

interface QueryTemplateDialogProps {
  open: boolean;
  onClose: () => void;
  template?: QueryTemplate | null;
  mode: "create" | "edit" | "execute";
  onSave?: (template: Omit<QueryTemplate, "id" | "createdAt" | "updatedAt">) => void;
  onExecute?: (sql: string) => void;
}

const PARAMETER_TYPES: { value: TemplateParameterType; label: string }[] = [
  { value: "string", label: "String" },
  { value: "integer", label: "Integer" },
  { value: "decimal", label: "Decimal" },
  { value: "boolean", label: "Boolean" },
  { value: "date", label: "Date" },
  { value: "datetime", label: "Date/Time" },
  { value: "text", label: "Text (multiline)" },
];

export function QueryTemplateDialog({
  open,
  onClose,
  template,
  mode,
  onSave,
  onExecute,
}: QueryTemplateDialogProps) {
  // Create/Edit mode state
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [sql, setSql] = useState("");
  const [category, setCategory] = useState("");
  const [tags, setTags] = useState("");
  const [parameters, setParameters] = useState<TemplateParameter[]>([]);

  // Execute mode state
  const [parameterValues, setParameterValues] = useState<TemplateParameterValues>({});
  const [validationErrors, setValidationErrors] = useState<Record<string, string>>({});
  const [previewSql, setPreviewSql] = useState("");

  // Initialize form when template changes
  useEffect(() => {
    if (template) {
      setName(template.name);
      setDescription(template.description || "");
      setSql(template.sql);
      setCategory(template.category || "");
      setTags(template.tags?.join(", ") || "");
      setParameters(template.parameters);

      // Initialize parameter values with defaults
      const initialValues: TemplateParameterValues = {};
      template.parameters.forEach((param) => {
        initialValues[param.name] = param.defaultValue || "";
      });
      setParameterValues(initialValues);
    } else {
      // Reset form
      setName("");
      setDescription("");
      setSql("");
      setCategory("");
      setTags("");
      setParameters([]);
      setParameterValues({});
    }
    setValidationErrors({});
  }, [template, open]);

  // Auto-detect parameters from SQL when editing
  useEffect(() => {
    if (mode !== "execute") {
      const detected = extractParameterDefinitions(sql);

      // Merge with existing parameters to preserve settings
      const newParams = detected.map((d) => {
        const existing = parameters.find((p) => p.name === d.name);
        if (existing) {
          return { ...existing, type: d.type };
        }
        return {
          name: d.name,
          label: d.name.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()),
          type: d.type,
          required: true,
          defaultValue: "",
          description: "",
        };
      });

      setParameters(newParams);
    }
  }, [sql, mode]);

  // Generate preview SQL when parameter values change
  useEffect(() => {
    if (mode === "execute" && template) {
      let preview = template.sql;

      // Substitute values with SQL formatting
      Object.entries(parameterValues).forEach(([name, value]) => {
        const param = template.parameters.find((p) => p.name === name);
        const formattedValue = param && value
          ? formatParameterForSql(value, param.type)
          : `{{${name}}}`;
        preview = preview.replace(
          new RegExp(`\\{\\{${name}(?::[a-zA-Z]+)?\\}\\}`, "g"),
          formattedValue
        );
      });

      setPreviewSql(preview);
    }
  }, [parameterValues, template, mode]);

  const handleParameterChange = useCallback(
    (index: number, field: keyof TemplateParameter, value: any) => {
      setParameters((prev) => {
        const newParams = [...prev];
        newParams[index] = { ...newParams[index], [field]: value };
        return newParams;
      });
    },
    []
  );

  const handleRemoveParameter = useCallback((index: number) => {
    setParameters((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleAddParameter = useCallback(() => {
    setParameters((prev) => [
      ...prev,
      {
        name: `param_${prev.length + 1}`,
        label: `Parameter ${prev.length + 1}`,
        type: "string",
        required: false,
        defaultValue: "",
        description: "",
      },
    ]);
  }, []);

  const handleValueChange = useCallback(
    (paramName: string, value: string) => {
      setParameterValues((prev) => ({ ...prev, [paramName]: value }));

      // Validate on change
      const param = template?.parameters.find((p) => p.name === paramName);
      if (param) {
        const result = validateParameterValue(value, param.type);
        setValidationErrors((prev) => {
          if (result.valid) {
            const { [paramName]: _, ...rest } = prev;
            return rest;
          }
          return { ...prev, [paramName]: result.error || "Invalid value" };
        });
      }
    },
    [template]
  );

  const validateAll = useCallback(() => {
    if (!template) return true;

    const errors: Record<string, string> = {};
    let valid = true;

    template.parameters.forEach((param) => {
      const value = parameterValues[param.name] || "";

      // Check required
      if (param.required && !value) {
        errors[param.name] = "This field is required";
        valid = false;
        return;
      }

      // Validate type
      if (value) {
        const result = validateParameterValue(value, param.type);
        if (!result.valid) {
          errors[param.name] = result.error || "Invalid value";
          valid = false;
        }
      }
    });

    setValidationErrors(errors);
    return valid;
  }, [template, parameterValues]);

  const handleSave = useCallback(() => {
    if (!name.trim() || !sql.trim()) return;

    onSave?.({
      name: name.trim(),
      description: description.trim() || undefined,
      sql: sql.trim(),
      parameters,
      category: category.trim() || undefined,
      tags: tags
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
    });

    onClose();
  }, [name, description, sql, parameters, category, tags, onSave, onClose]);

  const handleExecute = useCallback(() => {
    if (!validateAll()) return;
    if (!template) return;

    // Build final SQL with parameter values
    let finalSql = template.sql;
    template.parameters.forEach((param) => {
      const value = parameterValues[param.name] || param.defaultValue || "";
      const formattedValue = formatParameterForSql(value, param.type);
      finalSql = finalSql.replace(
        new RegExp(`\\{\\{${param.name}(?::[a-zA-Z]+)?\\}\\}`, "g"),
        formattedValue
      );
    });

    onExecute?.(finalSql);
    onClose();
  }, [template, parameterValues, validateAll, onExecute, onClose]);

  const renderCreateEditForm = () => (
    <div className="space-y-4">
      {/* Name */}
      <div className="space-y-2">
        <Label htmlFor="name">Template Name *</Label>
        <Input
          id="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g., Find Users by Status"
        />
      </div>

      {/* Description */}
      <div className="space-y-2">
        <Label htmlFor="description">Description</Label>
        <Textarea
          id="description"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="What does this template do?"
          rows={2}
        />
      </div>

      {/* SQL */}
      <div className="space-y-2">
        <Label htmlFor="sql">
          SQL Query * <span className="text-xs text-muted-foreground ml-2">
            Use {"{{param_name}}"} or {"{{param_name:type}}"} for parameters
          </span>
        </Label>
        <Textarea
          id="sql"
          value={sql}
          onChange={(e) => setSql(e.target.value)}
          placeholder="SELECT * FROM users WHERE status = {{status:string}} AND created_at > {{start_date:date}}"
          className="font-mono text-sm"
          rows={6}
        />
      </div>

      {/* Parameters */}
      {parameters.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <Label>Parameters</Label>
            <Button
              variant="outline"
              size="sm"
              onClick={handleAddParameter}
              className="h-7"
            >
              <Plus className="h-3 w-3 mr-1" />
              Add
            </Button>
          </div>

          <div className="space-y-3 max-h-[200px] overflow-auto pr-2">
            {parameters.map((param, index) => (
              <div
                key={index}
                className="p-3 border rounded-lg bg-muted/30 space-y-2"
              >
                <div className="flex items-center justify-between">
                  <code className="text-xs bg-zinc-800 px-2 py-0.5 rounded">
                    {`{{${param.name}}}`}
                  </code>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleRemoveParameter(index)}
                    className="h-6 w-6 p-0 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>

                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <Label className="text-xs">Label</Label>
                    <Input
                      value={param.label}
                      onChange={(e) =>
                        handleParameterChange(index, "label", e.target.value)
                      }
                      className="h-8 text-sm"
                    />
                  </div>
                  <div>
                    <Label className="text-xs">Type</Label>
                    <Select
                      value={param.type}
                      onValueChange={(v) =>
                        handleParameterChange(index, "type", v)
                      }
                    >
                      <SelectTrigger className="h-8 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {PARAMETER_TYPES.map((t) => (
                          <SelectItem key={t.value} value={t.value}>
                            {t.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <Label className="text-xs">Default Value</Label>
                    <Input
                      value={param.defaultValue || ""}
                      onChange={(e) =>
                        handleParameterChange(index, "defaultValue", e.target.value)
                      }
                      className="h-8 text-sm"
                      placeholder="Optional"
                    />
                  </div>
                  <div className="flex items-end gap-2 pb-1">
                    <Checkbox
                      id={`required-${index}`}
                      checked={param.required}
                      onCheckedChange={(checked) =>
                        handleParameterChange(index, "required", !!checked)
                      }
                    />
                    <Label htmlFor={`required-${index}`} className="text-xs">
                      Required
                    </Label>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Category and Tags */}
      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="category">Category</Label>
          <Input
            id="category"
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            placeholder="e.g., Users, Reports"
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="tags">Tags (comma-separated)</Label>
          <Input
            id="tags"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder="e.g., user, status, report"
          />
        </div>
      </div>
    </div>
  );

  const renderExecuteForm = () => (
    <div className="space-y-4">
      {/* Template Info */}
      <div className="p-3 bg-muted/30 rounded-lg">
        <h4 className="font-medium text-sm mb-1">{template?.name}</h4>
        {template?.description && (
          <p className="text-xs text-muted-foreground">{template.description}</p>
        )}
        {template?.tags && template.tags.length > 0 && (
          <div className="flex gap-1 mt-2">
            {template.tags.map((tag) => (
              <Badge key={tag} variant="secondary" className="text-xs">
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Parameter Inputs */}
      {template?.parameters && template.parameters.length > 0 ? (
        <div className="space-y-3">
          <Label className="flex items-center gap-2">
            <Info className="h-4 w-4 text-muted-foreground" />
            Fill in the parameters below
          </Label>

          <ScrollArea className="max-h-[200px] pr-4">
            <div className="space-y-3">
              {template.parameters.map((param) => (
                <div key={param.name} className="space-y-1">
                  <Label htmlFor={param.name} className="text-sm">
                    {param.label}
                    {param.required && <span className="text-destructive ml-1">*</span>}
                  </Label>
                  {param.description && (
                    <p className="text-xs text-muted-foreground">{param.description}</p>
                  )}
                  {param.type === "text" ? (
                    <Textarea
                      id={param.name}
                      value={parameterValues[param.name] || ""}
                      onChange={(e) => handleValueChange(param.name, e.target.value)}
                      placeholder={`Enter ${param.label.toLowerCase()}`}
                      rows={3}
                      className={validationErrors[param.name] ? "border-destructive" : ""}
                    />
                  ) : param.type === "boolean" ? (
                    <Select
                      value={parameterValues[param.name] || ""}
                      onValueChange={(v) => handleValueChange(param.name, v)}
                    >
                      <SelectTrigger className={validationErrors[param.name] ? "border-destructive" : ""}>
                        <SelectValue placeholder="Select..." />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="true">True</SelectItem>
                        <SelectItem value="false">False</SelectItem>
                      </SelectContent>
                    </Select>
                  ) : (
                    <Input
                      id={param.name}
                      type={param.type === "date" ? "date" : param.type === "datetime" ? "datetime-local" : "text"}
                      value={parameterValues[param.name] || ""}
                      onChange={(e) => handleValueChange(param.name, e.target.value)}
                      placeholder={
                        param.type === "integer"
                          ? "e.g., 123"
                          : param.type === "decimal"
                            ? "e.g., 123.45"
                            : `Enter ${param.label.toLowerCase()}`
                      }
                      className={validationErrors[param.name] ? "border-destructive" : ""}
                    />
                  )}
                  {validationErrors[param.name] && (
                    <p className="text-xs text-destructive flex items-center gap-1">
                      <AlertCircle className="h-3 w-3" />
                      {validationErrors[param.name]}
                    </p>
                  )}
                </div>
              ))}
            </div>
          </ScrollArea>
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">
          This template has no parameters.
        </p>
      )}

      {/* SQL Preview */}
      <div className="space-y-2">
        <Label className="flex items-center gap-2">
          <Code className="h-4 w-4 text-muted-foreground" />
          SQL Preview
        </Label>
        <div className="p-3 bg-zinc-900 rounded-lg border">
          <pre className="text-xs font-mono text-zinc-300 whitespace-pre-wrap overflow-auto max-h-[120px]">
            {previewSql || template?.sql}
          </pre>
        </div>
      </div>
    </div>
  );

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle>
            {mode === "create"
              ? "Create Query Template"
              : mode === "edit"
                ? "Edit Query Template"
                : "Execute Template"}
          </DialogTitle>
          <DialogDescription>
            {mode === "execute"
              ? "Fill in the parameter values and execute the query."
              : "Create a reusable query template with parameters."}
          </DialogDescription>
        </DialogHeader>

        <ScrollArea className="flex-1 pr-4">
          <div className="py-4">
            {mode === "execute" ? renderExecuteForm() : renderCreateEditForm()}
          </div>
        </ScrollArea>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            <X className="h-4 w-4 mr-2" />
            Cancel
          </Button>
          {mode === "execute" ? (
            <Button onClick={handleExecute}>
              <Play className="h-4 w-4 mr-2" />
              Execute
            </Button>
          ) : (
            <Button onClick={handleSave} disabled={!name.trim() || !sql.trim()}>
              <Save className="h-4 w-4 mr-2" />
              Save Template
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
