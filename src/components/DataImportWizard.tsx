import { useState, useEffect, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Upload,
  FileSpreadsheet,
  Table,
  Settings,
  CheckCircle,
  AlertCircle,
  ChevronRight,
  ChevronLeft,
  X,
  Loader2,
  ArrowRight,
  FileText,
} from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Checkbox } from "./ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import {
  previewImportFile,
  importDataToTable,
  getTablesForImport,
  getTableColumnsForImport,
} from "../api/data-import";
import type {
  ImportPreview,
  ColumnMapping,
  DataImportOptions,
  ImportResult,
  TableColumn,
  ImportStep,
} from "../types/data-import";

interface DataImportWizardProps {
  connectionId: string;
  defaultSchema?: string;
  defaultTable?: string;
  onClose: () => void;
  onSuccess?: (result: ImportResult) => void;
}

const STEPS: { id: ImportStep; label: string; icon: React.ReactNode }[] = [
  { id: "select-file", label: "Select File", icon: <Upload className="w-4 h-4" /> },
  { id: "preview", label: "Preview", icon: <FileSpreadsheet className="w-4 h-4" /> },
  { id: "mapping", label: "Map Columns", icon: <Table className="w-4 h-4" /> },
  { id: "options", label: "Options", icon: <Settings className="w-4 h-4" /> },
  { id: "importing", label: "Import", icon: <Loader2 className="w-4 h-4" /> },
  { id: "complete", label: "Complete", icon: <CheckCircle className="w-4 h-4" /> },
];

const DATA_TYPES = [
  "TEXT",
  "INTEGER",
  "BIGINT",
  "DECIMAL",
  "NUMERIC",
  "REAL",
  "DOUBLE PRECISION",
  "BOOLEAN",
  "DATE",
  "TIMESTAMP",
  "TIME",
  "UUID",
  "JSON",
  "JSONB",
];

