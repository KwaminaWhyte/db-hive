import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactFlow, {
  Background,
  BackgroundVariant,
  Controls,
  Edge,
  MiniMap,
  Node,
  NodeTypes,
  Panel,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
} from 'reactflow';
import 'reactflow/dist/style.css';
import { Download, RefreshCw } from 'lucide-react';
import {
  ColumnInfo,
  ForeignKeyInfo,
  TableInfo,
  TableSchema,
} from '../types/database';
import { Button } from './ui/button';
import { Card } from './ui/card';

/**
 * Custom node component for rendering table information
 */
function TableNode({ data }: { data: TableNodeData }) {
  const { tableName, columns, primaryKeys, foreignKeys } = data;

  return (
    <Card className="min-w-[250px] shadow-lg border-2 border-amber-200">
      {/* Table header */}
      <div className="bg-linear-to-r from-amber-500 to-amber-600 text-white px-4 py-2 font-semibold rounded-t-md">
        {tableName}
      </div>

      {/* Column list */}
      <div className="divide-y divide-gray-200">
        {columns.map((col, idx) => {
          const isPk = primaryKeys.includes(col.name);
          const isFk = foreignKeys.includes(col.name);

          return (
            <div
              key={idx}
              className={`px-4 py-2 text-sm ${
                isPk ? 'bg-amber-50 font-semibold' : ''
              }`}
            >
              <div className="flex items-center gap-2">
                {isPk && (
                  <span className="text-amber-600 font-bold text-xs">PK</span>
                )}
                {isFk && !isPk && (
                  <span className="text-blue-600 font-bold text-xs">FK</span>
                )}
                <span className={isPk ? 'text-amber-900' : 'text-gray-700'}>
                  {col.name}
                </span>
              </div>
              <div className="text-xs text-gray-500 ml-8">{col.dataType}</div>
            </div>
          );
        })}
      </div>
    </Card>
  );
}

interface TableNodeData {
  tableName: string;
  columns: ColumnInfo[];
  primaryKeys: string[];
  foreignKeys: string[];
}

interface ERDiagramProps {
  connectionId: string;
  schema: string;
}

/**
 * ER Diagram Flow Component (internal - wrapped by ReactFlowProvider)
 */
