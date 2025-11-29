/**
 * Query Template Types
 *
 * Query templates are SQL queries with parameter placeholders
 * that can be filled in at execution time.
 *
 * Parameter syntax: {{parameter_name}} or {{parameter_name:type}}
 * Example: SELECT * FROM users WHERE id = {{user_id:integer}}
 */

/**
 * Parameter types supported in query templates
 */
export type TemplateParameterType =
  | "string"
  | "integer"
  | "decimal"
  | "boolean"
  | "date"
  | "datetime"
  | "text";

/**
 * A template parameter definition
 */
export interface TemplateParameter {
  /** Parameter name (as it appears in the template) */
  name: string;

  /** Display label for the parameter */
  label: string;

  /** Parameter type for validation */
  type: TemplateParameterType;

  /** Default value */
  defaultValue?: string;

  /** Whether the parameter is required */
  required: boolean;

  /** Description/hint for the user */
  description?: string;

  /** Validation pattern (regex) */
  validationPattern?: string;

  /** Options for select-type parameters */
  options?: string[];
}

/**
 * A query template with parameters
 */
export interface QueryTemplate {
  /** Unique identifier (UUID) */
  id: string;

  /** Template name */
  name: string;

  /** Template description */
  description?: string;

  /** SQL query with parameter placeholders */
  sql: string;

  /** Parameter definitions */
  parameters: TemplateParameter[];

  /** Category/folder for organization */
  category?: string;

  /** Tags for searching */
  tags?: string[];

  /** ISO 8601 timestamp of creation */
  createdAt: string;

  /** ISO 8601 timestamp of last update */
  updatedAt: string;
}

/**
 * Parameter values filled in by the user
 */
export interface TemplateParameterValues {
  [parameterName: string]: string;
}

/**
 * Extract parameter names from a template SQL string
 * Supports {{param}} and {{param:type}} syntax
 */
export function extractParameters(sql: string): string[] {
  const regex = /\{\{([a-zA-Z_][a-zA-Z0-9_]*)(?::([a-zA-Z]+))?\}\}/g;
  const parameters: string[] = [];
  let match;

  while ((match = regex.exec(sql)) !== null) {
    if (!parameters.includes(match[1])) {
      parameters.push(match[1]);
    }
  }

  return parameters;
}

/**
 * Extract parameter definitions with types from a template SQL string
 */
export function extractParameterDefinitions(
  sql: string
): { name: string; type: TemplateParameterType }[] {
  const regex = /\{\{([a-zA-Z_][a-zA-Z0-9_]*)(?::([a-zA-Z]+))?\}\}/g;
  const parameters: { name: string; type: TemplateParameterType }[] = [];
  const seen = new Set<string>();
  let match;

  while ((match = regex.exec(sql)) !== null) {
    const name = match[1];
    if (!seen.has(name)) {
      seen.add(name);
      const type = (match[2] as TemplateParameterType) || "string";
      parameters.push({ name, type });
    }
  }

  return parameters;
}

/**
 * Substitute parameter values into a template SQL string
 */
export function substituteParameters(
  sql: string,
  values: TemplateParameterValues
): string {
  return sql.replace(
    /\{\{([a-zA-Z_][a-zA-Z0-9_]*)(?::([a-zA-Z]+))?\}\}/g,
    (match, paramName) => {
      if (paramName in values) {
        return values[paramName];
      }
      return match; // Leave unsubstituted if no value
    }
  );
}

/**
 * Validate a parameter value against its type
 */
export function validateParameterValue(
  value: string,
  type: TemplateParameterType
): { valid: boolean; error?: string } {
  if (!value) {
    return { valid: true }; // Empty values handled by required check
  }

  switch (type) {
    case "integer":
      if (!/^-?\d+$/.test(value)) {
        return { valid: false, error: "Must be a valid integer" };
      }
      break;

    case "decimal":
      if (!/^-?\d+\.?\d*$/.test(value)) {
        return { valid: false, error: "Must be a valid decimal number" };
      }
      break;

    case "boolean":
      if (!["true", "false", "1", "0", "yes", "no"].includes(value.toLowerCase())) {
        return { valid: false, error: "Must be true/false, yes/no, or 1/0" };
      }
      break;

    case "date":
      if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
        return { valid: false, error: "Must be in YYYY-MM-DD format" };
      }
      break;

    case "datetime":
      if (!/^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2})?/.test(value)) {
        return {
          valid: false,
          error: "Must be in YYYY-MM-DD HH:MM:SS format",
        };
      }
      break;
  }

  return { valid: true };
}

/**
 * Format a parameter value for SQL based on its type
 * Adds appropriate quoting and escaping
 */
export function formatParameterForSql(
  value: string,
  type: TemplateParameterType
): string {
  switch (type) {
    case "string":
    case "text":
    case "date":
    case "datetime":
      // Escape single quotes and wrap in quotes
      return `'${value.replace(/'/g, "''")}'`;

    case "integer":
    case "decimal":
      return value;

    case "boolean":
      const lower = value.toLowerCase();
      if (["true", "1", "yes"].includes(lower)) {
        return "true";
      }
      return "false";

    default:
      return `'${value.replace(/'/g, "''")}'`;
  }
}
