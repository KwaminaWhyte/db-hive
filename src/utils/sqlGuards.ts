/**
 * SQL write-query guards
 *
 * Pure analysis helpers that detect potentially destructive SQL statements
 * so the UI can prompt the user for confirmation before execution.
 */

export type DestructiveWarningKind =
  | "delete-no-where"
  | "update-no-where"
  | "drop-table"
  | "truncate"
  | "drop-column"
  | "drop-database"
  | "drop-schema";

export interface DestructiveWarning {
  kind: DestructiveWarningKind;
  /** The (normalized) offending statement. */
  statement: string;
  /** Short preview of the original statement, truncated for display. */
  preview: string;
}

/**
 * Strip SQL comments and string/identifier literals so keywords embedded in
 * them don't trigger the guard. Replacement preserves length roughly by
 * substituting spaces, keeping regex anchoring semantics intact.
 */
function stripCommentsAndLiterals(sql: string): string {
  let out = "";
  let i = 0;
  const n = sql.length;

  while (i < n) {
    const ch = sql[i];
    const next = sql[i + 1];

    // Line comment: -- ... \n
    if (ch === "-" && next === "-") {
      while (i < n && sql[i] !== "\n") {
        out += " ";
        i++;
      }
      continue;
    }

    // Block comment: /* ... */
    if (ch === "/" && next === "*") {
      out += "  ";
      i += 2;
      while (i < n && !(sql[i] === "*" && sql[i + 1] === "/")) {
        out += sql[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < n) {
        out += "  ";
        i += 2;
      }
      continue;
    }

    // Single-quoted string
    if (ch === "'") {
      out += " ";
      i++;
      while (i < n) {
        if (sql[i] === "'" && sql[i + 1] === "'") {
          out += "  ";
          i += 2;
          continue;
        }
        if (sql[i] === "'") {
          out += " ";
          i++;
          break;
        }
        out += sql[i] === "\n" ? "\n" : " ";
        i++;
      }
      continue;
    }

    // Double-quoted identifier/string
    if (ch === '"') {
      out += " ";
      i++;
      while (i < n) {
        if (sql[i] === '"' && sql[i + 1] === '"') {
          out += "  ";
          i += 2;
          continue;
        }
        if (sql[i] === '"') {
          out += " ";
          i++;
          break;
        }
        out += sql[i] === "\n" ? "\n" : " ";
        i++;
      }
      continue;
    }

    out += ch;
    i++;
  }

  return out;
}

function preview(stmt: string, max = 200): string {
  const s = stmt.replace(/\s+/g, " ").trim();
  return s.length > max ? s.slice(0, max - 1) + "…" : s;
}

/**
 * Analyze a SQL string and return a list of destructive-operation warnings.
 * Empty array means the query is safe to run without confirmation.
 */
export function analyzeDestructive(sql: string): DestructiveWarning[] {
  if (!sql || !sql.trim()) return [];

  const stripped = stripCommentsAndLiterals(sql);

  // Naive split on semicolons. Since strings/comments are stripped, any ';'
  // left is a genuine statement terminator.
  const rawStatements = stripped.split(";");

  const warnings: DestructiveWarning[] = [];

  for (const raw of rawStatements) {
    const stmt = raw.replace(/\s+/g, " ").trim();
    if (!stmt) continue;

    const hasWhere = /\bWHERE\b/i.test(stmt);

    if (/^\s*DELETE\s+FROM\b/i.test(stmt) && !hasWhere) {
      warnings.push({ kind: "delete-no-where", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*UPDATE\b/i.test(stmt) && !hasWhere) {
      warnings.push({ kind: "update-no-where", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*DROP\s+TABLE\b/i.test(stmt)) {
      warnings.push({ kind: "drop-table", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*TRUNCATE\b/i.test(stmt)) {
      warnings.push({ kind: "truncate", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*ALTER\s+TABLE\b.*\bDROP\s+COLUMN\b/i.test(stmt)) {
      warnings.push({ kind: "drop-column", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*DROP\s+DATABASE\b/i.test(stmt)) {
      warnings.push({ kind: "drop-database", statement: stmt, preview: preview(stmt) });
      continue;
    }

    if (/^\s*DROP\s+SCHEMA\b/i.test(stmt)) {
      warnings.push({ kind: "drop-schema", statement: stmt, preview: preview(stmt) });
      continue;
    }
  }

  return warnings;
}

export function describeWarning(kind: DestructiveWarningKind): string {
  switch (kind) {
    case "delete-no-where":
      return "DELETE without a WHERE clause will remove ALL rows from the table.";
    case "update-no-where":
      return "UPDATE without a WHERE clause will modify ALL rows in the table.";
    case "drop-table":
      return "DROP TABLE will permanently remove the table and all of its data.";
    case "truncate":
      return "TRUNCATE will permanently remove all rows from the table.";
    case "drop-column":
      return "ALTER TABLE DROP COLUMN will permanently remove the column and its data.";
    case "drop-database":
      return "DROP DATABASE will permanently remove the database and all of its contents.";
    case "drop-schema":
      return "DROP SCHEMA will permanently remove the schema and all objects within it.";
  }
}