export function DataImportWizard({
  connectionId,
  defaultSchema,
  defaultTable,
  onClose,
  onSuccess,
}: DataImportWizardProps) {
  const [currentStep, setCurrentStep] = useState<ImportStep>("select-file");
  const [filePath, setFilePath] = useState<string>("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Target table state
  const [tables, setTables] = useState<string[]>([]);
  const [selectedTable, setSelectedTable] = useState(defaultTable || "");
  const [targetColumns, setTargetColumns] = useState<TableColumn[]>([]);
  const [schema, _setSchema] = useState(defaultSchema || "public");

  // Column mappings
  const [columnMappings, setColumnMappings] = useState<ColumnMapping[]>([]);

  // Import options
  const [options, setOptions] = useState<Partial<DataImportOptions>>({
    skip_rows: 0,
    create_table: false,
    truncate_before: false,
    batch_size: 1000,
    delimiter: ",",
    first_row_is_header: true,
    sheet_name: null,
  });

  // Import result
  const [importResult, setImportResult] = useState<ImportResult | null>(null);

  // Load tables on mount
  useEffect(() => {
    async function loadTables() {
      try {
        const tableList = await getTablesForImport(connectionId, schema);
        setTables(tableList);
      } catch (err) {
        console.error("Failed to load tables:", err);
      }
    }
    loadTables();
  }, [connectionId, schema]);

  // Load target columns when table is selected
  useEffect(() => {
    async function loadColumns() {
      if (!selectedTable) {
        setTargetColumns([]);
        return;
      }
      try {
        const columns = await getTableColumnsForImport(
          connectionId,
          selectedTable,
          schema
        );
        setTargetColumns(columns);
      } catch (err) {
        console.error("Failed to load columns:", err);
      }
    }
    loadColumns();
  }, [connectionId, selectedTable, schema]);

  // Auto-map columns when preview and target columns are available
  useEffect(() => {
    if (!preview || !targetColumns.length) return;

    const mappings: ColumnMapping[] = preview.columns.map((sourceCol, idx) => {
      // Try to find a matching target column (case-insensitive)
      const matchingTarget = targetColumns.find(
        (tc) => tc.name.toLowerCase() === sourceCol.toLowerCase()
      );

      return {
        source_column: sourceCol,
        target_column: matchingTarget?.name || "",
        target_type: preview.detected_types[idx] || "TEXT",
        default_value: null,
        skip: false,
      };
    });

    setColumnMappings(mappings);
  }, [preview, targetColumns]);

  const handleSelectFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Data Files",
            extensions: ["csv", "xlsx", "xls"],
          },
        ],
      });

      if (selected && typeof selected === "string") {
        setFilePath(selected);
        setError(null);
      }
    } catch (err) {
      setError(`Failed to select file: ${err}`);
    }
  };

  const handlePreviewFile = async () => {
    if (!filePath) return;

    setLoading(true);
    setError(null);

    try {
      const previewData = await previewImportFile(
        filePath,
        100,
        options.sheet_name || undefined
      );
      setPreview(previewData);
      setCurrentStep("preview");
    } catch (err) {
      setError(`Failed to preview file: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleImport = async () => {
    if (!filePath || !selectedTable) return;

    setCurrentStep("importing");
    setLoading(true);
    setError(null);

    try {
      const importOptions: DataImportOptions = {
        table_name: selectedTable,
        schema: schema || null,
        column_mappings: columnMappings.filter((m) => !m.skip && m.target_column),
        skip_rows: options.skip_rows || 0,
        create_table: options.create_table || false,
        truncate_before: options.truncate_before || false,
        batch_size: options.batch_size || 1000,
        delimiter: options.delimiter || null,
        sheet_name: options.sheet_name || null,
        first_row_is_header: options.first_row_is_header ?? true,
      };

      const result = await importDataToTable(connectionId, filePath, importOptions);
      setImportResult(result);
      setCurrentStep("complete");

      if (result.success && onSuccess) {
        onSuccess(result);
      }
    } catch (err) {
      setError(`Import failed: ${err}`);
      setCurrentStep("options");
    } finally {
      setLoading(false);
    }
  };

  const updateMapping = useCallback(
    (index: number, updates: Partial<ColumnMapping>) => {
      setColumnMappings((prev) => {
        const newMappings = [...prev];
        newMappings[index] = { ...newMappings[index], ...updates };
        return newMappings;
      });
    },
    []
  );

  const getStepIndex = (step: ImportStep) => STEPS.findIndex((s) => s.id === step);

  const canProceed = () => {
    switch (currentStep) {
      case "select-file":
        return !!filePath;
      case "preview":
        return !!preview && !!selectedTable;
      case "mapping":
        return columnMappings.some((m) => !m.skip && m.target_column);
      case "options":
        return true;
      default:
        return false;
    }
  };

  const goNext = () => {
    const currentIndex = getStepIndex(currentStep);
    if (currentIndex < STEPS.length - 1) {
      const nextStep = STEPS[currentIndex + 1].id;
      if (nextStep === "importing") {
        handleImport();
      } else {
        setCurrentStep(nextStep);
      }
    }
  };

  const goBack = () => {
    const currentIndex = getStepIndex(currentStep);
    if (currentIndex > 0) {
      setCurrentStep(STEPS[currentIndex - 1].id);
    }
  };

  const renderStepIndicator = () => (
    <div className="flex items-center justify-center gap-2 mb-6 px-4">
      {STEPS.slice(0, -2).map((step, idx) => {
        const isActive = currentStep === step.id;
        const isPast = getStepIndex(currentStep) > idx;

        return (
          <div key={step.id} className="flex items-center">
            <div
              className={`flex items-center gap-2 px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
                isActive
                  ? "bg-blue-500 text-white"
                  : isPast
                    ? "bg-green-500/20 text-green-400"
                    : "bg-zinc-800 text-zinc-500"
              }`}
            >
              {isPast ? <CheckCircle className="w-3 h-3" /> : step.icon}
              <span className="hidden sm:inline">{step.label}</span>
            </div>
            {idx < STEPS.length - 3 && (
              <ChevronRight className="w-4 h-4 text-zinc-600 mx-1" />
            )}
          </div>
        );
      })}
    </div>
  );

  const renderFileSelection = () => (
    <div className="flex-1 flex flex-col items-center justify-center p-8">
      <div
        onClick={handleSelectFile}
        className="w-full max-w-md border-2 border-dashed border-zinc-700 rounded-lg p-12 text-center cursor-pointer hover:border-blue-500 hover:bg-blue-500/5 transition-colors"
      >
        <Upload className="w-12 h-12 mx-auto text-zinc-500 mb-4" />
        <p className="text-lg font-medium text-zinc-300 mb-2">
          Click to select a file
        </p>
        <p className="text-sm text-zinc-500">
          Supports CSV, Excel (.xlsx, .xls)
        </p>
      </div>

      {filePath && (
        <div className="mt-6 p-4 bg-zinc-800/50 rounded-lg flex items-center gap-3 w-full max-w-md">
          <FileText className="w-5 h-5 text-blue-400" />
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-zinc-200 truncate">
              {filePath.split("/").pop()}
            </p>
            <p className="text-xs text-zinc-500 truncate">{filePath}</p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setFilePath("")}
            className="text-zinc-400 hover:text-zinc-200"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>
      )}

      {error && (
        <div className="mt-4 p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-sm flex items-center gap-2">
          <AlertCircle className="w-4 h-4" />
          {error}
        </div>
      )}
    </div>
  );

  const renderPreview = () => (
    <div className="flex-1 flex flex-col overflow-hidden p-4">
      <div className="flex items-center gap-4 mb-4">
        <div className="flex-1">
          <Label className="text-zinc-400 text-xs mb-1 block">Target Table</Label>
          <Select value={selectedTable} onValueChange={setSelectedTable}>
            <SelectTrigger className="w-full bg-zinc-800 border-zinc-700">
              <SelectValue placeholder="Select a table..." />
            </SelectTrigger>
            <SelectContent>
              {tables.map((table) => (
                <SelectItem key={table} value={table}>
                  {table}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {preview?.sheet_names && preview.sheet_names.length > 0 && (
          <div className="flex-1">
            <Label className="text-zinc-400 text-xs mb-1 block">Sheet</Label>
            <Select
              value={options.sheet_name || preview.sheet_names[0]}
              onValueChange={(v) => setOptions((o) => ({ ...o, sheet_name: v }))}
            >
              <SelectTrigger className="w-full bg-zinc-800 border-zinc-700">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {preview.sheet_names.map((sheet) => (
                  <SelectItem key={sheet} value={sheet}>
                    {sheet}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
      </div>

      <div className="flex items-center gap-4 mb-4 text-xs text-zinc-500">
        <span>
          File type: <strong className="text-zinc-300">{preview?.file_type}</strong>
        </span>
        <span>
          Columns: <strong className="text-zinc-300">{preview?.columns.length}</strong>
        </span>
        <span>
          Total rows:{" "}
          <strong className="text-zinc-300">
            {preview?.total_rows?.toLocaleString() || "Unknown"}
          </strong>
        </span>
      </div>

      <div className="flex-1 overflow-auto border border-zinc-800 rounded-lg">
        <table className="w-full text-sm">
          <thead className="bg-zinc-800/50 sticky top-0">
            <tr>
              {preview?.columns.map((col, idx) => (
                <th
                  key={idx}
                  className="px-3 py-2 text-left font-medium text-zinc-300 border-b border-zinc-700"
                >
                  <div>{col}</div>
                  <div className="text-xs font-normal text-zinc-500">
                    {preview?.detected_types[idx]}
                  </div>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {preview?.rows.slice(0, 50).map((row, rowIdx) => (
              <tr key={rowIdx} className="hover:bg-zinc-800/30">
                {row.map((cell, cellIdx) => (
                  <td
                    key={cellIdx}
                    className="px-3 py-2 text-zinc-400 border-b border-zinc-800 max-w-[200px] truncate"
                  >
                    {cell || <span className="text-zinc-600 italic">NULL</span>}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );

  const renderColumnMapping = () => (
    <div className="flex-1 overflow-auto p-4">
      <div className="space-y-2">
        <div className="grid grid-cols-12 gap-2 text-xs font-medium text-zinc-500 px-2 mb-2">
          <div className="col-span-3">Source Column</div>
          <div className="col-span-1 text-center">
            <ArrowRight className="w-4 h-4 inline" />
          </div>
          <div className="col-span-3">Target Column</div>
          <div className="col-span-2">Data Type</div>
          <div className="col-span-2">Default Value</div>
          <div className="col-span-1 text-center">Skip</div>
        </div>

        {columnMappings.map((mapping, idx) => (
          <div
            key={idx}
            className={`grid grid-cols-12 gap-2 items-center p-2 rounded-lg ${
              mapping.skip ? "opacity-50 bg-zinc-900" : "bg-zinc-800/30"
            }`}
          >
            <div className="col-span-3">
              <div className="text-sm text-zinc-200">{mapping.source_column}</div>
              <div className="text-xs text-zinc-500">
                {preview?.detected_types[idx]}
              </div>
            </div>

            <div className="col-span-1 text-center">
              <ArrowRight className="w-4 h-4 text-zinc-600" />
            </div>

            <div className="col-span-3">
              <Select
                value={mapping.target_column || "__none__"}
                onValueChange={(v) =>
                  updateMapping(idx, {
                    target_column: v === "__none__" ? "" : v,
                  })
                }
                disabled={mapping.skip}
              >
                <SelectTrigger className="w-full bg-zinc-800 border-zinc-700 h-8 text-sm">
                  <SelectValue placeholder="Select column..." />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">
                    <span className="text-zinc-500">-- Skip --</span>
                  </SelectItem>
                  {targetColumns.map((col) => (
                    <SelectItem key={col.name} value={col.name}>
                      {col.name}
                      <span className="text-xs text-zinc-500 ml-2">
                        ({col.data_type})
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="col-span-2">
              <Select
                value={mapping.target_type || "TEXT"}
                onValueChange={(v) => updateMapping(idx, { target_type: v })}
                disabled={mapping.skip}
              >
                <SelectTrigger className="w-full bg-zinc-800 border-zinc-700 h-8 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {DATA_TYPES.map((type) => (
                    <SelectItem key={type} value={type}>
                      {type}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="col-span-2">
              <Input
                placeholder="NULL"
                value={mapping.default_value || ""}
                onChange={(e) =>
                  updateMapping(idx, {
                    default_value: e.target.value || null,
                  })
                }
                disabled={mapping.skip}
                className="h-8 bg-zinc-800 border-zinc-700 text-sm"
              />
            </div>

            <div className="col-span-1 text-center">
              <Checkbox
                checked={mapping.skip}
                onCheckedChange={(checked) =>
                  updateMapping(idx, { skip: !!checked })
                }
              />
            </div>
          </div>
        ))}
      </div>
    </div>
  );

  const renderOptions = () => (
    <div className="flex-1 p-6">
      <div className="max-w-lg mx-auto space-y-6">
        <div className="space-y-4">
          <h3 className="text-sm font-medium text-zinc-300">Import Settings</h3>

          <div className="flex items-center justify-between">
            <div>
              <Label className="text-zinc-300">First row is header</Label>
              <p className="text-xs text-zinc-500">
                Skip the first row as column names
              </p>
            </div>
            <Checkbox
              checked={options.first_row_is_header}
              onCheckedChange={(checked) =>
                setOptions((o) => ({ ...o, first_row_is_header: !!checked }))
              }
            />
          </div>

          <div className="flex items-center justify-between">
            <div>
              <Label className="text-zinc-300">Truncate table before import</Label>
              <p className="text-xs text-zinc-500">
                Delete all existing data before importing
              </p>
            </div>
            <Checkbox
              checked={options.truncate_before}
              onCheckedChange={(checked) =>
                setOptions((o) => ({ ...o, truncate_before: !!checked }))
              }
            />
          </div>

          <div>
            <Label className="text-zinc-300 mb-2 block">Batch size</Label>
            <Input
              type="number"
              value={options.batch_size}
              onChange={(e) =>
                setOptions((o) => ({ ...o, batch_size: parseInt(e.target.value) || 1000 }))
              }
              className="w-32 bg-zinc-800 border-zinc-700"
            />
            <p className="text-xs text-zinc-500 mt-1">
              Number of rows per transaction
            </p>
          </div>

          {preview?.file_type === "csv" && (
            <div>
              <Label className="text-zinc-300 mb-2 block">CSV Delimiter</Label>
              <Select
                value={options.delimiter || ","}
                onValueChange={(v) => setOptions((o) => ({ ...o, delimiter: v }))}
              >
                <SelectTrigger className="w-32 bg-zinc-800 border-zinc-700">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value=",">Comma (,)</SelectItem>
                  <SelectItem value=";">Semicolon (;)</SelectItem>
                  <SelectItem value="\t">Tab</SelectItem>
                  <SelectItem value="|">Pipe (|)</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}

          <div>
            <Label className="text-zinc-300 mb-2 block">Skip rows</Label>
            <Input
              type="number"
              value={options.skip_rows}
              onChange={(e) =>
                setOptions((o) => ({ ...o, skip_rows: parseInt(e.target.value) || 0 }))
              }
              className="w-32 bg-zinc-800 border-zinc-700"
            />
            <p className="text-xs text-zinc-500 mt-1">
              Skip this many rows from the beginning
            </p>
          </div>
        </div>

        <div className="p-4 bg-zinc-800/50 rounded-lg">
          <h4 className="text-sm font-medium text-zinc-300 mb-2">Import Summary</h4>
          <div className="space-y-1 text-sm text-zinc-400">
            <p>
              File: <span className="text-zinc-200">{filePath.split("/").pop()}</span>
            </p>
            <p>
              Target table:{" "}
              <span className="text-zinc-200">
                {schema}.{selectedTable}
              </span>
            </p>
            <p>
              Columns to import:{" "}
              <span className="text-zinc-200">
                {columnMappings.filter((m) => !m.skip && m.target_column).length}
              </span>
            </p>
            <p>
              Estimated rows:{" "}
              <span className="text-zinc-200">
                {preview?.total_rows?.toLocaleString() || "Unknown"}
              </span>
            </p>
          </div>
        </div>
      </div>
    </div>
  );

  const renderImporting = () => (
    <div className="flex-1 flex flex-col items-center justify-center p-8">
      <Loader2 className="w-12 h-12 text-blue-500 animate-spin mb-4" />
      <p className="text-lg font-medium text-zinc-300 mb-2">Importing data...</p>
      <p className="text-sm text-zinc-500">
        Please wait while your data is being imported
      </p>
    </div>
  );

  const renderComplete = () => (
    <div className="flex-1 flex flex-col items-center justify-center p-8">
      {importResult?.success ? (
        <>
          <CheckCircle className="w-16 h-16 text-green-500 mb-4" />
          <p className="text-xl font-medium text-zinc-200 mb-2">Import Complete!</p>
          <div className="text-center text-zinc-400 space-y-1">
            <p>
              <span className="text-green-400 font-medium">
                {importResult.rows_imported.toLocaleString()}
              </span>{" "}
              rows imported successfully
            </p>
            {importResult.rows_failed > 0 && (
              <p>
                <span className="text-red-400 font-medium">
                  {importResult.rows_failed.toLocaleString()}
                </span>{" "}
                rows failed
              </p>
            )}
          </div>
          {importResult.errors.length > 0 && (
            <div className="mt-4 p-4 bg-red-500/10 border border-red-500/20 rounded-lg max-w-md max-h-40 overflow-auto">
              <p className="text-sm font-medium text-red-400 mb-2">Errors:</p>
              <ul className="text-xs text-red-300 space-y-1">
                {importResult.errors.slice(0, 10).map((err, idx) => (
                  <li key={idx}>{err}</li>
                ))}
                {importResult.errors.length > 10 && (
                  <li>...and {importResult.errors.length - 10} more</li>
                )}
              </ul>
            </div>
          )}
        </>
      ) : (
        <>
          <AlertCircle className="w-16 h-16 text-red-500 mb-4" />
          <p className="text-xl font-medium text-zinc-200 mb-2">Import Failed</p>
          <p className="text-zinc-400">{error || "An unknown error occurred"}</p>
        </>
      )}
    </div>
  );

  const renderStepContent = () => {
    switch (currentStep) {
      case "select-file":
        return renderFileSelection();
      case "preview":
        return renderPreview();
      case "mapping":
        return renderColumnMapping();
      case "options":
        return renderOptions();
      case "importing":
        return renderImporting();
      case "complete":
        return renderComplete();
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-zinc-900 border border-zinc-800 rounded-lg shadow-xl w-[900px] h-[700px] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-800">
          <h2 className="text-lg font-semibold text-zinc-200">Import Data</h2>
          <Button
            variant="ghost"
            size="sm"
            onClick={onClose}
            className="text-zinc-400 hover:text-zinc-200"
          >
            <X className="w-5 h-5" />
          </Button>
        </div>

        {/* Step indicator */}
        {currentStep !== "importing" && currentStep !== "complete" && (
          <div className="py-4 border-b border-zinc-800">{renderStepIndicator()}</div>
        )}

        {/* Content */}
        {renderStepContent()}

        {/* Footer */}
        {currentStep !== "importing" && (
          <div className="flex items-center justify-between px-6 py-4 border-t border-zinc-800">
            <div>
              {currentStep !== "select-file" && currentStep !== "complete" && (
                <Button
                  variant="outline"
                  onClick={goBack}
                  className="border-zinc-700"
                >
                  <ChevronLeft className="w-4 h-4 mr-2" />
                  Back
                </Button>
              )}
            </div>

            <div className="flex gap-3">
              {currentStep === "complete" ? (
                <Button onClick={onClose}>Close</Button>
              ) : (
                <>
                  <Button variant="outline" onClick={onClose} className="border-zinc-700">
                    Cancel
                  </Button>
                  {currentStep === "select-file" ? (
                    <Button
                      onClick={handlePreviewFile}
                      disabled={!filePath || loading}
                    >
                      {loading ? (
                        <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                      ) : null}
                      Preview
                      <ChevronRight className="w-4 h-4 ml-2" />
                    </Button>
                  ) : (
                    <Button onClick={goNext} disabled={!canProceed() || loading}>
                      {currentStep === "options" ? "Start Import" : "Next"}
                      <ChevronRight className="w-4 h-4 ml-2" />
                    </Button>
                  )}
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
