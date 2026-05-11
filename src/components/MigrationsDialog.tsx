/**
 * MigrationsDialog
 *
 * Three-step wizard:
 *   1. Pick source and target connections + schema
 *   2. Review structured diff
 *   3. Preview generated SQL and apply
 */

import { FC, useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import Editor from "@monaco-editor/react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ChevronRight, ChevronDown, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/components/theme-provider";
import type { ConnectionProfile } from "@/types/database";
import type {
  SchemaDiff,
  TableDiff,
  ApplyResult,
} from "@/types/migrations";

interface MigrationsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type Step = "select" | "diff" | "sql";

export const MigrationsDialog: FC<MigrationsDialogProps> = ({
  open,
  onOpenChange,
}) => {
  const { theme } = useTheme();
  const [step, setStep] = useState<Step>("select");
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [sourceId, setSourceId] = useState<string>("");
  const [targetId, setTargetId] = useState<string>("");
  const [schema, setSchema] = useState<string>("public");
  const [diff, setDiff] = useState<SchemaDiff | null>(null);
  const [sql, setSql] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [useTransaction, setUseTransaction] = useState(true);
  const [confirmApply, setConfirmApply] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!open) return;
    setStep("select");
    setDiff(null);
    setSql([]);
    setConfirmApply(false);
    invoke<ConnectionProfile[]>("list_connection_profiles")
      .then(setProfiles)
      .catch((e) => toast.error(`Failed to load connections: ${e}`));
  }, [open]);

  const runDiff = async () => {
    if (!sourceId || !targetId) {
      toast.error("Select both source and target connections");
      return;
    }
    setLoading(true);
    try {
      const result = await invoke<SchemaDiff>("compute_schema_diff", {
        sourceConnectionId: sourceId,
        targetConnectionId: targetId,
        schema,
      });
      setDiff(result);
      setStep("diff");
    } catch (e: any) {
      toast.error(`Diff failed: ${e?.message ?? String(e)}`);
    } finally {
      setLoading(false);
    }
  };

  const runGenerate = async () => {
    if (!diff) return;
    setLoading(true);
    try {
      const result = await invoke<string[]>("generate_migration", {
        diff,
        targetConnectionId: targetId,
      });
      setSql(result);
      setStep("sql");
    } catch (e: any) {
      toast.error(`Generate failed: ${e?.message ?? String(e)}`);
    } finally {
      setLoading(false);
    }
  };

  const runApply = async () => {
    if (!confirmApply) {
      toast.error("Confirm apply first");
      return;
    }
    setLoading(true);
    try {
      const result = await invoke<ApplyResult>("apply_migration", {
        connectionId: targetId,
        statements: sql,
        useTransaction,
      });
      if (result.error) {
        toast.error(
          `Failed after ${result.succeeded}/${result.executed} statements: ${result.error}`
        );
      } else {
        toast.success(`Applied ${result.succeeded} statement(s)`);
        onOpenChange(false);
      }
    } catch (e: any) {
      toast.error(`Apply failed: ${e?.message ?? String(e)}`);
    } finally {
      setLoading(false);
    }
  };

  const sqlText = useMemo(() => sql.join(";\n\n") + (sql.length ? ";" : ""), [sql]);

  const toggleExpand = (key: string) =>
    setExpanded((prev) => ({ ...prev, [key]: !prev[key] }));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Schema Migrations</DialogTitle>
          <DialogDescription>
            Compare two connections and generate migration SQL to sync schemas.
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-hidden flex flex-col min-h-[400px]">
          {step === "select" && (
            <div className="space-y-4 p-1">
              <div className="space-y-2">
                <Label>Source (desired schema)</Label>
                <Select value={sourceId} onValueChange={setSourceId}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select source connection" />
                  </SelectTrigger>
                  <SelectContent>
                    {profiles.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        {p.name} ({p.driver})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label>Target (will be modified)</Label>
                <Select value={targetId} onValueChange={setTargetId}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select target connection" />
                  </SelectTrigger>
                  <SelectContent>
                    {profiles.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        {p.name} ({p.driver})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label>Schema</Label>
                <Input
                  value={schema}
                  onChange={(e) => setSchema(e.target.value)}
                  placeholder="public"
                />
              </div>
              <p className="text-xs text-muted-foreground">
                Both connections must be active (connected) before running.
              </p>
            </div>
          )}

          {step === "diff" && diff && (
            <ScrollArea className="flex-1 pr-3">
              <DiffView diff={diff} expanded={expanded} onToggle={toggleExpand} />
            </ScrollArea>
          )}

          {step === "sql" && (
            <div className="flex-1 flex flex-col gap-2 min-h-[300px]">
              <div className="flex-1 border rounded overflow-hidden">
                <Editor
                  height="100%"
                  language="sql"
                  value={sqlText}
                  theme={theme === "dark" ? "vs-dark" : "vs-light"}
                  options={{
                    readOnly: true,
                    minimap: { enabled: false },
                    fontSize: 13,
                    wordWrap: "on",
                  }}
                />
              </div>
              <div className="flex items-center gap-4 text-sm">
                <label className="flex items-center gap-2">
                  <Checkbox
                    checked={useTransaction}
                    onCheckedChange={(v) => setUseTransaction(!!v)}
                  />
                  Run inside a transaction
                </label>
                <label className="flex items-center gap-2">
                  <Checkbox
                    checked={confirmApply}
                    onCheckedChange={(v) => setConfirmApply(!!v)}
                  />
                  I understand this will modify the target database
                </label>
              </div>
              <p className="text-xs text-muted-foreground">
                {sql.length} statement(s) will be executed against the target.
              </p>
            </div>
          )}
        </div>

        <DialogFooter className="gap-2">
          {step === "select" && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button onClick={runDiff} disabled={loading}>
                {loading && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                Compute Diff
              </Button>
            </>
          )}
          {step === "diff" && (
            <>
              <Button variant="outline" onClick={() => setStep("select")}>
                Back
              </Button>
              <Button
                onClick={runGenerate}
                disabled={loading || !diff || isEmptyDiff(diff)}
              >
                {loading && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                Generate SQL
              </Button>
            </>
          )}
          {step === "sql" && (
            <>
              <Button variant="outline" onClick={() => setStep("diff")}>
                Back
              </Button>
              <Button
                onClick={runApply}
                disabled={loading || !confirmApply || sql.length === 0}
                variant="destructive"
              >
                {loading && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                Apply Migration
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

function isEmptyDiff(d: SchemaDiff): boolean {
  return (
    d.addedTables.length === 0 &&
    d.removedTables.length === 0 &&
    d.modifiedTables.length === 0
  );
}

interface DiffViewProps {
  diff: SchemaDiff;
  expanded: Record<string, boolean>;
  onToggle: (key: string) => void;
}

const DiffView: FC<DiffViewProps> = ({ diff, expanded, onToggle }) => {
  if (isEmptyDiff(diff)) {
    return (
      <div className="py-8 text-center text-sm text-muted-foreground">
        Schemas are identical. No migration needed.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {diff.addedTables.length > 0 && (
        <Section title={`Added tables (${diff.addedTables.length})`} color="text-green-600">
          {diff.addedTables.map((t) => (
            <div key={`a-${t.table.schema}.${t.table.name}`} className="py-1 text-sm">
              + {t.table.schema}.{t.table.name} ({t.columns.length} columns)
            </div>
          ))}
        </Section>
      )}
      {diff.removedTables.length > 0 && (
        <Section title={`Removed tables (${diff.removedTables.length})`} color="text-red-600">
          {diff.removedTables.map((t) => (
            <div key={`r-${t.table.schema}.${t.table.name}`} className="py-1 text-sm">
              - {t.table.schema}.{t.table.name}
            </div>
          ))}
        </Section>
      )}
      {diff.modifiedTables.length > 0 && (
        <Section
          title={`Modified tables (${diff.modifiedTables.length})`}
          color="text-yellow-600"
        >
          {diff.modifiedTables.map((td) => (
            <TableDiffRow
              key={`m-${td.schema}.${td.name}`}
              td={td}
              expanded={!!expanded[`${td.schema}.${td.name}`]}
              onToggle={() => onToggle(`${td.schema}.${td.name}`)}
            />
          ))}
        </Section>
      )}
    </div>
  );
};

const Section: FC<{ title: string; color: string; children: React.ReactNode }> = ({
  title,
  color,
  children,
}) => (
  <div>
    <div className={`text-sm font-semibold mb-1 ${color}`}>{title}</div>
    <div className="pl-2 border-l">{children}</div>
  </div>
);

const TableDiffRow: FC<{
  td: TableDiff;
  expanded: boolean;
  onToggle: () => void;
}> = ({ td, expanded, onToggle }) => {
  const totalChanges =
    td.addedColumns.length +
    td.removedColumns.length +
    td.modifiedColumns.length +
    td.addedIndexes.length +
    td.removedIndexes.length +
    td.addedFks.length +
    td.removedFks.length;

  return (
    <div className="py-1">
      <button
        onClick={onToggle}
        className="flex items-center gap-1 text-sm w-full text-left hover:bg-accent/50 px-1 rounded"
      >
        {expanded ? (
          <ChevronDown className="h-3 w-3" />
        ) : (
          <ChevronRight className="h-3 w-3" />
        )}
        <span>
          {td.schema}.{td.name}
        </span>
        <span className="text-xs text-muted-foreground">({totalChanges} changes)</span>
      </button>
      {expanded && (
        <div className="pl-6 pt-1 space-y-0.5 text-xs font-mono">
          {td.addedColumns.map((c) => (
            <div key={`ac-${c.name}`} className="text-green-600">
              + column {c.name} {c.dataType}
            </div>
          ))}
          {td.removedColumns.map((c) => (
            <div key={`rc-${c.name}`} className="text-red-600">
              - column {c.name}
            </div>
          ))}
          {td.modifiedColumns.map((c) => (
            <div key={`mc-${c.name}`} className="text-yellow-600">
              ~ column {c.name}: {c.target.dataType} → {c.source.dataType}
            </div>
          ))}
          {td.addedIndexes.map((i) => (
            <div key={`ai-${i.name}`} className="text-green-600">
              + index {i.name} ({i.columns.join(", ")})
            </div>
          ))}
          {td.removedIndexes.map((i) => (
            <div key={`ri-${i.name}`} className="text-red-600">
              - index {i.name}
            </div>
          ))}
          {td.addedFks.map((f) => (
            <div key={`af-${f.name}`} className="text-green-600">
              + fk {f.name} → {f.referencedTable}
            </div>
          ))}
          {td.removedFks.map((f) => (
            <div key={`rf-${f.name}`} className="text-red-600">
              - fk {f.name}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
