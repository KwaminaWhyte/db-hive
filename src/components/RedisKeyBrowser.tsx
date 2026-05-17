import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  KeyRound,
  RefreshCw,
  Search,
  Trash2,
  Copy,
  Loader2,
  Database,
} from "lucide-react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { QueryExecutionResult } from "@/types";
import { toast } from "sonner";

interface RedisKeyBrowserProps {
  connectionId: string;
}

/** Redis key data types reported by the TYPE command */
type RedisType =
  | "string"
  | "hash"
  | "list"
  | "set"
  | "zset"
  | "stream"
  | "none";

/** Parsed value for a selected key, discriminated by Redis type */
type KeyValue =
  | { kind: "string"; value: string }
  | { kind: "hash"; entries: Array<{ field: string; value: string }> }
  | { kind: "list"; items: string[] }
  | { kind: "set"; members: string[] }
  | { kind: "zset"; entries: Array<{ member: string; score: string }> }
  | { kind: "raw"; cells: string[] };

const MAX_DISPLAYED_KEYS = 2000;

/** Extract a human-readable error message from a Tauri/DbError reject */
function errMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    const m = (err as { message?: unknown }).message;
    if (typeof m === "string") return m;
  }
  return String(err);
}

/** Coerce any Redis reply cell into a flat string */
function cellToString(cell: unknown): string {
  if (cell === null || cell === undefined) return "";
  if (typeof cell === "string") return cell;
  if (typeof cell === "number" || typeof cell === "boolean") return String(cell);
  return JSON.stringify(cell);
}

/**
 * Flatten a QueryExecutionResult into a single ordered list of string cells.
 * The Redis driver maps array replies into multiple rows and scalar replies
 * into a single cell, so we read defensively across rows/columns.
 */
function flattenResult(res: QueryExecutionResult): string[] {
  const out: string[] = [];
  for (const row of res.rows ?? []) {
    if (Array.isArray(row)) {
      for (const cell of row) {
        if (Array.isArray(cell)) {
          for (const inner of cell) out.push(cellToString(inner));
        } else {
          out.push(cellToString(cell));
        }
      }
    } else {
      out.push(cellToString(row));
    }
  }
  return out;
}

/** Type badge color mapping consistent with the codebase's small-badge style */
function typeBadgeClass(type: RedisType): string {
  switch (type) {
    case "string":
      return "text-blue-500 bg-blue-500/10";
    case "hash":
      return "text-orange-500 bg-orange-500/10";
    case "list":
      return "text-green-500 bg-green-500/10";
    case "set":
      return "text-purple-500 bg-purple-500/10";
    case "zset":
      return "text-pink-500 bg-pink-500/10";
    case "stream":
      return "text-cyan-500 bg-cyan-500/10";
    default:
      return "text-muted-foreground bg-muted";
  }
}

