import type { ComponentType, Ref } from "react";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import type { PosterViewModel } from "./posterTypes";

export type ReportPosterRenderProps = {
  data: PosterViewModel;
  posterRef?: Ref<HTMLElement | null>;
  posterId?: string;
};

/** 开发者维护的周报海报风格 id。不是运行时插件键。 */
export const DEFAULT_REPORT_POSTER_STYLE_ID = "dark-analytics" as const;

export type ReportPosterStyleId = typeof DEFAULT_REPORT_POSTER_STYLE_ID;

/** 选择器色块：固定色板，不跟随应用主题。 */
export type ReportPosterStyleSwatch = {
  background: string;
  accent: string;
};

export type ReportPosterStyleComponent = ComponentType<ReportPosterRenderProps>;

export type ReportPosterStyle = {
  id: ReportPosterStyleId;
  label: string;
  swatch: ReportPosterStyleSwatch;
  Component: ReportPosterStyleComponent;
};

const DARK_ANALYTICS_STYLE: ReportPosterStyle = {
  id: "dark-analytics",
  label: "深色分析",
  swatch: {
    background: "#070b16",
    accent: "#8b6cff",
  },
  Component: DarkAnalyticsPoster,
};

export const REPORT_POSTER_STYLES: readonly ReportPosterStyle[] = [DARK_ANALYTICS_STYLE];

const REPORT_POSTER_STYLE_BY_ID = new Map<string, ReportPosterStyle>(
  REPORT_POSTER_STYLES.map((style) => [style.id, style]),
);

export function isReportPosterStyleId(value: unknown): value is ReportPosterStyleId {
  return typeof value === "string" && REPORT_POSTER_STYLE_BY_ID.has(value);
}

/** 缺省、空串或未知 id 一律回退默认风格，避免偏好损坏后无法出图。 */
export function resolveReportPosterStyleId(value: unknown): ReportPosterStyleId {
  return isReportPosterStyleId(value) ? value : DEFAULT_REPORT_POSTER_STYLE_ID;
}

export function resolveReportPosterStyle(value: unknown): ReportPosterStyle {
  return REPORT_POSTER_STYLE_BY_ID.get(resolveReportPosterStyleId(value)) ?? DARK_ANALYTICS_STYLE;
}
