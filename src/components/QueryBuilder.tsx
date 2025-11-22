/**
 * Visual Query Builder Component
 *
 * Main container component that orchestrates the entire visual query builder experience.
 * Provides a split view with query builder controls and real-time SQL preview.
 */

import { FC, useState, useMemo, useCallback } from 'react';
import Editor from '@monaco-editor/react';
import {
  Play,
  RotateCcw,
  ChevronDown,
  ChevronRight,
  Copy,
  Check,
  AlertCircle,
} from 'lucide-react';
import { useTheme } from './theme-provider';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Button } from './ui/button';
import { Alert, AlertDescription, AlertTitle } from './ui/alert';
import { Badge } from './ui/badge';
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from './ui/collapsible';
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from './ui/resizable';
import { useQueryBuilder } from '@/hooks/useQueryBuilder';
import type { DbDriver } from '@/types/database';
import { TableSelector } from './queryBuilder/TableSelector';
import { ColumnSelector } from './queryBuilder/ColumnSelector';
import { JoinBuilder } from './queryBuilder/JoinBuilder';
import { FilterBuilder } from './queryBuilder/FilterBuilder';
import { GroupByPanel } from './queryBuilder/GroupByPanel';
import { OrderByPanel } from './queryBuilder/OrderByPanel';
import { LimitOffsetControls } from './queryBuilder/LimitOffsetControls';

interface QueryBuilderProps {
  /** Connection ID for fetching metadata */
  connectionId: string;

  /** Database driver type */
  driver: DbDriver;

  /** Current database context */
  currentDatabase: string | null;

  /** Callback when execute is clicked */
  onExecute?: (sql: string) => void;

  /** Whether query is currently executing */
  loading?: boolean;
}

/**
 * Section expand/collapse state
 */
interface SectionState {
  tables: boolean;
  columns: boolean;
  joins: boolean;
  filters: boolean;
  groupBy: boolean;
  orderBy: boolean;
  limitOffset: boolean;
}

