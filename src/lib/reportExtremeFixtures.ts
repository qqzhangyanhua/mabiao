import type { OverviewDto, ReportDto } from "../types";

export const emptyReportTotals: OverviewDto = {
  total_tokens: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  reasoning_tokens: 0,
  session_count: 0,
  cost: null,
  unpriced: false,
  cost_breakdown: { input: null, output: null, cache_read: null, cache_creation: null },
  cost_sources: { native: null, user: null, snapshot: null, unpriced_records: 0 },
};

const WEEK_DATES = [
  "2026-08-10",
  "2026-08-11",
  "2026-08-12",
  "2026-08-13",
  "2026-08-14",
  "2026-08-15",
  "2026-08-16",
] as const;

export function weekDays(tokensByDate: Record<string, number>) {
  return WEEK_DATES.map((date) => ({ date, total_tokens: tokensByDate[date] ?? 0 }));
}

export function extremeReportDto(overrides: Partial<ReportDto> = {}): ReportDto {
  return {
    period_kind: "week",
    offset: 0,
    start_date: "2026-08-10",
    end_date: "2026-08-16",
    has_data: true,
    totals: { ...emptyReportTotals, total_tokens: 80, session_count: 1, cost: null },
    days: weekDays({ "2026-08-12": 80 }),
    sources: [{ name: "claude", pct: 100 }],
    models: ["claude-sonnet-5"],
    insights: [
      { kind: "night_share", night_tokens: 80, total_tokens: 80, pct: 100 },
      { kind: "peak_hours", start_hour: 0, end_hour: 4 },
      { kind: "busiest_day", weekday: 2 },
      {
        kind: "top_session",
        by: "tokens",
        source: "claude",
        session_id: "only",
        project: "/proj/a",
        cost: null,
        total_tokens: 80,
      },
    ],
    ...overrides,
  };
}

export type ExtremeReportCase = {
  id: string;
  label: string;
  dto: ReportDto;
};

/** 与 Rust 组合用例同一组稀疏/极端 DTO，给 reportCopy 与本机目视共用。 */
export const EXTREME_REPORT_CASES: ExtremeReportCase[] = [
  {
    id: "single-night",
    label: "单条凌晨记录（深夜 100% / 单日 / 单来源 / 无费用）",
    dto: extremeReportDto(),
  },
  {
    id: "single-day",
    label: "只有一天有数据（周四两场 / 深夜 0% / 费用为零）",
    dto: extremeReportDto({
      totals: { ...emptyReportTotals, total_tokens: 80, session_count: 2, cost: 0 },
      days: weekDays({ "2026-08-13": 80 }),
      insights: [
        { kind: "night_share", night_tokens: 0, total_tokens: 80, pct: 0 },
        { kind: "peak_hours", start_hour: 13, end_hour: 17 },
        { kind: "busiest_day", weekday: 3 },
        {
          kind: "top_session",
          by: "tokens",
          source: "claude",
          session_id: "thu-b",
          project: "/proj/a",
          cost: 0,
          total_tokens: 50,
        },
      ],
    }),
  },
];
