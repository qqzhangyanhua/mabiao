import type { EChartsOption } from "echarts";
import type {
  ApplicationEfficiency,
  ApplicationTrendPoint,
  CodeVolumeDailyPoint,
  CursorSessionDailyPoint,
  NamedAmount,
  SeriesPoint,
} from "../types";
import { chartClickDataIndex } from "./chartClick";
import { formatCompact } from "./format";
import type { ResolvedTheme } from "../hooks/useTheme";

export type ChartTheme = ResolvedTheme;

export const modelPalette = [
  "#8b6cff",
  "#3b82f6",
  "#22d3ee",
  "#64748b",
  "#f59e0b",
  "#34d399",
  "#f472b6",
];

/** 模型环图把 Top 3 之外的用量合并到这一扇区；点选时不应写入模型筛选。 */
export const MODEL_OTHER_SLICE = "其他";

const palettes: Record<
  ChartTheme,
  {
    input: string;
    output: string;
    cacheRead: string;
    cacheCreation: string;
    reasoning: string;
    axis: string;
    text: string;
    axisLabel: string;
    split: string;
    tooltipBg: string;
    tooltipBorder: string;
    tooltipText: string;
    centerValue: string;
    emptySlice: string;
  }
> = {
  dark: {
    input: "#8b6cff",
    output: "#22d3ee",
    cacheRead: "#34d399",
    cacheCreation: "#f59e0b",
    reasoning: "#f472b6",
    axis: "rgba(148, 163, 184, 0.28)",
    text: "#8b97ab",
    axisLabel: "#c9d4e5",
    split: "rgba(148, 163, 184, 0.08)",
    tooltipBg: "#121a2b",
    tooltipBorder: "rgba(255,255,255,0.08)",
    tooltipText: "#e8eef7",
    centerValue: "#f3f6fb",
    emptySlice: "rgba(148,163,184,0.18)",
  },
  light: {
    input: "#7c5cff",
    output: "#0e7490",
    cacheRead: "#047857",
    cacheCreation: "#d97706",
    reasoning: "#db2777",
    axis: "rgba(71, 85, 105, 0.25)",
    text: "#64748b",
    axisLabel: "#334155",
    split: "rgba(71, 85, 105, 0.1)",
    tooltipBg: "#ffffff",
    tooltipBorder: "rgba(15,23,42,0.1)",
    tooltipText: "#0f172a",
    centerValue: "#0f172a",
    emptySlice: "rgba(100,116,139,0.18)",
  },
};

export function chartPalette(theme: ChartTheme = "dark") {
  return palettes[theme];
}

function paletteFor(theme: ChartTheme = "dark") {
  return palettes[theme];
}

function tooltipBase(theme: ChartTheme) {
  const p = paletteFor(theme);
  return {
    backgroundColor: p.tooltipBg,
    borderColor: p.tooltipBorder,
    textStyle: { color: p.tooltipText, fontSize: 12 },
  };
}

function seriesPointTooltip(points: SeriesPoint[]) {
  return (raw: unknown): string => {
    const items = Array.isArray(raw) ? raw : [raw];
    const index = chartClickDataIndex(items[0]);
    const point = index == null ? undefined : points[index];
    if (!point) {
      return "";
    }
    const lines = [
      formatBucket(point.bucket),
      `总量 ${formatCompact(point.total_tokens)}`,
      `输入 ${formatCompact(point.input_tokens)}`,
      `输出 ${formatCompact(point.output_tokens)}`,
      `缓存 ${formatCompact(point.cache_read_tokens + point.cache_creation_tokens)}`,
      `推理 ${formatCompact(point.reasoning_tokens)}`,
    ];
    if (point.cost != null) {
      lines.push(`费用 $${point.cost.toFixed(2)}`);
    }
    return lines.join("<br/>");
  };
}

