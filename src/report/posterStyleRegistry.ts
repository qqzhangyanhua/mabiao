import type { ComponentType, Ref } from "react";
import { BauhausPoster } from "./bauhausPoster";
import { CastConcretePoster } from "./castConcretePoster";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import { FuseBeadPoster } from "./fuseBeadPoster";
import { InkWashPoster } from "./inkWashPoster";
import { LightGlassPoster } from "./lightGlassPoster";
import { NewsprintPoster } from "./newsprintPoster";
import { TicketStubPoster } from "./ticketStubPoster";
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
  {
    id: "light-glass",
    label: "浅色磨砂",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "lightGlassPoster.css",
    swatch: {
      background: "#eef6f8",
      accent: "#3d9aa8",
    },
    Component: LightGlassPoster,
  },
  {
    id: "bauhaus-print",
    label: "构成海报",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "bauhausPoster.css",
    swatch: {
      background: "#f6f1e6",
      accent: "#e30613",
    },
    Component: BauhausPoster,
  },
  {
    id: "newsprint",
    label: "旧报号外",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "newsprintPoster.css",
    swatch: {
      background: "#e7d6b4",
      accent: "#1c1610",
    },
    Component: NewsprintPoster,
  },
  {
    id: "ink-wash",
    label: "水墨手札",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "inkWashPoster.css",
    swatch: {
      background: "#f4efe6",
      accent: "#9c3b32",
    },
    Component: InkWashPoster,
  },
  {
    id: "ticket-stub",
    label: "票据存根",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "ticketStubPoster.css",
    swatch: {
      background: "#f3ead8",
      accent: "#c45c4a",
    },
    Component: TicketStubPoster,
  },
  {
    id: "fuse-bead",
    label: "拼豆海报",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "fuseBeadPoster.css",
    swatch: {
      background: "#eef0f5",
      accent: "#8b5cf6",
    },
    Component: FuseBeadPoster,
  },
  {
    id: "cast-concrete",
    label: "清水混凝土",
    /** 相对 `src/report/` 的样式表；CSS 门禁从注册表读这份清单，不要另维护文件列表。 */
    stylesheet: "castConcretePoster.css",
    swatch: {
      background: "#b6b5af",
      accent: "#7a7872",
    },
    Component: CastConcretePoster,
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
  return (
    REPORT_POSTER_STYLE_BY_ID.get(resolveReportPosterStyleId(value)) ?? REPORT_POSTER_STYLES[0]
  );
}
