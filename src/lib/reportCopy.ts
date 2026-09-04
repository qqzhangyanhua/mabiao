import type { PosterStat, PosterViewModel } from "../report/posterTypes";
import type { ReportDto, ReportInsight, ReportPeriodKind } from "../types";
import { formatCompact, formatUsdAmount, projectLabel, sourceLabel } from "./format";
import { CUSTOM_PERIOD_MAX_DAYS } from "./reportPeriod";

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

function periodPhrases(periodKind: ReportPeriodKind): {
  kicker: string;
  totalUnit: string;
  burned: string;
} {
  if (periodKind === "month") {
    return { kicker: "码表 · 月报", totalUnit: "本月 token", burned: "这月" };
  }
  if (periodKind === "custom") {
    return { kicker: "码表 · 区间", totalUnit: "区间 token", burned: "这段时间" };
  }
  return { kicker: REPORT_KICKER, totalUnit: REPORT_TOTAL_UNIT, burned: "这周" };
}

export function totalsComment(
  totalTokens: number,
  periodKind: ReportPeriodKind = "week",
): string {
  return `你${periodPhrases(periodKind).burned}烧掉了 ${formatCompact(totalTokens)} token。`;
}

export function periodStatusCopy(periodKind: ReportPeriodKind): {
  loading: string;
  failed: string;
  help: string;
  emptyHint: string;
} {
  if (periodKind === "month") {
    return {
      loading: "正在生成月报…",
      failed: "月报加载失败",
      help: "选已经结束的自然月。点一天即取该月；进行中的一个月不可选。",
      emptyHint: "不会生成空海报。可以改日期，或往前切到更早的周期。",
    };
  }
  if (periodKind === "custom") {
    return {
      loading: "正在生成区间报告…",
      failed: "区间报告加载失败",
      help: `起止日都含在内，可选到今天。最长 ${CUSTOM_PERIOD_MAX_DAYS} 天。`,
      emptyHint: "不会生成空海报。可以改起止日期。",
    };
  }
  return {
    loading: "正在生成周报…",
    failed: "周报加载失败",
    help: "选已经结束的自然周。点一天即取该周；进行中的一周不可选。",
    emptyHint: "不会生成空海报。可以改日期，或往前切到更早的周期。",
  };
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
        comment: project ? `${amount} · ${projectLabel(project)}` : amount,
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
  const comments = [totalsComment(dto.totals.total_tokens, dto.period_kind)];
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
  const phrases = periodPhrases(dto.period_kind);
  return {
    kicker: phrases.kicker,
    rangeLabel: periodRangeLabel(dto.start_date, dto.end_date),
    totalTokensLabel: formatCompact(dto.totals.total_tokens),
    totalUnit: phrases.totalUnit,
    totalCostLabel:
      dto.totals.cost != null && dto.totals.cost > 0 ? formatUsdAmount(dto.totals.cost) : null,
    comments,
    days: dto.days.map((day, index) => ({
      label: barLabel(dto, day.date, index),
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

function barLabel(dto: ReportDto, date: string, index: number): string {
  if (dto.period_kind === "week") {
    return BAR_LABELS[index] ?? "";
  }
  const parts = parseDateParts(date);
  return parts ? String(parts.day) : "";
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