export function areaTrendOption(points: SeriesPoint[], theme: ChartTheme = "dark"): EChartsOption {
  const p = paletteFor(theme);
  const showSymbol = points.length <= 48;
  return {
    tooltip: {
      ...tooltipBase(theme),
      trigger: "axis",
      formatter: seriesPointTooltip(points),
    },
    legend: {
      data: ["输入 Token", "输出 Token"],
      top: 0,
      right: 32,
      itemWidth: 10,
      itemHeight: 10,
      textStyle: { color: p.text, fontSize: 11 },
    },
    grid: { left: 8, right: 8, top: 30, bottom: 8, containLabel: true },
    xAxis: {
      type: "category",
      boundaryGap: false,
      data: points.map((point) => formatBucket(point.bucket)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (v: number) => formatCompact(v),
      },
    },
    series: [
      {
        name: "输入 Token",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol,
        cursor: "pointer",
        data: points.map((point) => point.input_tokens),
        lineStyle: { width: 2.4, color: p.input },
        itemStyle: { color: p.input },
        areaStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(139,108,255,0.38)" },
              { offset: 1, color: "rgba(139,108,255,0.02)" },
            ],
          },
        },
      },
      {
        name: "输出 Token",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol,
        cursor: "pointer",
        data: points.map((point) => point.output_tokens),
        lineStyle: { width: 2.4, color: p.output },
        itemStyle: { color: p.output },
        areaStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(34,211,238,0.32)" },
              { offset: 1, color: "rgba(34,211,238,0.02)" },
            ],
          },
        },
      },
    ],
  };
}

export function cursorSessionDailyOption(
  points: CursorSessionDailyPoint[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = paletteFor(theme);
  return {
    tooltip: {
      ...tooltipBase(theme),
      trigger: "axis",
    },
    legend: {
      data: ["会话数", "轮次数"],
      top: 0,
      right: 32,
      itemWidth: 10,
      itemHeight: 10,
      textStyle: { color: p.text, fontSize: 11 },
    },
    grid: { left: 8, right: 8, top: 30, bottom: 8, containLabel: true },
    xAxis: {
      type: "category",
      boundaryGap: false,
      data: points.map((point) => formatBucket(point.bucket)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (v: number) => formatCompact(v),
      },
    },
    series: [
      {
        name: "会话数",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol: true,
        data: points.map((point) => point.session_count),
        lineStyle: { width: 2.4, color: p.input },
        itemStyle: { color: p.input },
      },
      {
        name: "轮次数",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol: true,
        data: points.map((point) => point.turn_count),
        lineStyle: { width: 2.4, color: p.output },
        itemStyle: { color: p.output },
      },
    ],
  };
}

export function codeVolumeDailyOption(
  points: CodeVolumeDailyPoint[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = paletteFor(theme);
  return {
    tooltip: {
      ...tooltipBase(theme),
      trigger: "axis",
    },
    legend: {
      data: ["新增行", "删除行", "AI 生成行"],
      top: 0,
      right: 32,
      itemWidth: 10,
      itemHeight: 10,
      textStyle: { color: p.text, fontSize: 11 },
    },
    grid: { left: 8, right: 8, top: 30, bottom: 8, containLabel: true },
    xAxis: {
      type: "category",
      boundaryGap: false,
      data: points.map((point) => formatBucket(point.bucket)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (v: number) => formatCompact(v),
      },
    },
    series: [
      {
        name: "新增行",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol: true,
        data: points.map((point) => point.lines_added),
        lineStyle: { width: 2.4, color: p.input },
        itemStyle: { color: p.input },
      },
      {
        name: "删除行",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol: true,
        data: points.map((point) => point.lines_deleted),
        lineStyle: { width: 2.4, color: p.output },
        itemStyle: { color: p.output },
      },
      {
        name: "AI 生成行",
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 7,
        showSymbol: true,
        data: points.map((point) => point.composer_lines_added),
        lineStyle: { width: 2.4, color: "#f59e0b" },
        itemStyle: { color: "#f59e0b" },
      },
    ],
  };
}

export function barTrendOption(points: SeriesPoint[], theme: ChartTheme = "dark"): EChartsOption {
  const p = paletteFor(theme);
  return {
    tooltip: { ...tooltipBase(theme), trigger: "axis" },
    grid: { left: 8, right: 8, top: 16, bottom: 8, containLabel: true },
    xAxis: {
      type: "category",
      data: points.map((point) => formatBucket(point.bucket)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (v: number) => formatCompact(v),
      },
    },
    series: [
      {
        type: "bar",
        name: "token",
        data: points.map((point) => point.total_tokens),
        barMaxWidth: 28,
        itemStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: "#8b6cff" },
              { offset: 1, color: "#3b82f6" },
            ],
          },
          borderRadius: [6, 6, 0, 0],
        },
      },
    ],
  };
}

