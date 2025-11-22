/**
 * Query Builder State Management Hook
 *
 * Provides state management and actions for the visual query builder.
 */

import { useReducer, useCallback, useMemo } from 'react';
import type {
  QueryBuilderState,
  QueryBuilderAction,
  SelectedColumn,
  QueryTable,
  JoinCondition,
  WhereCondition,
  ConditionGroup,
  GroupByClause,
  HavingCondition,
  OrderByClause,
  SortDirection,
} from '../types/queryBuilder';
import { getInitialQueryBuilderState } from '../types/queryBuilder';
import { generateSQL, validateQueryBuilderState } from '../lib/sqlGenerator';
import type { DbDriver } from '../types/database';

/**
 * Query builder reducer
 */
function queryBuilderReducer(
  state: QueryBuilderState,
  action: QueryBuilderAction
): QueryBuilderState {
  switch (action.type) {
    case 'ADD_TABLE':
      return {
        ...state,
        tables: [...state.tables, action.payload],
      };

    case 'REMOVE_TABLE': {
      const { alias } = action.payload;
      return {
        ...state,
        tables: state.tables.filter((t) => t.alias !== alias),
        columns: state.columns.filter((c) => c.tableAlias !== alias),
        joins: state.joins.filter((j) => j.leftTable !== alias && j.rightTable !== alias),
        groupBy: state.groupBy.filter((g) => g.tableAlias !== alias),
      };
    }

    case 'ADD_COLUMN':
      return {
        ...state,
        columns: [...state.columns, action.payload],
      };

    case 'REMOVE_COLUMN':
      return {
        ...state,
        columns: state.columns.filter((c) => c.id !== action.payload.id),
      };

    case 'UPDATE_COLUMN':
      return {
        ...state,
        columns: state.columns.map((c) =>
          c.id === action.payload.id ? { ...c, ...action.payload.updates } : c
        ),
      };

    case 'ADD_JOIN':
      return {
        ...state,
        joins: [...state.joins, action.payload],
      };

    case 'REMOVE_JOIN':
      return {
        ...state,
        joins: state.joins.filter((j) => j.id !== action.payload.id),
      };

    case 'UPDATE_JOIN':
      return {
        ...state,
        joins: state.joins.map((j) =>
          j.id === action.payload.id ? { ...j, ...action.payload.updates } : j
        ),
      };

    case 'SET_WHERE':
      return {
        ...state,
        where: action.payload,
      };

    case 'ADD_WHERE_CONDITION': {
      if (!state.where) {
        // Create initial group
        return {
          ...state,
          where: {
            id: crypto.randomUUID(),
            operator: 'AND',
            conditions: [action.payload.condition],
            groups: [],
          },
        };
      }

      // Add to existing group
      const addConditionToGroup = (group: ConditionGroup): ConditionGroup => {
        if (!action.payload.groupId || group.id === action.payload.groupId) {
          return {
            ...group,
            conditions: [...group.conditions, action.payload.condition],
          };
        }

        if (group.groups) {
          return {
            ...group,
            groups: group.groups.map(addConditionToGroup),
          };
        }

        return group;
      };

      return {
        ...state,
        where: addConditionToGroup(state.where),
      };
    }

    case 'REMOVE_WHERE_CONDITION': {
      if (!state.where) {
        return state;
      }

      const removeConditionFromGroup = (group: ConditionGroup): ConditionGroup | null => {
        const updatedConditions = group.conditions.filter(
          (c) => c.id !== action.payload.conditionId
        );

        const updatedGroups = group.groups
          ?.map(removeConditionFromGroup)
          .filter((g): g is ConditionGroup => g !== null);

        // If group is empty, return null to remove it
        if (updatedConditions.length === 0 && (!updatedGroups || updatedGroups.length === 0)) {
          return null;
        }

        return {
          ...group,
          conditions: updatedConditions,
          groups: updatedGroups,
        };
      };

      const updatedWhere = removeConditionFromGroup(state.where);

      return {
        ...state,
        where: updatedWhere || undefined,
      };
    }

    case 'ADD_GROUP_BY':
      return {
        ...state,
        groupBy: [...state.groupBy, action.payload],
      };

    case 'REMOVE_GROUP_BY':
      return {
        ...state,
        groupBy: state.groupBy.filter(
          (g) =>
            !(
              g.tableAlias === action.payload.tableAlias &&
              g.columnName === action.payload.columnName
            )
        ),
      };

    case 'ADD_HAVING':
      return {
        ...state,
        having: [...(state.having || []), action.payload],
      };

    case 'REMOVE_HAVING':
      return {
        ...state,
        having: state.having?.filter((h) => h.id !== action.payload.id),
      };

    case 'ADD_ORDER_BY':
      return {
        ...state,
        orderBy: [...state.orderBy, action.payload],
      };

    case 'REMOVE_ORDER_BY':
      return {
        ...state,
        orderBy: state.orderBy.filter((o) => o.id !== action.payload.id),
      };

    case 'UPDATE_ORDER_BY':
      return {
        ...state,
        orderBy: state.orderBy.map((o) =>
          o.id === action.payload.id ? { ...o, direction: action.payload.direction } : o
        ),
      };

    case 'SET_LIMIT':
      return {
        ...state,
        limit: action.payload,
      };

    case 'SET_OFFSET':
      return {
        ...state,
        offset: action.payload,
      };

    case 'SET_DISTINCT':
      return {
        ...state,
        distinct: action.payload,
      };

    case 'RESET':
      return getInitialQueryBuilderState(state.driver);

    default:
      return state;
  }
}

