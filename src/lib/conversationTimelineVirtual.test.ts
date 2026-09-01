import { describe, expect, it } from "vitest";
import {
  pruneTimelineMeasurements,
  TIMELINE_OVERSCAN,
  TIMELINE_ROW_ESTIMATE,
  timelineAnchorAtOffset,
  timelineOffsetAt,
  timelineScrollCorrection,
  timelineScrollTopForAnchor,
  timelineVisibleRange,
} from "./conversationTimelineVirtual";

const keysOf = (count: number) => Array.from({ length: count }, (_, index) => `e${index}`);

describe("timelineVisibleRange", () => {
  it("空列表不挂行", () => {
    expect(
      timelineVisibleRange({
        scrollTop: 0,
        viewportHeight: 640,
        keys: [],
        measured: new Map(),
      }),
    ).toEqual({ start: 0, end: 0, paddingTop: 0, paddingBottom: 0, totalHeight: 0 });
  });

  it("内容不足一屏时全部挂上", () => {
    const keys = keysOf(4);
    const range = timelineVisibleRange({
      scrollTop: 0,
      viewportHeight: 640,
      keys,
      measured: new Map(),
    });
    expect(range.start).toBe(0);
    expect(range.end).toBe(4);
    expect(range.paddingTop).toBe(0);
    expect(range.paddingBottom).toBe(0);
    expect(range.totalHeight).toBe(4 * TIMELINE_ROW_ESTIMATE);
  });

  it("1000 条时中间视口大约挂 30–50 行", () => {
    const keys = keysOf(1000);
    const range = timelineVisibleRange({
      scrollTop: 48_000,
      viewportHeight: 640,
      keys,
      measured: new Map(),
    });
    const mounted = range.end - range.start;
    expect(mounted).toBeGreaterThanOrEqual(30);
    expect(mounted).toBeLessThanOrEqual(50);
    expect(range.paddingTop + range.paddingBottom + mounted * TIMELINE_ROW_ESTIMATE).toBe(
      range.totalHeight,
    );
    expect(range.totalHeight).toBe(1000 * TIMELINE_ROW_ESTIMATE);
  });

  it("贴底时只挂末尾窗口，不把开头 200 条画进 DOM", () => {
    const keys = keysOf(1000);
    const range = timelineVisibleRange({
      scrollTop: 0,
      viewportHeight: 640,
      keys,
      measured: new Map(),
      preferEnd: true,
    });
    expect(range.end).toBe(1000);
    expect(range.start).toBeGreaterThan(900);
    expect(range.end - range.start).toBeLessThanOrEqual(TIMELINE_OVERSCAN * 2 + 20);
  });

  it("已测量高度参与起止计算", () => {
    const keys = keysOf(10);
    const measured = new Map([
      ["e0", 400],
      ["e1", 400],
      ["e2", 400],
    ]);
    const range = timelineVisibleRange({
      scrollTop: 500,
      viewportHeight: 500,
      keys,
      measured,
      overscan: 0,
      estimate: 100,
    });
    expect(range.start).toBe(1);
    expect(range.end).toBe(3);
    expect(range.paddingTop).toBe(400);
  });
});

describe("timelineAnchorAtOffset / timelineScrollTopForAnchor", () => {
  it("记录第一条未完全滚出视口的行，prepend 后按新偏移还原", () => {
    const before = keysOf(3);
    const measured = new Map<string, number>();
    const anchor = timelineAnchorAtOffset(before, 150, measured, 100);
    expect(anchor).toEqual({ key: "e1", offset: -50 });

    const after = ["p0", "p1", "p2", ...before];
    expect(timelineScrollTopForAnchor(anchor!, after, measured, 100)).toBe(450);
  });

  it("跳过闸门行，锚定到事件行", () => {
    const keys = ["gate:before", "event:a", "event:b"];
    const eligible = new Set(["event:a", "event:b"]);
    expect(timelineAnchorAtOffset(keys, 0, new Map(), 80, eligible)).toEqual({
      key: "event:a",
      offset: 80,
    });
  });
});

describe("timelineScrollCorrection", () => {
  it("视口上方的行变高时把 scrollTop 一起推下去", () => {
    expect(
      timelineScrollCorrection({
        itemOffset: 0,
        previousHeight: 96,
        nextHeight: 180,
        scrollTop: 200,
      }),
    ).toBe(84);
  });

  it("视口内或下方的行变高不改 scrollTop", () => {
    expect(
      timelineScrollCorrection({
        itemOffset: 180,
        previousHeight: 96,
        nextHeight: 180,
        scrollTop: 200,
      }),
    ).toBe(0);
  });
});

describe("pruneTimelineMeasurements", () => {
  it("丢掉已经不在窗口里的行高", () => {
    const measured = new Map([
      ["a", 10],
      ["b", 20],
      ["c", 30],
    ]);
    pruneTimelineMeasurements(measured, ["b"]);
    expect([...measured.keys()]).toEqual(["b"]);
  });
});

describe("timelineOffsetAt", () => {
  it("按测量值累加，缺测用估计", () => {
    const keys = keysOf(3);
    const measured = new Map([["e1", 40]]);
    expect(timelineOffsetAt(keys, 2, measured, 10)).toBe(50);
    expect(timelineOffsetAt(keys, 99, measured, 10)).toBe(60);
  });
});
