import type { PosterViewModel } from "../report/posterTypes";
import type { ReportDto, ReportInsight } from "../types";
import { formatCompact, formatUsdAmount } from "./format";

export const REPORT_KICKER = "码表 · 周报";
export const REPORT_TOTAL_UNIT = "本周 token";

export type InsightCopy = {
  headline: string;
  comment: string;
};

const WEEKDAYS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"] as const;

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
  if (night) {
    comments.push(insightCopy(night).comment);
  }
  if (peak) {
    comments.push(insightCopy(peak).comment);
  }
  return {
    kicker: REPORT_KICKER,
    rangeLabel: periodRangeLabel(dto.start_date, dto.end_date),
    totalTokensLabel: formatCompact(dto.totals.total_tokens),
    totalUnit: REPORT_TOTAL_UNIT,
    totalCostLabel: dto.totals.cost == null ? null : formatUsdAmount(dto.totals.cost),
    comments,
    days: [],
    sources: [],
    stats: [],
  };
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