/** Pretty-print a string if it parses as JSON, otherwise return it unchanged */
function maybePrettyJson(value: string): string {
  const trimmed = value.trim();
  if (
    (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
    (trimmed.startsWith("[") && trimmed.endsWith("]"))
  ) {
    try {
      return JSON.stringify(JSON.parse(trimmed), null, 2);
    } catch {
      return value;
    }
  }
  return value;
}

export function RedisKeyBrowser({ connectionId }: RedisKeyBrowserProps) {
  const [pattern, setPattern] = useState<string>("*");
  const [patternInput, setPatternInput] = useState<string>("*");
  const [keys, setKeys] = useState<string[]>([]);
  const [cursor, setCursor] = useState<string>("0");
  const [exhausted, setExhausted] = useState<boolean>(false);
  const [scanning, setScanning] = useState<boolean>(false);
  const [dbSize, setDbSize] = useState<number | null>(null);

  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [selectedType, setSelectedType] = useState<RedisType | null>(null);
  const [selectedTtl, setSelectedTtl] = useState<number | null>(null);
  const [selectedValue, setSelectedValue] = useState<KeyValue | null>(null);
  const [valueLoading, setValueLoading] = useState<boolean>(false);

  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<boolean>(false);

  /** Run a raw Redis command via the generic execute_query bridge */
  const runCommand = useCallback(
    async (sql: string): Promise<QueryExecutionResult> => {
      return invoke<QueryExecutionResult>("execute_query", {
        connectionId,
        sql,
      });
    },
    [connectionId]
  );

  /** Fetch DBSIZE for the keys badge (best-effort) */
  const fetchDbSize = useCallback(async () => {
    try {
      const res = await runCommand("DBSIZE");
      const cells = flattenResult(res);
      const n = parseInt(cells[0] ?? "", 10);
      setDbSize(Number.isNaN(n) ? null : n);
    } catch {
      setDbSize(null);
    }
  }, [runCommand]);

  /**
   * Continue (or start) a SCAN at the current cursor. When `reset` is true the
   * accumulated list and cursor are cleared first (used on refresh / pattern
   * change).
   */
  const scan = useCallback(
    async (reset: boolean, scanCursor: string, scanPattern: string) => {
      setScanning(true);
      try {
        const safePattern = scanPattern.trim() === "" ? "*" : scanPattern.trim();
        const res = await runCommand(
          `SCAN ${scanCursor} MATCH ${safePattern} COUNT 200`
        );
        const cells = flattenResult(res);
        // Reply shape: [ newCursor, key1, key2, ... ] once flattened.
        const newCursor = cells[0] ?? "0";
        const foundKeys = cells.slice(1);
        setCursor(newCursor);
        setExhausted(newCursor === "0");
        setKeys((prev) => {
          const base = reset ? [] : prev;
          const merged = [...base, ...foundKeys];
          // De-duplicate while preserving order (SCAN may repeat keys).
          const seen = new Set<string>();
          const unique: string[] = [];
          for (const k of merged) {
            if (!seen.has(k)) {
              seen.add(k);
              unique.push(k);
            }
          }
          return unique;
        });
      } catch (err) {
        toast.error(`Scan failed: ${errMessage(err)}`);
      } finally {
        setScanning(false);
      }
    },
    [runCommand]
  );

  // Initial load + whenever the active pattern changes: reset and rescan.
  useEffect(() => {
    setKeys([]);
    setCursor("0");
    setExhausted(false);
    setSelectedKey(null);
    setSelectedType(null);
    setSelectedTtl(null);
    setSelectedValue(null);
    scan(true, "0", pattern);
    fetchDbSize();
  }, [pattern, connectionId, scan, fetchDbSize]);

  const handleApplyPattern = useCallback(() => {
    const next = patternInput.trim() === "" ? "*" : patternInput.trim();
    setPattern(next);
  }, [patternInput]);

  const handleRefresh = useCallback(() => {
    // Re-trigger the effect by resetting cursor/list explicitly.
    setKeys([]);
    setCursor("0");
    setExhausted(false);
    setSelectedKey(null);
    setSelectedType(null);
    setSelectedTtl(null);
    setSelectedValue(null);
    scan(true, "0", pattern);
    fetchDbSize();
  }, [pattern, scan, fetchDbSize]);

  const handleScanMore = useCallback(() => {
    if (exhausted || scanning) return;
    scan(false, cursor, pattern);
  }, [exhausted, scanning, cursor, pattern, scan]);

  /** Load TYPE + TTL + the type-appropriate value for a key */
  const selectKey = useCallback(
    async (key: string) => {
      setSelectedKey(key);
      setSelectedType(null);
      setSelectedTtl(null);
      setSelectedValue(null);
      setValueLoading(true);
      try {
        const typeRes = await runCommand(`TYPE ${key}`);
        const type = (flattenResult(typeRes)[0] ?? "none") as RedisType;
        setSelectedType(type);

        const ttlRes = await runCommand(`TTL ${key}`);
        const ttlRaw = parseInt(flattenResult(ttlRes)[0] ?? "", 10);
        setSelectedTtl(Number.isNaN(ttlRaw) ? -2 : ttlRaw);

        let value: KeyValue;
        switch (type) {
          case "string": {
            const r = await runCommand(`GET ${key}`);
            value = { kind: "string", value: flattenResult(r).join("") };
            break;
          }
          case "hash": {
            const r = await runCommand(`HGETALL ${key}`);
            const flat = flattenResult(r);
            const entries: Array<{ field: string; value: string }> = [];
            for (let i = 0; i < flat.length; i += 2) {
              entries.push({
                field: flat[i] ?? "",
                value: flat[i + 1] ?? "",
              });
            }
            value = { kind: "hash", entries };
            break;
          }
          case "list": {
            const r = await runCommand(`LRANGE ${key} 0 200`);
            value = { kind: "list", items: flattenResult(r) };
            break;
          }
          case "set": {
            const r = await runCommand(`SMEMBERS ${key}`);
            value = { kind: "set", members: flattenResult(r) };
            break;
          }
          case "zset": {
            const r = await runCommand(`ZRANGE ${key} 0 200 WITHSCORES`);
            const flat = flattenResult(r);
            const entries: Array<{ member: string; score: string }> = [];
            for (let i = 0; i < flat.length; i += 2) {
              entries.push({
                member: flat[i] ?? "",
                score: flat[i + 1] ?? "",
              });
            }
            value = { kind: "zset", entries };
            break;
          }
          default: {
            // stream / none / anything unexpected: show raw cells read-only.
            const r = await runCommand(`GET ${key}`).catch(() => null);
            const cells = r ? flattenResult(r) : [];
            value = { kind: "raw", cells };
            break;
          }
        }
        setSelectedValue(value);
      } catch (err) {
        toast.error(`Failed to load key: ${errMessage(err)}`);
        setSelectedValue(null);
      } finally {
        setValueLoading(false);
      }
    },
    [runCommand]
  );

  const copyKeyName = useCallback(async (key: string) => {
    try {
      await navigator.clipboard.writeText(key);
      toast.success("Key name copied");
    } catch {
      toast.error("Failed to copy to clipboard");
    }
  }, []);

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await runCommand(`DEL ${deleteTarget}`);
      setKeys((prev) => prev.filter((k) => k !== deleteTarget));
      if (selectedKey === deleteTarget) {
        setSelectedKey(null);
        setSelectedType(null);
        setSelectedTtl(null);
        setSelectedValue(null);
      }
      toast.success(`Deleted key "${deleteTarget}"`);
      setDeleteTarget(null);
      fetchDbSize();
    } catch (err) {
      toast.error(`Delete failed: ${errMessage(err)}`);
    } finally {
      setDeleting(false);
    }
  }, [deleteTarget, runCommand, selectedKey, fetchDbSize]);

  const displayedKeys = useMemo(
    () => keys.slice(0, MAX_DISPLAYED_KEYS),
    [keys]
  );

  const ttlLabel = useMemo(() => {
    if (selectedTtl === null) return "";
    if (selectedTtl === -1) return "no expiry";
    if (selectedTtl === -2) return "expired/none";
    return `${selectedTtl}s`;
  }, [selectedTtl]);

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 border-b shrink-0">
        <div className="flex items-center gap-2">
          <KeyRound className="h-4 w-4 text-muted-foreground" />
          <h3 className="text-sm font-semibold">Redis Keys</h3>
        </div>
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
          <Input
            className="h-8 pl-8 text-sm"
            placeholder="Key pattern (e.g. user:*)"
            value={patternInput}
            onChange={(e) => setPatternInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleApplyPattern();
            }}
          />
        </div>
        <Button
          variant="outline"
          size="sm"
          className="h-8"
          onClick={handleApplyPattern}
          disabled={scanning}
        >
          <Search className="h-3.5 w-3.5 mr-1" />
          Search
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-8"
          onClick={handleRefresh}
          disabled={scanning}
          title="Refresh"
        >
          <RefreshCw
            className={`h-4 w-4 ${scanning ? "animate-spin" : ""}`}
          />
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="h-8"
          onClick={handleScanMore}
          disabled={exhausted || scanning}
          title={exhausted ? "All keys scanned" : "Continue SCAN cursor"}
        >
          {scanning ? (
            <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
          ) : null}
          Scan more
        </Button>
        <Badge variant="secondary" className="ml-auto shrink-0">
          {dbSize !== null
            ? `${dbSize.toLocaleString()} keys`
            : `${keys.length} loaded`}
        </Badge>
      </div>

      {/* Two-pane layout */}
      <PanelGroup direction="horizontal" className="flex-1">
        {/* LEFT: key list */}
        <Panel defaultSize={35} minSize={20}>
          <div className="h-full flex flex-col">
            <div className="flex items-center justify-between px-3 py-1.5 border-b bg-muted/30 shrink-0">
              <span className="text-xs text-muted-foreground">
                {keys.length.toLocaleString()} key
                {keys.length === 1 ? "" : "s"} loaded
                {keys.length > MAX_DISPLAYED_KEYS
                  ? ` (showing ${MAX_DISPLAYED_KEYS.toLocaleString()})`
                  : ""}
              </span>
              {exhausted && keys.length > 0 && (
                <span className="text-[10px] text-muted-foreground">
                  scan complete
                </span>
              )}
            </div>
            {scanning && keys.length === 0 ? (
              <div className="flex-1 flex items-center justify-center">
                <div className="text-center space-y-2">
                  <Loader2 className="h-6 w-6 mx-auto animate-spin text-muted-foreground" />
                  <p className="text-sm text-muted-foreground">
                    Scanning keys...
                  </p>
                </div>
              </div>
            ) : displayedKeys.length === 0 ? (
              <div className="flex-1 flex items-center justify-center p-4">
                <div className="text-center space-y-2">
                  <KeyRound className="h-10 w-10 mx-auto text-muted-foreground opacity-50" />
                  <p className="text-muted-foreground">No keys match</p>
                  <p className="text-sm text-muted-foreground">
                    Try a different pattern or scan more.
                  </p>
                </div>
              </div>
            ) : (
              <ScrollArea className="flex-1">
                <div className="py-1">
                  {displayedKeys.map((key) => {
                    const isSelected = key === selectedKey;
                    return (
                      <div
                        key={key}
                        onClick={() => selectKey(key)}
                        className={`group flex items-center gap-2 px-3 py-1.5 cursor-pointer text-sm border-l-2 ${
                          isSelected
                            ? "bg-muted border-l-primary"
                            : "border-l-transparent hover:bg-muted/50"
                        }`}
                      >
                        <KeyRound className="h-3 w-3 shrink-0 text-muted-foreground" />
                        <span className="font-mono truncate flex-1">
                          {key}
                        </span>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                          onClick={(e) => {
                            e.stopPropagation();
                            setDeleteTarget(key);
                          }}
                          title="Delete key"
                        >
                          <Trash2 className="h-3 w-3 text-destructive" />
                        </Button>
                      </div>
                    );
                  })}
                </div>
              </ScrollArea>
            )}
          </div>
        </Panel>

        <PanelResizeHandle className="w-1 bg-border hover:bg-primary/50 transition-colors" />

        {/* RIGHT: value viewer */}
        <Panel defaultSize={65} minSize={30}>
          {selectedKey === null ? (
            <div className="h-full flex items-center justify-center p-4">
              <div className="text-center space-y-2">
                <Database className="h-10 w-10 mx-auto text-muted-foreground opacity-50" />
                <p className="text-muted-foreground">No key selected</p>
                <p className="text-sm text-muted-foreground">
                  Select a key from the list to inspect its value.
                </p>
              </div>
            </div>
          ) : (
            <div className="h-full flex flex-col">
              {/* Key header */}
              <div className="flex items-start justify-between gap-3 px-3 py-2 border-b shrink-0">
                <div className="min-w-0 space-y-1">
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => copyKeyName(selectedKey)}
                      className="font-mono text-sm truncate hover:text-primary flex items-center gap-1.5"
                      title="Copy key name"
                    >
                      <span className="truncate">{selectedKey}</span>
                      <Copy className="h-3 w-3 shrink-0 opacity-60" />
                    </button>
                  </div>
                  <div className="flex items-center gap-2">
                    {selectedType && (
                      <span
                        className={`text-[10px] font-bold px-1.5 py-0.5 rounded leading-none ${typeBadgeClass(
                          selectedType
                        )}`}
                      >
                        {selectedType.toUpperCase()}
                      </span>
                    )}
                    {selectedTtl !== null && (
                      <span className="text-xs text-muted-foreground">
                        TTL: {ttlLabel}
                      </span>
                    )}
                  </div>
                </div>
                <Button
                  variant="destructive"
                  size="sm"
                  className="h-8 shrink-0"
                  onClick={() => setDeleteTarget(selectedKey)}
                >
                  <Trash2 className="h-3.5 w-3.5 mr-1" />
                  Delete
                </Button>
              </div>

              {/* Value */}
              <div className="flex-1 overflow-hidden">
                {valueLoading ? (
                  <div className="h-full flex items-center justify-center">
                    <div className="text-center space-y-2">
                      <Loader2 className="h-6 w-6 mx-auto animate-spin text-muted-foreground" />
                      <p className="text-sm text-muted-foreground">
                        Loading value...
                      </p>
                    </div>
                  </div>
                ) : !selectedValue ? (
                  <div className="h-full flex items-center justify-center">
                    <p className="text-muted-foreground">
                      No value to display
                    </p>
                  </div>
                ) : selectedValue.kind === "string" ? (
                  <ScrollArea className="h-full">
                    <pre className="p-3 text-sm font-mono whitespace-pre-wrap break-all">
                      {maybePrettyJson(selectedValue.value)}
                    </pre>
                  </ScrollArea>
                ) : selectedValue.kind === "hash" ? (
                  <ScrollArea className="h-full">
                    <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
                      <TableHeader className="sticky top-0 bg-background z-10">
                        <TableRow>
                          <TableHead className="whitespace-nowrap">
                            Field
                          </TableHead>
                          <TableHead className="whitespace-nowrap">
                            Value
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {selectedValue.entries.map((e, i) => (
                          <TableRow key={`${e.field}-${i}`}>
                            <TableCell className="font-mono text-xs align-top">
                              {e.field}
                            </TableCell>
                            <TableCell className="font-mono text-xs whitespace-pre-wrap break-all">
                              {e.value}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </ScrollArea>
                ) : selectedValue.kind === "list" ? (
                  <ScrollArea className="h-full">
                    <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
                      <TableHeader className="sticky top-0 bg-background z-10">
                        <TableRow>
                          <TableHead className="w-16 whitespace-nowrap">
                            Index
                          </TableHead>
                          <TableHead className="whitespace-nowrap">
                            Value
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {selectedValue.items.map((item, i) => (
                          <TableRow key={i}>
                            <TableCell className="font-mono text-xs text-muted-foreground">
                              {i}
                            </TableCell>
                            <TableCell className="font-mono text-xs whitespace-pre-wrap break-all">
                              {item}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </ScrollArea>
                ) : selectedValue.kind === "set" ? (
                  <ScrollArea className="h-full">
                    <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
                      <TableHeader className="sticky top-0 bg-background z-10">
                        <TableRow>
                          <TableHead className="whitespace-nowrap">
                            Member
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {selectedValue.members.map((m, i) => (
                          <TableRow key={i}>
                            <TableCell className="font-mono text-xs whitespace-pre-wrap break-all">
                              {m}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </ScrollArea>
                ) : selectedValue.kind === "zset" ? (
                  <ScrollArea className="h-full">
                    <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
                      <TableHeader className="sticky top-0 bg-background z-10">
                        <TableRow>
                          <TableHead className="whitespace-nowrap">
                            Member
                          </TableHead>
                          <TableHead className="w-32 whitespace-nowrap">
                            Score
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {selectedValue.entries.map((e, i) => (
                          <TableRow key={`${e.member}-${i}`}>
                            <TableCell className="font-mono text-xs whitespace-pre-wrap break-all">
                              {e.member}
                            </TableCell>
                            <TableCell className="font-mono text-xs text-muted-foreground">
                              {e.score}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </ScrollArea>
                ) : (
                  <ScrollArea className="h-full">
                    <pre className="p-3 text-sm font-mono whitespace-pre-wrap break-all">
                      {selectedValue.cells.join("\n") ||
                        "(empty / unsupported type — read-only)"}
                    </pre>
                  </ScrollArea>
                )}
              </div>
            </div>
          )}
        </Panel>
      </PanelGroup>

      {/* Delete confirmation */}
      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete key?</AlertDialogTitle>
            <AlertDialogDescription>
              This action cannot be undone. This will permanently delete the
              key{" "}
              <span className="font-mono font-semibold">
                {deleteTarget}
              </span>{" "}
              from Redis.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                confirmDelete();
              }}
              disabled={deleting}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {deleting ? "Deleting..." : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

export default RedisKeyBrowser;
