import { describe, expect, it } from "vitest";
import type { Filter } from "../types";
import {
  applicationLabel,
  applicationSourceOptions,
  weeklyCountLabel,
  callRangeWindow,
  customRangeFilter,
  deltaPct,
  filterWithCallRange,
  formatBytes,
  formatClock,
  formatCompact,
  formatCost,
  formatDelta,
  formatDuration,
  formatHoursMinutes,
  formatRangeLabel,
  formatRatio,
  formatTokens,
  formatUsd,
  formatWindowClock,
  previousFilter,
  projectLabel,
  providerChannel,
  rangeFromPreset,
  relativeTime,
  shortId,
} from "./format";

const emptyFilter: Filter = {
  from: null,
  to: null,
  sources: [],
  models: [],
  projects: [],
  providers: [],
};

describe("formatTokens", () => {
  it("uses zh-CN grouping", () => {
    expect(formatTokens(1234567)).toBe("1,234,567");
    expect(formatTokens(0)).toBe("0");
  });
});

describe("formatCompact", () => {
  it("keeps small numbers as-is", () => {
    expect(formatCompact(0)).toBe("0");
    expect(formatCompact(9999)).toBe("9,999");
  });

  it("compacts thousands/millions/billions with trimmed decimals", () => {
    expect(formatCompact(12_000)).toBe("12K");
    expect(formatCompact(12_500)).toBe("12.5K");
    expect(formatCompact(1_234_000)).toBe("1.23M");
    expect(formatCompact(2_000_000_000)).toBe("2B");
  });

  it("handles negative numbers using the absolute value for thresholds", () => {
    expect(formatCompact(-12_000)).toBe("-12K");
  });
});

describe("formatUsd", () => {
  it("shows a dash only when null and unpriced", () => {
    expect(formatUsd(null, true)).toBe("—");
    expect(formatUsd(null, false)).toBe("$0.00");
    expect(formatUsd(1.5, false)).toBe("$1.50");
  });
});

describe("formatCost", () => {
  it("shows a dash only when unpriced and null, else 4 decimals", () => {
    expect(formatCost(null, true)).toBe("—");
    expect(formatCost(null, false)).toBe("0");
    expect(formatCost(0.06434, false)).toBe("0.0643");
  });
});

describe("deltaPct", () => {
  it("returns null when there is no previous value", () => {
    expect(deltaPct(10, null)).toBeNull();
  });

  it("treats previous=0 as 0% only when current is also 0, else null (undefined growth)", () => {
    expect(deltaPct(0, 0)).toBe(0);
    expect(deltaPct(5, 0)).toBeNull();
  });

  it("computes signed percentage change", () => {
    expect(deltaPct(150, 100)).toBe(50);
    expect(deltaPct(50, 100)).toBe(-50);
  });
});

describe("formatDelta", () => {
  it("passes through null", () => {
    expect(formatDelta(null)).toBeNull();
  });

  it("treats tiny changes as flat", () => {
    expect(formatDelta(0.01)).toEqual({ text: "持平 vs 上期", tone: "flat" });
  });

  it("formats up/down with an arrow and one decimal", () => {
    expect(formatDelta(12.34)).toEqual({ text: "↑ 12.3% vs 上期", tone: "up" });
    expect(formatDelta(-12.34)).toEqual({ text: "↓ 12.3% vs 上期", tone: "down" });
  });

  it("accepts a custom comparison label", () => {
    expect(formatDelta(10, "上一有数据日")).toEqual({
      text: "↑ 10.0% vs 上一有数据日",
      tone: "up",
    });
  });
});

describe("applicationLabel", () => {
  it("maps known sources to display names", () => {
    expect(applicationLabel("claude")).toBe("Claude Code");
    expect(applicationLabel("codex")).toBe("Codex");
    expect(applicationLabel("cursor")).toBe("Cursor");
    expect(applicationLabel("factory")).toBe("Droid");
    expect(applicationLabel("droid")).toBe("Droid");
    expect(applicationLabel("omp")).toBe("OMP");
    expect(applicationLabel("antigravity")).toBe("Antigravity");
    expect(applicationLabel("devin")).toBe("Devin");
  });

  it("always offers Cursor in the application source list", () => {
    expect(applicationSourceOptions(["claude", "codex"])).toEqual(["claude", "codex", "cursor"]);
    expect(applicationSourceOptions(["cursor"])).toEqual(["cursor"]);
  });

  it("labels cursor weekly rows as events", () => {
    expect(weeklyCountLabel("cursor", 12)).toBe("共 12 条事件");
    expect(weeklyCountLabel("claude", 3)).toBe("共 3 个会话");
  });

  it("falls back to the raw source id when unknown", () => {
    expect(applicationLabel("some_new_source")).toBe("some_new_source");
  });
});

