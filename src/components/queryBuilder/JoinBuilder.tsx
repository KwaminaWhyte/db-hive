import { FC, useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, Trash2, Edit2, Check, X, Link2 } from 'lucide-react';
import {
  JoinCondition,
  JoinType,
  QueryTable,
} from '@/types/queryBuilder';
import type { DbDriver } from '@/types/database';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { cn } from '@/lib/utils';

interface JoinBuilderProps {
  connectionId: string;
  currentDatabase: string | null;
  driver: DbDriver;
  tables: QueryTable[];
  joins: JoinCondition[];
  onAddJoin: (join: JoinCondition) => void;
  onRemoveJoin: (id: string) => void;
  onUpdateJoin: (id: string, updates: Partial<JoinCondition>) => void;
}

interface ForeignKeyInfo {
  fromTable: string;
  fromColumn: string;
  toTable: string;
  toColumn: string;
}

interface SuggestedJoin extends ForeignKeyInfo {
  type: JoinType;
}

interface EditingJoin {
  id: string | null;
  leftTable: string;
  leftColumn: string;
  type: JoinType;
  rightTable: string;
  rightColumn: string;
}

const JOIN_TYPES: JoinType[] = ['INNER', 'LEFT', 'RIGHT', 'FULL OUTER', 'CROSS'];

const getJoinTypeBadgeColor = (type: JoinType): string => {
  switch (type) {
    case 'INNER':
      return 'bg-blue-500 text-white border-blue-600';
    case 'LEFT':
      return 'bg-green-500 text-white border-green-600';
    case 'RIGHT':
      return 'bg-orange-500 text-white border-orange-600';
    case 'FULL OUTER':
      return 'bg-purple-500 text-white border-purple-600';
    case 'CROSS':
      return 'bg-gray-500 text-white border-gray-600';
    default:
      return '';
  }
};

