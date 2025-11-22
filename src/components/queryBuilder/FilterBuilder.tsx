import { FC, useCallback } from 'react';
import {
  QueryTable,
  ConditionGroup,
  WhereCondition,
  ComparisonOperator,
  LogicalOperator,
} from '@/types/queryBuilder';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { X, Plus, PlusCircle } from 'lucide-react';
import { cn } from '@/lib/utils';

/**
 * Props for FilterBuilder component
 */
interface FilterBuilderProps {
  /** Available tables and columns */
  tables: QueryTable[];
  /** Root condition group */
  where: ConditionGroup | undefined;
  /** Callback to add a condition to a group */
  onAddCondition: (condition: WhereCondition, groupId?: string) => void;
  /** Callback to remove a condition */
  onRemoveCondition: (conditionId: string, groupId?: string) => void;
  /** Callback to set the entire WHERE clause */
  onSetWhere: (where: ConditionGroup | undefined) => void;
}

/**
 * All available comparison operators
 */
const COMPARISON_OPERATORS: ComparisonOperator[] = [
  '=',
  '!=',
  '>',
  '<',
  '>=',
  '<=',
  'LIKE',
  'NOT LIKE',
  'IN',
  'NOT IN',
  'IS NULL',
  'IS NOT NULL',
  'BETWEEN',
];

/**
 * Operators that don't require a value input
 */
const NO_VALUE_OPERATORS: ComparisonOperator[] = ['IS NULL', 'IS NOT NULL'];

/**
 * Operators that require multiple values
 */
const MULTI_VALUE_OPERATORS: ComparisonOperator[] = ['IN', 'NOT IN'];

/**
 * Operators that require two value inputs
 */
const BETWEEN_OPERATORS: ComparisonOperator[] = ['BETWEEN'];

/**
 * Maximum nesting depth for condition groups
 */
const MAX_NESTING_DEPTH = 3;

/**
 * Individual condition row component
 */
interface ConditionRowProps {
  condition: WhereCondition;
  tables: QueryTable[];
  onUpdate: (updates: Partial<WhereCondition>) => void;
  onRemove: () => void;
}

