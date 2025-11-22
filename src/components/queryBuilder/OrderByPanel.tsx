import { FC, useState } from 'react';
import { ArrowUpDown, GripVertical, Plus, Trash2, ArrowUp, ArrowDown } from 'lucide-react';
import type { QueryTable, OrderByClause, SortDirection } from '@/types/queryBuilder';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

interface OrderByPanelProps {
  tables: QueryTable[];
  orderBy: OrderByClause[];
  onAdd: (clause: OrderByClause) => void;
  onRemove: (id: string) => void;
  onUpdate: (id: string, direction: SortDirection) => void;
  onReorder?: (clauses: OrderByClause[]) => void;
}

export const OrderByPanel: FC<OrderByPanelProps> = ({
  tables,
  orderBy,
  onAdd,
  onRemove,
  onUpdate,
  onReorder,
}) => {
  const [isAddingClause, setIsAddingClause] = useState(false);
  const [selectedTable, setSelectedTable] = useState<string>('');
  const [selectedColumn, setSelectedColumn] = useState<string>('');
  const [selectedDirection, setSelectedDirection] = useState<SortDirection>('ASC');

  const handleAddClause = () => {
    if (!selectedTable || !selectedColumn) {
      return;
    }

    const newClause: OrderByClause = {
      id: `order-${Date.now()}`,
      tableAlias: selectedTable,
      columnName: selectedColumn,
      direction: selectedDirection,
    };

    onAdd(newClause);

    // Reset form
    setSelectedTable('');
    setSelectedColumn('');
    setSelectedDirection('ASC');
    setIsAddingClause(false);
  };

  const toggleDirection = (id: string, currentDirection: SortDirection) => {
    const newDirection: SortDirection = currentDirection === 'ASC' ? 'DESC' : 'ASC';
    onUpdate(id, newDirection);
  };

  const getTableColumns = (tableAlias: string): Array<{ name: string; dataType: string }> => {
    const table = tables.find((t) => t.alias === tableAlias);
    return table?.columns || [];
  };

  const getColumnLabel = (tableAlias: string, columnName: string): string => {
    return `${tableAlias}.${columnName}`;
  };

  return (
    <div className="flex flex-col gap-3">
      {/* Existing ORDER BY clauses */}
      {orderBy.length > 0 ? (
        <div className="space-y-2">
          {orderBy.map((clause, index) => (
            <div
              key={clause.id}
              className="flex items-center gap-2 p-2 rounded-lg border border-input bg-background hover:bg-accent/50 transition-colors"
            >
              {/* Drag handle (if reordering is supported) */}
              {onReorder && (
                <button
                  className="cursor-grab active:cursor-grabbing text-muted-foreground hover:text-foreground"
                  aria-label="Reorder"
                >
                  <GripVertical className="size-4" />
                </button>
              )}

              {/* Order number */}
              <span className="text-xs text-muted-foreground font-medium w-6">
                {index + 1}.
              </span>

              {/* Column name */}
              <div className="flex-1 flex items-center gap-2">
                <span className="text-sm font-medium">
                  {getColumnLabel(clause.tableAlias, clause.columnName)}
                </span>
                <Badge variant="outline" className="text-xs">
                  {getTableColumns(clause.tableAlias).find(
                    (col) => col.name === clause.columnName
                  )?.dataType || 'unknown'}
                </Badge>
              </div>

              {/* Direction toggle button */}
              <Button
                variant="outline"
                size="sm"
                onClick={() => toggleDirection(clause.id, clause.direction)}
                className="gap-1.5"
              >
                {clause.direction === 'ASC' ? (
                  <>
                    <ArrowUp className="size-3.5" />
                    ASC
                  </>
                ) : (
                  <>
                    <ArrowDown className="size-3.5" />
                    DESC
                  </>
                )}
              </Button>

              {/* Delete button */}
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => onRemove(clause.id)}
                aria-label="Remove ORDER BY clause"
              >
                <Trash2 className="size-4 text-destructive" />
              </Button>
            </div>
          ))}
        </div>
      ) : (
        <div className="flex flex-col items-center justify-center py-8 text-center text-muted-foreground">
          <ArrowUpDown className="size-8 mb-2 opacity-50" />
          <p className="text-sm">No sorting applied</p>
          <p className="text-xs mt-1">Add an ORDER BY clause to sort results</p>
        </div>
      )}

      {/* Add new ORDER BY clause */}
      {isAddingClause ? (
        <div className="flex flex-col gap-3 p-3 rounded-lg border border-input bg-accent/20">
          <div className="flex gap-2">
            {/* Table selector */}
            <div className="flex-1">
              <Select value={selectedTable} onValueChange={setSelectedTable}>
                <SelectTrigger size="sm">
                  <SelectValue placeholder="Select table" />
                </SelectTrigger>
                <SelectContent>
                  {tables.map((table) => (
                    <SelectItem key={table.alias} value={table.alias}>
                      {table.alias} ({table.tableName})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Column selector */}
            <div className="flex-1">
              <Select
                value={selectedColumn}
                onValueChange={setSelectedColumn}
                disabled={!selectedTable}
              >
                <SelectTrigger size="sm">
                  <SelectValue placeholder="Select column" />
                </SelectTrigger>
                <SelectContent>
                  {selectedTable &&
                    getTableColumns(selectedTable).map((column) => (
                      <SelectItem key={column.name} value={column.name}>
                        {column.name}
                        <span className="text-xs text-muted-foreground ml-2">
                          ({column.dataType})
                        </span>
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>

            {/* Direction selector */}
            <div className="w-28">
              <Select
                value={selectedDirection}
                onValueChange={(value) => setSelectedDirection(value as SortDirection)}
              >
                <SelectTrigger size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="ASC">
                    <ArrowUp className="size-3.5 mr-1" />
                    ASC
                  </SelectItem>
                  <SelectItem value="DESC">
                    <ArrowDown className="size-3.5 mr-1" />
                    DESC
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="flex gap-2 justify-end">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setIsAddingClause(false);
                setSelectedTable('');
                setSelectedColumn('');
                setSelectedDirection('ASC');
              }}
            >
              Cancel
            </Button>
            <Button
              variant="default"
              size="sm"
              onClick={handleAddClause}
              disabled={!selectedTable || !selectedColumn}
            >
              Add
            </Button>
          </div>
        </div>
      ) : (
        <Button
          variant="outline"
          size="sm"
          onClick={() => setIsAddingClause(true)}
          disabled={tables.length === 0}
          className="gap-2"
        >
          <Plus className="size-4" />
          Add ORDER BY
        </Button>
      )}
    </div>
  );
};