export const JoinBuilder: FC<JoinBuilderProps> = ({
  connectionId,
  currentDatabase,
  driver,
  tables,
  joins,
  onAddJoin,
  onRemoveJoin,
  onUpdateJoin,
}) => {
  const [isAdding, setIsAdding] = useState(false);
  const [editingJoin, setEditingJoin] = useState<EditingJoin | null>(null);
  const [foreignKeys, setForeignKeys] = useState<ForeignKeyInfo[]>([]);
  const [loadingForeignKeys, setLoadingForeignKeys] = useState(false);

  // Fetch foreign keys when tables change
  useEffect(() => {
    const fetchForeignKeys = async () => {
      if (tables.length < 2 || !currentDatabase) return;
      if (driver === 'MongoDb' || driver === 'Sqlite') {
        // SQLite and MongoDB don't support foreign keys well
        setForeignKeys([]);
        return;
      }

      setLoadingForeignKeys(true);
      try {
        const allForeignKeys: ForeignKeyInfo[] = [];

        for (const table of tables) {
          const result = await invoke<any[]>('get_foreign_keys', {
            connectionId,
            database: currentDatabase,
            schema: table.schema,
            table: table.tableName,
          });

          const fks = result.map((fk) => ({
            fromTable: table.alias,
            fromColumn: fk.column_name || fk.columnName,
            toTable: tables.find(
              (t) => t.tableName === (fk.foreign_table_name || fk.foreignTableName)
            )?.alias || '',
            toColumn: fk.foreign_column_name || fk.foreignColumnName,
          }));

          allForeignKeys.push(...fks.filter((fk) => fk.toTable));
        }

        setForeignKeys(allForeignKeys);
      } catch (error) {
        console.error('Failed to fetch foreign keys:', error);
        setForeignKeys([]);
      } finally {
        setLoadingForeignKeys(false);
      }
    };

    fetchForeignKeys();
  }, [tables, connectionId, currentDatabase, driver]);

  // Generate suggested joins from foreign keys
  const suggestedJoins = useMemo<SuggestedJoin[]>(() => {
    if (foreignKeys.length === 0) return [];

    return foreignKeys.map((fk) => ({
      ...fk,
      type: 'LEFT' as JoinType, // Default to LEFT JOIN for suggestions
    }));
  }, [foreignKeys]);

  const startAddingJoin = () => {
    if (tables.length < 2) return;

    setEditingJoin({
      id: null,
      leftTable: tables[0].alias,
      leftColumn: tables[0].columns[0]?.name || '',
      type: 'INNER',
      rightTable: tables[1].alias,
      rightColumn: tables[1].columns[0]?.name || '',
    });
    setIsAdding(true);
  };

  const startEditingJoin = (join: JoinCondition) => {
    setEditingJoin({
      id: join.id,
      leftTable: join.leftTable,
      leftColumn: join.leftColumn,
      type: join.type,
      rightTable: join.rightTable,
      rightColumn: join.rightColumn,
    });
  };

  const cancelEditing = () => {
    setEditingJoin(null);
    setIsAdding(false);
  };

  const saveJoin = () => {
    if (!editingJoin) return;

    const leftTable = tables.find((t) => t.alias === editingJoin.leftTable);
    const rightTable = tables.find((t) => t.alias === editingJoin.rightTable);

    if (!leftTable || !rightTable) {
      console.error('Invalid table selection');
      return;
    }

    // Validate columns exist
    const leftColumnExists = leftTable.columns.some(
      (c) => c.name === editingJoin.leftColumn
    );
    const rightColumnExists = rightTable.columns.some(
      (c) => c.name === editingJoin.rightColumn
    );

    // For CROSS JOIN, we don't need columns
    if (editingJoin.type !== 'CROSS' && (!leftColumnExists || !rightColumnExists)) {
      console.error('Invalid column selection');
      return;
    }

    if (editingJoin.id) {
      // Update existing join
      onUpdateJoin(editingJoin.id, {
        leftTable: editingJoin.leftTable,
        leftColumn: editingJoin.leftColumn,
        type: editingJoin.type,
        rightTable: editingJoin.rightTable,
        rightColumn: editingJoin.rightColumn,
      });
    } else {
      // Add new join
      const newJoin: JoinCondition = {
        id: `join-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
        type: editingJoin.type,
        leftTable: editingJoin.leftTable,
        leftColumn: editingJoin.leftColumn,
        rightTable: editingJoin.rightTable,
        rightColumn: editingJoin.rightColumn,
      };
      onAddJoin(newJoin);
    }

    cancelEditing();
  };

  const addSuggestedJoin = (suggested: SuggestedJoin) => {
    const newJoin: JoinCondition = {
      id: `join-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      type: suggested.type,
      leftTable: suggested.fromTable,
      leftColumn: suggested.fromColumn,
      rightTable: suggested.toTable,
      rightColumn: suggested.toColumn,
    };
    onAddJoin(newJoin);
  };

  const getTableColumns = (tableAlias: string) => {
    const table = tables.find((t) => t.alias === tableAlias);
    return table?.columns || [];
  };

  // Empty state: not enough tables
  if (tables.length < 2) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>JOIN Conditions</CardTitle>
          <CardDescription>
            Add at least 2 tables to create JOINs
          </CardDescription>
        </CardHeader>
      </Card>
    );
  }

  const showCrossJoinWarning = editingJoin?.type === 'CROSS';

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Link2 className="size-4" />
          JOIN Conditions
        </CardTitle>
        <CardDescription>
          Define how tables are joined together
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        {/* Suggested JOINs */}
        {suggestedJoins.length > 0 && !loadingForeignKeys && (
          <div className="space-y-2">
            <h4 className="text-sm font-medium text-muted-foreground">
              Suggested JOINs (based on foreign keys)
            </h4>
            <div className="flex flex-wrap gap-2">
              {suggestedJoins.map((suggested, index) => (
                <Button
                  key={`suggested-${index}`}
                  variant="outline"
                  size="sm"
                  onClick={() => addSuggestedJoin(suggested)}
                  className="text-xs"
                >
                  <Plus className="size-3" />
                  {suggested.fromTable}.{suggested.fromColumn} → {suggested.toTable}.
                  {suggested.toColumn} ({suggested.type})
                </Button>
              ))}
            </div>
          </div>
        )}

        {/* Existing JOINs */}
        {joins.length > 0 && (
          <div className="space-y-2">
            <h4 className="text-sm font-medium">Defined JOINs</h4>
            <div className="space-y-2">
              {joins.map((join) => (
                <div
                  key={join.id}
                  className="flex items-start gap-2 rounded-lg border p-3"
                >
                  {editingJoin?.id === join.id ? (
                    // Edit mode
                    <div className="flex-1 space-y-3">
                      <div className="grid grid-cols-[1fr_auto_1fr] gap-2 items-center">
                        {/* Left Table */}
                        <Select
                          value={editingJoin.leftTable}
                          onValueChange={(value) =>
                            setEditingJoin({
                              ...editingJoin,
                              leftTable: value,
                              leftColumn: getTableColumns(value)[0]?.name || '',
                            })
                          }
                        >
                          <SelectTrigger size="sm">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {tables.map((table) => (
                              <SelectItem key={table.alias} value={table.alias}>
                                {table.alias}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>

                        {/* JOIN Type */}
                        <Select
                          value={editingJoin.type}
                          onValueChange={(value) =>
                            setEditingJoin({
                              ...editingJoin,
                              type: value as JoinType,
                            })
                          }
                        >
                          <SelectTrigger size="sm">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {JOIN_TYPES.map((type) => (
                              <SelectItem key={type} value={type}>
                                {type}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>

                        {/* Right Table */}
                        <Select
                          value={editingJoin.rightTable}
                          onValueChange={(value) =>
                            setEditingJoin({
                              ...editingJoin,
                              rightTable: value,
                              rightColumn: getTableColumns(value)[0]?.name || '',
                            })
                          }
                        >
                          <SelectTrigger size="sm">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {tables.map((table) => (
                              <SelectItem key={table.alias} value={table.alias}>
                                {table.alias}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>

                      {/* Column selection (not for CROSS JOIN) */}
                      {!showCrossJoinWarning && (
                        <div className="grid grid-cols-[1fr_auto_1fr] gap-2 items-center">
                          {/* Left Column */}
                          <Select
                            value={editingJoin.leftColumn}
                            onValueChange={(value) =>
                              setEditingJoin({ ...editingJoin, leftColumn: value })
                            }
                          >
                            <SelectTrigger size="sm">
                              <SelectValue placeholder="Select column" />
                            </SelectTrigger>
                            <SelectContent>
                              {getTableColumns(editingJoin.leftTable).map((col) => (
                                <SelectItem key={col.name} value={col.name}>
                                  {col.name} ({col.dataType})
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>

                          <span className="text-sm text-muted-foreground">=</span>

                          {/* Right Column */}
                          <Select
                            value={editingJoin.rightColumn}
                            onValueChange={(value) =>
                              setEditingJoin({ ...editingJoin, rightColumn: value })
                            }
                          >
                            <SelectTrigger size="sm">
                              <SelectValue placeholder="Select column" />
                            </SelectTrigger>
                            <SelectContent>
                              {getTableColumns(editingJoin.rightTable).map((col) => (
                                <SelectItem key={col.name} value={col.name}>
                                  {col.name} ({col.dataType})
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                      )}

                      {showCrossJoinWarning && (
                        <div className="text-xs text-amber-600 bg-amber-50 dark:bg-amber-950/20 p-2 rounded">
                          CROSS JOIN creates a Cartesian product (all possible combinations)
                        </div>
                      )}

                      <div className="flex gap-2">
                        <Button size="sm" onClick={saveJoin}>
                          <Check className="size-3" />
                          Save
                        </Button>
                        <Button size="sm" variant="outline" onClick={cancelEditing}>
                          <X className="size-3" />
                          Cancel
                        </Button>
                      </div>
                    </div>
                  ) : (
                    // Display mode
                    <>
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <Badge
                            className={cn(
                              'font-mono text-xs',
                              getJoinTypeBadgeColor(join.type)
                            )}
                          >
                            {join.type}
                          </Badge>
                        </div>
                        <div className="text-sm font-mono">
                          {join.type === 'CROSS' ? (
                            <span className="text-muted-foreground">
                              {join.leftTable} × {join.rightTable}
                            </span>
                          ) : (
                            <span className="text-muted-foreground">
                              {join.leftTable}.
                              <span className="text-foreground">{join.leftColumn}</span> ={' '}
                              {join.rightTable}.
                              <span className="text-foreground">{join.rightColumn}</span>
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="flex gap-1">
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          onClick={() => startEditingJoin(join)}
                        >
                          <Edit2 className="size-3" />
                        </Button>
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          onClick={() => onRemoveJoin(join.id)}
                        >
                          <Trash2 className="size-3 text-destructive" />
                        </Button>
                      </div>
                    </>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Add new JOIN form */}
        {isAdding && editingJoin && (
          <div className="space-y-3 rounded-lg border p-3 bg-muted/50">
            <h4 className="text-sm font-medium">New JOIN</h4>

            <div className="grid grid-cols-[1fr_auto_1fr] gap-2 items-center">
              {/* Left Table */}
              <Select
                value={editingJoin.leftTable}
                onValueChange={(value) =>
                  setEditingJoin({
                    ...editingJoin,
                    leftTable: value,
                    leftColumn: getTableColumns(value)[0]?.name || '',
                  })
                }
              >
                <SelectTrigger size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {tables.map((table) => (
                    <SelectItem key={table.alias} value={table.alias}>
                      {table.alias}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              {/* JOIN Type */}
              <Select
                value={editingJoin.type}
                onValueChange={(value) =>
                  setEditingJoin({
                    ...editingJoin,
                    type: value as JoinType,
                  })
                }
              >
                <SelectTrigger size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {JOIN_TYPES.map((type) => (
                    <SelectItem key={type} value={type}>
                      {type}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              {/* Right Table */}
              <Select
                value={editingJoin.rightTable}
                onValueChange={(value) =>
                  setEditingJoin({
                    ...editingJoin,
                    rightTable: value,
                    rightColumn: getTableColumns(value)[0]?.name || '',
                  })
                }
              >
                <SelectTrigger size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {tables.map((table) => (
                    <SelectItem key={table.alias} value={table.alias}>
                      {table.alias}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Column selection (not for CROSS JOIN) */}
            {!showCrossJoinWarning && (
              <div className="grid grid-cols-[1fr_auto_1fr] gap-2 items-center">
                {/* Left Column */}
                <Select
                  value={editingJoin.leftColumn}
                  onValueChange={(value) =>
                    setEditingJoin({ ...editingJoin, leftColumn: value })
                  }
                >
                  <SelectTrigger size="sm">
                    <SelectValue placeholder="Select column" />
                  </SelectTrigger>
                  <SelectContent>
                    {getTableColumns(editingJoin.leftTable).map((col) => (
                      <SelectItem key={col.name} value={col.name}>
                        {col.name} ({col.dataType})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <span className="text-sm text-muted-foreground">=</span>

                {/* Right Column */}
                <Select
                  value={editingJoin.rightColumn}
                  onValueChange={(value) =>
                    setEditingJoin({ ...editingJoin, rightColumn: value })
                  }
                >
                  <SelectTrigger size="sm">
                    <SelectValue placeholder="Select column" />
                  </SelectTrigger>
                  <SelectContent>
                    {getTableColumns(editingJoin.rightTable).map((col) => (
                      <SelectItem key={col.name} value={col.name}>
                        {col.name} ({col.dataType})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}

            {showCrossJoinWarning && (
              <div className="text-xs text-amber-600 bg-amber-50 dark:bg-amber-950/20 p-2 rounded">
                CROSS JOIN creates a Cartesian product (all possible combinations)
              </div>
            )}

            <div className="flex gap-2">
              <Button size="sm" onClick={saveJoin}>
                <Check className="size-3" />
                Add JOIN
              </Button>
              <Button size="sm" variant="outline" onClick={cancelEditing}>
                <X className="size-3" />
                Cancel
              </Button>
            </div>
          </div>
        )}

        {/* Empty state or Add button */}
        {joins.length === 0 && !isAdding && (
          <div className="text-center py-8 text-muted-foreground">
            <p className="text-sm mb-3">No JOINs defined</p>
            <Button size="sm" onClick={startAddingJoin}>
              <Plus className="size-3" />
              Add JOIN
            </Button>
          </div>
        )}

        {joins.length > 0 && !isAdding && (
          <Button size="sm" onClick={startAddingJoin} variant="outline">
            <Plus className="size-3" />
            Add Another JOIN
          </Button>
        )}
      </CardContent>
    </Card>
  );
};