export const QueryBuilder: FC<QueryBuilderProps> = ({
  connectionId,
  driver,
  currentDatabase,
  onExecute,
  loading = false,
}) => {
  const { theme } = useTheme();
  const { state, sql, validation, actions } = useQueryBuilder(driver);

  // Section expand/collapse state
  const [sections, setSections] = useState<SectionState>({
    tables: true,
    columns: true,
    joins: true,
    filters: true,
    groupBy: true,
    orderBy: true,
    limitOffset: true,
  });

  // Copy to clipboard state
  const [copied, setCopied] = useState(false);

  // Determine Monaco theme based on current theme
  const getMonacoTheme = () => {
    if (theme === 'dark') return 'vs-dark';
    if (theme === 'light') return 'vs';
    const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    return isDark ? 'vs-dark' : 'vs';
  };

  // Toggle section expand/collapse
  const toggleSection = useCallback((section: keyof SectionState) => {
    setSections((prev) => ({ ...prev, [section]: !prev[section] }));
  }, []);

  // Handle execute button click
  const handleExecute = useCallback(() => {
    if (validation.valid && onExecute) {
      onExecute(sql);
    }
  }, [validation.valid, sql, onExecute]);

  // Handle reset button click
  const handleReset = useCallback(() => {
    actions.reset();
  }, [actions]);

  // Handle copy to clipboard
  const handleCopySql = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(sql);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      console.error('Failed to copy SQL:', error);
    }
  }, [sql]);

  // Check if joins section should be shown
  const showJoins = useMemo(() => state.tables.length > 1, [state.tables.length]);

  // Check if group by section should be shown
  const showGroupBy = useMemo(() => {
    return state.columns.some((col) => col.aggregate);
  }, [state.columns]);

  // Count of items in each section
  const sectionCounts = useMemo(
    () => ({
      tables: state.tables.length,
      columns: state.columns.length,
      joins: state.joins.length,
      filters: state.where
        ? state.where.conditions.length + (state.where.groups?.length || 0)
        : 0,
      groupBy: state.groupBy.length,
      orderBy: state.orderBy.length,
      limitOffset: (state.limit ? 1 : 0) + (state.offset ? 1 : 0),
    }),
    [state]
  );

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="border-b bg-card px-6 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <h2 className="text-xl font-semibold">Visual Query Builder</h2>
            {currentDatabase && (
              <Badge variant="outline" className="text-xs">
                {currentDatabase}
              </Badge>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleReset}
                  disabled={state.tables.length === 0}
                  className="gap-2"
                >
                  <RotateCcw className="h-4 w-4" />
                  Reset
                </Button>
              </TooltipTrigger>
              <TooltipContent>Clear all query builder state</TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="default"
                  size="sm"
                  onClick={handleExecute}
                  disabled={!validation.valid || loading}
                  className="gap-2"
                >
                  <Play className="h-4 w-4" />
                  Execute Query
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {validation.valid
                  ? 'Execute the generated SQL query'
                  : 'Fix validation errors to execute'}
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
      </div>

      {/* Validation Errors */}
      {!validation.valid && validation.errors.length > 0 && (
        <div className="px-6 pt-4">
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertTitle>Validation Errors</AlertTitle>
            <AlertDescription>
              <ul className="list-disc list-inside space-y-1 mt-2">
                {validation.errors.map((error, index) => (
                  <li key={index} className="text-sm">
                    {error}
                  </li>
                ))}
              </ul>
            </AlertDescription>
          </Alert>
        </div>
      )}

      {/* Main Content - Resizable Panels */}
      <div className="flex-1 overflow-hidden">
        <ResizablePanelGroup direction="horizontal">
          {/* Query Builder Panel */}
          <ResizablePanel defaultSize={60} minSize={40}>
            <div className="h-full overflow-auto p-6 space-y-4">
              {/* Table Selection Section */}
              <Collapsible
                open={sections.tables}
                onOpenChange={() => toggleSection('tables')}
              >
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {sections.tables ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">Table Selection</CardTitle>
                        </div>
                        <Badge variant="secondary" className="text-xs">
                          {sectionCounts.tables} selected
                        </Badge>
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      <TableSelector
                        connectionId={connectionId}
                        currentDatabase={currentDatabase}
                        driver={driver}
                        selectedTables={state.tables}
                        onAddTable={actions.addTable}
                        onRemoveTable={actions.removeTable}
                      />
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>

              {/* Column Selection Section */}
              <Collapsible
                open={sections.columns}
                onOpenChange={() => toggleSection('columns')}
              >
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {sections.columns ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">Column Selection</CardTitle>
                        </div>
                        <Badge variant="secondary" className="text-xs">
                          {sectionCounts.columns || 'All columns (*)'}
                        </Badge>
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      {state.tables.length === 0 ? (
                        <p className="text-sm text-muted-foreground text-center py-4">
                          Select tables first to choose columns
                        </p>
                      ) : (
                        <ColumnSelector
                          tables={state.tables}
                          selectedColumns={state.columns}
                          onAddColumn={actions.addColumn}
                          onRemoveColumn={actions.removeColumn}
                          onUpdateColumn={actions.updateColumn}
                        />
                      )}
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>

              {/* JOIN Builder Section */}
              {showJoins && (
                <Collapsible
                  open={sections.joins}
                  onOpenChange={() => toggleSection('joins')}
                >
                  <Card>
                    <CollapsibleTrigger asChild>
                      <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-2">
                            {sections.joins ? (
                              <ChevronDown className="h-4 w-4 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="h-4 w-4 text-muted-foreground" />
                            )}
                            <CardTitle className="text-base">JOIN Conditions</CardTitle>
                          </div>
                          <Badge variant="secondary" className="text-xs">
                            {sectionCounts.joins} joins
                          </Badge>
                        </div>
                      </CardHeader>
                    </CollapsibleTrigger>
                    <CollapsibleContent>
                      <CardContent>
                        <JoinBuilder
                          connectionId={connectionId}
                          currentDatabase={currentDatabase}
                          driver={driver}
                          tables={state.tables}
                          joins={state.joins}
                          onAddJoin={actions.addJoin}
                          onRemoveJoin={actions.removeJoin}
                          onUpdateJoin={actions.updateJoin}
                        />
                      </CardContent>
                    </CollapsibleContent>
                  </Card>
                </Collapsible>
              )}

              {/* Filter Builder Section */}
              <Collapsible
                open={sections.filters}
                onOpenChange={() => toggleSection('filters')}
              >
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {sections.filters ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">Filters (WHERE)</CardTitle>
                        </div>
                        <Badge variant="secondary" className="text-xs">
                          {sectionCounts.filters} conditions
                        </Badge>
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      {state.tables.length === 0 ? (
                        <p className="text-sm text-muted-foreground text-center py-4">
                          Select tables first to add filters
                        </p>
                      ) : (
                        <FilterBuilder
                          tables={state.tables}
                          where={state.where}
                          onAddCondition={actions.addWhereCondition}
                          onRemoveCondition={actions.removeWhereCondition}
                          onSetWhere={actions.setWhere}
                        />
                      )}
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>

              {/* GROUP BY Section */}
              {showGroupBy && (
                <Collapsible
                  open={sections.groupBy}
                  onOpenChange={() => toggleSection('groupBy')}
                >
                  <Card>
                    <CollapsibleTrigger asChild>
                      <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-2">
                            {sections.groupBy ? (
                              <ChevronDown className="h-4 w-4 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="h-4 w-4 text-muted-foreground" />
                            )}
                            <CardTitle className="text-base">GROUP BY</CardTitle>
                          </div>
                          <Badge variant="secondary" className="text-xs">
                            {sectionCounts.groupBy} columns
                          </Badge>
                        </div>
                      </CardHeader>
                    </CollapsibleTrigger>
                    <CollapsibleContent>
                      <CardContent>
                        <GroupByPanel
                          tables={state.tables}
                          selectedColumns={state.columns}
                          groupBy={state.groupBy}
                          having={state.having}
                          onAddGroupBy={actions.addGroupBy}
                          onRemoveGroupBy={actions.removeGroupBy}
                          onAddHaving={actions.addHaving}
                          onRemoveHaving={actions.removeHaving}
                        />
                      </CardContent>
                    </CollapsibleContent>
                  </Card>
                </Collapsible>
              )}

              {/* ORDER BY Section */}
              <Collapsible
                open={sections.orderBy}
                onOpenChange={() => toggleSection('orderBy')}
              >
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {sections.orderBy ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">ORDER BY</CardTitle>
                        </div>
                        <Badge variant="secondary" className="text-xs">
                          {sectionCounts.orderBy} columns
                        </Badge>
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      {state.tables.length === 0 ? (
                        <p className="text-sm text-muted-foreground text-center py-4">
                          Select tables first to add sorting
                        </p>
                      ) : (
                        <OrderByPanel
                          tables={state.tables}
                          orderBy={state.orderBy}
                          onAdd={actions.addOrderBy}
                          onRemove={actions.removeOrderBy}
                          onUpdate={actions.updateOrderBy}
                        />
                      )}
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>

              {/* LIMIT/OFFSET Section */}
              <Collapsible
                open={sections.limitOffset}
                onOpenChange={() => toggleSection('limitOffset')}
              >
                <Card>
                  <CollapsibleTrigger asChild>
                    <CardHeader className="cursor-pointer hover:bg-accent/50 transition-colors pb-3">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          {sections.limitOffset ? (
                            <ChevronDown className="h-4 w-4 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-4 w-4 text-muted-foreground" />
                          )}
                          <CardTitle className="text-base">LIMIT / OFFSET</CardTitle>
                        </div>
                        {sectionCounts.limitOffset > 0 && (
                          <Badge variant="secondary" className="text-xs">
                            Configured
                          </Badge>
                        )}
                      </div>
                    </CardHeader>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <CardContent>
                      <LimitOffsetControls
                        limit={state.limit}
                        offset={state.offset}
                        onSetLimit={actions.setLimit}
                        onSetOffset={actions.setOffset}
                      />
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>
            </div>
          </ResizablePanel>

          <ResizableHandle withHandle />

          {/* SQL Preview Panel */}
          <ResizablePanel defaultSize={40} minSize={30}>
            <div className="h-full flex flex-col bg-card">
              {/* SQL Preview Header */}
              <div className="border-b px-4 py-3 flex items-center justify-between">
                <h3 className="font-semibold text-sm">Generated SQL</h3>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={handleCopySql}
                      className="h-7 w-7"
                    >
                      {copied ? (
                        <Check className="h-3.5 w-3.5 text-green-500" />
                      ) : (
                        <Copy className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    {copied ? 'Copied!' : 'Copy SQL to clipboard'}
                  </TooltipContent>
                </Tooltip>
              </div>

              {/* SQL Editor (Read-only) */}
              <div className="flex-1 overflow-hidden">
                <Editor
                  height="100%"
                  defaultLanguage="sql"
                  language="sql"
                  theme={getMonacoTheme()}
                  value={sql}
                  options={{
                    readOnly: true,
                    minimap: { enabled: false },
                    fontSize: 13,
                    lineNumbers: 'on',
                    automaticLayout: true,
                    scrollBeyondLastLine: false,
                    wordWrap: 'on',
                    wrappingStrategy: 'advanced',
                    padding: { top: 16, bottom: 16 },
                    renderLineHighlight: 'none',
                    scrollbar: {
                      vertical: 'auto',
                      horizontal: 'auto',
                    },
                  }}
                  loading={
                    <div className="flex items-center justify-center h-full">
                      <div className="text-muted-foreground text-sm">
                        Loading SQL preview...
                      </div>
                    </div>
                  }
                />
              </div>

              {/* SQL Stats */}
              <div className="border-t px-4 py-2 bg-muted/30">
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>
                    {state.tables.length} table{state.tables.length !== 1 ? 's' : ''}
                    {state.columns.length > 0 &&
                      `, ${state.columns.length} column${state.columns.length !== 1 ? 's' : ''}`}
                  </span>
                  <span>
                    {sql.split('\n').length} line{sql.split('\n').length !== 1 ? 's' : ''}
                  </span>
                </div>
              </div>
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
    </div>
  );
};
