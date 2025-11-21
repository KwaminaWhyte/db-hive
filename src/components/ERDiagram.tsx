import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactFlow, {
  Background,
  BackgroundVariant,
  Controls,
  Edge,
  Handle,
  MarkerType,
  MiniMap,
  Node,
  NodeTypes,
  Panel,
  Position,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
} from 'reactflow';
import 'reactflow/dist/style.css';
import dagre from 'dagre';
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

  // Determine if this is a junction table (many-to-many relationship table)
  const isJunctionTable = tableName.includes('_has_') ||
                          (foreignKeys.length >= 2 && columns.length <= 5);

  // Limit columns shown to prevent overly tall nodes
  const maxColumns = 10;
  const displayColumns = columns.slice(0, maxColumns);
  const hasMoreColumns = columns.length > maxColumns;

  return (
    <Card className={`w-[300px] shadow-xl relative transition-shadow hover:shadow-2xl ${
      isJunctionTable
        ? 'border-2 border-blue-300 bg-blue-50/30'
        : 'border-2 border-amber-200 bg-white'
    }`}>
      {/* Connection handles for edges (top-to-bottom flow) */}
      <Handle
        type="target"
        position={Position.Top}
        className="w-3 h-3 !bg-amber-500 hover:!bg-amber-600 transition-colors"
        id="top"
      />
      <Handle
        type="source"
        position={Position.Bottom}
        className="w-3 h-3 !bg-amber-500 hover:!bg-amber-600 transition-colors"
        id="bottom"
      />

      {/* Table header */}
      <div className={`text-white px-4 py-2.5 font-semibold rounded-t-md ${
        isJunctionTable
          ? 'bg-linear-to-r from-blue-500 to-blue-600'
          : 'bg-linear-to-r from-amber-500 to-amber-600'
      }`}>
        <div className="flex items-center justify-between">
          <span className="truncate">{tableName}</span>
          {isJunctionTable && (
            <span className="text-xs bg-white/20 px-2 py-0.5 rounded">M:N</span>
          )}
        </div>
      </div>

      {/* Column list */}
      <div className="divide-y divide-gray-200 max-h-[400px] overflow-y-auto">
        {displayColumns.map((col, idx) => {
          const isPk = primaryKeys.includes(col.name);
          const isFk = foreignKeys.includes(col.name);

          return (
            <div
              key={idx}
              className={`px-4 py-2 text-sm transition-colors hover:bg-gray-50 ${
                isPk ? 'bg-amber-50 font-semibold' : ''
              }`}
            >
              <div className="flex items-center gap-2">
                {isPk && (
                  <span className="text-amber-600 font-bold text-xs px-1.5 py-0.5 bg-amber-100 rounded">
                    PK
                  </span>
                )}
                {isFk && !isPk && (
                  <span className="text-blue-600 font-bold text-xs px-1.5 py-0.5 bg-blue-100 rounded">
                    FK
                  </span>
                )}
                <span className={`truncate flex-1 ${isPk ? 'text-amber-900' : 'text-gray-700'}`}>
                  {col.name}
                </span>
              </div>
              <div className="text-xs text-gray-500 ml-8 truncate">{col.dataType}</div>
            </div>
          );
        })}
        {hasMoreColumns && (
          <div className="px-4 py-2 text-xs text-gray-500 italic text-center bg-gray-50">
            +{columns.length - maxColumns} more columns...
          </div>
        )}
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

// Define nodeTypes outside component to prevent re-creation on every render
const nodeTypes: NodeTypes = {
  table: TableNode,
};

/**
 * Use dagre to calculate automatic layout for nodes
 */
const getLayoutedElements = (nodes: Node[], edges: Edge[]) => {
  const dagreGraph = new dagre.graphlib.Graph();
  dagreGraph.setDefaultEdgeLabel(() => ({}));

  // Configure dagre graph with optimized spacing for database schemas
  dagreGraph.setGraph({
    rankdir: 'TB',          // Top to bottom layout
    align: 'UL',            // Align nodes to upper-left
    nodesep: 180,           // Horizontal spacing between nodes (increased)
    edgesep: 60,            // Spacing between edges
    ranksep: 250,           // Vertical spacing between ranks (increased)
    marginx: 120,           // Horizontal margin
    marginy: 120,           // Vertical margin
    acyclicer: 'greedy',    // Algorithm for handling cycles
    ranker: 'network-simplex', // Best ranker for hierarchical layouts
  });

  // Add nodes to dagre graph with realistic dimensions
  nodes.forEach((node) => {
    // Calculate height based on number of columns (capped at 10 per UI)
    const columnCount = (node.data as TableNodeData).columns?.length || 5;
    const displayedColumns = Math.min(columnCount, 10);
    const hasMoreIndicator = columnCount > 10 ? 1 : 0;

    // Header (45px) + columns (35px each) + more indicator (30px if present)
    const nodeHeight = 45 + (displayedColumns * 35) + (hasMoreIndicator * 30);
    const nodeWidth = 300; // Wider for better readability

    dagreGraph.setNode(node.id, {
      width: nodeWidth,
      height: Math.max(nodeHeight, 150) // Minimum 150px
    });
  });

  // Add edges to dagre graph
  edges.forEach((edge) => {
    dagreGraph.setEdge(edge.source, edge.target);
  });

  // Calculate layout
  dagre.layout(dagreGraph);

  // Apply calculated positions to nodes
  const layoutedNodes = nodes.map((node) => {
    const nodeWithPosition = dagreGraph.node(node.id);
    const nodeWidth = nodeWithPosition.width;
    const nodeHeight = nodeWithPosition.height;

    return {
      ...node,
      position: {
        x: nodeWithPosition.x - nodeWidth / 2,  // Center horizontally
        y: nodeWithPosition.y - nodeHeight / 2, // Center vertically
      },
    };
  });

  return { nodes: layoutedNodes, edges };
};

/**
 * ER Diagram Flow Component (internal - wrapped by ReactFlowProvider)
 */
function ERDiagramFlow({ connectionId, schema }: ERDiagramProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { fitView } = useReactFlow();

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

      if (tables.length === 0) {
        setError('No tables found in this schema');
        setLoading(false);
        return;
      }

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
      const flowNodes: Node[] = tableSchemas.map((tableSchema) => {
        const primaryKeys = tableSchema.columns
          .filter((col) => col.isPrimaryKey)
          .map((col) => col.name);

        const foreignKeyColumns = foreignKeys
          .filter((fk) => fk.table === tableSchema.table.name)
          .flatMap((fk) => fk.columns);

        return {
          id: tableSchema.table.name,
          type: 'table',
          position: { x: 0, y: 0 }, // Will be overwritten by dagre
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
          sourceHandle: 'bottom',
          target: fk.referencedTable,
          targetHandle: 'top',
          label,
          type: 'smoothstep',
          animated: true,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color: '#f59e0b',
          },
          style: { stroke: '#f59e0b', strokeWidth: 2 },
          labelStyle: { fontSize: 11, fontWeight: 600, fill: '#78350f' },
          labelBgStyle: { fill: '#fffbeb', fillOpacity: 0.95 },
        };
      });

      // 6. Apply dagre layout
      const { nodes: layoutedNodes, edges: layoutedEdges } = getLayoutedElements(
        flowNodes,
        flowEdges
      );

      setNodes(layoutedNodes);
      setEdges(layoutedEdges);

      // Auto-fit the view after a short delay
      setTimeout(() => {
        fitView({ padding: 0.2, duration: 800 });
      }, 150);
    } catch (err) {
      console.error('Failed to fetch ER diagram data:', err);
      setError(
        err instanceof Error ? err.message : 'Failed to load ER diagram'
      );
    } finally {
      setLoading(false);
    }
  }, [connectionId, schema, fitView]);

  // Load data on mount and when connection/schema changes
  useEffect(() => {
    fetchERData();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId, schema]);

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
        minZoom={0.05}
        maxZoom={1.5}
        defaultEdgeOptions={{
          type: 'smoothstep',
          animated: false, // Disable animation for better performance with many edges
        }}
        proOptions={{ hideAttribution: true }}
        nodesDraggable={true}
        nodesConnectable={false}
        elementsSelectable={true}
        selectNodesOnDrag={false}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={16}
          size={1.5}
          color="#f59e0b"
          style={{ opacity: 0.15 }}
        />
        <Controls
          showZoom
          showFitView
          showInteractive={false}
        />
        <MiniMap
          nodeColor={(node) => {
            const data = node.data as TableNodeData;
            const isJunction = data.tableName.includes('_has_') ||
                              (data.foreignKeys?.length >= 2 && data.columns?.length <= 5);
            return isJunction ? '#3b82f6' : '#f59e0b';
          }}
          nodeStrokeWidth={3}
          maskColor="rgba(0, 0, 0, 0.15)"
          style={{
            backgroundColor: '#fffbeb',
            border: '2px solid #fbbf24',
            borderRadius: '8px'
          }}
        />

        {/* Control panel */}
        <Panel position="top-right" className="bg-white/95 backdrop-blur-sm rounded-lg shadow-xl p-2 flex gap-2 border border-gray-200">
          <Button
            onClick={fetchERData}
            variant="outline"
            size="sm"
            title="Refresh diagram"
            className="hover:bg-amber-50"
          >
            <RefreshCw className="w-4 h-4" />
          </Button>
          <Button
            onClick={handleExportSVG}
            variant="outline"
            size="sm"
            title="Export as SVG"
            className="hover:bg-amber-50"
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
 * Uses dagre for automatic hierarchical layout.
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
