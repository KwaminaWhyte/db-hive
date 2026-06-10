import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
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
  Trash2,
  Copy,
  Loader2,
  X,
} from "lucide-react";
import { QueryExecutionResult } from "@/types";
import { toast } from "sonner";

interface RedisValuePanelProps {
  connectionId: string;
  redisKey: string;
  onClose: () => void;
}

type RedisType =
  | "string"
  | "hash"
  | "list"
  | "set"
  | "zset"
  | "stream"
  | "none";

type KeyValue =
  | { kind: "string"; value: string }
  | { kind: "hash"; entries: Array<{ field: string; value: string }> }
  | { kind: "list"; items: string[] }
  | { kind: "set"; members: string[] }
  | { kind: "zset"; entries: Array<{ member: string; score: string }> }
  | { kind: "raw"; cells: string[] };

function errMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    const m = (err as { message?: unknown }).message;
    if (typeof m === "string") return m;
  }
  return String(err);
}

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

function typeBadgeClass(type: RedisType): string {
  switch (type) {
    case "string":
      return "text-info bg-info/10";
    case "hash":
      return "text-warning bg-warning/10";
    case "list":
      return "text-success bg-success/10";
    case "set":
      return "text-primary bg-primary/10";
    case "zset":
      return "text-primary bg-primary/10";
    case "stream":
      return "text-info bg-info/10";
    default:
      return "text-muted-foreground bg-muted";
  }
}

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

/**
 * Single Redis key inspector rendered as a main-area tab.
 *
 * Opened from the left schema tree (RedisSchemaTree) when a key is clicked.
 * Loads TYPE + TTL + the type-appropriate value via the Redis driver's
 * raw-command `execute_query` bridge.
 */
