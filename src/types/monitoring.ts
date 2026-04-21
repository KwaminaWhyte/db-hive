/**
 * Monitoring Types
 *
 * Types for real-time server-side monitoring: active queries, process list,
 * and aggregate server metrics. Mirrors Rust structs in
 * src-tauri/src/commands/monitoring.rs (serde rename_all = "camelCase").
 */

export interface ActiveQuery {
  pid: number;
  user: string | null;
  database: string | null;
  clientAddr: string | null;
  queryStart: string | null;
  state: string | null;
  queryText: string | null;
  durationMs: number | null;
}

export interface ServerStats {
  numericConnections: number;
  activeConnections: number;
  cacheHitRatio: number | null;
  /**
   * For PostgreSQL this is the cumulative transaction counter
   * (xact_commit + xact_rollback); the frontend diffs successive samples
   * to derive a per-second rate. For MySQL it is the cumulative
   * commit+rollback counter.
   */
  transactionsPerSec: number | null;
  deadlocks: number | null;
}

/**
 * Rolling sample used by the metrics chart.
 */
export interface MetricsSample {
  t: number;
  connections: number;
  active: number;
  txPerSec: number;
}
