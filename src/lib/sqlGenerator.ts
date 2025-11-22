/**
 * SQL Generation from Query Builder State
 *
 * Converts visual query builder state into executable SQL queries
 * with database-specific syntax handling.
 */

import type {
  QueryBuilderState,
  SelectedColumn,
  JoinCondition,
  ConditionGroup,
  WhereCondition,
  HavingCondition,
} from '../types/queryBuilder';
import type { DbDriver } from '../types/database';

/**
 * Quote identifier based on database driver
 */
function quoteIdentifier(identifier: string, driver: DbDriver): string {
  switch (driver) {
    case 'MySql':
      return `\`${identifier}\``;
    case 'Postgres':
    case 'Sqlite':
    case 'SqlServer':
      return `"${identifier}"`;
    case 'MongoDb':
      // MongoDB doesn't use SQL quoting
      return identifier;
    default:
      return `"${identifier}"`;
  }
}

/**
 * Format value for SQL based on type
 */
function formatValue(value: string | number | boolean | null | undefined): string {
  if (value === null || value === undefined) {
    return 'NULL';
  }

  if (typeof value === 'boolean') {
    return value ? 'TRUE' : 'FALSE';
  }

  if (typeof value === 'number') {
    return value.toString();
  }

  // String - escape single quotes
  return `'${value.replace(/'/g, "''")}'`;
}

/**
 * Generate SELECT clause
 */
function generateSelectClause(
  columns: SelectedColumn[],
  driver: DbDriver,
  distinct: boolean = false
): string {
  if (columns.length === 0) {
    return 'SELECT *';
  }

  const distinctKeyword = distinct ? 'DISTINCT ' : '';
  const columnStrings = columns.map((col) => {
    const tableColumn = `${quoteIdentifier(col.tableAlias, driver)}.${quoteIdentifier(col.columnName, driver)}`;

    let columnExpr = tableColumn;

    // Apply DISTINCT to individual column if specified
    if (col.distinct && !distinct) {
      columnExpr = `DISTINCT ${columnExpr}`;
    }

    // Apply aggregate function
    if (col.aggregate) {
      if (col.aggregate === 'COUNT_DISTINCT') {
        columnExpr = `COUNT(DISTINCT ${tableColumn})`;
      } else {
        columnExpr = `${col.aggregate}(${tableColumn})`;
      }
    }

    // Add alias if specified
    if (col.alias) {
      columnExpr += ` AS ${quoteIdentifier(col.alias, driver)}`;
    }

    return columnExpr;
  });

  return `SELECT ${distinctKeyword}${columnStrings.join(', ')}`;
}

/**
 * Generate FROM clause with first table
 */
function generateFromClause(
  firstTable: QueryBuilderState['tables'][0],
  driver: DbDriver
): string {
  const tableName = `${quoteIdentifier(firstTable.schema, driver)}.${quoteIdentifier(firstTable.tableName, driver)}`;
  return `FROM ${tableName} AS ${quoteIdentifier(firstTable.alias, driver)}`;
}

/**
 * Generate JOIN clauses
 */
function generateJoinClauses(
  joins: JoinCondition[],
  tables: QueryBuilderState['tables'],
  driver: DbDriver
): string {
  if (joins.length === 0) {
    return '';
  }

  return joins
    .map((join) => {
      // Find the right table
      const rightTable = tables.find((t) => t.alias === join.rightTable);
      if (!rightTable) {
        return '';
      }

      const tableName = `${quoteIdentifier(rightTable.schema, driver)}.${quoteIdentifier(rightTable.tableName, driver)}`;
      const leftCol = `${quoteIdentifier(join.leftTable, driver)}.${quoteIdentifier(join.leftColumn, driver)}`;
      const rightCol = `${quoteIdentifier(join.rightTable, driver)}.${quoteIdentifier(join.rightColumn, driver)}`;

      return `${join.type} JOIN ${tableName} AS ${quoteIdentifier(join.rightTable, driver)} ON ${leftCol} = ${rightCol}`;
    })
    .filter(Boolean)
    .join('\n');
}

