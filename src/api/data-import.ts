/**
 * Data Import API
 * Functions for importing data from CSV and Excel files
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  ImportPreview,
  DataImportOptions,
  ImportResult,
  TableColumn,
} from "../types/data-import";

/**
 * Preview a file for import (CSV or Excel)
 */
export async function previewImportFile(
  filePath: string,
  previewRows: number = 100,
  sheetName?: string
): Promise<ImportPreview> {
  return invoke<ImportPreview>("preview_import_file", {
    filePath,
    previewRows,
    sheetName: sheetName ?? null,
  });
}

/**
 * Import data from a file into a database table
 */
export async function importDataToTable(
  connectionId: string,
  filePath: string,
  options: DataImportOptions
): Promise<ImportResult> {
  return invoke<ImportResult>("import_data_to_table", {
    connectionId,
    filePath,
    options,
  });
}

/**
 * Get list of tables available for import
 */
export async function getTablesForImport(
  connectionId: string,
  schema?: string
): Promise<string[]> {
  return invoke<string[]>("get_tables_for_import", {
    connectionId,
    schema: schema ?? null,
  });
}

/**
 * Get columns for a specific table
 */
export async function getTableColumnsForImport(
  connectionId: string,
  tableName: string,
  schema?: string
): Promise<TableColumn[]> {
  return invoke<TableColumn[]>("get_table_columns_for_import", {
    connectionId,
    tableName,
    schema: schema ?? null,
  });
}
