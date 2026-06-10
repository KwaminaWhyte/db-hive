import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import {
  ChevronRight,
  ChevronDown,
  KeyRound,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { QueryExecutionResult } from "@/types";
import { toast } from "sonner";

/** Redis TYPE name + the label shown in the tree */
const GROUPS: ReadonlyArray<{ type: string; label: string; color: string }> = [
  { type: "string", label: "Strings", color: "text-blue-500" },
  { type: "hash", label: "Hashes", color: "text-orange-500" },
  { type: "list", label: "Lists", color: "text-green-500" },
  { type: "set", label: "Sets", color: "text-purple-500" },
  { type: "zset", label: "Sorted Sets", color: "text-pink-500" },
];

const SCAN_COUNT = 300;

interface RedisSchemaTreeProps {
  connectionId: string;
  /** Search box value from SchemaExplorer; used as a SCAN MATCH pattern */
  searchQuery: string;
  /** Open the clicked key's value in a main-area tab */
  onSelectKey: (key: string) => void;
  /** Currently open key, for row highlighting */
  activeKey?: string | null;
}

interface GroupState {
  expanded: boolean;
  keys: string[];
  cursor: string;
  exhausted: boolean;
  loading: boolean;
  loaded: boolean;
}

function emptyGroup(): GroupState {
  return {
    expanded: false,
    keys: [],
    cursor: "0",
    exhausted: false,
    loading: false,
    loaded: false,
  };
}

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

/** Build a SCAN MATCH pattern from the free-text search box */
function toPattern(q: string): string {
  const t = q.trim();
  if (t === "") return "*";
  // If the user already wrote a glob, respect it; otherwise wrap with *...*
  return /[*?[\]]/.test(t) ? t : `*${t}*`;
}

/**
 * Redis left-sidebar tree. Replaces the relational schema/table tree when the
 * active connection is Redis. Top-level nodes are the five key-type groups;
 * expanding one runs a server-side type-filtered SCAN
 * (`SCAN <cursor> MATCH <pattern> COUNT n TYPE <t>`) so no per-key TYPE calls
 * are needed. Clicking a key opens its value in a main-area tab.
 */
export function RedisSchemaTree({
  connectionId,
  searchQuery,
  onSelectKey,
  activeKey,
}: RedisSchemaTreeProps) {
  const [groups, setGroups] = useState<Record<string, GroupState>>(() => {
    const init: Record<string, GroupState> = {};
    for (const g of GROUPS) init[g.type] = emptyGroup();
    return init;
  });
  // Track which groups are currently expanded so a search reset can reload them.
  const expandedRef = useRef<Set<string>>(new Set());

  const runCommand = useCallback(
    async (sql: string): Promise<QueryExecutionResult> =>
      invoke<QueryExecutionResult>("execute_query", { connectionId, sql }),
    [connectionId]
  );

  const scanGroup = useCallback(
    async (type: string, reset: boolean) => {
      setGroups((prev) => ({
        ...prev,
        [type]: { ...prev[type], loading: true },
      }));
      try {
        const pattern = toPattern(searchQuery);
        const startCursor = reset ? "0" : groups[type]?.cursor ?? "0";
        const res = await runCommand(
          `SCAN ${startCursor} MATCH ${pattern} COUNT ${SCAN_COUNT} TYPE ${type}`
        );
        const cells = flattenResult(res);
        const newCursor = cells[0] ?? "0";
        const found = cells.slice(1);
        setGroups((prev) => {
          const base = reset ? [] : prev[type].keys;
          const seen = new Set<string>();
          const unique: string[] = [];
          for (const k of [...base, ...found]) {
            if (!seen.has(k)) {
              seen.add(k);
              unique.push(k);
            }
          }
          return {
            ...prev,
            [type]: {
              ...prev[type],
              keys: unique,
              cursor: newCursor,
              exhausted: newCursor === "0",
              loading: false,
              loaded: true,
            },
          };
        });
      } catch (err) {
        toast.error(`Scan failed: ${errMessage(err)}`);
        setGroups((prev) => ({
          ...prev,
          [type]: { ...prev[type], loading: false, loaded: true },
        }));
      }
    },
    [runCommand, searchQuery, groups]
  );

  const toggleGroup = useCallback(
    (type: string) => {
      setGroups((prev) => {
        const wasExpanded = prev[type].expanded;
        if (wasExpanded) {
          expandedRef.current.delete(type);
        } else {
          expandedRef.current.add(type);
        }
        return {
          ...prev,
          [type]: { ...prev[type], expanded: !wasExpanded },
        };
      });
      // Lazy-load on first expand
      const g = groups[type];
      if (!g.expanded && !g.loaded && !g.loading) {
        scanGroup(type, true);
      }
    },
    [groups, scanGroup]
  );

  // When the search pattern changes, reload any already-expanded groups and
  // drop cached keys for collapsed ones.
  useEffect(() => {
    setGroups((prev) => {
      const next: Record<string, GroupState> = {};
      for (const g of GROUPS) {
        const wasExpanded = prev[g.type].expanded;
        next[g.type] = { ...emptyGroup(), expanded: wasExpanded };
      }
      return next;
    });
    for (const type of expandedRef.current) {
      scanGroup(type, true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchQuery, connectionId]);

  const handleRefresh = useCallback(() => {
    for (const type of expandedRef.current) {
      scanGroup(type, true);
    }
  }, [scanGroup]);

  return (
    <div className="h-full flex flex-col min-h-0">
      <div className="flex items-center justify-between px-3 py-1.5 border-b bg-muted/30 shrink-0">
        <span className="text-xs font-medium text-muted-foreground">
          Redis Keys
        </span>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          onClick={handleRefresh}
          title="Refresh expanded groups"
          aria-label="Refresh expanded groups"
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto">
        <div className="py-1">
          {GROUPS.map((g) => {
            const state = groups[g.type];
            return (
              <div key={g.type}>
                {/* Group row */}
                <div
                  onClick={() => toggleGroup(g.type)}
                  className="flex items-center gap-1.5 px-2 py-1.5 cursor-pointer hover:bg-muted/50 text-sm select-none"
                >
                  {state.expanded ? (
                    <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  )}
                  <KeyRound className={`h-3.5 w-3.5 shrink-0 ${g.color}`} />
                  <span className="font-medium flex-1">{g.label}</span>
                  {state.loaded && (
                    <span className="text-[10px] text-muted-foreground tabular-nums">
                      {state.keys.length}
                      {!state.exhausted ? "+" : ""}
                    </span>
                  )}
                  {state.loading && (
                    <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
                  )}
                </div>

                {/* Keys */}
                {state.expanded && (
                  <div>
                    {state.loaded && state.keys.length === 0 && !state.loading ? (
                      <div className="pl-9 pr-2 py-1.5 text-xs text-muted-foreground">
                        No keys
                      </div>
                    ) : (
                      state.keys.map((key) => {
                        const isActive = key === activeKey;
                        return (
                          <div
                            key={key}
                            onClick={() => onSelectKey(key)}
                            className={`flex items-center gap-1.5 pl-9 pr-2 py-1 cursor-pointer text-sm border-l-2 ${
                              isActive
                                ? "bg-muted border-l-primary"
                                : "border-l-transparent hover:bg-muted/50"
                            }`}
                            title={key}
                          >
                            <span className="font-mono text-xs truncate">
                              {key}
                            </span>
                          </div>
                        );
                      })
                    )}
                    {state.expanded && !state.exhausted && state.loaded && (
                      <button
                        type="button"
                        onClick={() => scanGroup(g.type, false)}
                        disabled={state.loading}
                        className="w-full pl-9 pr-2 py-1.5 text-left text-xs text-primary hover:underline disabled:opacity-50"
                      >
                        {state.loading ? "Loading…" : "Load more…"}
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export default RedisSchemaTree;