/**
 * Generate WHERE condition
 */
function generateWhereCondition(condition: WhereCondition, driver: DbDriver): string {
  const column = `${quoteIdentifier(condition.tableAlias, driver)}.${quoteIdentifier(condition.columnName, driver)}`;

  switch (condition.operator) {
    case 'IS NULL':
      return `${column} IS NULL`;

    case 'IS NOT NULL':
      return `${column} IS NOT NULL`;

    case 'IN':
      if (!condition.values || condition.values.length === 0) {
        return `${column} IN ()`;
      }
      return `${column} IN (${condition.values.map((v) => formatValue(v)).join(', ')})`;

    case 'NOT IN':
      if (!condition.values || condition.values.length === 0) {
        return `${column} NOT IN ()`;
      }
      return `${column} NOT IN (${condition.values.map((v) => formatValue(v)).join(', ')})`;

    case 'BETWEEN':
      return `${column} BETWEEN ${formatValue(condition.value)} AND ${formatValue(condition.value2)}`;

    default:
      return `${column} ${condition.operator} ${formatValue(condition.value)}`;
  }
}

/**
 * Generate WHERE clause from condition group
 */
function generateConditionGroup(group: ConditionGroup, driver: DbDriver, indent: string = ''): string {
  const conditions: string[] = [];

  // Add direct conditions
  for (const condition of group.conditions) {
    conditions.push(generateWhereCondition(condition, driver));
  }

  // Add nested groups
  if (group.groups) {
    for (const nestedGroup of group.groups) {
      const nested = generateConditionGroup(nestedGroup, driver, indent + '  ');
      if (nested) {
        conditions.push(`(${nested})`);
      }
    }
  }

  if (conditions.length === 0) {
    return '';
  }

  return conditions.join(` ${group.operator} `);
}

/**
 * Generate WHERE clause
 */
function generateWhereClause(where: ConditionGroup | undefined, driver: DbDriver): string {
  if (!where) {
    return '';
  }

  const conditions = generateConditionGroup(where, driver);
  if (!conditions) {
    return '';
  }

  return `WHERE ${conditions}`;
}

/**
 * Generate GROUP BY clause
 */
function generateGroupByClause(
  groupBy: QueryBuilderState['groupBy'],
  driver: DbDriver
): string {
  if (groupBy.length === 0) {
    return '';
  }

  const columns = groupBy.map(
    (g) => `${quoteIdentifier(g.tableAlias, driver)}.${quoteIdentifier(g.columnName, driver)}`
  );

  return `GROUP BY ${columns.join(', ')}`;
}

/**
 * Generate HAVING condition
 */
function generateHavingCondition(condition: HavingCondition, driver: DbDriver): string {
  const column = `${quoteIdentifier(condition.tableAlias, driver)}.${quoteIdentifier(condition.columnName, driver)}`;
  const aggregateExpr =
    condition.aggregate === 'COUNT_DISTINCT'
      ? `COUNT(DISTINCT ${column})`
      : `${condition.aggregate}(${column})`;

  return `${aggregateExpr} ${condition.operator} ${formatValue(condition.value)}`;
}

/**
 * Generate HAVING clause
 */
function generateHavingClause(
  having: HavingCondition[] | undefined,
  driver: DbDriver
): string {
  if (!having || having.length === 0) {
    return '';
  }

  const conditions = having.map((h) => generateHavingCondition(h, driver));
  return `HAVING ${conditions.join(' AND ')}`;
}

/**
 * Generate ORDER BY clause
 */
function generateOrderByClause(
  orderBy: QueryBuilderState['orderBy'],
  driver: DbDriver
): string {
  if (orderBy.length === 0) {
    return '';
  }

  const columns = orderBy.map(
    (o) =>
      `${quoteIdentifier(o.tableAlias, driver)}.${quoteIdentifier(o.columnName, driver)} ${o.direction}`
  );

  return `ORDER BY ${columns.join(', ')}`;
}

