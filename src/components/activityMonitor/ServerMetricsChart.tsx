/**
 * ServerMetricsChart Component
 *
 * Polls `get_server_stats` every 2 seconds while mounted and renders a
 * rolling 60-sample line chart of connections and transactions/second.
 * Transactions/sec is computed by diffing the cumulative counter between
 * successive samples, which is why the first sample is skipped.
 */

import { FC, useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { MetricsSample, ServerStats } from "@/types/monitoring";

interface ServerMetricsChartProps {
  connectionId?: string;
  pollInterval?: number;
  maxSamples?: number;
}

interface DbErrorShape {
  kind?: string;
  message?: string;
}

export const ServerMetricsChart: FC<ServerMetricsChartProps> = ({
  connectionId,
  pollInterval = 2000,
  maxSamples = 60,
}) => {
  const [samples, setSamples] = useState<MetricsSample[]>([]);
  const [latest, setLatest] = useState<ServerStats | null>(null);
  const [unsupported, setUnsupported] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const prevCounterRef = useRef<{ t: number; xact: number } | null>(null);
  const mountedRef = useRef(true);

  const fetchStats = useCallback(async () => {
    if (!connectionId) return;
    try {
      const stats = await invoke<ServerStats>("get_server_stats", {
        connectionId,
      });
      if (!mountedRef.current) return;
      setLatest(stats);
      setUnsupported(null);
      setError(null);

      const now = Date.now();
      const xactCounter = stats.transactionsPerSec ?? 0;
      let rate = 0;
      const prev = prevCounterRef.current;
      if (prev) {
        const dt = (now - prev.t) / 1000;
        if (dt > 0) {
          const delta = xactCounter - prev.xact;
          rate = delta > 0 ? delta / dt : 0;
        }
      }
      prevCounterRef.current = { t: now, xact: xactCounter };

      setSamples((prev) => {
        const next: MetricsSample = {
          t: now,
          connections: stats.numericConnections,
          active: stats.activeConnections,
          txPerSec: Number(rate.toFixed(2)),
        };
        const out = [...prev, next];
        if (out.length > maxSamples) out.splice(0, out.length - maxSamples);
        return out;
      });
    } catch (e) {
      const err = e as DbErrorShape;
      if (err?.kind === "invalid_input") {
        setUnsupported(err.message ?? "Not supported for this driver");
      } else {
        setError(err?.message ?? String(e));
      }
    }
  }, [connectionId, maxSamples]);

  useEffect(() => {
    mountedRef.current = true;
    // Reset per-connection state so switching connections doesn't show stale rates.
    setSamples([]);
    prevCounterRef.current = null;
    fetchStats();
    if (!connectionId || pollInterval <= 0) return;
    const id = window.setInterval(fetchStats, pollInterval);
    return () => {
      mountedRef.current = false;
      window.clearInterval(id);
    };
  }, [fetchStats, connectionId, pollInterval]);

  if (!connectionId) {
    return (
      <div className="text-sm text-muted-foreground p-6 text-center">
        Select a connection to view server metrics.
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

  const formatTime = (t: number) => {
    const d = new Date(t);
    return `${d.getHours().toString().padStart(2, "0")}:${d
      .getMinutes()
      .toString()
      .padStart(2, "0")}:${d.getSeconds().toString().padStart(2, "0")}`;
  };

  return (
    <div className="space-y-3">
      {latest && (
        <div className="grid grid-cols-4 gap-2">
          <StatTile label="Connections" value={latest.numericConnections} />
          <StatTile label="Active" value={latest.activeConnections} />
          <StatTile
            label="Cache hit"
            value={
              latest.cacheHitRatio != null
                ? `${(latest.cacheHitRatio * 100).toFixed(1)}%`
                : "-"
            }
          />
          <StatTile label="Deadlocks" value={latest.deadlocks ?? "-"} />
        </div>
      )}

      {error && (
        <div className="text-xs text-destructive border border-destructive/30 rounded px-3 py-2">
          {error}
        </div>
      )}

      <div className="h-[220px] border rounded-md p-2">
        {samples.length < 2 ? (
          <div className="h-full flex items-center justify-center text-xs text-muted-foreground">
            Collecting samples...
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={samples}>
              <CartesianGrid strokeDasharray="3 3" opacity={0.25} />
              <XAxis
                dataKey="t"
                tickFormatter={formatTime}
                tick={{ fontSize: 10 }}
                minTickGap={40}
              />
              <YAxis yAxisId="left" tick={{ fontSize: 10 }} />
              <YAxis
                yAxisId="right"
                orientation="right"
                tick={{ fontSize: 10 }}
              />
              <Tooltip
                labelFormatter={(v) => formatTime(v as number)}
                contentStyle={{ fontSize: "11px" }}
              />
              <Legend wrapperStyle={{ fontSize: "11px" }} />
              <Line
                yAxisId="left"
                type="monotone"
                dataKey="connections"
                stroke="#3b82f6"
                dot={false}
                name="Connections"
                isAnimationActive={false}
              />
              <Line
                yAxisId="left"
                type="monotone"
                dataKey="active"
                stroke="#10b981"
                dot={false}
                name="Active"
                isAnimationActive={false}
              />
              <Line
                yAxisId="right"
                type="monotone"
                dataKey="txPerSec"
                stroke="#f59e0b"
                dot={false}
                name="Tx/sec"
                isAnimationActive={false}
              />
            </LineChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  );
};

const StatTile: FC<{ label: string; value: number | string }> = ({
  label,
  value,
}) => (
  <div className="border rounded-md p-2">
    <div className="text-lg font-semibold leading-none">{value}</div>
    <div className="text-[10px] text-muted-foreground mt-1">{label}</div>
  </div>
);
