/**
 * Query Error Sentinels
 *
 * Typed errors used to distinguish user-initiated cancellations from real
 * query failures so callers can treat them as silent no-ops (no error
 * banner, no history entry).
 */

/**
 * Thrown when the user cancels query execution (e.g. dismissing the
 * destructive-query guard). Catch handlers should detect this with
 * `instanceof QueryCancelledError` and return early without surfacing
 * an error state or recording history.
 */
export class QueryCancelledError extends Error {
  constructor(message = "Query cancelled by user") {
    super(message);
    this.name = "QueryCancelledError";
  }
}