/**
 * Generate LIMIT and OFFSET clauses
 */
function generateLimitOffsetClause(
  limit: number | undefined,
  offset: number | undefined,
  driver: DbDriver
): string {
  const parts: string[] = [];

  if (limit !== undefined) {
    parts.push(`LIMIT ${limit}`);
  }

  if (offset !== undefined) {
    if (driver === 'SqlServer') {
      // SQL Server uses OFFSET...FETCH instead of LIMIT
      parts.push(`OFFSET ${offset} ROWS`);
      if (limit !== undefined) {
        parts.push(`FETCH NEXT ${limit} ROWS ONLY`);
      }
    } else {
      parts.push(`OFFSET ${offset}`);
    }
  }

  return parts.join(' ');
}

/**
 * Generate complete SQL query from query builder state
 */
export function generateSQL(state: QueryBuilderState): string {
  if (state.tables.length === 0) {
    return '-- No tables selected';
  }

  const parts: string[] = [];

  // SELECT clause
  parts.push(generateSelectClause(state.columns, state.driver, state.distinct));

  // FROM clause (first table)
  parts.push(generateFromClause(state.tables[0], state.driver));

  // JOIN clauses
  const joins = generateJoinClauses(state.joins, state.tables, state.driver);
  if (joins) {
    parts.push(joins);
  }

  // WHERE clause
  const where = generateWhereClause(state.where, state.driver);
  if (where) {
    parts.push(where);
  }

  // GROUP BY clause
  const groupBy = generateGroupByClause(state.groupBy, state.driver);
  if (groupBy) {
    parts.push(groupBy);
  }

  // HAVING clause
  const having = generateHavingClause(state.having, state.driver);
  if (having) {
    parts.push(having);
  }

  // ORDER BY clause
  const orderBy = generateOrderByClause(state.orderBy, state.driver);
  if (orderBy) {
    parts.push(orderBy);
  }

  // LIMIT and OFFSET
  const limitOffset = generateLimitOffsetClause(state.limit, state.offset, state.driver);
  if (limitOffset) {
    parts.push(limitOffset);
  }

  return parts.join('\n') + ';';
}

/**
 * Validate query builder state
 */
export function validateQueryBuilderState(state: QueryBuilderState): {
  valid: boolean;
  errors: string[];
} {
  const errors: string[] = [];

  // Must have at least one table
  if (state.tables.length === 0) {
    errors.push('At least one table must be selected');
  }

  // If columns are selected, they must reference existing tables
  for (const col of state.columns) {
    const tableExists = state.tables.some((t) => t.alias === col.tableAlias);
    if (!tableExists) {
      errors.push(`Column references non-existent table: ${col.tableAlias}`);
    }
  }

  // Joins must reference existing tables
  for (const join of state.joins) {
    const leftExists = state.tables.some((t) => t.alias === join.leftTable);
    const rightExists = state.tables.some((t) => t.alias === join.rightTable);

    if (!leftExists) {
      errors.push(`JOIN references non-existent left table: ${join.leftTable}`);
    }
    if (!rightExists) {
      errors.push(`JOIN references non-existent right table: ${join.rightTable}`);
    }
  }

  // GROUP BY requires columns to be aggregated or in GROUP BY
  if (state.groupBy.length > 0 && state.columns.length > 0) {
    for (const col of state.columns) {
      const isInGroupBy = state.groupBy.some(
        (g) => g.tableAlias === col.tableAlias && g.columnName === col.columnName
      );
      const isAggregated = !!col.aggregate;

      if (!isInGroupBy && !isAggregated) {
        errors.push(
          `Column ${col.tableAlias}.${col.columnName} must be in GROUP BY or use an aggregate function`
        );
      }
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}
