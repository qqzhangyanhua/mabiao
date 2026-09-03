import type { PosterStat, PosterViewModel } from "../report/posterTypes";
import type { ReportDto, ReportInsight } from "../types";
import { formatCompact, formatUsdAmount, sourceLabel } from "./format";

export const REPORT_KICKER = "码表 · 周报";
export const REPORT_TOTAL_UNIT = "本周 token";

export type InsightCopy = {
  headline: string;
  comment: string;
};

const WEEKDAYS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"] as const;
const BAR_LABELS = ["一", "二", "三", "四", "五", "六", "日"] as const;
const SHARE_COLORS = [
  "#8b6cff",
  "#3b82f6",
  "#22d3ee",
  "#f59e0b",
  "#34d399",
  "#f472b6",
  "#64748b",
] as const;

export function periodRangeLabel(startDate: string, endDate: string): string {
  const start = parseDateParts(startDate);
  const end = parseDateParts(endDate);
  if (!start || !end) {
    return "";
  }
  if (start.year === end.year) {
    return `${start.year}年${start.month}月${start.day}日 – ${end.month}月${end.day}日`;
  }
  return `${start.year}年${start.month}月${start.day}日 – ${end.year}年${end.month}月${end.day}日`;
}

export function totalsComment(totalTokens: number): string {
  return `你这周烧掉了 ${formatCompact(totalTokens)} token。`;
}

export function weekdayLabel(weekday: number): string {
  return WEEKDAYS[weekday] ?? "";
}

export function hourLabel(hour: number): string {
  return `${String(hour).padStart(2, "0")}:00`;
}

export function insightCopy(insight: ReportInsight): InsightCopy {
  switch (insight.kind) {
    case "night_share": {
      if (insight.night_tokens === 0) {
        return { headline: "0%", comment: "这个周期没有在凌晨烧过 token。" };
      }
      if (insight.night_tokens === insight.total_tokens) {
        return { headline: "100%", comment: "这个周期的 token 全是凌晨烧掉的。" };
      }
      return {
        headline: `${insight.pct}%`,
        comment: `你 ${insight.pct}% 的 token 是在凌晨烧的。`,
      };
    }
    case "peak_hours": {
      const start = hourLabel(insight.start_hour);
      const end = hourLabel(insight.end_hour);
      return {
        headline: `${start} – ${end}`,
        comment: `最活跃的时段是 ${start} 到 ${end}。`,
      };
    }
    case "busiest_day": {
      const day = weekdayLabel(insight.weekday);
      return { headline: "最忙的一天", comment: day };
    }
    case "top_session": {
      const headline = insight.by === "cost" ? "最贵的一次" : "消耗最多的一次";
      const amount =
        insight.by === "cost" && insight.cost != null
          ? formatUsdAmount(insight.cost)
          : `${formatCompact(insight.total_tokens)} token`;
      const project = insight.project?.trim();
      return {
        headline,
        comment: project ? `${amount} · ${project}` : amount,
      };
    }
    default: {
      const exhausted: never = insight;
      return exhausted;
    }
  }
}

export function toPosterViewModel(dto: ReportDto): PosterViewModel | null {
  if (!dto.has_data) {
    return null;
  }
  const comments = [totalsComment(dto.totals.total_tokens)];
  const night = dto.insights.find((insight) => insight.kind === "night_share");
  const peak = dto.insights.find((insight) => insight.kind === "peak_hours");
  const busiest = dto.insights.find((insight) => insight.kind === "busiest_day");
  const topSession = dto.insights.find((insight) => insight.kind === "top_session");
  if (night) {
    comments.push(insightCopy(night).comment);
  }
  if (peak) {
    comments.push(insightCopy(peak).comment);
  }
  const stats: PosterStat[] = [];
  if (busiest) {
    const copy = insightCopy(busiest);
    stats.push({ label: copy.headline, value: copy.comment });
  }
  if (dto.models.length > 0) {
    stats.push({
      label: modelRankLabel(dto.models.length),
      value: dto.models.join(" · "),
    });
  }
  if (topSession) {
    const copy = insightCopy(topSession);
    stats.push({ label: copy.headline, value: copy.comment });
  }
  return {
    kicker: REPORT_KICKER,
    rangeLabel: periodRangeLabel(dto.start_date, dto.end_date),
    totalTokensLabel: formatCompact(dto.totals.total_tokens),
    totalUnit: REPORT_TOTAL_UNIT,
    totalCostLabel: dto.totals.cost == null ? null : formatUsdAmount(dto.totals.cost),
    comments,
    days: dto.days.map((day, index) => ({
      label: BAR_LABELS[index] ?? "",
      tokens: day.total_tokens,
    })),
    sources: dto.sources.map((slice, index) => ({
      label: sourceLabel(slice.name),
      pct: slice.pct,
      color: SHARE_COLORS[index % SHARE_COLORS.length] ?? SHARE_COLORS[0],
    })),
    stats,
  };
}

function modelRankLabel(count: number): string {
  if (count <= 1) {
    return "模型";
  }
  return `模型 Top ${count}`;
}

function parseDateParts(value: string): { year: number; month: number; day: number } | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) {
    return null;
  }
  return {
    year: Number(match[1]),
    month: Number(match[2]),
    day: Number(match[3]),
  };
}

