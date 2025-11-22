/**
 * Visual Query Builder types
 *
 * These types define the structure for building SQL queries visually
 * through a drag-and-drop interface.
 */

import type { DbDriver } from './database';

/**
 * Aggregate functions supported in SELECT clause
 */
export type AggregateFunction =
  | 'COUNT'
  | 'SUM'
  | 'AVG'
  | 'MIN'
  | 'MAX'
  | 'COUNT_DISTINCT';

/**
 * Comparison operators for WHERE conditions
 */
export type ComparisonOperator =
  | '='
  | '!='
  | '>'
  | '<'
  | '>='
  | '<='
  | 'LIKE'
  | 'NOT LIKE'
  | 'IN'
  | 'NOT IN'
  | 'IS NULL'
  | 'IS NOT NULL'
  | 'BETWEEN';

/**
 * Logical operators for combining conditions
 */
export type LogicalOperator = 'AND' | 'OR';

/**
 * JOIN types
 */
export type JoinType = 'INNER' | 'LEFT' | 'RIGHT' | 'FULL OUTER' | 'CROSS';

/**
 * Sort direction
 */
export type SortDirection = 'ASC' | 'DESC';

/**
 * Selected column with optional transformations
 */
export interface SelectedColumn {
  /** Unique ID for this column selection */
  id: string;
  /** Table alias this column belongs to */
  tableAlias: string;
  /** Column name */
  columnName: string;
  /** Optional alias for the column in results */
  alias?: string;
  /** Optional aggregate function */
  aggregate?: AggregateFunction;
  /** Whether to apply DISTINCT */
  distinct?: boolean;
}

/**
 * Table in the query
 */
export interface QueryTable {
  /** Unique alias for this table in the query */
  alias: string;
  /** Schema name */
  schema: string;
  /** Table name */
  tableName: string;
  /** Columns available in this table */
  columns: Array<{
    name: string;
    dataType: string;
  }>;
}

/**
 * JOIN condition between tables
 */
export interface JoinCondition {
  /** Unique ID for this join */
  id: string;
  /** Type of join */
  type: JoinType;
  /** Left table alias */
  leftTable: string;
  /** Left column name */
  leftColumn: string;
  /** Right table alias */
  rightTable: string;
  /** Right column name */
  rightColumn: string;
}

/**
 * Single WHERE condition
 */
export interface WhereCondition {
  /** Unique ID for this condition */
  id: string;
  /** Table alias */
  tableAlias: string;
  /** Column name */
  columnName: string;
  /** Comparison operator */
  operator: ComparisonOperator;
  /** Value to compare against (null for IS NULL/IS NOT NULL) */
  value?: string | number | boolean | null;
  /** Second value for BETWEEN operator */
  value2?: string | number;
  /** Array of values for IN operator */
  values?: Array<string | number>;
}

/**
 * Group of WHERE conditions combined with AND/OR
 */
export interface ConditionGroup {
  /** Unique ID for this group */
  id: string;
  /** Logical operator combining conditions in this group */
  operator: LogicalOperator;
  /** Conditions in this group */
  conditions: WhereCondition[];
  /** Nested condition groups */
  groups?: ConditionGroup[];
}

/**
 * ORDER BY clause
 */
export interface OrderByClause {
  /** Unique ID */
  id: string;
  /** Table alias */
  tableAlias: string;
  /** Column name */
  columnName: string;
  /** Sort direction */
  direction: SortDirection;
}

/**
 * GROUP BY clause
 */
export interface GroupByClause {
  /** Table alias */
  tableAlias: string;
  /** Column name */
  columnName: string;
}

/**
 * HAVING condition (for aggregated results)
 */
export interface HavingCondition {
  /** Unique ID */
  id: string;
  /** Table alias */
  tableAlias: string;
  /** Column name */
  columnName: string;
  /** Aggregate function */
  aggregate: AggregateFunction;
  /** Comparison operator */
  operator: ComparisonOperator;
  /** Value to compare against */
  value: string | number;
}

/**
 * Complete query builder state
 */
export interface QueryBuilderState {
  /** Database driver type (affects SQL syntax) */
  driver: DbDriver;

  /** Tables in the query */
  tables: QueryTable[];

  /** Selected columns */
  columns: SelectedColumn[];

  /** JOIN conditions */
  joins: JoinCondition[];

  /** WHERE conditions */
  where?: ConditionGroup;

  /** GROUP BY clauses */
  groupBy: GroupByClause[];

  /** HAVING conditions (for aggregations) */
  having?: HavingCondition[];

  /** ORDER BY clauses */
  orderBy: OrderByClause[];

  /** LIMIT clause */
  limit?: number;

  /** OFFSET clause */
  offset?: number;

  /** Whether to use DISTINCT on entire result set */
  distinct?: boolean;
}

/**
 * Query builder action types
 */
export type QueryBuilderAction =
  | { type: 'ADD_TABLE'; payload: QueryTable }
  | { type: 'REMOVE_TABLE'; payload: { alias: string } }
  | { type: 'ADD_COLUMN'; payload: SelectedColumn }
  | { type: 'REMOVE_COLUMN'; payload: { id: string } }
  | { type: 'UPDATE_COLUMN'; payload: { id: string; updates: Partial<SelectedColumn> } }
  | { type: 'ADD_JOIN'; payload: JoinCondition }
  | { type: 'REMOVE_JOIN'; payload: { id: string } }
  | { type: 'UPDATE_JOIN'; payload: { id: string; updates: Partial<JoinCondition> } }
  | { type: 'SET_WHERE'; payload: ConditionGroup | undefined }
  | { type: 'ADD_WHERE_CONDITION'; payload: { groupId?: string; condition: WhereCondition } }
  | { type: 'REMOVE_WHERE_CONDITION'; payload: { groupId?: string; conditionId: string } }
  | { type: 'ADD_CONDITION_GROUP'; payload: { parentGroupId?: string; group: ConditionGroup } }
  | { type: 'REMOVE_CONDITION_GROUP'; payload: { groupId: string } }
  | { type: 'ADD_GROUP_BY'; payload: GroupByClause }
  | { type: 'REMOVE_GROUP_BY'; payload: { tableAlias: string; columnName: string } }
  | { type: 'ADD_HAVING'; payload: HavingCondition }
  | { type: 'REMOVE_HAVING'; payload: { id: string } }
  | { type: 'ADD_ORDER_BY'; payload: OrderByClause }
  | { type: 'REMOVE_ORDER_BY'; payload: { id: string } }
  | { type: 'UPDATE_ORDER_BY'; payload: { id: string; direction: SortDirection } }
  | { type: 'SET_LIMIT'; payload: number | undefined }
  | { type: 'SET_OFFSET'; payload: number | undefined }
  | { type: 'SET_DISTINCT'; payload: boolean }
  | { type: 'RESET' };

/**
 * Get initial/empty query builder state
 */
export function getInitialQueryBuilderState(driver: DbDriver): QueryBuilderState {
  return {
    driver,
    tables: [],
    columns: [],
    joins: [],
    groupBy: [],
    orderBy: [],
  };
}