const ConditionRow: FC<ConditionRowProps> = ({
  condition,
  tables,
  onUpdate,
  onRemove,
}) => {
  // Get all columns from all tables
  const allColumns = tables.flatMap((table) =>
    table.columns.map((col) => ({
      tableAlias: table.alias,
      columnName: col.name,
      dataType: col.dataType,
      label: `${table.alias}.${col.name}`,
    }))
  );

  // Find the selected column's data type
  const selectedColumn = allColumns.find(
    (col) =>
      col.tableAlias === condition.tableAlias &&
      col.columnName === condition.columnName
  );

  const dataType = selectedColumn?.dataType.toLowerCase() || 'text';

  // Determine if operator needs value input
  const needsValue = !NO_VALUE_OPERATORS.includes(condition.operator);
  const needsMultiValue = MULTI_VALUE_OPERATORS.includes(condition.operator);
  const needsBetween = BETWEEN_OPERATORS.includes(condition.operator);

  // Handle value changes
  const handleValueChange = (value: string) => {
    if (needsMultiValue) {
      // For IN/NOT IN, parse comma-separated values
      const values = value
        .split(',')
        .map((v) => v.trim())
        .filter((v) => v.length > 0);
      onUpdate({ values });
    } else if (dataType.includes('int') || dataType.includes('numeric') || dataType.includes('decimal')) {
      // Parse as number
      const numValue = parseFloat(value);
      onUpdate({ value: isNaN(numValue) ? value : numValue });
    } else if (dataType.includes('bool')) {
      // Parse as boolean
      onUpdate({ value: value === 'true' });
    } else {
      // Keep as string
      onUpdate({ value });
    }
  };

  const handleValue2Change = (value: string) => {
    if (dataType.includes('int') || dataType.includes('numeric') || dataType.includes('decimal')) {
      const numValue = parseFloat(value);
      onUpdate({ value2: isNaN(numValue) ? value : numValue });
    } else {
      onUpdate({ value2: value });
    }
  };

  return (
    <div className="flex items-center gap-2 p-2 bg-background rounded-md border border-border">
      {/* Column selector */}
      <Select
        value={`${condition.tableAlias}.${condition.columnName}`}
        onValueChange={(value) => {
          const [tableAlias, columnName] = value.split('.');
          onUpdate({ tableAlias, columnName });
        }}
      >
        <SelectTrigger className="w-[200px]" size="sm">
          <SelectValue placeholder="Select column" />
        </SelectTrigger>
        <SelectContent>
          {allColumns.map((col) => (
            <SelectItem key={col.label} value={col.label}>
              {col.label}
              <span className="text-muted-foreground ml-2 text-xs">
                ({col.dataType})
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Operator selector */}
      <Select
        value={condition.operator}
        onValueChange={(value) => {
          const operator = value as ComparisonOperator;
          onUpdate({ operator });

          // Clear values when switching to operators that don't need them
          if (NO_VALUE_OPERATORS.includes(operator)) {
            onUpdate({ operator, value: undefined, value2: undefined, values: undefined });
          }
        }}
      >
        <SelectTrigger className="w-[140px]" size="sm">
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

      {/* Value input(s) */}
      {needsValue && (
        <>
          {needsMultiValue ? (
            // Multi-value input for IN/NOT IN
            <Input
              type="text"
              placeholder="value1, value2, value3"
              value={condition.values?.join(', ') || ''}
              onChange={(e) => handleValueChange(e.target.value)}
              className="flex-1 h-8"
            />
          ) : needsBetween ? (
            // Two inputs for BETWEEN
            <>
              <Input
                type={dataType.includes('int') || dataType.includes('numeric') ? 'number' : 'text'}
                placeholder="From"
                value={condition.value?.toString() || ''}
                onChange={(e) => handleValueChange(e.target.value)}
                className="w-[120px] h-8"
              />
              <span className="text-muted-foreground text-sm">and</span>
              <Input
                type={dataType.includes('int') || dataType.includes('numeric') ? 'number' : 'text'}
                placeholder="To"
                value={condition.value2?.toString() || ''}
                onChange={(e) => handleValue2Change(e.target.value)}
                className="w-[120px] h-8"
              />
            </>
          ) : dataType.includes('bool') ? (
            // Checkbox for boolean
            <div className="flex items-center gap-2">
              <Checkbox
                checked={condition.value === true}
                onCheckedChange={(checked) => onUpdate({ value: checked === true })}
              />
              <Label className="text-sm">True</Label>
            </div>
          ) : (
            // Single value input
            <Input
              type={
                dataType.includes('int') || dataType.includes('numeric') || dataType.includes('decimal')
                  ? 'number'
                  : dataType.includes('date') || dataType.includes('time')
                  ? 'date'
                  : 'text'
              }
              placeholder={
                condition.operator === 'LIKE' || condition.operator === 'NOT LIKE'
                  ? '%value%'
                  : 'Value'
              }
              value={condition.value?.toString() || ''}
              onChange={(e) => handleValueChange(e.target.value)}
              className="flex-1 h-8"
            />
          )}
        </>
      )}

      {/* Remove button */}
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={onRemove}
        className="shrink-0"
      >
        <X className="size-4" />
      </Button>
    </div>
  );
};

/**
 * Condition group component with recursive rendering
 */
interface ConditionGroupComponentProps {
  group: ConditionGroup;
  tables: QueryTable[];
  depth: number;
  onUpdateGroup: (groupId: string, updates: Partial<ConditionGroup>) => void;
  onAddCondition: (groupId: string, condition: WhereCondition) => void;
  onRemoveCondition: (groupId: string, conditionId: string) => void;
  onAddNestedGroup: (parentGroupId: string, group: ConditionGroup) => void;
  onRemoveGroup: (groupId: string) => void;
}

const ConditionGroupComponent: FC<ConditionGroupComponentProps> = ({
  group,
  tables,
  depth,
  onUpdateGroup,
  onAddCondition,
  onRemoveCondition,
  onAddNestedGroup,
  onRemoveGroup,
}) => {
  const canNest = depth < MAX_NESTING_DEPTH;

  const handleAddCondition = () => {
    // Create a new condition with default values
    const firstColumn = tables[0]?.columns[0];
    if (!firstColumn || !tables[0]) return;

    const newCondition: WhereCondition = {
      id: crypto.randomUUID(),
      tableAlias: tables[0].alias,
      columnName: firstColumn.name,
      operator: '=',
      value: '',
    };

    onAddCondition(group.id, newCondition);
  };

  const handleAddNestedGroup = () => {
    const newGroup: ConditionGroup = {
      id: crypto.randomUUID(),
      operator: 'AND',
      conditions: [],
      groups: [],
    };

    onAddNestedGroup(group.id, newGroup);
  };

  const handleUpdateCondition = (conditionId: string, updates: Partial<WhereCondition>) => {
    const updatedConditions = group.conditions.map((c) =>
      c.id === conditionId ? { ...c, ...updates } : c
    );
    onUpdateGroup(group.id, { conditions: updatedConditions });
  };

  return (
    <div
      className={cn(
        'rounded-lg border p-4 space-y-3',
        depth === 0
          ? 'border-border bg-muted/30'
          : depth === 1
          ? 'border-border/70 bg-background'
          : 'border-border/50 bg-muted/10'
      )}
      style={{ marginLeft: depth > 0 ? `${depth * 16}px` : 0 }}
    >
      {/* Group header with operator toggle */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-muted-foreground">
            Combine with:
          </span>
          <Select
            value={group.operator}
            onValueChange={(value) =>
              onUpdateGroup(group.id, { operator: value as LogicalOperator })
            }
          >
            <SelectTrigger className="w-[100px]" size="sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="AND">AND</SelectItem>
              <SelectItem value="OR">OR</SelectItem>
            </SelectContent>
          </Select>
          <Badge variant="outline" className="ml-2">
            {group.conditions.length} condition(s)
          </Badge>
        </div>

        {/* Remove group button (not for root) */}
        {depth > 0 && (
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => onRemoveGroup(group.id)}
          >
            <X className="size-4" />
          </Button>
        )}
      </div>

      {/* Conditions list */}
      <div className="space-y-2">
        {group.conditions.map((condition, index) => (
          <div key={condition.id}>
            {index > 0 && (
              <div className="text-xs text-muted-foreground font-medium py-1">
                {group.operator}
              </div>
            )}
            <ConditionRow
              condition={condition}
              tables={tables}
              onUpdate={(updates) => handleUpdateCondition(condition.id, updates)}
              onRemove={() => onRemoveCondition(group.id, condition.id)}
            />
          </div>
        ))}

        {/* Nested groups */}
        {group.groups?.map((nestedGroup, index) => (
          <div key={nestedGroup.id}>
            {(group.conditions.length > 0 || index > 0) && (
              <div className="text-xs text-muted-foreground font-medium py-1">
                {group.operator}
              </div>
            )}
            <ConditionGroupComponent
              group={nestedGroup}
              tables={tables}
              depth={depth + 1}
              onUpdateGroup={onUpdateGroup}
              onAddCondition={onAddCondition}
              onRemoveCondition={onRemoveCondition}
              onAddNestedGroup={onAddNestedGroup}
              onRemoveGroup={onRemoveGroup}
            />
          </div>
        ))}
      </div>

      {/* Action buttons */}
      <div className="flex gap-2 pt-2">
        <Button
          variant="outline"
          size="sm"
          onClick={handleAddCondition}
          disabled={tables.length === 0}
        >
          <Plus className="size-4" />
          Add Condition
        </Button>

        {canNest && (
          <Button
            variant="outline"
            size="sm"
            onClick={handleAddNestedGroup}
            disabled={tables.length === 0}
          >
            <PlusCircle className="size-4" />
            Add Group
          </Button>
        )}
      </div>
    </div>
  );
};

/**
 * FilterBuilder - Visual WHERE clause builder
 *
 * Provides a UI for building complex WHERE conditions with:
 * - Nested condition groups
 * - AND/OR logical operators
 * - All SQL comparison operators
 * - Type-aware value inputs
 * - Visual tree structure
 */
export const FilterBuilder: FC<FilterBuilderProps> = ({
  tables,
  where,
  onAddCondition,
  onRemoveCondition,
  onSetWhere,
}) => {
  // Update a group recursively
  const updateGroup = useCallback(
    (groupId: string, updates: Partial<ConditionGroup>) => {
      if (!where) return;

      const updateGroupRecursive = (group: ConditionGroup): ConditionGroup => {
        if (group.id === groupId) {
          return { ...group, ...updates };
        }

        if (group.groups) {
          return {
            ...group,
            groups: group.groups.map(updateGroupRecursive),
          };
        }

        return group;
      };

      onSetWhere(updateGroupRecursive(where));
    },
    [where, onSetWhere]
  );

  // Add a nested group
  const addNestedGroup = useCallback(
    (parentGroupId: string, newGroup: ConditionGroup) => {
      if (!where) return;

      const addGroupRecursive = (group: ConditionGroup): ConditionGroup => {
        if (group.id === parentGroupId) {
          return {
            ...group,
            groups: [...(group.groups || []), newGroup],
          };
        }

        if (group.groups) {
          return {
            ...group,
            groups: group.groups.map(addGroupRecursive),
          };
        }

        return group;
      };

      onSetWhere(addGroupRecursive(where));
    },
    [where, onSetWhere]
  );

  // Remove a group
  const removeGroup = useCallback(
    (groupId: string) => {
      if (!where) return;

      const removeGroupRecursive = (group: ConditionGroup): ConditionGroup => {
        if (group.groups) {
          return {
            ...group,
            groups: group.groups
              .filter((g) => g.id !== groupId)
              .map(removeGroupRecursive),
          };
        }

        return group;
      };

      onSetWhere(removeGroupRecursive(where));
    },
    [where, onSetWhere]
  );

  // Add condition to a specific group
  const handleAddCondition = useCallback(
    (groupId: string, condition: WhereCondition) => {
      onAddCondition(condition, groupId);
    },
    [onAddCondition]
  );

  // Remove condition from a specific group
  const handleRemoveCondition = useCallback(
    (groupId: string, conditionId: string) => {
      onRemoveCondition(conditionId, groupId);
    },
    [onRemoveCondition]
  );

  // Initialize WHERE clause if empty
  const handleInitialize = () => {
    const firstColumn = tables[0]?.columns[0];
    if (!firstColumn || !tables[0]) return;

    const rootGroup: ConditionGroup = {
      id: crypto.randomUUID(),
      operator: 'AND',
      conditions: [
        {
          id: crypto.randomUUID(),
          tableAlias: tables[0].alias,
          columnName: firstColumn.name,
          operator: '=',
          value: '',
        },
      ],
      groups: [],
    };

    onSetWhere(rootGroup);
  };

  // Empty state
  if (!where) {
    return (
      <div className="flex flex-col items-center justify-center py-12 px-4 border border-dashed border-border rounded-lg bg-muted/20">
        <p className="text-sm text-muted-foreground mb-4">
          No filter conditions
        </p>
        <p className="text-xs text-muted-foreground mb-4">
          Click &quot;Add Condition&quot; to filter results
        </p>
        <Button
          variant="outline"
          onClick={handleInitialize}
          disabled={tables.length === 0}
        >
          <Plus className="size-4" />
          Add Condition
        </Button>
        {tables.length === 0 && (
          <p className="text-xs text-muted-foreground mt-2">
            Add tables first to create conditions
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">WHERE Conditions</h3>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onSetWhere(undefined)}
        >
          Clear All
        </Button>
      </div>

      <ConditionGroupComponent
        group={where}
        tables={tables}
        depth={0}
        onUpdateGroup={updateGroup}
        onAddCondition={handleAddCondition}
        onRemoveCondition={handleRemoveCondition}
        onAddNestedGroup={addNestedGroup}
        onRemoveGroup={removeGroup}
      />
    </div>
  );
};
