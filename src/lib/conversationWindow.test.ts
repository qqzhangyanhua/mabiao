import { describe, expect, it } from "vitest";
import {
  applyConversationEventPage,
  emptyConversationEventWindow,
  latestPageAnchor,
  nextEarlierAnchor,
  nextLaterAnchor,
  shouldResetConversationEventWindow,
} from "./conversationWindow";

const page = (
  sequences: number[],
  neighbors: { before?: boolean; after?: boolean } = {},
) => ({
  events: sequences.map((sequence) => ({ sequence })),
  has_more_before: neighbors.before ?? false,
  has_more_after: neighbors.after ?? false,
});

describe("latestPageAnchor", () => {
  it("打开会话锚定最新一页", () => {
    expect(latestPageAnchor()).toEqual({ type: "last" });
  });
});

describe("nextEarlierAnchor / nextLaterAnchor", () => {
  it("有更早页时用当前最小序号向前取", () => {
    expect(
      nextEarlierAnchor({
        events: [{ sequence: 7 }, { sequence: 8 }, { sequence: 9 }],
        hasMoreBefore: true,
        hasMoreAfter: false,
      }),
    ).toEqual({ type: "before", sequence: 7 });
  });

  it("已经到头或窗口为空时不再向前", () => {
    expect(
      nextEarlierAnchor({
        events: [{ sequence: 0 }],
        hasMoreBefore: false,
        hasMoreAfter: true,
      }),
    ).toBeNull();
    expect(nextEarlierAnchor(emptyConversationEventWindow())).toBeNull();
  });

  it("有更新页时用当前最大序号向后取", () => {
    expect(
      nextLaterAnchor({
        events: [{ sequence: 0 }, { sequence: 1 }],
        hasMoreBefore: false,
        hasMoreAfter: true,
      }),
    ).toEqual({ type: "after", sequence: 1 });
    expect(
      nextLaterAnchor({
        events: [{ sequence: 0 }],
        hasMoreBefore: false,
        hasMoreAfter: false,
      }),
    ).toBeNull();
  });
});

describe("applyConversationEventPage", () => {
  it("replace 用新页覆盖窗口", () => {
    const current = applyConversationEventPage(
      emptyConversationEventWindow(),
      page([7, 8, 9], { before: true }),
      "replace",
    );
    expect(current).toEqual({
      events: [{ sequence: 7 }, { sequence: 8 }, { sequence: 9 }],
      hasMoreBefore: true,
      hasMoreAfter: false,
    });
  });

  it("prepend 把更早一页接到前面，且不丢已加载的页", () => {
    const latest = applyConversationEventPage(
      emptyConversationEventWindow(),
      page([7, 8, 9], { before: true }),
      "replace",
    );
    const merged = applyConversationEventPage(
      latest,
      page([4, 5, 6], { before: true, after: true }),
      "prepend",
    );
    expect(merged.events.map((event) => event.sequence)).toEqual([4, 5, 6, 7, 8, 9]);
    expect(merged.hasMoreBefore).toBe(true);
    expect(merged.hasMoreAfter).toBe(false);
  });

  it("append 把更晚一页接到后面", () => {
    const first = applyConversationEventPage(
      emptyConversationEventWindow(),
      page([0, 1, 2], { after: true }),
      "replace",
    );
    const merged = applyConversationEventPage(first, page([3, 4], { before: true }), "append");
    expect(merged.events.map((event) => event.sequence)).toEqual([0, 1, 2, 3, 4]);
    expect(merged.hasMoreBefore).toBe(false);
    expect(merged.hasMoreAfter).toBe(false);
  });

  it("同一序号不重复插入", () => {
    const current = applyConversationEventPage(
      emptyConversationEventWindow(),
      page([7, 8, 9], { before: true }),
      "replace",
    );
    const merged = applyConversationEventPage(current, page([8, 9], { before: true }), "prepend");
    expect(merged.events.map((event) => event.sequence)).toEqual([7, 8, 9]);
  });
});

describe("shouldResetConversationEventWindow", () => {
  it("revision 变化时重置，首次赋值不重置", () => {
    expect(shouldResetConversationEventWindow(null, "rev-1")).toBe(false);
    expect(shouldResetConversationEventWindow("rev-1", "rev-1")).toBe(false);
    expect(shouldResetConversationEventWindow("rev-1", "rev-2")).toBe(true);
  });
});
