import { useState, useMemo, useCallback } from "react";
import {
  BarChart,
  Bar,
  LineChart,
  Line,
  PieChart,
  Pie,
  ScatterChart,
  Scatter,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  Cell,
  Area,
  AreaChart,
} from "recharts";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { Label } from "./ui/label";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import {
  BarChart3,
  LineChart as LineChartIcon,
  PieChart as PieChartIcon,
  ScatterChart as ScatterChartIcon,
  AreaChart as AreaChartIcon,
  Settings2,
  Download,
} from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "./ui/popover";

interface ResultsChartProps {
  columns: string[];
  rows: any[][];
}

type ChartType = "bar" | "line" | "pie" | "scatter" | "area";

const CHART_COLORS = [
  "#3b82f6", // blue
  "#10b981", // emerald
  "#f59e0b", // amber
  "#ef4444", // red
  "#8b5cf6", // violet
  "#ec4899", // pink
  "#06b6d4", // cyan
  "#84cc16", // lime
  "#f97316", // orange
  "#6366f1", // indigo
];

export function ResultsChart({ columns, rows }: ResultsChartProps) {
  const [chartType, setChartType] = useState<ChartType>("bar");
  const [xAxisColumn, setXAxisColumn] = useState<string>("");
  const [yAxisColumns, setYAxisColumns] = useState<string[]>([]);
  const [showGrid, setShowGrid] = useState(true);
  const [showLegend, setShowLegend] = useState(true);

  // Analyze columns to detect numeric columns
  const columnAnalysis = useMemo(() => {
    const analysis: Record<string, { isNumeric: boolean; sample: any[] }> = {};

    columns.forEach((col, colIdx) => {
      const values = rows.slice(0, 100).map((row) => row[colIdx]);
      const numericValues = values.filter(
        (v) => v !== null && !isNaN(Number(v))
      );
      const isNumeric = numericValues.length > values.length * 0.5;

      analysis[col] = {
        isNumeric,
        sample: values.slice(0, 5),
      };
    });

    return analysis;
  }, [columns, rows]);

  // Get numeric and categorical columns
  const numericColumns = useMemo(
    () => columns.filter((col) => columnAnalysis[col]?.isNumeric),
    [columns, columnAnalysis]
  );

  const categoricalColumns = useMemo(
    () => columns.filter((col) => !columnAnalysis[col]?.isNumeric),
    [columns, columnAnalysis]
  );

  // Auto-select columns on mount
  useMemo(() => {
    if (!xAxisColumn && columns.length > 0) {
      // Prefer categorical for X axis, or first column
      const defaultX = categoricalColumns[0] || columns[0];
      setXAxisColumn(defaultX);
    }

    if (yAxisColumns.length === 0 && numericColumns.length > 0) {
      // Select first numeric column for Y axis
      setYAxisColumns([numericColumns[0]]);
    }
  }, [columns, categoricalColumns, numericColumns]);

  // Transform data for charts
  const chartData = useMemo(() => {
    if (!xAxisColumn) return [];

    const xIdx = columns.indexOf(xAxisColumn);
    if (xIdx === -1) return [];

    // Group data by X axis value for aggregation
    const grouped = new Map<string, Record<string, number>>();

    rows.forEach((row) => {
      const xValue = String(row[xIdx] ?? "null");

      if (!grouped.has(xValue)) {
        grouped.set(xValue, {});
      }

      const entry = grouped.get(xValue)!;

      yAxisColumns.forEach((yCol) => {
        const yIdx = columns.indexOf(yCol);
        if (yIdx !== -1) {
          const yValue = Number(row[yIdx]) || 0;
          entry[yCol] = (entry[yCol] || 0) + yValue;
        }
      });
    });

    // Convert to array format
    const data = Array.from(grouped.entries()).map(([xValue, yValues]) => ({
      name: xValue,
      ...yValues,
    }));

    // Limit to reasonable number of data points
    return data.slice(0, 100);
  }, [columns, rows, xAxisColumn, yAxisColumns]);

  // Pie chart specific data (only uses first Y column)
  const pieData = useMemo(() => {
    if (chartType !== "pie" || !xAxisColumn || yAxisColumns.length === 0)
      return [];

    const yCol = yAxisColumns[0];
    return chartData
      .map((item) => ({
        name: item.name,
        value: (item as Record<string, number | string>)[yCol] || 0,
      }))
      .slice(0, 10); // Limit pie slices
  }, [chartType, chartData, xAxisColumn, yAxisColumns]);

  const toggleYColumn = useCallback((col: string) => {
    setYAxisColumns((prev) => {
      if (prev.includes(col)) {
        return prev.filter((c) => c !== col);
      }
      return [...prev, col];
    });
  }, []);

  const handleExportChart = useCallback(() => {
    // Get the SVG element from the chart
    const chartContainer = document.querySelector(".recharts-wrapper svg");
    if (!chartContainer) return;

    // Create a canvas to convert SVG to PNG
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Get SVG data
    const svgData = new XMLSerializer().serializeToString(chartContainer);
    const svgBlob = new Blob([svgData], {
      type: "image/svg+xml;charset=utf-8",
    });
    const url = URL.createObjectURL(svgBlob);

    const img = new Image();
    img.onload = () => {
      canvas.width = img.width * 2;
      canvas.height = img.height * 2;
      ctx.scale(2, 2);
      ctx.fillStyle = "#18181b";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(img, 0, 0);
      URL.revokeObjectURL(url);

      // Download as PNG
      const pngUrl = canvas.toDataURL("image/png");
      const link = document.createElement("a");
      link.download = `chart-${Date.now()}.png`;
      link.href = pngUrl;
      link.click();
    };
    img.src = url;
  }, []);

  const renderChart = () => {
    if (chartData.length === 0 || yAxisColumns.length === 0) {
      return (
        <div className="flex-1 flex items-center justify-center text-muted-foreground">
          <p>Select X and Y axis columns to visualize data</p>
        </div>
      );
    }

    const commonProps = {
      data: chartData,
      margin: { top: 20, right: 30, left: 20, bottom: 60 },
    };

    switch (chartType) {
      case "bar":
        return (
          <ResponsiveContainer width="100%" height="100%">
            <BarChart {...commonProps}>
              {showGrid && <CartesianGrid strokeDasharray="3 3" stroke="#333" />}
              <XAxis
                dataKey="name"
                tick={{ fill: "#888", fontSize: 11 }}
                angle={-45}
                textAnchor="end"
                height={80}
              />
              <YAxis tick={{ fill: "#888", fontSize: 11 }} />
              <Tooltip
                contentStyle={{
                  backgroundColor: "#27272a",
                  border: "1px solid #3f3f46",
                  borderRadius: "8px",
                }}
              />
              {showLegend && <Legend />}
              {yAxisColumns.map((col, idx) => (
                <Bar
                  key={col}
                  dataKey={col}
                  fill={CHART_COLORS[idx % CHART_COLORS.length]}
                  radius={[4, 4, 0, 0]}
                />
              ))}
            </BarChart>
          </ResponsiveContainer>
        );

      case "line":
        return (
          <ResponsiveContainer width="100%" height="100%">
            <LineChart {...commonProps}>
              {showGrid && <CartesianGrid strokeDasharray="3 3" stroke="#333" />}
              <XAxis
                dataKey="name"
                tick={{ fill: "#888", fontSize: 11 }}
                angle={-45}
                textAnchor="end"
                height={80}
              />
              <YAxis tick={{ fill: "#888", fontSize: 11 }} />
              <Tooltip
                contentStyle={{
                  backgroundColor: "#27272a",
                  border: "1px solid #3f3f46",
                  borderRadius: "8px",
                }}
              />
              {showLegend && <Legend />}
              {yAxisColumns.map((col, idx) => (
                <Line
                  key={col}
                  type="monotone"
                  dataKey={col}
                  stroke={CHART_COLORS[idx % CHART_COLORS.length]}
                  strokeWidth={2}
                  dot={{ fill: CHART_COLORS[idx % CHART_COLORS.length] }}
                />
              ))}
            </LineChart>
          </ResponsiveContainer>
        );

      case "area":
        return (
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart {...commonProps}>
              {showGrid && <CartesianGrid strokeDasharray="3 3" stroke="#333" />}
              <XAxis
                dataKey="name"
                tick={{ fill: "#888", fontSize: 11 }}
                angle={-45}
                textAnchor="end"
                height={80}
              />
              <YAxis tick={{ fill: "#888", fontSize: 11 }} />
              <Tooltip
                contentStyle={{
                  backgroundColor: "#27272a",
                  border: "1px solid #3f3f46",
                  borderRadius: "8px",
                }}
              />
              {showLegend && <Legend />}
              {yAxisColumns.map((col, idx) => (
                <Area
                  key={col}
                  type="monotone"
                  dataKey={col}
                  stroke={CHART_COLORS[idx % CHART_COLORS.length]}
                  fill={CHART_COLORS[idx % CHART_COLORS.length]}
                  fillOpacity={0.3}
                />
              ))}
            </AreaChart>
          </ResponsiveContainer>
        );

      case "pie":
        return (
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <Pie
                data={pieData}
                cx="50%"
                cy="50%"
                labelLine={false}
                label={({ name, percent }) =>
                  `${name} (${((percent ?? 0) * 100).toFixed(0)}%)`
                }
                outerRadius={150}
                fill="#8884d8"
                dataKey="value"
              >
                {pieData.map((_, index) => (
                  <Cell
                    key={`cell-${index}`}
                    fill={CHART_COLORS[index % CHART_COLORS.length]}
                  />
                ))}
              </Pie>
              <Tooltip
                contentStyle={{
                  backgroundColor: "#27272a",
                  border: "1px solid #3f3f46",
                  borderRadius: "8px",
                }}
              />
              {showLegend && <Legend />}
            </PieChart>
          </ResponsiveContainer>
        );

      case "scatter":
        return (
          <ResponsiveContainer width="100%" height="100%">
            <ScatterChart margin={{ top: 20, right: 30, left: 20, bottom: 20 }}>
              {showGrid && <CartesianGrid strokeDasharray="3 3" stroke="#333" />}
              <XAxis
                dataKey="name"
                name={xAxisColumn}
                tick={{ fill: "#888", fontSize: 11 }}
              />
              <YAxis
                dataKey={yAxisColumns[0]}
                name={yAxisColumns[0]}
                tick={{ fill: "#888", fontSize: 11 }}
              />
              <Tooltip
                cursor={{ strokeDasharray: "3 3" }}
                contentStyle={{
                  backgroundColor: "#27272a",
                  border: "1px solid #3f3f46",
                  borderRadius: "8px",
                }}
              />
              {showLegend && <Legend />}
              <Scatter
                name={yAxisColumns[0] || "Data"}
                data={chartData}
                fill={CHART_COLORS[0]}
              />
            </ScatterChart>
          </ResponsiveContainer>
        );

      default:
        return null;
    }
  };

  if (columns.length === 0 || rows.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-muted-foreground p-8">
        <p>No data available for visualization</p>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* Chart Controls */}
      <div className="flex items-center gap-4 p-4 border-b bg-muted/30">
        {/* Chart Type Selector */}
        <div className="flex items-center gap-2">
          <Button
            variant={chartType === "bar" ? "default" : "outline"}
            size="sm"
            onClick={() => setChartType("bar")}
            title="Bar Chart"
          >
            <BarChart3 className="h-4 w-4" />
          </Button>
          <Button
            variant={chartType === "line" ? "default" : "outline"}
            size="sm"
            onClick={() => setChartType("line")}
            title="Line Chart"
          >
            <LineChartIcon className="h-4 w-4" />
          </Button>
          <Button
            variant={chartType === "area" ? "default" : "outline"}
            size="sm"
            onClick={() => setChartType("area")}
            title="Area Chart"
          >
            <AreaChartIcon className="h-4 w-4" />
          </Button>
          <Button
            variant={chartType === "pie" ? "default" : "outline"}
            size="sm"
            onClick={() => setChartType("pie")}
            title="Pie Chart"
          >
            <PieChartIcon className="h-4 w-4" />
          </Button>
          <Button
            variant={chartType === "scatter" ? "default" : "outline"}
            size="sm"
            onClick={() => setChartType("scatter")}
            title="Scatter Chart"
          >
            <ScatterChartIcon className="h-4 w-4" />
          </Button>
        </div>

        <div className="h-6 w-px bg-border" />

        {/* X Axis Selector */}
        <div className="flex items-center gap-2">
          <Label className="text-xs text-muted-foreground whitespace-nowrap">
            X Axis:
          </Label>
          <Select value={xAxisColumn} onValueChange={setXAxisColumn}>
            <SelectTrigger className="w-[140px] h-8 bg-background">
              <SelectValue placeholder="Select column" />
            </SelectTrigger>
            <SelectContent>
              {columns.filter(Boolean).map((col) => (
                <SelectItem key={col} value={col}>
                  {col}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {/* Y Axis Selector */}
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline" size="sm" className="h-8">
              Y Axis: {yAxisColumns.length > 0 ? yAxisColumns.join(", ") : "None"}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-56" align="start">
            <div className="space-y-2">
              <p className="text-sm font-medium">Select Y Axis Columns</p>
              <p className="text-xs text-muted-foreground mb-3">
                Select one or more numeric columns
              </p>
              <div className="space-y-2 max-h-[200px] overflow-auto">
                {numericColumns.length > 0 ? (
                  numericColumns.filter(Boolean).map((col) => (
                    <div
                      key={col}
                      className="flex items-center gap-2 cursor-pointer"
                      onClick={() => toggleYColumn(col)}
                    >
                      <Checkbox
                        id={`y-${col}`}
                        checked={yAxisColumns.includes(col)}
                        onCheckedChange={() => toggleYColumn(col)}
                      />
                      <label
                        htmlFor={`y-${col}`}
                        className="text-sm cursor-pointer flex-1"
                      >
                        {col}
                      </label>
                    </div>
                  ))
                ) : (
                  <p className="text-xs text-muted-foreground">
                    No numeric columns found
                  </p>
                )}
              </div>
            </div>
          </PopoverContent>
        </Popover>

        <div className="flex-1" />

        {/* Chart Options */}
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline" size="sm" className="h-8">
              <Settings2 className="h-4 w-4" />
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-48" align="end">
            <div className="space-y-3">
              <p className="text-sm font-medium">Chart Options</p>
              <div className="flex items-center gap-2">
                <Checkbox
                  id="showGrid"
                  checked={showGrid}
                  onCheckedChange={(checked) => setShowGrid(!!checked)}
                />
                <label htmlFor="showGrid" className="text-sm cursor-pointer">
                  Show Grid
                </label>
              </div>
              <div className="flex items-center gap-2">
                <Checkbox
                  id="showLegend"
                  checked={showLegend}
                  onCheckedChange={(checked) => setShowLegend(!!checked)}
                />
                <label htmlFor="showLegend" className="text-sm cursor-pointer">
                  Show Legend
                </label>
              </div>
            </div>
          </PopoverContent>
        </Popover>

        {/* Export Button */}
        <Button
          variant="outline"
          size="sm"
          className="h-8"
          onClick={handleExportChart}
          title="Export chart as PNG"
        >
          <Download className="h-4 w-4" />
        </Button>
      </div>

      {/* Chart Area */}
      <div className="flex-1 p-4">{renderChart()}</div>
    </div>
  );
}
