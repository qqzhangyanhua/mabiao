import type { ComponentType, Ref } from "react";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import type { PosterViewModel } from "./posterTypes";

export type ReportPosterRenderProps = {
  data: PosterViewModel;
  posterRef?: Ref<HTMLElement | null>;
  posterId?: string;
};

/** 选择器色块：固定色板，不跟随应用主题。 */
export type ReportPosterStyleSwatch = {
  background: string;
  accent: string;
};

export type ReportPosterStyleComponent = ComponentType<ReportPosterRenderProps>;

export const REPORT_POSTER_STYLES = [
  {
    id: "dark-analytics",
    label: "深色分析",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "poster.css",
    swatch: {
      background: "#070b16",
      accent: "#8b6cff",
    },
    Component: DarkAnalyticsPoster,
  },
] as const;

/** 开发者维护的周报海报风格 id。新增内置风格时往 `REPORT_POSTER_STYLES` 加一行即可。 */
export type ReportPosterStyleId = (typeof REPORT_POSTER_STYLES)[number]["id"];

export const DEFAULT_REPORT_POSTER_STYLE_ID = "dark-analytics" satisfies ReportPosterStyleId;

export type ReportPosterStyle = {
  id: ReportPosterStyleId;
  label: string;
  stylesheet: string;
  swatch: ReportPosterStyleSwatch;
  Component: ReportPosterStyleComponent;
};

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
  return REPORT_POSTER_STYLE_BY_ID.get(resolveReportPosterStyleId(value)) ?? REPORT_POSTER_STYLES[0];
}