export function RedisValuePanel({
  connectionId,
  redisKey,
  onClose,
}: RedisValuePanelProps) {
  const [type, setType] = useState<RedisType | null>(null);
  const [ttl, setTtl] = useState<number | null>(null);
  const [value, setValue] = useState<KeyValue | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [showDelete, setShowDelete] = useState<boolean>(false);
  const [deleting, setDeleting] = useState<boolean>(false);

  const runCommand = useCallback(
    async (sql: string): Promise<QueryExecutionResult> =>
      invoke<QueryExecutionResult>("execute_query", { connectionId, sql }),
    [connectionId]
  );

  const load = useCallback(async () => {
    setLoading(true);
    setType(null);
    setTtl(null);
    setValue(null);
    try {
      const typeRes = await runCommand(`TYPE ${redisKey}`);
      const t = (flattenResult(typeRes)[0] ?? "none") as RedisType;
      setType(t);

      const ttlRes = await runCommand(`TTL ${redisKey}`);
      const ttlRaw = parseInt(flattenResult(ttlRes)[0] ?? "", 10);
      setTtl(Number.isNaN(ttlRaw) ? -2 : ttlRaw);

      let v: KeyValue;
      switch (t) {
        case "string": {
          const r = await runCommand(`GET ${redisKey}`);
          v = { kind: "string", value: flattenResult(r).join("") };
          break;
        }
        case "hash": {
          const r = await runCommand(`HGETALL ${redisKey}`);
          const flat = flattenResult(r);
          const entries: Array<{ field: string; value: string }> = [];
          for (let i = 0; i < flat.length; i += 2) {
            entries.push({ field: flat[i] ?? "", value: flat[i + 1] ?? "" });
          }
          v = { kind: "hash", entries };
          break;
        }
        case "list": {
          const r = await runCommand(`LRANGE ${redisKey} 0 200`);
          v = { kind: "list", items: flattenResult(r) };
          break;
        }
        case "set": {
          const r = await runCommand(`SMEMBERS ${redisKey}`);
          v = { kind: "set", members: flattenResult(r) };
          break;
        }
        case "zset": {
          const r = await runCommand(`ZRANGE ${redisKey} 0 200 WITHSCORES`);
          const flat = flattenResult(r);
          const entries: Array<{ member: string; score: string }> = [];
          for (let i = 0; i < flat.length; i += 2) {
            entries.push({ member: flat[i] ?? "", score: flat[i + 1] ?? "" });
          }
          v = { kind: "zset", entries };
          break;
        }
        default: {
          const r = await runCommand(`GET ${redisKey}`).catch(() => null);
          const cells = r ? flattenResult(r) : [];
          v = { kind: "raw", cells };
          break;
        }
      }
      setValue(v);
    } catch (err) {
      toast.error(`Failed to load key: ${errMessage(err)}`);
      setValue(null);
    } finally {
      setLoading(false);
    }
  }, [runCommand, redisKey]);

  useEffect(() => {
    load();
  }, [load]);

  const copyKeyName = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(redisKey);
      toast.success("Key name copied");
    } catch {
      toast.error("Failed to copy to clipboard");
    }
  }, [redisKey]);

  const confirmDelete = useCallback(async () => {
    setDeleting(true);
    try {
      await runCommand(`DEL ${redisKey}`);
      toast.success(`Deleted key "${redisKey}"`);
      setShowDelete(false);
      onClose();
    } catch (err) {
      toast.error(`Delete failed: ${errMessage(err)}`);
    } finally {
      setDeleting(false);
    }
  }, [runCommand, redisKey, onClose]);

  const ttlLabel = useMemo(() => {
    if (ttl === null) return "";
    if (ttl === -1) return "no expiry";
    if (ttl === -2) return "expired/none";
    return `${ttl}s`;
  }, [ttl]);

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 px-3 py-2 border-b shrink-0">
        <div className="min-w-0 space-y-1">
          <button
            type="button"
            onClick={copyKeyName}
            className="font-mono text-sm truncate hover:text-primary flex items-center gap-1.5 max-w-full"
            title="Copy key name"
          >
            <KeyRound className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <span className="truncate">{redisKey}</span>
            <Copy className="h-3 w-3 shrink-0 opacity-60" />
          </button>
          <div className="flex items-center gap-2">
            {type && (
              <span
                className={`text-[10px] font-bold px-1.5 py-0.5 rounded leading-none ${typeBadgeClass(
                  type
                )}`}
              >
                {type.toUpperCase()}
              </span>
            )}
            {ttl !== null && (
              <span className="text-xs text-muted-foreground">
                TTL: {ttlLabel}
              </span>
            )}
          </div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Button
            variant="ghost"
            size="sm"
            className="h-8"
            onClick={load}
            disabled={loading}
            title="Refresh"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </Button>
          <Button
            variant="destructive"
            size="sm"
            className="h-8"
            onClick={() => setShowDelete(true)}
          >
            <Trash2 className="h-3.5 w-3.5 mr-1" />
            Delete
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-8"
            onClick={onClose}
            title="Close tab"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Value */}
      <div className="flex-1 overflow-hidden">
        {loading ? (
          <div className="h-full flex items-center justify-center">
            <div className="text-center space-y-2">
              <Loader2 className="h-6 w-6 mx-auto animate-spin text-muted-foreground" />
              <p className="text-sm text-muted-foreground">Loading value...</p>
            </div>
          </div>
        ) : !value ? (
          <div className="h-full flex items-center justify-center">
            <p className="text-muted-foreground">No value to display</p>
          </div>
        ) : value.kind === "string" ? (
          <ScrollArea className="h-full">
            <pre className="p-3 text-sm font-mono whitespace-pre-wrap break-all">
              {maybePrettyJson(value.value)}
            </pre>
          </ScrollArea>
        ) : value.kind === "hash" ? (
          <ScrollArea className="h-full">
            <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
              <TableHeader className="sticky top-0 bg-background z-10">
                <TableRow>
                  <TableHead className="whitespace-nowrap">Field</TableHead>
                  <TableHead className="whitespace-nowrap">Value</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {value.entries.map((e, i) => (
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
        ) : value.kind === "list" ? (
          <ScrollArea className="h-full">
            <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
              <TableHeader className="sticky top-0 bg-background z-10">
                <TableRow>
                  <TableHead className="w-16 whitespace-nowrap">Index</TableHead>
                  <TableHead className="whitespace-nowrap">Value</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {value.items.map((item, i) => (
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
        ) : value.kind === "set" ? (
          <ScrollArea className="h-full">
            <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
              <TableHeader className="sticky top-0 bg-background z-10">
                <TableRow>
                  <TableHead className="whitespace-nowrap">Member</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {value.members.map((m, i) => (
                  <TableRow key={i}>
                    <TableCell className="font-mono text-xs whitespace-pre-wrap break-all">
                      {m}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </ScrollArea>
        ) : value.kind === "zset" ? (
          <ScrollArea className="h-full">
            <Table className="[&_th]:border-r [&_th]:border-border [&_td]:border-r [&_td]:border-border">
              <TableHeader className="sticky top-0 bg-background z-10">
                <TableRow>
                  <TableHead className="whitespace-nowrap">Member</TableHead>
                  <TableHead className="w-32 whitespace-nowrap">Score</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {value.entries.map((e, i) => (
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
              {value.cells.join("\n") ||
                "(empty / unsupported type — read-only)"}
            </pre>
          </ScrollArea>
        )}
      </div>

      {/* Delete confirmation */}
      <AlertDialog
        open={showDelete}
        onOpenChange={(open) => {
          if (!open) setShowDelete(false);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete key?</AlertDialogTitle>
            <AlertDialogDescription>
              This action cannot be undone. This will permanently delete the key{" "}
              <span className="font-mono font-semibold">{redisKey}</span> from
              Redis.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>Cancel</AlertDialogCancel>
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

export default RedisValuePanel;
