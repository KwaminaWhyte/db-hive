import { FC, useState, useMemo } from 'react';
import { Plus, X, AlertTriangle } from 'lucide-react';
import {
  QueryTable,
  GroupByClause,
  SelectedColumn,
  HavingCondition,
  AggregateFunction,
  ComparisonOperator,
} from '../../types/queryBuilder';
import { Button } from '../ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';
import { Badge } from '../ui/badge';
import { Alert, AlertDescription } from '../ui/alert';
import { Input } from '../ui/input';
import { Card } from '../ui/card';

interface GroupByPanelProps {
  tables: QueryTable[];
  groupBy: GroupByClause[];
  selectedColumns: SelectedColumn[];
  having: HavingCondition[] | undefined;
  onAddGroupBy: (clause: GroupByClause) => void;
  onRemoveGroupBy: (tableAlias: string, columnName: string) => void;
  onAddHaving: (condition: HavingCondition) => void;
  onRemoveHaving: (id: string) => void;
}

interface ColumnOption {
  tableAlias: string;
  tableName: string;
  columnName: string;
  dataType: string;
}

const AGGREGATE_FUNCTIONS: AggregateFunction[] = [
  'COUNT',
  'SUM',
  'AVG',
  'MIN',
  'MAX',
  'COUNT_DISTINCT',
];

const COMPARISON_OPERATORS: ComparisonOperator[] = [
  '=',
  '!=',
  '>',
  '<',
  '>=',
  '<=',
];

