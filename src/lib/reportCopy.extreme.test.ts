import { describe, expect, it } from "vitest";
import type { ReportDto } from "../types";
import { emptyReportTotals, EXTREME_REPORT_CASES, extremeReportDto } from "./reportExtremeFixtures";
import { toPosterViewModel } from "./reportCopy";

const PLACEHOLDER_RE = /暂无数据|——|未命名会话/;

function posterText(poster: NonNullable<ReturnType<typeof toPosterViewModel>>): string {
  return [
    poster.kicker,
    poster.rangeLabel,
    poster.totalTokensLabel,
    poster.totalUnit,
    poster.totalCostLabel ?? "",
    ...poster.comments,
    ...poster.days.map((day) => `${day.label}:${day.tokens}`),
    ...poster.sources.map((source) => `${source.label} ${source.pct}%`),
    ...poster.stats.flatMap((stat) => [stat.label, stat.value]),
  ].join("\n");
}

function expectSendablePoster(poster: ReturnType<typeof toPosterViewModel>) {
  expect(poster).not.toBeNull();
  if (!poster) {
    return;
  }
  expect(poster.kicker).toBe("码表 · 周报");
  expect(poster.rangeLabel).toBe("2026年8月10日 – 8月16日");
  expect(poster.totalTokensLabel.length).toBeGreaterThan(0);
  expect(poster.totalUnit).toBe("本周 token");
  expect(poster.comments).toHaveLength(3);
  expect(poster.days).toHaveLength(7);
  expect(poster.sources.length).toBeGreaterThan(0);
  expect(poster.stats).toHaveLength(3);
  expect(poster.stats[0]?.label).toBe("最忙的一天");
  expect(poster.stats[0]?.value).toMatch(/^周[一二三四五六日]$/);
  expect(poster.stats[1]?.label).toMatch(/^模型( Top [23])?$/);
  expect(poster.stats[2]?.label).toMatch(/^(最贵的一次|消耗最多的一次)$/);
  for (const stat of poster.stats) {
    expect(stat.value.length).toBeGreaterThan(0);
  }
  expect(posterText(poster)).not.toMatch(PLACEHOLDER_RE);
  expect(poster.totalCostLabel ?? "").not.toMatch(/^\$0\.0+$/);
}

function caseDto(id: string): ReportDto {
  const found = EXTREME_REPORT_CASES.find((item) => item.id === id);
  if (!found) {
    throw new Error(`missing extreme case ${id}`);
  }
  return found.dto;
}

describe("extreme poster composition", () => {
  it("keeps every listed extreme combination sendable", () => {
    for (const item of EXTREME_REPORT_CASES) {
      expectSendablePoster(toPosterViewModel(item.dto));
    }
  });

  it("keeps all seven slots on a single night-time unpriced record", () => {
    const poster = toPosterViewModel(caseDto("single-night"));
    expectSendablePoster(poster);
    expect(poster?.totalCostLabel).toBeNull();
    expect(poster?.days.map((day) => day.tokens)).toEqual([0, 0, 80, 0, 0, 0, 0]);
    expect(poster?.sources).toEqual([{ label: "Claude Code", pct: 100, color: "#8b6cff" }]);
    expect(poster?.comments).toEqual([
      "你这周烧掉了 80 token。",
      "这个周期的 token 全是凌晨烧掉的。",
      "最活跃的时段是 00:00 到 04:00。",
    ]);
    expect(poster?.stats).toEqual([
      { label: "最忙的一天", value: "周三" },
      { label: "模型", value: "claude-sonnet-5" },
      { label: "消耗最多的一次", value: "80 token · /proj/a" },
    ]);
  });

  it("keeps copy sane when night share is 0%", () => {
    const poster = toPosterViewModel(
      extremeReportDto({
        insights: [
          { kind: "night_share", night_tokens: 0, total_tokens: 80, pct: 0 },
          { kind: "peak_hours", start_hour: 11, end_hour: 15 },
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
      }),
    );
    expectSendablePoster(poster);
    expect(poster?.comments[1]).toBe("这个周期没有在凌晨烧过 token。");
    expect(poster?.comments[2]).toBe("最活跃的时段是 11:00 到 15:00。");
  });

  it("omits the cost row when the period cost is zero and still has no placeholders", () => {
    const poster = toPosterViewModel(caseDto("single-day"));
    expectSendablePoster(poster);
    expect(poster?.totalCostLabel).toBeNull();
    expect(poster?.days.map((day) => day.tokens)).toEqual([0, 0, 0, 80, 0, 0, 0]);
    expect(poster?.comments[1]).toBe("这个周期没有在凌晨烧过 token。");
    expect(poster?.stats).toEqual([
      { label: "最忙的一天", value: "周四" },
      { label: "模型", value: "claude-sonnet-5" },
      { label: "消耗最多的一次", value: "50 token · /proj/a" },
    ]);
  });

  it("keeps a single source as one 100% slice without empty slots", () => {
    const poster = toPosterViewModel(
      extremeReportDto({
        models: ["opus"],
        sources: [{ name: "claude", pct: 100 }],
      }),
    );
    expectSendablePoster(poster);
    expect(poster?.sources).toHaveLength(1);
    expect(poster?.sources[0]?.pct).toBe(100);
    expect(poster?.stats[1]).toEqual({ label: "模型", value: "opus" });
  });

  it("does not treat a real sub-cent cost as a zero placeholder", () => {
    const poster = toPosterViewModel(
      extremeReportDto({
        totals: { ...emptyReportTotals, total_tokens: 80, session_count: 1, cost: 0.02 },
      }),
    );
    expect(poster?.totalCostLabel).toBe("$0.02");
    expectSendablePoster(poster);
  });
});