/**
 * Hook for managing query builder state
 */
export function useQueryBuilder(driver: DbDriver) {
  const [state, dispatch] = useReducer(
    queryBuilderReducer,
    getInitialQueryBuilderState(driver)
  );

  // Generate SQL from current state
  const sql = useMemo(() => generateSQL(state), [state]);

  // Validate current state
  const validation = useMemo(() => validateQueryBuilderState(state), [state]);

  // Actions
  const addTable = useCallback((table: QueryTable) => {
    dispatch({ type: 'ADD_TABLE', payload: table });
  }, []);

  const removeTable = useCallback((alias: string) => {
    dispatch({ type: 'REMOVE_TABLE', payload: { alias } });
  }, []);

  const addColumn = useCallback((column: SelectedColumn) => {
    dispatch({ type: 'ADD_COLUMN', payload: column });
  }, []);

  const removeColumn = useCallback((id: string) => {
    dispatch({ type: 'REMOVE_COLUMN', payload: { id } });
  }, []);

  const updateColumn = useCallback((id: string, updates: Partial<SelectedColumn>) => {
    dispatch({ type: 'UPDATE_COLUMN', payload: { id, updates } });
  }, []);

  const addJoin = useCallback((join: JoinCondition) => {
    dispatch({ type: 'ADD_JOIN', payload: join });
  }, []);

  const removeJoin = useCallback((id: string) => {
    dispatch({ type: 'REMOVE_JOIN', payload: { id } });
  }, []);

  const updateJoin = useCallback((id: string, updates: Partial<JoinCondition>) => {
    dispatch({ type: 'UPDATE_JOIN', payload: { id, updates } });
  }, []);

  const addWhereCondition = useCallback((condition: WhereCondition, groupId?: string) => {
    dispatch({ type: 'ADD_WHERE_CONDITION', payload: { condition, groupId } });
  }, []);

  const removeWhereCondition = useCallback((conditionId: string, groupId?: string) => {
    dispatch({ type: 'REMOVE_WHERE_CONDITION', payload: { conditionId, groupId } });
  }, []);

  const addGroupBy = useCallback((groupBy: GroupByClause) => {
    dispatch({ type: 'ADD_GROUP_BY', payload: groupBy });
  }, []);

  const removeGroupBy = useCallback((tableAlias: string, columnName: string) => {
    dispatch({ type: 'REMOVE_GROUP_BY', payload: { tableAlias, columnName } });
  }, []);

  const addHaving = useCallback((having: HavingCondition) => {
    dispatch({ type: 'ADD_HAVING', payload: having });
  }, []);

  const removeHaving = useCallback((id: string) => {
    dispatch({ type: 'REMOVE_HAVING', payload: { id } });
  }, []);

  const addOrderBy = useCallback((orderBy: OrderByClause) => {
    dispatch({ type: 'ADD_ORDER_BY', payload: orderBy });
  }, []);

  const removeOrderBy = useCallback((id: string) => {
    dispatch({ type: 'REMOVE_ORDER_BY', payload: { id } });
  }, []);

  const updateOrderBy = useCallback((id: string, direction: SortDirection) => {
    dispatch({ type: 'UPDATE_ORDER_BY', payload: { id, direction } });
  }, []);

  const setLimit = useCallback((limit: number | undefined) => {
    dispatch({ type: 'SET_LIMIT', payload: limit });
  }, []);

  const setOffset = useCallback((offset: number | undefined) => {
    dispatch({ type: 'SET_OFFSET', payload: offset });
  }, []);

  const setDistinct = useCallback((distinct: boolean) => {
    dispatch({ type: 'SET_DISTINCT', payload: distinct });
  }, []);

  const setWhere = useCallback((where: ConditionGroup | undefined) => {
    dispatch({ type: 'SET_WHERE', payload: where });
  }, []);

  const reset = useCallback(() => {
    dispatch({ type: 'RESET' });
  }, []);

  return {
    state,
    sql,
    validation,
    actions: {
      addTable,
      removeTable,
      addColumn,
      removeColumn,
      updateColumn,
      addJoin,
      removeJoin,
      updateJoin,
      addWhereCondition,
      removeWhereCondition,
      setWhere,
      addGroupBy,
      removeGroupBy,
      addHaving,
      removeHaving,
      addOrderBy,
      removeOrderBy,
      updateOrderBy,
      setLimit,
      setOffset,
      setDistinct,
      reset,
    },
  };
}
