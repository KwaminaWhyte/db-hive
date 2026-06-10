/**
 * DbError Formatting
 *
 * The Rust backend serializes `DbError` over IPC as `{ kind, message }`
 * (see `src-tauri/src/models/error.rs`), where `message` is the Display
 * output — a human prefix plus the raw driver error, e.g.
 * `"Connection failed: connection refused (os error 61)"`.
 *
 * Tauri `invoke()` rejections are NOT guaranteed to be that shape: commands
 * returning `Result<T, String>` reject with a plain string, and frontend
 * code can also throw regular `Error` objects. `formatDbError` accepts all
 * of these and produces a human headline plus the raw message as `detail`
 * so callers can show "friendly title + technical description" (UX-08).
 */

/** Error kinds emitted by `DbError::serialize` in `models/error.rs`. */
export type DbErrorKind =
  | "connection"
  | "query"
  | "auth"
  | "timeout"
  | "invalid_input"
  | "not_found"
  | "internal"
  | "credential"
  | "import"
  | "ai";

export interface FormattedDbError {
  /** Short, human-friendly summary suitable for a toast title or heading. */
  headline: string;
  /**
   * The raw backend/driver message, preserved for diagnostics. Omitted when
   * it would be identical to `headline` (nothing extra to show).
   */
  detail?: string;
  /** The structured `DbError.kind` when the error had one. */
  kind?: DbErrorKind | string;
}

/** Fixed headlines per structured error kind. */
const KIND_HEADLINES: Record<string, string> = {
  connection: "Could not connect to the database",
  auth: "Authentication failed — check username and password",
  timeout: "The operation timed out",
  not_found: "The requested resource was not found",
  internal: "An internal error occurred",
  credential: "Could not access saved credentials",
  import: "Import failed",
  ai: "AI request failed",
};

/**
 * Display prefixes added by `DbError`'s `thiserror` attributes. Stripped
 * when the message itself is used as the headline (query / invalid_input),
 * since the underlying text is the user-actionable part.
 */
const MESSAGE_PREFIXES = [
  "Connection failed: ",
  "Query execution failed: ",
  "Authentication failed: ",
  "Operation timed out: ",
  "Invalid input: ",
  "Not found: ",
  "Internal error: ",
  "Credential error: ",
  "Import error: ",
  "AI error: ",
];

function stripKnownPrefix(message: string): string {
  for (const prefix of MESSAGE_PREFIXES) {
    if (message.startsWith(prefix)) {
      return message.slice(prefix.length);
    }
  }
  return message;
}

function isStructuredDbError(
  err: unknown
): err is { kind: string; message: string } {
  return (
    typeof err === "object" &&
    err !== null &&
    typeof (err as { kind?: unknown }).kind === "string" &&
    typeof (err as { message?: unknown }).message === "string"
  );
}

/** Attach `detail` only when it adds information beyond the headline. */
function withDetail(
  headline: string,
  raw: string,
  kind?: string
): FormattedDbError {
  const detail = raw && raw !== headline ? raw : undefined;
  return kind ? { headline, detail, kind } : { headline, detail };
}

/**
 * Convert any error thrown by `invoke()` (structured `DbError`, plain
 * string, or `Error`) into a human headline + raw detail.
 */
export function formatDbError(err: unknown): FormattedDbError {
  // Structured DbError from the Rust backend: { kind, message }
  if (isStructuredDbError(err)) {
    const { kind, message } = err;

    // Query and validation errors carry user-actionable text — surface it.
    if (kind === "query" || kind === "invalid_input") {
      return withDetail(stripKnownPrefix(message), message, kind);
    }

    const headline = KIND_HEADLINES[kind];
    if (headline) {
      return withDetail(headline, message, kind);
    }

    // Unknown kind (future backend additions) — degrade to the message.
    return withDetail(stripKnownPrefix(message), message, kind);
  }

  // Plain string rejection (commands with `Result<T, String>` errors)
  if (typeof err === "string") {
    const trimmed = err.trim();
    if (trimmed) {
      return withDetail(stripKnownPrefix(trimmed), trimmed);
    }
    return { headline: "Something went wrong" };
  }

  // Regular Error object thrown in frontend code
  if (err instanceof Error) {
    const message = err.message.trim();
    if (message) {
      return withDetail(stripKnownPrefix(message), message);
    }
    return { headline: "Something went wrong" };
  }

  // Anything else (null, undefined, odd objects)
  const fallback = err == null ? "" : String(err);
  if (fallback && fallback !== "[object Object]") {
    return withDetail(stripKnownPrefix(fallback), fallback);
  }
  return { headline: "Something went wrong" };
}
