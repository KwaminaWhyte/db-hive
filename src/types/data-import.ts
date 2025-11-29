/**
 * Data Import Types
 * Types for importing data from CSV and Excel files
 */

export interface ImportPreview {
  columns: string[];
  rows: string[][];
  total_rows: number | null;
  detected_types: string[];
  file_type: string;
  sheet_names: string[] | null;
}

export interface ColumnMapping {
  source_column: string;
  target_column: string;
  target_type: string | null;
  default_value: string | null;
  skip: boolean;
}

export interface DataImportOptions {
  table_name: string;
  schema: string | null;
  column_mappings: ColumnMapping[];
  skip_rows: number;
  create_table: boolean;
  truncate_before: boolean;
  batch_size: number;
  delimiter: string | null;
  sheet_name: string | null;
  first_row_is_header: boolean;
}

export interface ImportResult {
  rows_imported: number;
  rows_failed: number;
  errors: string[];
  success: boolean;
}

export interface TableColumn {
  name: string;
  data_type: string;
  is_nullable: boolean;
  is_primary_key: boolean;
  default_value: string | null;
}

export type ImportStep = "select-file" | "preview" | "mapping" | "options" | "importing" | "complete";
