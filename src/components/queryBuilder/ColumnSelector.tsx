import { FC, useState, DragEvent } from 'react';
import { GripVertical, X } from 'lucide-react';
import type { QueryTable, SelectedColumn, AggregateFunction } from '@/types/queryBuilder';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';

interface ColumnSelectorProps {
  tables: QueryTable[];
  selectedColumns: SelectedColumn[];
  onAddColumn: (column: SelectedColumn) => void;
  onRemoveColumn: (id: string) => void;
  onUpdateColumn: (id: string, updates: Partial<SelectedColumn>) => void;
}

const AGGREGATE_FUNCTIONS: Array<{ value: AggregateFunction | 'none'; label: string }> = [
  { value: 'none', label: 'None' },
  { value: 'COUNT', label: 'COUNT' },
  { value: 'SUM', label: 'SUM' },
  { value: 'AVG', label: 'AVG' },
  { value: 'MIN', label: 'MIN' },
  { value: 'MAX', label: 'MAX' },
  { value: 'COUNT_DISTINCT', label: 'COUNT DISTINCT' },
];

export const ColumnSelector: FC<ColumnSelectorProps> = ({
  tables,
  selectedColumns,
  onAddColumn,
  onRemoveColumn,
  onUpdateColumn,
}) => {
  const [draggedColumnId, setDraggedColumnId] = useState<string | null>(null);
  const [dragOverColumnId, setDragOverColumnId] = useState<string | null>(null);

  // Generate unique ID for new column selections
  const generateColumnId = () => `col_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;

  // Handle adding a column from the available list
  const handleAddColumn = (table: QueryTable, columnName: string) => {
    const newColumn: SelectedColumn = {
      id: generateColumnId(),
      tableAlias: table.alias,
      columnName,
      alias: '',
      aggregate: undefined,
      distinct: false,
    };
    onAddColumn(newColumn);
  };

  // Check if a column is already selected
  const isColumnSelected = (tableAlias: string, columnName: string) => {
    return selectedColumns.some(
      (col) => col.tableAlias === tableAlias && col.columnName === columnName
    );
  };

  // Validate alias uniqueness
  const isAliasUnique = (alias: string, currentId: string) => {
    if (!alias.trim()) return true; // Empty alias is valid
    return !selectedColumns.some(
      (col) => col.id !== currentId && col.alias === alias.trim()
    );
  };

  // Drag and drop handlers
  const handleDragStart = (e: DragEvent<HTMLDivElement>, columnId: string) => {
    setDraggedColumnId(columnId);
    e.dataTransfer.effectAllowed = 'move';
  };

  const handleDragOver = (e: DragEvent<HTMLDivElement>, columnId: string) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (draggedColumnId !== columnId) {
      setDragOverColumnId(columnId);
    }
  };

  const handleDragLeave = () => {
    setDragOverColumnId(null);
  };

  const handleDrop = (e: DragEvent<HTMLDivElement>, targetColumnId: string) => {
    e.preventDefault();
    setDragOverColumnId(null);

    if (!draggedColumnId || draggedColumnId === targetColumnId) {
      return;
    }

    // Reorder columns
    const draggedIndex = selectedColumns.findIndex((col) => col.id === draggedColumnId);
    const targetIndex = selectedColumns.findIndex((col) => col.id === targetColumnId);

    if (draggedIndex === -1 || targetIndex === -1) return;

    // This is handled by parent component - we just need to emit events
    // For now, we'll notify via a console warning as reordering requires state restructuring
    console.warn('Column reordering requires parent component to handle array reordering');
  };

  const handleDragEnd = () => {
    setDraggedColumnId(null);
    setDragOverColumnId(null);
  };

  // Handle aggregate function change
  const handleAggregateChange = (columnId: string, value: string) => {
    const aggregate = value === 'none' ? undefined : (value as AggregateFunction);
    onUpdateColumn(columnId, { aggregate });
  };

  // Handle alias change
  const handleAliasChange = (columnId: string, alias: string) => {
    onUpdateColumn(columnId, { alias });
  };

  // Handle distinct toggle
  const handleDistinctToggle = (columnId: string, checked: boolean) => {
    onUpdateColumn(columnId, { distinct: checked });
  };

  // Get column data type from table
  const getColumnDataType = (tableAlias: string, columnName: string): string => {
    const table = tables.find((t) => t.alias === tableAlias);
    const column = table?.columns.find((c) => c.name === columnName);
    return column?.dataType || 'unknown';
  };

  return (
    <div className="flex h-full gap-4">
      {/* Available Columns Panel */}
      <div className="flex-1 flex flex-col border rounded-lg bg-background">
        <div className="border-b px-4 py-3">
          <h3 className="text-sm font-semibold">Available Columns</h3>
          <p className="text-xs text-muted-foreground mt-1">
            Click columns to add them to your query
          </p>
        </div>

        <div className="flex-1 overflow-auto p-2">
          {tables.length === 0 ? (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
              No tables selected
            </div>
          ) : (
            <div className="space-y-3">
              {tables.map((table) => (
                <div key={table.alias} className="space-y-1">
                  <div className="px-2 py-1.5 bg-muted/50 rounded-md">
                    <p className="text-xs font-medium">
                      {table.tableName} ({table.alias})
                    </p>
                    <p className="text-xs text-muted-foreground">{table.schema}</p>
                  </div>

                  <div className="space-y-0.5 pl-2">
                    {table.columns.map((column) => {
                      const selected = isColumnSelected(table.alias, column.name);
                      return (
                        <button
                          key={`${table.alias}.${column.name}`}
                          onClick={() => !selected && handleAddColumn(table, column.name)}
                          disabled={selected}
                          className={cn(
                            'w-full text-left px-2 py-1.5 rounded text-xs transition-colors',
                            selected
                              ? 'bg-muted/30 text-muted-foreground cursor-not-allowed opacity-60'
                              : 'hover:bg-accent hover:text-accent-foreground cursor-pointer'
                          )}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <span className="font-mono">{column.name}</span>
                            <span className="text-muted-foreground text-xs">
                              {column.dataType}
                            </span>
                          </div>
                          {selected && (
                            <span className="text-xs text-muted-foreground ml-1">(added)</span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Selected Columns Panel */}
      <div className="flex-1 flex flex-col border rounded-lg bg-background">
        <div className="border-b px-4 py-3">
          <h3 className="text-sm font-semibold">Selected Columns</h3>
          <p className="text-xs text-muted-foreground mt-1">
            Configure aliases, aggregates, and order
          </p>
        </div>

        <div className="flex-1 overflow-auto p-2">
          {selectedColumns.length === 0 ? (
            <div className="flex items-center justify-center h-full text-sm text-muted-foreground text-center px-4">
              No columns selected - click columns from the list to add them
            </div>
          ) : (
            <div className="space-y-2">
              {selectedColumns.map((column) => {
                const dataType = getColumnDataType(column.tableAlias, column.columnName);
                const aggregateValue = column.aggregate || 'none';

                return (
                  <div
                    key={column.id}
                    draggable
                    onDragStart={(e) => handleDragStart(e, column.id)}
                    onDragOver={(e) => handleDragOver(e, column.id)}
                    onDragLeave={handleDragLeave}
                    onDrop={(e) => handleDrop(e, column.id)}
                    onDragEnd={handleDragEnd}
                    className={cn(
                      'border rounded-lg p-3 bg-card transition-all cursor-move',
                      draggedColumnId === column.id && 'opacity-50',
                      dragOverColumnId === column.id && 'border-primary border-2'
                    )}
                  >
                    <div className="flex items-start gap-2">
                      {/* Drag Handle */}
                      <div className="pt-2 text-muted-foreground cursor-grab active:cursor-grabbing">
                        <GripVertical className="size-4" />
                      </div>

                      {/* Column Info and Controls */}
                      <div className="flex-1 space-y-2">
                        {/* Column Name and Type */}
                        <div className="flex items-center justify-between">
                          <div className="flex-1">
                            <p className="text-sm font-medium font-mono">
                              {column.tableAlias}.{column.columnName}
                            </p>
                            <p className="text-xs text-muted-foreground">{dataType}</p>
                          </div>
                        </div>

                        {/* Controls Grid */}
                        <div className="grid grid-cols-2 gap-2">
                          {/* Alias Input */}
                          <div className="space-y-1">
                            <label className="text-xs text-muted-foreground">Alias</label>
                            <Input
                              type="text"
                              placeholder="Optional alias"
                              value={column.alias || ''}
                              onChange={(e) => handleAliasChange(column.id, e.target.value)}
                              className="h-8 text-xs"
                              aria-invalid={
                                column.alias && !isAliasUnique(column.alias, column.id)
                                  ? true
                                  : undefined
                              }
                            />
                            {column.alias && !isAliasUnique(column.alias, column.id) && (
                              <p className="text-xs text-destructive">Alias must be unique</p>
                            )}
                          </div>

                          {/* Aggregate Function */}
                          <div className="space-y-1">
                            <label className="text-xs text-muted-foreground">Aggregate</label>
                            <Select
                              value={aggregateValue}
                              onValueChange={(value) =>
                                handleAggregateChange(column.id, value)
                              }
                            >
                              <SelectTrigger size="sm" className="h-8 text-xs">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {AGGREGATE_FUNCTIONS.map((func) => (
                                  <SelectItem key={func.value} value={func.value}>
                                    {func.label}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                        </div>

                        {/* Distinct Checkbox */}
                        <div className="flex items-center gap-2">
                          <Checkbox
                            id={`distinct-${column.id}`}
                            checked={column.distinct || false}
                            onCheckedChange={(checked) =>
                              handleDistinctToggle(column.id, checked === true)
                            }
                          />
                          <label
                            htmlFor={`distinct-${column.id}`}
                            className="text-xs text-muted-foreground cursor-pointer select-none"
                          >
                            DISTINCT
                          </label>
                        </div>
                      </div>

                      {/* Remove Button */}
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        onClick={() => onRemoveColumn(column.id)}
                        className="text-muted-foreground hover:text-destructive"
                      >
                        <X className="size-4" />
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