function ERDiagramFlow({ connectionId, schema }: ERDiagramProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { fitView } = useReactFlow();

  const nodeTypes: NodeTypes = {
    table: TableNode,
  };

  /**
   * Fetch ER diagram data from backend
   */
  const fetchERData = useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      // 1. Fetch all tables in the schema
      const tables = await invoke<TableInfo[]>('get_tables', {
        connectionId,
        schema,
      });

      // 2. Fetch table schemas (columns) for each table
      const tableSchemas = await Promise.all(
        tables.map((table) =>
          invoke<TableSchema>('get_table_schema', {
            connectionId,
            schema,
            table: table.name,
          })
        )
      );

      // 3. Fetch foreign keys for the schema
      const foreignKeys = await invoke<ForeignKeyInfo[]>('get_foreign_keys', {
        connectionId,
        schema,
      });

      // 4. Transform data into reactflow nodes
      const flowNodes: Node[] = tableSchemas.map((tableSchema, idx) => {
        const primaryKeys = tableSchema.columns
          .filter((col) => col.isPrimaryKey)
          .map((col) => col.name);

        const foreignKeyColumns = foreignKeys
          .filter((fk) => fk.table === tableSchema.table.name)
          .flatMap((fk) => fk.columns);

        return {
          id: tableSchema.table.name,
          type: 'table',
          position: {
            x: (idx % 3) * 350,
            y: Math.floor(idx / 3) * 300,
          },
          data: {
            tableName: tableSchema.table.name,
            columns: tableSchema.columns,
            primaryKeys,
            foreignKeys: foreignKeyColumns,
          } as TableNodeData,
        };
      });

      // 5. Transform foreign keys into reactflow edges
      const flowEdges: Edge[] = foreignKeys.map((fk, idx) => {
        const label = fk.columns
          .map((col, i) => `${col} → ${fk.referencedColumns[i]}`)
          .join(', ');

        return {
          id: `${fk.name}-${idx}`,
          source: fk.table,
          target: fk.referencedTable,
          label,
          type: 'smoothstep',
          animated: true,
          style: { stroke: '#f59e0b', strokeWidth: 2 },
          labelStyle: { fontSize: 10, fontWeight: 500 },
          labelBgStyle: { fill: '#fffbeb' },
        };
      });

      setNodes(flowNodes);
      setEdges(flowEdges);

      // Auto-fit the view after a short delay
      setTimeout(() => fitView({ padding: 0.2 }), 50);
    } catch (err) {
      console.error('Failed to fetch ER diagram data:', err);
      setError(
        err instanceof Error ? err.message : 'Failed to load ER diagram'
      );
    } finally {
      setLoading(false);
    }
  }, [connectionId, schema, fitView, setNodes, setEdges]);

  // Load data on mount
  useEffect(() => {
    fetchERData();
  }, [fetchERData]);

  /**
   * Export diagram as SVG
   */
  const handleExportSVG = useCallback(() => {
    const svg = document.querySelector('.react-flow__viewport');
    if (!svg) return;

    const serializer = new XMLSerializer();
    const svgString = serializer.serializeToString(svg);
    const blob = new Blob([svgString], { type: 'image/svg+xml' });
    const url = URL.createObjectURL(blob);

    const link = document.createElement('a');
    link.href = url;
    link.download = `er-diagram-${schema}.svg`;
    link.click();

    URL.revokeObjectURL(url);
  }, [schema]);

  if (error) {
    return (
      <div className="flex items-center justify-center h-full">
        <Card className="p-6 max-w-md">
          <h3 className="text-lg font-semibold text-red-600 mb-2">
            Error Loading ER Diagram
          </h3>
          <p className="text-gray-600 mb-4">{error}</p>
          <Button onClick={fetchERData} variant="outline">
            <RefreshCw className="w-4 h-4 mr-2" />
            Retry
          </Button>
        </Card>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Card className="p-6">
          <div className="flex items-center gap-3">
            <RefreshCw className="w-5 h-5 animate-spin text-amber-600" />
            <span className="text-gray-700">Loading ER diagram...</span>
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="w-full h-full">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        fitView
        minZoom={0.1}
        maxZoom={2}
      >
        <Background variant={BackgroundVariant.Dots} gap={12} size={1} />
        <Controls />
        <MiniMap
          nodeColor={() => '#fbbf24'}
          maskColor="rgba(0, 0, 0, 0.1)"
          style={{ backgroundColor: '#fffbeb' }}
        />

        {/* Control panel */}
        <Panel position="top-right" className="bg-white/90 rounded-lg shadow-lg p-2 flex gap-2">
          <Button
            onClick={fetchERData}
            variant="outline"
            size="sm"
            title="Refresh diagram"
          >
            <RefreshCw className="w-4 h-4" />
          </Button>
          <Button
            onClick={handleExportSVG}
            variant="outline"
            size="sm"
            title="Export as SVG"
          >
            <Download className="w-4 h-4" />
          </Button>
        </Panel>
      </ReactFlow>
    </div>
  );
}

/**
 * ER Diagram Component
 *
 * Visualizes entity-relationship diagrams for a database schema.
 * Shows tables as nodes with columns, primary keys, and foreign keys.
 * Shows relationships as edges between tables.
 *
 * @param connectionId - The active database connection ID
 * @param schema - The schema to visualize
 */
export function ERDiagram({ connectionId, schema }: ERDiagramProps) {
  return (
    <ReactFlowProvider>
      <ERDiagramFlow connectionId={connectionId} schema={schema} />
    </ReactFlowProvider>
  );
}