export const GroupByPanel: FC<GroupByPanelProps> = ({
  tables,
  groupBy,
  selectedColumns,
  having,
  onAddGroupBy,
  onRemoveGroupBy,
  onAddHaving,
  onRemoveHaving,
}) => {
  // State for adding new GROUP BY
  const [newGroupByTable, setNewGroupByTable] = useState<string>('');
  const [newGroupByColumn, setNewGroupByColumn] = useState<string>('');

  // State for adding new HAVING
  const [newHavingTable, setNewHavingTable] = useState<string>('');
  const [newHavingColumn, setNewHavingColumn] = useState<string>('');
  const [newHavingAggregate, setNewHavingAggregate] = useState<AggregateFunction>('COUNT');
  const [newHavingOperator, setNewHavingOperator] = useState<ComparisonOperator>('=');
  const [newHavingValue, setNewHavingValue] = useState<string>('');

  // Get all available columns
  const allColumns = useMemo((): ColumnOption[] => {
    return tables.flatMap((table) =>
      table.columns.map((col) => ({
        tableAlias: table.alias,
        tableName: table.tableName,
        columnName: col.name,
        dataType: col.dataType,
      }))
    );
  }, [tables]);

  // Get available columns for GROUP BY (excluding already grouped)
  const availableGroupByColumns = useMemo((): ColumnOption[] => {
    return allColumns.filter(
      (col) =>
        !groupBy.some(
          (gb) => gb.tableAlias === col.tableAlias && gb.columnName === col.columnName
        )
    );
  }, [allColumns, groupBy]);

  // Get columns for selected table (GROUP BY)
  const groupByTableColumns = useMemo((): ColumnOption[] => {
    if (!newGroupByTable) return [];
    return availableGroupByColumns.filter((col) => col.tableAlias === newGroupByTable);
  }, [newGroupByTable, availableGroupByColumns]);

  // Get columns for selected table (HAVING)
  const havingTableColumns = useMemo((): ColumnOption[] => {
    if (!newHavingTable) return [];
    return allColumns.filter((col) => col.tableAlias === newHavingTable);
  }, [newHavingTable, allColumns]);

  // Validate selected columns against GROUP BY
  const invalidColumns = useMemo((): SelectedColumn[] => {
    if (groupBy.length === 0) return [];

    return selectedColumns.filter((col) => {
      // Column has aggregate function - valid
      if (col.aggregate) return false;

      // Column is in GROUP BY - valid
      const isInGroupBy = groupBy.some(
        (gb) => gb.tableAlias === col.tableAlias && gb.columnName === col.columnName
      );

      return !isInGroupBy;
    });
  }, [selectedColumns, groupBy]);

  // Handle add GROUP BY
  const handleAddGroupBy = () => {
    if (!newGroupByTable || !newGroupByColumn) return;

    onAddGroupBy({
      tableAlias: newGroupByTable,
      columnName: newGroupByColumn,
    });

    // Reset form
    setNewGroupByTable('');
    setNewGroupByColumn('');
  };

  // Handle add HAVING
  const handleAddHaving = () => {
    if (!newHavingTable || !newHavingColumn || !newHavingValue) return;

    // Parse value as number if possible
    let value: string | number = newHavingValue;
    const numValue = parseFloat(newHavingValue);
    if (!isNaN(numValue)) {
      value = numValue;
    }

    onAddHaving({
      id: `having-${Date.now()}-${Math.random()}`,
      tableAlias: newHavingTable,
      columnName: newHavingColumn,
      aggregate: newHavingAggregate,
      operator: newHavingOperator,
      value,
    });

    // Reset form
    setNewHavingTable('');
    setNewHavingColumn('');
    setNewHavingAggregate('COUNT');
    setNewHavingOperator('=');
    setNewHavingValue('');
  };

  // Get table name for alias
  const getTableName = (alias: string): string => {
    const table = tables.find((t) => t.alias === alias);
    return table ? table.tableName : alias;
  };

  // Format aggregate function for display
  const formatAggregate = (agg: AggregateFunction): string => {
    return agg === 'COUNT_DISTINCT' ? 'COUNT(DISTINCT)' : agg;
  };

  return (
    <div className="flex flex-col gap-6">
      {/* Validation Warning */}
      {invalidColumns.length > 0 && (
        <Alert variant="destructive">
          <AlertTriangle className="h-4 w-4" />
          <AlertDescription>
            <div className="font-medium mb-1">Invalid columns in SELECT:</div>
            <ul className="list-disc list-inside text-sm">
              {invalidColumns.map((col) => (
                <li key={col.id}>
                  Column '{getTableName(col.tableAlias)}.{col.columnName}' must be in GROUP BY or
                  use an aggregate function
                </li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      )}

      {/* GROUP BY Section */}
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-foreground">GROUP BY Columns</h3>
          {groupBy.length > 0 && (
            <Badge variant="secondary" className="text-xs">
              {groupBy.length} {groupBy.length === 1 ? 'column' : 'columns'}
            </Badge>
          )}
        </div>

        {/* GROUP BY List */}
        {groupBy.length > 0 ? (
          <div className="flex flex-col gap-2">
            {groupBy.map((clause, index) => (
              <Card
                key={`${clause.tableAlias}-${clause.columnName}`}
                className="flex items-center justify-between p-3 bg-muted/50"
              >
                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="text-xs">
                    {index + 1}
                  </Badge>
                  <span className="text-sm font-medium text-foreground">
                    {getTableName(clause.tableAlias)}.{clause.columnName}
                  </span>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onRemoveGroupBy(clause.tableAlias, clause.columnName)}
                  className="h-6 w-6 p-0 hover:bg-destructive/10 hover:text-destructive"
                >
                  <X className="h-3 w-3" />
                </Button>
              </Card>
            ))}
          </div>
        ) : (
          <div className="p-4 border border-dashed rounded-lg bg-muted/30">
            <p className="text-sm text-muted-foreground text-center">No GROUP BY clauses</p>
            <p className="text-xs text-muted-foreground text-center mt-1">
              Add columns to group results by
            </p>
          </div>
        )}

        {/* Add GROUP BY Form */}
        {availableGroupByColumns.length > 0 && (
          <div className="flex flex-col gap-3 p-4 border rounded-lg bg-card">
            <div className="text-xs font-medium text-muted-foreground uppercase">
              Add GROUP BY Column
            </div>

            <div className="grid grid-cols-2 gap-2">
              {/* Table Select */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Table</label>
                <Select value={newGroupByTable} onValueChange={setNewGroupByTable}>
                  <SelectTrigger className="h-9">
                    <SelectValue placeholder="Select table" />
                  </SelectTrigger>
                  <SelectContent>
                    {tables.map((table) => (
                      <SelectItem key={table.alias} value={table.alias}>
                        {table.tableName} ({table.alias})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {/* Column Select */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Column</label>
                <Select
                  value={newGroupByColumn}
                  onValueChange={setNewGroupByColumn}
                  disabled={!newGroupByTable}
                >
                  <SelectTrigger className="h-9">
                    <SelectValue placeholder="Select column" />
                  </SelectTrigger>
                  <SelectContent>
                    {groupByTableColumns.map((col) => (
                      <SelectItem key={col.columnName} value={col.columnName}>
                        {col.columnName}
                        <span className="text-xs text-muted-foreground ml-2">
                          ({col.dataType})
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <Button
              onClick={handleAddGroupBy}
              disabled={!newGroupByTable || !newGroupByColumn}
              className="w-full"
              size="sm"
            >
              <Plus className="h-3 w-3 mr-1" />
              Add GROUP BY
            </Button>
          </div>
        )}

        {/* Help Text */}
        {groupBy.length === 0 && selectedColumns.some((col) => col.aggregate) && (
          <Alert>
            <AlertDescription className="text-xs">
              You have aggregate functions in your SELECT. Add GROUP BY columns to group results by
              specific fields.
            </AlertDescription>
          </Alert>
        )}
      </div>

      {/* HAVING Section */}
      {groupBy.length > 0 && (
        <div className="flex flex-col gap-4 pt-4 border-t">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold text-foreground">HAVING Conditions</h3>
            {having && having.length > 0 && (
              <Badge variant="secondary" className="text-xs">
                {having.length} {having.length === 1 ? 'condition' : 'conditions'}
              </Badge>
            )}
          </div>

          <p className="text-xs text-muted-foreground">
            Filter aggregated results (e.g., COUNT &gt; 5)
          </p>

          {/* HAVING List */}
          {having && having.length > 0 ? (
            <div className="flex flex-col gap-2">
              {having.map((condition) => (
                <Card
                  key={condition.id}
                  className="flex items-center justify-between p-3 bg-muted/50"
                >
                  <div className="flex items-center gap-2">
                    <Badge variant="secondary" className="text-xs font-mono">
                      {formatAggregate(condition.aggregate)}
                    </Badge>
                    <span className="text-sm text-foreground">
                      ({getTableName(condition.tableAlias)}.{condition.columnName})
                    </span>
                    <Badge variant="outline" className="text-xs">
                      {condition.operator}
                    </Badge>
                    <span className="text-sm font-medium text-foreground">
                      {condition.value}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onRemoveHaving(condition.id)}
                    className="h-6 w-6 p-0 hover:bg-destructive/10 hover:text-destructive"
                  >
                    <X className="h-3 w-3" />
                  </Button>
                </Card>
              ))}
            </div>
          ) : (
            <div className="p-4 border border-dashed rounded-lg bg-muted/30">
              <p className="text-sm text-muted-foreground text-center">No HAVING conditions</p>
              <p className="text-xs text-muted-foreground text-center mt-1">
                Filter aggregated results
              </p>
            </div>
          )}

          {/* Add HAVING Form */}
          <div className="flex flex-col gap-3 p-4 border rounded-lg bg-card">
            <div className="text-xs font-medium text-muted-foreground uppercase">
              Add HAVING Condition
            </div>

            <div className="grid grid-cols-2 gap-2">
              {/* Table Select */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Table</label>
                <Select value={newHavingTable} onValueChange={setNewHavingTable}>
                  <SelectTrigger className="h-9">
                    <SelectValue placeholder="Select table" />
                  </SelectTrigger>
                  <SelectContent>
                    {tables.map((table) => (
                      <SelectItem key={table.alias} value={table.alias}>
                        {table.tableName} ({table.alias})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {/* Column Select */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Column</label>
                <Select
                  value={newHavingColumn}
                  onValueChange={setNewHavingColumn}
                  disabled={!newHavingTable}
                >
                  <SelectTrigger className="h-9">
                    <SelectValue placeholder="Select column" />
                  </SelectTrigger>
                  <SelectContent>
                    {havingTableColumns.map((col) => (
                      <SelectItem key={col.columnName} value={col.columnName}>
                        {col.columnName}
                        <span className="text-xs text-muted-foreground ml-2">
                          ({col.dataType})
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {/* Aggregate Function */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Function</label>
                <Select
                  value={newHavingAggregate}
                  onValueChange={(v) => setNewHavingAggregate(v as AggregateFunction)}
                >
                  <SelectTrigger className="h-9">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {AGGREGATE_FUNCTIONS.map((agg) => (
                      <SelectItem key={agg} value={agg}>
                        {formatAggregate(agg)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {/* Operator */}
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Operator</label>
                <Select
                  value={newHavingOperator}
                  onValueChange={(v) => setNewHavingOperator(v as ComparisonOperator)}
                >
                  <SelectTrigger className="h-9">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {COMPARISON_OPERATORS.map((op) => (
                      <SelectItem key={op} value={op}>
                        {op}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {/* Value Input */}
            <div className="flex flex-col gap-1.5">
              <label className="text-xs text-muted-foreground">Value</label>
              <Input
                value={newHavingValue}
                onChange={(e) => setNewHavingValue(e.target.value)}
                placeholder="Enter value"
                className="h-9"
              />
            </div>

            <Button
              onClick={handleAddHaving}
              disabled={!newHavingTable || !newHavingColumn || !newHavingValue}
              className="w-full"
              size="sm"
            >
              <Plus className="h-3 w-3 mr-1" />
              Add HAVING Condition
            </Button>
          </div>
        </div>
      )}
    </div>
  );
};
