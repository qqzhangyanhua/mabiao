import { describe, expect, it } from "vitest";
import type { OverviewDto, ReportDto, ReportInsight } from "../types";
import {
  hourLabel,
  insightCopy,
  periodRangeLabel,
  toPosterViewModel,
  totalsComment,
  weekdayLabel,
} from "./reportCopy";

const emptyTotals: OverviewDto = {
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

function dto(partial: Partial<ReportDto> & Pick<ReportDto, "has_data" | "totals">): ReportDto {
  return {
    period_kind: "week",
    offset: 0,
    start_date: "2026-08-10",
    end_date: "2026-08-16",
    days: [],
    sources: [],
    models: [],
    insights: [],
    ...partial,
  };
}

describe("periodRangeLabel", () => {
  it("omits the end year when the range stays in one year", () => {
    expect(periodRangeLabel("2026-08-10", "2026-08-16")).toBe("2026年8月10日 – 8月16日");
  });

  it("keeps both years when the range crosses New Year", () => {
    expect(periodRangeLabel("2025-12-29", "2026-01-04")).toBe("2025年12月29日 – 2026年1月4日");
  });
});

describe("totalsComment", () => {
  it("uses compact tokens in a second-person sentence", () => {
    expect(totalsComment(12_400_000)).toBe("你这周烧掉了 12.4M token。");
  });
});

describe("insightCopy", () => {
  it("uses token zeros rather than rounded percent for night share extremes", () => {
    const none: ReportInsight = { kind: "night_share", night_tokens: 0, total_tokens: 100, pct: 0 };
    const all: ReportInsight = {
      kind: "night_share",
      night_tokens: 100,
      total_tokens: 100,
      pct: 100,
    };
    const mid: ReportInsight = {
      kind: "night_share",
      night_tokens: 43,
      total_tokens: 100,
      pct: 43,
    };
    expect(insightCopy(none).comment).toBe("这个周期没有在凌晨烧过 token。");
    expect(insightCopy(all).comment).toBe("这个周期的 token 全是凌晨烧掉的。");
    expect(insightCopy(mid)).toEqual({
      headline: "43%",
      comment: "你 43% 的 token 是在凌晨烧的。",
    });
  });

  it("formats peak hours as a closed-open clock range, including midnight wrap", () => {
    expect(insightCopy({ kind: "peak_hours", start_hour: 22, end_hour: 2 })).toEqual({
      headline: "22:00 – 02:00",
      comment: "最活跃的时段是 22:00 到 02:00。",
    });
    expect(hourLabel(0)).toBe("00:00");
  });

  it("maps weekday 0-6 to 周一 through 周日", () => {
    expect(weekdayLabel(0)).toBe("周一");
    expect(weekdayLabel(6)).toBe("周日");
    expect(insightCopy({ kind: "busiest_day", weekday: 2 })).toEqual({
      headline: "最忙的一天",
      comment: "周三",
    });
  });

  it("labels the top session by cost or tokens and omits empty project names", () => {
    const priced = insightCopy({
      kind: "top_session",
      by: "cost",
      source: "claude",
      session_id: "s1",
      project: "/proj/a",
      cost: 4.2,
      total_tokens: 20,
    });
    expect(priced).toEqual({
      headline: "最贵的一次",
      comment: "$4.20 · /proj/a",
    });
    const tokens = insightCopy({
      kind: "top_session",
      by: "tokens",
      source: "codex",
      session_id: "s2",
      project: "  ",
      cost: null,
      total_tokens: 12_000,
    });
    expect(tokens).toEqual({
      headline: "消耗最多的一次",
      comment: "12K token",
    });
  });

  it("formats a zero-cost top session as tokens without placeholders", () => {
    const copy = insightCopy({
      kind: "top_session",
      by: "tokens",
      source: "codex",
      session_id: "s0",
      project: null,
      cost: 0,
      total_tokens: 80,
    });
    expect(copy).toEqual({
      headline: "消耗最多的一次",
      comment: "80 token",
    });
    expect(`${copy.headline}${copy.comment}`).not.toMatch(/暂无数据|——|未命名会话|\$0/);
  });

  it("never emits placeholder copy", () => {
    const copies = [
      insightCopy({ kind: "night_share", night_tokens: 0, total_tokens: 0, pct: 0 }),
      insightCopy({
        kind: "top_session",
        by: "cost",
        source: "claude",
        session_id: "s",
        project: null,
        cost: null,
        total_tokens: 0,
      }),
    ];
    for (const copy of copies) {
      expect(`${copy.headline}${copy.comment}`).not.toMatch(/暂无数据|——|未命名会话/);
    }
  });
});

describe("toPosterViewModel", () => {
  it("returns null when the period has no usage records", () => {
    expect(toPosterViewModel(dto({ has_data: false, totals: emptyTotals }))).toBeNull();
  });

  it("maps totals to the poster and omits the cost row when unpriced", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 12_400_000, session_count: 3, cost: null },
      }),
    );
    expect(poster).toMatchObject({
      kicker: "码表 · 周报",
      rangeLabel: "2026年8月10日 – 8月16日",
      totalTokensLabel: "12.4M",
      totalUnit: "本周 token",
      totalCostLabel: null,
      comments: ["你这周烧掉了 12.4M token。"],
      days: [],
      sources: [],
      stats: [],
    });
  });

  it("formats native cost with formatUsdAmount", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 100, session_count: 1, cost: 18.6 },
      }),
    );
    expect(poster?.totalCostLabel).toBe("$18.60");
  });

  it("maps seven daily bars with short weekday labels and keeps zero-token days", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        days: [
          { date: "2026-08-10", total_tokens: 10 },
          { date: "2026-08-11", total_tokens: 0 },
          { date: "2026-08-12", total_tokens: 50 },
          { date: "2026-08-13", total_tokens: 0 },
          { date: "2026-08-14", total_tokens: 20 },
          { date: "2026-08-15", total_tokens: 0 },
          { date: "2026-08-16", total_tokens: 0 },
        ],
      }),
    );
    expect(poster?.days).toEqual([
      { label: "一", tokens: 10 },
      { label: "二", tokens: 0 },
      { label: "三", tokens: 50 },
      { label: "四", tokens: 0 },
      { label: "五", tokens: 20 },
      { label: "六", tokens: 0 },
      { label: "日", tokens: 0 },
    ]);
  });

  it("puts busiest day into the stats row as a weekday name", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        insights: [{ kind: "busiest_day", weekday: 2 }],
      }),
    );
    expect(poster?.stats).toEqual([{ label: "最忙的一天", value: "周三" }]);
  });

  it("maps a single source as one 100% share slice with the source label", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        sources: [{ name: "claude", pct: 100 }],
      }),
    );
    expect(poster?.sources).toEqual([{ label: "Claude Code", pct: 100, color: "#8b6cff" }]);
  });

  it("maps multiple sources using DTO integer percents without recomputing", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 100, session_count: 3 },
        sources: [
          { name: "claude", pct: 50 },
          { name: "codex", pct: 30 },
          { name: "grok", pct: 20 },
        ],
      }),
    );
    expect(poster?.sources).toEqual([
      { label: "Claude Code", pct: 50, color: "#8b6cff" },
      { label: "Codex", pct: 30, color: "#3b82f6" },
      { label: "Grok CLI", pct: 20, color: "#22d3ee" },
    ]);
  });

  it("labels a single model as 模型 without Top 1", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        models: ["claude-sonnet-5"],
      }),
    );
    expect(poster?.stats).toEqual([{ label: "模型", value: "claude-sonnet-5" }]);
  });

  it("joins two models as 模型 Top 2 and three as 模型 Top 3", () => {
    const two = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 2 },
        models: ["opus", "gpt-5"],
      }),
    );
    expect(two?.stats).toEqual([{ label: "模型 Top 2", value: "opus · gpt-5" }]);
    const three = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 3 },
        models: ["opus", "gpt-5", "grok-4"],
      }),
    );
    expect(three?.stats).toEqual([{ label: "模型 Top 3", value: "opus · gpt-5 · grok-4" }]);
  });

  it("puts the model rank after busiest day in the stats row", () => {
    const poster = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        models: ["opus"],
        insights: [{ kind: "busiest_day", weekday: 2 }],
      }),
    );
    expect(poster?.stats).toEqual([
      { label: "最忙的一天", value: "周三" },
      { label: "模型", value: "opus" },
    ]);
  });

  it("puts the top session into the stats row after models", () => {
    const priced = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1, cost: 4.2 },
        models: ["opus"],
        insights: [
          { kind: "busiest_day", weekday: 2 },
          {
            kind: "top_session",
            by: "cost",
            source: "claude",
            session_id: "s1",
            project: "/proj/a",
            cost: 4.2,
            total_tokens: 20,
          },
        ],
      }),
    );
    expect(priced?.stats).toEqual([
      { label: "最忙的一天", value: "周三" },
      { label: "模型", value: "opus" },
      { label: "最贵的一次", value: "$4.20 · /proj/a" },
    ]);

    const tokens = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 80, session_count: 1 },
        insights: [
          {
            kind: "top_session",
            by: "tokens",
            source: "codex",
            session_id: "s0",
            project: null,
            cost: 0,
            total_tokens: 80,
          },
        ],
      }),
    );
    expect(tokens?.stats).toEqual([{ label: "消耗最多的一次", value: "80 token" }]);
  });

  it("appends night-share and peak-hours comments, including 0% and 100%", () => {
    const none = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 100, session_count: 1 },
        insights: [
          { kind: "night_share", night_tokens: 0, total_tokens: 100, pct: 0 },
          { kind: "peak_hours", start_hour: 9, end_hour: 13 },
        ],
      }),
    );
    expect(none?.comments).toEqual([
      "你这周烧掉了 100 token。",
      "这个周期没有在凌晨烧过 token。",
      "最活跃的时段是 09:00 到 13:00。",
    ]);

    const all = toPosterViewModel(
      dto({
        has_data: true,
        totals: { ...emptyTotals, total_tokens: 40, session_count: 1 },
        insights: [
          { kind: "night_share", night_tokens: 40, total_tokens: 40, pct: 100 },
          { kind: "peak_hours", start_hour: 22, end_hour: 2 },
        ],
      }),
    );
    expect(all?.comments).toEqual([
      "你这周烧掉了 40 token。",
      "这个周期的 token 全是凌晨烧掉的。",
      "最活跃的时段是 22:00 到 02:00。",
    ]);
  });
});
