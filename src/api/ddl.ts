/**
 * DDL (Data Definition Language) API
 *
 * Functions for creating, altering, and dropping database objects.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  TableDefinition,
  AlterTableDefinition,
  DropTableDefinition,
  DdlResult,
} from "@/types/ddl";

/**
 * Preview CREATE TABLE SQL without executing it
 *
 * @param connectionId - ID of the active connection
 * @param table - Table definition with columns, constraints, etc.
 * @returns Generated SQL and success message
 */
export async function previewCreateTable(
  connectionId: string,
  table: TableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("preview_create_table", {
    connectionId,
    table,
  });
}

/**
 * Create a new table
 *
 * Generates and executes SQL statement(s) to create a new table.
 *
 * @param connectionId - ID of the active connection
 * @param table - Table definition with columns, constraints, etc.
 * @returns Executed SQL and success message
 */
export async function createTable(
  connectionId: string,
  table: TableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("create_table", {
    connectionId,
    table,
  });
}

/**
 * Preview ALTER TABLE SQL without executing it
 *
 * @param connectionId - ID of the active connection
 * @param alter - Table alteration definition with operations
 * @returns Generated SQL and success message
 */
export async function previewAlterTable(
  connectionId: string,
  alter: AlterTableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("preview_alter_table", {
    connectionId,
    alter,
  });
}

/**
 * Alter an existing table
 *
 * Generates and executes SQL statement(s) to modify an existing table structure.
 *
 * @param connectionId - ID of the active connection
 * @param alter - Table alteration definition with operations
 * @returns Executed SQL and success message
 */
export async function alterTable(
  connectionId: string,
  alter: AlterTableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("alter_table", {
    connectionId,
    alter,
  });
}

/**
 * Preview DROP TABLE SQL without executing it
 *
 * @param connectionId - ID of the active connection
 * @param drop - Table drop definition
 * @returns Generated SQL and success message
 */
export async function previewDropTable(
  connectionId: string,
  drop: DropTableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("preview_drop_table", {
    connectionId,
    drop,
  });
}

/**
 * Drop a table
 *
 * Generates and executes SQL statement to drop a table from the database.
 *
 * @param connectionId - ID of the active connection
 * @param drop - Table drop definition
 * @returns Executed SQL and success message
 */
export async function dropTable(
  connectionId: string,
  drop: DropTableDefinition
): Promise<DdlResult> {
  return invoke<DdlResult>("drop_table", {
    connectionId,
    drop,
  });
}