describe("projectLabel", () => {
  it("shows a placeholder for empty paths", () => {
    expect(projectLabel("")).toBe("未标注");
  });

  it("takes the last path segment, handling both slash styles", () => {
    expect(projectLabel("/Users/dev/workCode/ruoyi-ui-vue3")).toBe("ruoyi-ui-vue3");
    expect(projectLabel("C:\\Users\\dev\\project")).toBe("project");
  });
});

describe("shortId", () => {
  it("keeps short ids untouched", () => {
    expect(shortId("abc123")).toBe("abc123");
  });

  it("truncates long ids with an ellipsis", () => {
    expect(shortId("019f5abc-b360-79e4-bd7d-9a794da8cfc5")).toBe("019f5abc…");
  });
});

describe("relativeTime", () => {
  it("returns the input as-is for unparsable dates", () => {
    expect(relativeTime("not-a-date")).toBe("not-a-date");
  });

  it("computes age against an explicit now", () => {
    const now = Date.parse("2026-08-23T07:20:00.000Z");
    expect(relativeTime("2026-08-23T07:20:20.000Z", now)).toBe("刚刚");
    expect(relativeTime("2026-08-23T07:17:00.000Z", now)).toBe("3 分钟前");
    expect(relativeTime("2026-08-23T05:20:00.000Z", now)).toBe("2 小时前");
  });
});

describe("formatBytes", () => {
  it("formats a byte count with a unit suffix", () => {
    expect(formatBytes(15)).toBe("15 B");
    expect(formatBytes(0)).toBe("0 B");
  });
});

describe("formatWindowClock / formatClock", () => {
  it("returns the raw string for invalid dates", () => {
    expect(formatWindowClock("nope")).toBe("nope");
    expect(formatClock("nope")).toBe("nope");
  });

  it("returns a dash for a null clock value", () => {
    expect(formatClock(null)).toBe("—");
  });

  it("formats a valid ISO timestamp", () => {
    expect(formatClock("2026-08-17T05:31:13.000Z")).toMatch(/^2026-08-17 \d{2}:31:13$/);
  });
});

describe("formatHoursMinutes", () => {
  it("omits the hour part when under an hour", () => {
    expect(formatHoursMinutes(45)).toBe("45m");
  });

  it("includes both parts when an hour or more", () => {
    expect(formatHoursMinutes(125)).toBe("2h 5m");
  });

  it("clamps negative input to 0", () => {
    expect(formatHoursMinutes(-10)).toBe("0m");
  });
});

describe("formatDuration", () => {
  it("returns null when either side is missing or not later", () => {
    expect(formatDuration(null, "2026-08-18T10:00:00Z")).toBeNull();
    expect(formatDuration("2026-08-18T10:00:00Z", "2026-08-18T10:00:00Z")).toBeNull();
  });

  it("formats a positive span", () => {
    expect(formatDuration("2026-08-18T10:00:00Z", "2026-08-18T12:05:00Z")).toBe("2h 5m");
  });
});

describe("formatRatio", () => {
  it("shows a dash for null", () => {
    expect(formatRatio(null)).toBe("—");
  });

  it("keeps one decimal by default", () => {
    expect(formatRatio(2.56)).toBe("2.6");
  });
});

describe("formatRangeLabel", () => {
  it("shows 全部历史 for the all preset or a missing range", () => {
    expect(formatRangeLabel(emptyFilter, "all")).toBe("全部历史");
    expect(formatRangeLabel(emptyFilter, "7")).toBe("全部历史");
  });

  it("shows the date-only range for a bounded preset", () => {
    const filter: Filter = {
      ...emptyFilter,
      from: "2026-08-01T00:00:00Z",
      to: "2026-08-07T23:59:59Z",
    };
    expect(formatRangeLabel(filter, "7")).toBe("2026-08-01 ~ 2026-08-07");
  });
});

describe("providerChannel", () => {
  it("labels unset providers as 未标注", () => {
    expect(providerChannel("")).toBe("未标注");
    expect(providerChannel("（未标注）")).toBe("未标注");
  });

  it("recognizes well-known official providers", () => {
    expect(providerChannel("anthropic")).toBe("官方");
    expect(providerChannel("openai")).toBe("官方");
  });

  it("treats anything else as a relay/中转", () => {
    expect(providerChannel("some-third-party")).toBe("中转");
  });
});

