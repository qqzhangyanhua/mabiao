import type { EChartsOption } from "echarts";
import type { OfficialQuotaHistoryPoint } from "../types";
import { chartPalette, formatBucket, modelPalette, type ChartTheme } from "./chartTheme";

export type QuotaHistorySeries = {
  kind: string;
  label: string;
  points: Array<{ captured_at: string; used_percent: number }>;
};

export function groupQuotaHistory(
  points: OfficialQuotaHistoryPoint[],
  provider: string,
): QuotaHistorySeries[] {
  const series = new Map<string, QuotaHistorySeries>();
  for (const point of points) {
    if (point.provider !== provider || point.used_percent == null) {
      continue;
    }
    const current = series.get(point.window_kind) ?? {
      kind: point.window_kind,
      label: point.window_label || point.window_kind,
      points: [],
    };
    current.points.push({ captured_at: point.captured_at, used_percent: point.used_percent });
    series.set(point.window_kind, current);
  }
  return [...series.values()].filter((item) => item.points.length > 0);
}

export function quotaHistoryProviders(points: OfficialQuotaHistoryPoint[]): string[] {
  return [...new Set(points.map((point) => point.provider))];
}

export function quotaHistoryOption(
  series: QuotaHistorySeries[],
  theme: ChartTheme = "dark",
): EChartsOption {
  const p = chartPalette(theme);
  const categories = [
    ...new Set(series.flatMap((item) => item.points.map((point) => point.captured_at))),
  ].sort();
  return {
    tooltip: {
      backgroundColor: p.tooltipBg,
      borderColor: p.tooltipBorder,
      textStyle: { color: p.tooltipText, fontSize: 12 },
      trigger: "axis",
    },
    legend: {
      data: series.map((item) => item.label),
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
      data: categories.map((value) => formatHistoryTick(value)),
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      axisLabel: { color: p.text, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      min: 0,
      max: 100,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: p.split } },
      axisLabel: {
        color: p.text,
        fontSize: 11,
        formatter: (value: number) => `${value}%`,
      },
    },
    series: series.map((item, index) => {
      const byTime = new Map(item.points.map((point) => [point.captured_at, point.used_percent]));
      return {
        name: item.label,
        type: "line",
        smooth: 0.35,
        symbol: "circle",
        symbolSize: 6,
        showSymbol: categories.length <= 24,
        data: categories.map((capturedAt) => byTime.get(capturedAt) ?? null),
        lineStyle: { width: 2.2, color: modelPalette[index % modelPalette.length] },
        itemStyle: { color: modelPalette[index % modelPalette.length] },
      };
    }),
  };
}

function formatHistoryTick(capturedAt: string): string {
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(capturedAt)) {
    return `${capturedAt.slice(5, 10)} ${capturedAt.slice(11, 16)}`;
  }
  return formatBucket(capturedAt);
}
