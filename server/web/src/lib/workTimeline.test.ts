import { describe, expect, it } from "vitest";
import type { WorkSegment } from "../api/types";
import {
  DAY_MINUTES,
  dayStartIso,
  formatClock,
  formatDuration,
  laneCount,
  layoutSegments,
  minutesSinceDayStart,
  MIN_SEGMENT_MINUTES,
  weekdayLabel,
} from "./workTimeline";

const DAY_START = "2026-08-15T00:00:00.000Z";

function seg(overrides: Partial<WorkSegment> = {}): WorkSegment {
  return {
    id: 1,
    account_id: 2,
    account: "alice",
    source: "codex",
    session_id: "s1",
    title: "改登录",
    project: "/proj/a",
    project_id: 3,
    project_name: "mabiao",
    model: "gpt-5",
    start: "2026-08-15T01:00:00.000Z",
    end: "2026-08-15T02:00:00.000Z",
    total_tokens: 100,
    cost_snapshot: 1,
    unified_cost: 0.2,
    ...overrides,
  };
}

describe("minutesSinceDayStart", () => {
  it("computes minutes from an epoch difference", () => {
    expect(minutesSinceDayStart("2026-08-15T01:30:00.000Z", DAY_START)).toBe(90);
    expect(minutesSinceDayStart(DAY_START, DAY_START)).toBe(0);
  });

  it("clamps to [0, DAY_MINUTES] and treats junk as 0", () => {
    expect(minutesSinceDayStart("2026-08-14T23:00:00.000Z", DAY_START)).toBe(0);
    expect(minutesSinceDayStart("2026-08-16T05:00:00.000Z", DAY_START)).toBe(DAY_MINUTES);
    expect(minutesSinceDayStart("not-a-date", DAY_START)).toBe(0);
  });
});

describe("layoutSegments", () => {
  it("keeps non-overlapping segments on a single lane", () => {
    const layout = layoutSegments(
      [
        seg({ id: 1, session_id: "a", start: "2026-08-15T01:00:00.000Z", end: "2026-08-15T02:00:00.000Z" }),
        seg({ id: 2, session_id: "b", start: "2026-08-15T02:00:00.000Z", end: "2026-08-15T03:00:00.000Z" }),
      ],
      DAY_START,
    );
    expect(layout.every((item) => item.lane === 0)).toBe(true);
    expect(laneCount(layout)).toBe(1);
  });

  it("stacks overlapping segments and reuses a freed lane", () => {
    const layout = layoutSegments(
      [
        seg({ id: 1, session_id: "a", start: "2026-08-15T01:00:00.000Z", end: "2026-08-15T03:00:00.000Z" }),
        seg({ id: 2, session_id: "b", start: "2026-08-15T01:30:00.000Z", end: "2026-08-15T02:30:00.000Z" }),
        seg({ id: 3, session_id: "c", start: "2026-08-15T03:00:00.000Z", end: "2026-08-15T04:00:00.000Z" }),
      ],
      DAY_START,
    );
    const byId = new Map(layout.map((item) => [item.segment.session_id, item.lane]));
    expect(byId.get("a")).toBe(0);
    expect(byId.get("b")).toBe(1);
    expect(byId.get("c")).toBe(0);
    expect(laneCount(layout)).toBe(2);
  });

  it("gives zero-width segments a minimum visible width without mutating the source", () => {
    const zero = seg({
      session_id: "z",
      start: "2026-08-15T05:00:00.000Z",
      end: "2026-08-15T05:00:00.000Z",
    });
    const placed = layoutSegments([zero], DAY_START)[0];
    if (!placed) throw new Error("expected a placed segment");
    expect(placed.endMinutes - placed.startMinutes).toBe(MIN_SEGMENT_MINUTES);
    expect(placed.segment).toBe(zero);
  });
});

describe("formatDuration / formatClock / weekdayLabel", () => {
  it("formats duration in Chinese", () => {
    expect(formatDuration("2026-08-15T01:00:00.000Z", "2026-08-15T01:00:00.000Z")).toBe("不足 1 分钟");
    expect(formatDuration("2026-08-15T01:00:00.000Z", "2026-08-15T01:25:00.000Z")).toBe("25 分钟");
    expect(formatDuration("2026-08-15T01:00:00.000Z", "2026-08-15T03:00:00.000Z")).toBe("2 小时");
    expect(formatDuration("2026-08-15T01:00:00.000Z", "2026-08-15T03:10:00.000Z")).toBe("2 小时 10 分钟");
    expect(formatDuration("bad", "also-bad")).toBe("—");
  });

  it("formats a clock in local time", () => {
    expect(formatClock("2026-03-01T10:05:00Z")).toMatch(/^\d{2}:05$/);
    expect(formatClock("not-a-time")).toBe("—");
  });

  it("names the weekday", () => {
    expect(weekdayLabel("2026-03-01")).toBe("日");
    expect(weekdayLabel("not-a-day")).toBe("");
  });
});

describe("dayStartIso", () => {
  it("round-trips with minutesSinceDayStart", () => {
    const start = dayStartIso("2026-08-15");
    expect(minutesSinceDayStart(start, start)).toBe(0);
  });
});
