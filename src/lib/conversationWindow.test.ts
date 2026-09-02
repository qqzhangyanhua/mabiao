import { describe, expect, it } from "vitest";
import {
  advanceConversationEventWindow,
  applyConversationEventPage,
  aroundPageAnchor,
  emptyConversationEventWindow,
  firstPageAnchor,
  latestPageAnchor,
  nextEarlierAnchor,
  nextLaterAnchor,
  shouldResetConversationEventWindow,
  trimConversationEventWindow,
} from "./conversationWindow";

const page = (
  sequences: number[],
  neighbors: { before?: boolean; after?: boolean } = {},
) => ({
  events: sequences.map((sequence) => ({ sequence })),
  has_more_before: neighbors.before ?? false,
  has_more_after: neighbors.after ?? false,
});

describe("latestPageAnchor / firstPageAnchor", () => {
  it("打开会话锚定最新一页，跳到开头命中索引第一页", () => {
    expect(latestPageAnchor()).toEqual({ type: "last" });
    expect(firstPageAnchor()).toEqual({ type: "first" });
    expect(aroundPageAnchor(4)).toEqual({ type: "around", sequence: 4 });
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

describe("trimConversationEventWindow", () => {
  it("向更早一端越过上限时丢掉远离视口的更新一端", () => {
    const trimmed = trimConversationEventWindow(
      {
        events: [0, 1, 2, 3, 4, 5].map((sequence) => ({ sequence })),
        hasMoreBefore: true,
        hasMoreAfter: false,
      },
      { keep: "start", limit: 4 },
    );
    expect(trimmed.events.map((event) => event.sequence)).toEqual([0, 1, 2, 3]);
    expect(trimmed.hasMoreBefore).toBe(true);
    expect(trimmed.hasMoreAfter).toBe(true);
  });

  it("向更新一端越过上限时丢掉远离视口的更早一端", () => {
    const trimmed = trimConversationEventWindow(
      {
        events: [0, 1, 2, 3, 4, 5].map((sequence) => ({ sequence })),
        hasMoreBefore: false,
        hasMoreAfter: true,
      },
      { keep: "end", limit: 4 },
    );
    expect(trimmed.events.map((event) => event.sequence)).toEqual([2, 3, 4, 5]);
    expect(trimmed.hasMoreBefore).toBe(true);
    expect(trimmed.hasMoreAfter).toBe(true);
  });

  it("未越过上限时原窗口不动", () => {
    const current = {
      events: [1, 2, 3].map((sequence) => ({ sequence })),
      hasMoreBefore: true,
      hasMoreAfter: false,
    };
    expect(trimConversationEventWindow(current, { keep: "start", limit: 4 })).toEqual(current);
    expect(trimConversationEventWindow(current, { keep: "end", limit: 3 })).toEqual(current);
  });
});

describe("advanceConversationEventWindow", () => {
  it("从最新页一路向前再回来，窗口有界且序号连续", () => {
    let current = advanceConversationEventWindow(
      emptyConversationEventWindow(),
      page([6, 7, 8], { before: true }),
      "replace",
      6,
    );
    expect(current.events.map((event) => event.sequence)).toEqual([6, 7, 8]);

    current = advanceConversationEventWindow(
      current,
      page([3, 4, 5], { before: true, after: true }),
      "prepend",
      6,
    );
    expect(current.events.map((event) => event.sequence)).toEqual([3, 4, 5, 6, 7, 8]);
    expect(current.hasMoreBefore).toBe(true);
    expect(current.hasMoreAfter).toBe(false);

    current = advanceConversationEventWindow(
      current,
      page([0, 1, 2], { after: true }),
      "prepend",
      6,
    );
    expect(current.events.map((event) => event.sequence)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(current.hasMoreBefore).toBe(false);
    expect(current.hasMoreAfter).toBe(true);

    current = advanceConversationEventWindow(
      current,
      page([6, 7, 8], { before: true }),
      "append",
      6,
    );
    expect(current.events.map((event) => event.sequence)).toEqual([3, 4, 5, 6, 7, 8]);
    expect(current.hasMoreBefore).toBe(true);
    expect(current.hasMoreAfter).toBe(false);
  });
});