describe("rangeFromPreset", () => {
  it("returns an open range for the all preset", () => {
    expect(rangeFromPreset("all")).toEqual({ from: null, to: null });
  });

  it("computes a from/to window for numeric-day presets", () => {
    const seven = rangeFromPreset("7");
    expect(seven.from).not.toBeNull();
    expect(seven.to).not.toBeNull();
    const sevenMs = Date.parse(seven.to!) - Date.parse(seven.from!);
    expect(Math.round(sevenMs / (24 * 3600 * 1000))).toBe(7);

    const three = rangeFromPreset("3");
    expect(three.from).not.toBeNull();
    expect(three.to).not.toBeNull();
    const threeMs = Date.parse(three.to!) - Date.parse(three.from!);
    expect(Math.round(threeMs / (24 * 3600 * 1000))).toBe(3);
  });

  it("returns an open range for a zero-day numeric preset", () => {
    expect(rangeFromPreset("0")).toEqual({ from: null, to: null });
  });

  it("starts today at local midnight and month at the first of the month", () => {
    const today = rangeFromPreset("today");
    const month = rangeFromPreset("month");
    expect(today.from).not.toBeNull();
    expect(month.from).not.toBeNull();
    const todayStart = new Date(today.from!);
    const monthStart = new Date(month.from!);
    expect(todayStart.getHours()).toBe(0);
    expect(todayStart.getMinutes()).toBe(0);
    expect(monthStart.getDate()).toBe(1);
    expect(monthStart.getHours()).toBe(0);
  });
});

describe("customRangeFilter", () => {
  it("expands two date-only inputs to cover the full days", () => {
    const { from, to } = customRangeFilter("2026-08-01", "2026-08-01");
    expect(from).not.toBeNull();
    expect(to).not.toBeNull();
    expect(new Date(from!).getHours()).toBe(0);
    expect(new Date(to!).getHours()).toBe(23);
  });

  it("returns nulls for unparsable input", () => {
    expect(customRangeFilter("not-a-date", "also-not")).toEqual({ from: null, to: null });
  });
});

describe("callRangeWindow", () => {
  it("uses rolling windows for 当天 / 近 3 天 / 近 7 天", () => {
    const today = callRangeWindow("today", "", "");
    const three = callRangeWindow("3", "", "");
    expect(new Date(today.from!).getHours()).toBe(0);
    expect(Math.round((Date.parse(three.to!) - Date.parse(three.from!)) / (24 * 3600 * 1000))).toBe(
      3,
    );
  });

  it("uses the custom date range when both ends are valid", () => {
    const range = callRangeWindow("custom", "2026-08-01", "2026-08-02");
    expect(range.from).not.toBeNull();
    expect(range.to).not.toBeNull();
    expect(Date.parse(range.to!)).toBeGreaterThan(Date.parse(range.from!));
    expect(new Date(range.from!).getHours()).toBe(0);
    expect(new Date(range.to!).getHours()).toBe(23);
  });

  it("falls back to 近 7 天 when the custom range is incomplete", () => {
    const fallback = callRangeWindow("custom", "", "");
    expect(
      Math.round((Date.parse(fallback.to!) - Date.parse(fallback.from!)) / (24 * 3600 * 1000)),
    ).toBe(7);
  });
});

describe("filterWithCallRange", () => {
  it("overrides only the time window and keeps other dimensions", () => {
    const next = filterWithCallRange(
      { ...emptyFilter, providers: ["tongban"], models: ["gpt-5"] },
      "today",
      "",
      "",
    );
    expect(next.providers).toEqual(["tongban"]);
    expect(next.models).toEqual(["gpt-5"]);
    expect(next.from).not.toBeNull();
    expect(next.to).not.toBeNull();
  });
});

describe("previousFilter", () => {
  it("returns null outside of bounded presets", () => {
    expect(previousFilter(emptyFilter, "all")).toBeNull();
  });

  it("returns null when the filter has no bounded range", () => {
    expect(previousFilter(emptyFilter, "7")).toBeNull();
  });

  it("shifts the window back by its own length, ending where it started", () => {
    const filter: Filter = {
      ...emptyFilter,
      from: "2026-08-08T00:00:00.000Z",
      to: "2026-08-15T00:00:00.000Z",
    };
    const previous = previousFilter(filter, "7");
    expect(previous).not.toBeNull();
    expect(previous!.to).toBe(filter.from);
    expect(previous!.from).toBe("2026-08-01T00:00:00.000Z");
  });
});