export function applicationStackedTrendOption(
  points: ApplicationTrendPoint[],
  applications: ApplicationEfficiency[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = paletteFor(theme);
  return {
    tooltip: {
      ...tooltipBase(theme),
      trigger: "axis",
      axisPointer: { type: "shadow" },
    },
    legend: { show: false },
    grid: { left: 8, right: 18, top: 16, bottom: 8, containLabel: true },
    xAxis: {
      type: "category",
      data: points.map((point) => formatBucket(point.bucket)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      name: "Token",
      nameTextStyle: { color: p.text, fontSize: 11 },
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (value: number) => formatCompact(value),
      },
    },
    series: applications.map((application, index) => ({
      name: application.application,
      type: "bar",
      stack: "applications",
      barMaxWidth: 38,
      emphasis: { focus: "series" },
      itemStyle: {
        color: modelPalette[index % modelPalette.length],
        borderRadius: index === applications.length - 1 ? [5, 5, 0, 0] : 0,
      },
      data: points.map((point) => point.values[application.source] ?? 0),
    })),
  };
}

export function breakdownBarOption(
  labels: string[],
  values: number[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = paletteFor(theme);
  return {
    tooltip: { ...tooltipBase(theme), trigger: "axis", axisPointer: { type: "shadow" } },
    grid: { left: 8, right: 24, top: 8, bottom: 8, containLabel: true },
    xAxis: {
      type: "value",
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (v: number) => formatCompact(v),
      },
    },
    yAxis: {
      type: "category",
      data: labels,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: p.axisLabel, fontSize: 12 },
    },
    series: [
      {
        type: "bar",
        data: values,
        barMaxWidth: 16,
        itemStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 1,
            y2: 0,
            colorStops: [
              { offset: 0, color: "#5b4dff" },
              { offset: 1, color: "#22d3ee" },
            ],
          },
          borderRadius: [0, 8, 8, 0],
        },
      },
    ],
  };
}

export function donutOption(
  items: { name: string; value: number; color: string }[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = paletteFor(theme);
  const hasData = items.some((item) => item.value > 0);
  const slices = hasData
    ? items.filter((item) => item.value > 0)
    : [{ name: "暂无", value: 1, color: p.emptySlice }];
  return {
    tooltip: hasData
      ? {
          ...tooltipBase(theme),
          trigger: "item",
          formatter: (raw: unknown) => {
            const item = raw as { name: string; value: number; percent: number };
            return `${item.name}<br/>${formatCompact(item.value)} (${item.percent.toFixed(1)}%)`;
          },
        }
      : { show: false },
    series: [
      {
        type: "pie",
        radius: ["52%", "78%"],
        center: ["50%", "50%"],
        avoidLabelOverlap: false,
        label: { show: false },
        labelLine: { show: false },
        silent: !hasData,
        data: slices.map((item) => ({
          name: item.name,
          value: item.value,
          itemStyle: { color: item.color, borderWidth: 0 },
        })),
      },
    ],
  };
}

export function modelSlices(rows: NamedAmount[]): { name: string; value: number; color: string }[] {
  const top = rows.slice(0, 3);
  const rest = rows.slice(3);
  const items = top.map((row, i) => ({
    name: row.name,
    value: row.total_tokens,
    color: modelPalette[i] ?? "#64748b",
  }));
  const restTotal = rest.reduce((sum, row) => sum + row.total_tokens, 0);
  if (restTotal > 0) {
    items.push({ name: MODEL_OTHER_SLICE, value: restTotal, color: modelPalette[3] ?? "#64748b" });
  }
  return items;
}

export function formatBucket(bucket: string): string {
  if (/^\d{4}-\d{2}-\d{2}T\d{2}$/.test(bucket)) {
    return `${bucket.slice(5, 10)} ${bucket.slice(11, 13)}:00`;
  }
  if (/^\d{4}-\d{2}-\d{2}$/.test(bucket)) {
    return bucket.slice(5);
  }
  return bucket;
}
