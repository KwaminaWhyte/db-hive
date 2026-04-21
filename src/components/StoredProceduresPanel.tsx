import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronDown,
  ChevronRight,
  FunctionSquare,
  Play,
  RefreshCw,
  Code2,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Badge } from "./ui/badge";
import { ScrollArea } from "./ui/scroll-area";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { EmptyState } from "./EmptyState";
import { ErrorState } from "./ErrorState";
import { ProcedureInfo, QueryExecutionResult } from "@/types/database";
import { toast } from "sonner";

interface StoredProceduresPanelProps {
  connectionId: string;
  onExecute?: (sql: string) => void;
}

interface ArgInput {
  raw: string;
}

// Parse "(a integer, b text)" into ["a integer", "b text"].
// Naive split on top-level commas — good enough for display and prompting.
function parseArgs(signature: string): string[] {
  const inner = signature.trim().replace(/^\(/, "").replace(/\)$/, "").trim();
  if (!inner) return [];
  const out: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of inner) {
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      out.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) out.push(current.trim());
  return out;
}

export function StoredProceduresPanel({
  connectionId,
  onExecute,
}: StoredProceduresPanelProps) {
  const [procedures, setProcedures] = useState<ProcedureInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [definitions, setDefinitions] = useState<Record<string, string>>({});
  const [loadingDef, setLoadingDef] = useState<string | null>(null);
  const [execTarget, setExecTarget] = useState<ProcedureInfo | null>(null);
  const [execArgs, setExecArgs] = useState<ArgInput[]>([]);
  const [executing, setExecuting] = useState(false);

  const fetchProcedures = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<ProcedureInfo[]>("list_procedures", {
        connectionId,
        schema: null,
      });
      setProcedures(data);
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchProcedures();
  }, [connectionId]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return procedures;
    return procedures.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.schema.toLowerCase().includes(q),
    );
  }, [procedures, search]);

  const grouped = useMemo(() => {
    const map = new Map<string, ProcedureInfo[]>();
    for (const p of filtered) {
      if (!map.has(p.schema)) map.set(p.schema, []);
      map.get(p.schema)!.push(p);
    }
    return Array.from(map.entries()).sort((a, b) => a[0].localeCompare(b[0]));
  }, [filtered]);

  const keyOf = (p: ProcedureInfo) => `${p.schema}.${p.name}`;

  const toggleExpand = async (p: ProcedureInfo) => {
    const key = keyOf(p);
    if (expandedKey === key) {
      setExpandedKey(null);
      return;
    }
    setExpandedKey(key);
    if (!definitions[key]) {
      setLoadingDef(key);
      try {
        const def = await invoke<string>("get_procedure_definition", {
          connectionId,
          schema: p.schema,
          name: p.name,
        });
        setDefinitions((prev) => ({ ...prev, [key]: def }));
      } catch (err: any) {
        setDefinitions((prev) => ({
          ...prev,
          [key]: `-- Failed to load definition: ${err?.message || String(err)}`,
        }));
      } finally {
        setLoadingDef(null);
      }
    }
  };

  const openExecuteDialog = (p: ProcedureInfo) => {
    setExecTarget(p);
    setExecArgs(parseArgs(p.argumentSignature).map(() => ({ raw: "" })));
  };

  const runExecute = async () => {
    if (!execTarget) return;
    setExecuting(true);
    try {
      // Each input is parsed as JSON when possible, otherwise passed as a string.
      const args = execArgs.map(({ raw }) => {
        const trimmed = raw.trim();
        if (trimmed === "") return null;
        try {
          return JSON.parse(trimmed);
        } catch {
          return trimmed;
        }
      });
      const result = await invoke<QueryExecutionResult>("execute_procedure", {
        connectionId,
        schema: execTarget.schema,
        name: execTarget.name,
        args,
      });
      toast.success(
        `Executed ${execTarget.schema}.${execTarget.name} (${result.rows?.length ?? 0} rows)`,
      );
      if (onExecute) {
        const rendered = args
          .map((a) =>
            a === null
              ? "NULL"
              : typeof a === "string"
                ? `'${a.replace(/'/g, "''")}'`
                : typeof a === "number" || typeof a === "boolean"
                  ? String(a)
                  : `'${JSON.stringify(a).replace(/'/g, "''")}'`,
          )
          .join(", ");
        const sql =
          execTarget.kind === "procedure"
            ? `CALL "${execTarget.schema}"."${execTarget.name}"(${rendered});`
            : `SELECT "${execTarget.schema}"."${execTarget.name}"(${rendered});`;
        onExecute(sql);
      }
      setExecTarget(null);
    } catch (err: any) {
      toast.error("Execution failed", {
        description: err?.message || String(err),
      });
    } finally {
      setExecuting(false);
    }
  };

  if (loading) {
    return (
      <Card className="h-full">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <FunctionSquare className="h-4 w-4" />
            Procedures & Functions
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <RefreshCw className="h-4 w-4 animate-spin" />
            Loading routines...
          </div>
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <ErrorState
        title="Failed to load procedures"
        message={error}
        actions={[{ label: "Retry", onClick: fetchProcedures }]}
      />
    );
  }

  if (procedures.length === 0) {
    return (
      <EmptyState
        title="No stored procedures or functions"
        message="This database has no user-defined routines, or the driver does not expose them."
        icon={FunctionSquare}
        actions={[
          { label: "Refresh", onClick: fetchProcedures, icon: RefreshCw },
        ]}
      />
    );
  }

  return (
    <>
      <Card className="h-full flex flex-col">
        <CardHeader className="shrink-0">
          <div className="flex items-center justify-between gap-2">
            <CardTitle className="flex items-center gap-2 text-base">
              <FunctionSquare className="h-4 w-4" />
              Procedures & Functions
              <Badge variant="secondary">{procedures.length}</Badge>
            </CardTitle>
            <Button
              variant="ghost"
              size="icon"
              onClick={fetchProcedures}
              title="Refresh"
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
          </div>
          <Input
            placeholder="Search by name or schema..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </CardHeader>
        <CardContent className="flex-1 overflow-hidden p-0">
          <ScrollArea className="h-full">
            <div className="p-4 space-y-4">
              {grouped.map(([schema, items]) => (
                <div key={schema} className="space-y-1">
                  <div className="text-xs font-semibold uppercase text-muted-foreground px-2">
                    {schema}
                  </div>
                  {items.map((p) => {
                    const key = keyOf(p);
                    const isExpanded = expandedKey === key;
                    return (
                      <div
                        key={key}
                        className="rounded-md border bg-card text-card-foreground"
                      >
                        <div className="flex items-center gap-2 px-3 py-2">
                          <button
                            className="flex items-center gap-2 flex-1 min-w-0 text-left"
                            onClick={() => toggleExpand(p)}
                          >
                            {isExpanded ? (
                              <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
                            ) : (
                              <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0" />
                            )}
                            <Code2 className="h-4 w-4 text-blue-500 shrink-0" />
                            <span className="font-mono text-sm truncate">
                              {p.name}
                            </span>
                            <Badge
                              variant={
                                p.kind === "procedure" ? "default" : "secondary"
                              }
                              className="shrink-0"
                            >
                              {p.kind}
                            </Badge>
                            <span className="text-xs text-muted-foreground font-mono truncate">
                              {p.argumentSignature}
                            </span>
                            {p.returnType && (
                              <span className="text-xs text-muted-foreground shrink-0">
                                -&gt; {p.returnType}
                              </span>
                            )}
                          </button>
                          <Button
                            variant="outline"
                            size="sm"
                            onClick={() => openExecuteDialog(p)}
                          >
                            <Play className="h-3 w-3 mr-1" />
                            Execute
                          </Button>
                        </div>
                        {isExpanded && (
                          <div className="border-t bg-muted/40 px-3 py-2">
                            {loadingDef === key ? (
                              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                                <RefreshCw className="h-4 w-4 animate-spin" />
                                Loading definition...
                              </div>
                            ) : (
                              <pre className="text-xs font-mono whitespace-pre-wrap break-all max-h-96 overflow-auto">
                                {definitions[key] || "-- (no definition)"}
                              </pre>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>

      <Dialog
        open={!!execTarget}
        onOpenChange={(open) => !open && !executing && setExecTarget(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              Execute {execTarget?.schema}.{execTarget?.name}
            </DialogTitle>
            <DialogDescription className="font-mono text-xs">
              {execTarget?.argumentSignature}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            {execTarget && parseArgs(execTarget.argumentSignature).length === 0 ? (
              <p className="text-sm text-muted-foreground">
                This routine takes no arguments.
              </p>
            ) : (
              parseArgs(execTarget?.argumentSignature || "").map((arg, idx) => (
                <div key={idx} className="space-y-1">
                  <label className="text-xs font-mono text-muted-foreground">
                    {arg}
                  </label>
                  <Input
                    placeholder='Value (JSON-parsed; e.g. 42, "hello", true)'
                    value={execArgs[idx]?.raw ?? ""}
                    onChange={(e) => {
                      const next = [...execArgs];
                      next[idx] = { raw: e.target.value };
                      setExecArgs(next);
                    }}
                  />
                </div>
              ))
            )}
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setExecTarget(null)}
              disabled={executing}
            >
              Cancel
            </Button>
            <Button onClick={runExecute} disabled={executing}>
              {executing ? (
                <>
                  <RefreshCw className="h-4 w-4 mr-1 animate-spin" />
                  Running...
                </>
              ) : (
                <>
                  <Play className="h-4 w-4 mr-1" />
                  Run
                </>
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
