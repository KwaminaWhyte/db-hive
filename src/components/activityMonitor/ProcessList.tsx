/**
 * ProcessList Component
 *
 * Polls `get_active_queries` every 2 seconds while mounted and renders a table
 * of active database sessions. Supports cancelling/killing a session via
 * `kill_query`. Renders a graceful fallback message when the driver does not
 * support session introspection.
 */

import { FC, useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, XCircle } from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ActiveQuery } from "@/types/monitoring";

interface ProcessListProps {
  connectionId?: string;
  /** Poll interval in milliseconds. Defaults to 2000. */
  pollInterval?: number;
}

interface DbErrorShape {
  kind?: string;
  message?: string;
}

export const ProcessList: FC<ProcessListProps> = ({
  connectionId,
  pollInterval = 2000,
}) => {
  const [rows, setRows] = useState<ActiveQuery[]>([]);
  const [unsupported, setUnsupported] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [killing, setKilling] = useState<Set<number>>(new Set());
  const mountedRef = useRef(true);

  const fetchActive = useCallback(async () => {
    if (!connectionId) return;
    setLoading(true);
    try {
      const data = await invoke<ActiveQuery[]>("get_active_queries", {
        connectionId,
      });
      if (!mountedRef.current) return;
      setRows(data);
      setUnsupported(null);
      setError(null);
    } catch (e) {
      const err = e as DbErrorShape;
      if (err?.kind === "invalid_input") {
        setUnsupported(err.message ?? "Not supported for this driver");
        setRows([]);
        setError(null);
      } else {
        setError(err?.message ?? String(e));
      }
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, [connectionId]);

  useEffect(() => {
    mountedRef.current = true;
    fetchActive();
    if (!connectionId || pollInterval <= 0) return;
    const id = window.setInterval(fetchActive, pollInterval);
    return () => {
      mountedRef.current = false;
      window.clearInterval(id);
    };
  }, [fetchActive, connectionId, pollInterval]);

  const handleKill = useCallback(
    async (pid: number) => {
      if (!connectionId) return;
      setKilling((prev) => {
        const next = new Set(prev);
        next.add(pid);
        return next;
      });
      try {
        await invoke("kill_query", { connectionId, pid });
        await fetchActive();
      } catch (e) {
        const err = e as DbErrorShape;
        setError(err?.message ?? String(e));
      } finally {
        setKilling((prev) => {
          const next = new Set(prev);
          next.delete(pid);
          return next;
        });
      }
    },
    [connectionId, fetchActive]
  );

  if (!connectionId) {
    return (
      <div className="text-sm text-muted-foreground p-6 text-center">
        Select a connection to view active queries.
      </div>
    );
  }

  if (unsupported) {
    return (
      <div className="text-sm text-muted-foreground p-6 text-center">
        {unsupported}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="text-sm text-muted-foreground">
          {rows.length} active session{rows.length === 1 ? "" : "s"}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={fetchActive}
          disabled={loading}
          className="gap-2 h-7"
        >
          <RefreshCw className={`h-3 w-3 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </Button>
      </div>

      {error && (
        <div className="text-xs text-destructive border border-destructive/30 rounded px-3 py-2">
          {error}
        </div>
      )}

      {rows.length === 0 ? (
        <div className="text-sm text-muted-foreground text-center py-8">
          No active queries
        </div>
      ) : (
        <div className="border rounded-md overflow-auto max-h-[420px]">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[80px]">PID</TableHead>
                <TableHead>User</TableHead>
                <TableHead>DB</TableHead>
                <TableHead>Client</TableHead>
                <TableHead>State</TableHead>
                <TableHead className="w-[100px]">Duration</TableHead>
                <TableHead>Query</TableHead>
                <TableHead className="w-[60px]"></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow key={r.pid}>
                  <TableCell className="font-mono text-xs">{r.pid}</TableCell>
                  <TableCell className="text-xs">{r.user ?? "-"}</TableCell>
                  <TableCell className="text-xs">{r.database ?? "-"}</TableCell>
                  <TableCell className="text-xs">
                    {r.clientAddr ?? "-"}
                  </TableCell>
                  <TableCell>
                    <Badge variant="secondary" className="text-[10px]">
                      {r.state ?? "unknown"}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {r.durationMs != null ? `${r.durationMs}ms` : "-"}
                  </TableCell>
                  <TableCell
                    className="font-mono text-xs max-w-[360px] truncate"
                    title={r.queryText ?? ""}
                  >
                    {r.queryText ?? "-"}
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="h-7 w-7"
                      disabled={killing.has(r.pid)}
                      onClick={() => handleKill(r.pid)}
                      title="Cancel query"
                    >
                      <XCircle className="h-4 w-4 text-destructive" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
};
