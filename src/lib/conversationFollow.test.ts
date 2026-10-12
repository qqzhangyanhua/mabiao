import { describe, expect, it } from "vitest";
import {
  createConversationRequestGate,
  conversationJumpBehavior,
  conversationJumpScrollTop,
  conversationTimelineScrollTarget,
  isConversationResponseCurrent,
  conversationScrollLoadEdge,
  isNearConversationBottom,
  isNearConversationTop,
  nextConversationRevisionPollState,
  nextConversationFollowState,
} from "./conversationFollow";

describe("createConversationRequestGate", () => {
  it("serializes requests until the active request releases the gate", () => {
    const gate = createConversationRequestGate();

    expect(gate.acquire()).toBe(true);
    expect(gate.acquire()).toBe(false);

    gate.release();

    expect(gate.acquire()).toBe(true);
  });

  it("hands the latest foreground intent off after an active poll releases", () => {
    const gate = createConversationRequestGate<string>();
    expect(gate.acquire()).toBe(true);

    gate.queueLatest("session-a");
    gate.queueLatest("session-b");

    expect(gate.release()).toBe("session-b");
    expect(gate.acquire()).toBe(false);
    expect(gate.release()).toBeNull();
    expect(gate.acquire()).toBe(true);
  });

  it("can discard a pending foreground intent when the detail view closes", () => {
    const gate = createConversationRequestGate<string>();
    expect(gate.acquire()).toBe(true);
    gate.queueLatest("session-a");

    gate.clearPending();

    expect(gate.release()).toBeNull();
    expect(gate.acquire()).toBe(true);
  });
});

describe("isConversationResponseCurrent", () => {
  it("rejects responses after unmount even when the generation matches", () => {
    expect(
      isConversationResponseCurrent({ mounted: false, generation: 3, currentGeneration: 3 }),
    ).toBe(false);
  });

  it("rejects stale generations and accepts a mounted current response", () => {
    expect(
      isConversationResponseCurrent({ mounted: true, generation: 2, currentGeneration: 3 }),
    ).toBe(false);
    expect(
      isConversationResponseCurrent({ mounted: true, generation: 3, currentGeneration: 3 }),
    ).toBe(true);
  });
});

describe("isNearConversationTop", () => {
  it("treats a viewport within the threshold as being at the top", () => {
    expect(isNearConversationTop({ scrollTop: 41, clientHeight: 400, scrollHeight: 1000 })).toBe(
      false,
    );
    expect(isNearConversationTop({ scrollTop: 40, clientHeight: 400, scrollHeight: 1000 })).toBe(
      true,
    );
    expect(isNearConversationTop({ scrollTop: 0, clientHeight: 400, scrollHeight: 1000 })).toBe(
      true,
    );
  });
});

describe("conversationScrollLoadEdge", () => {
  const box = { scrollTop: 0, clientHeight: 400, scrollHeight: 2000 };

  it("loads earlier when the top edge is within reach and more history exists", () => {
    expect(
      conversationScrollLoadEdge({ ...box, scrollTop: 160, hasMoreBefore: true, hasMoreAfter: true }),
    ).toBe("earlier");
  });

  it("loads later when the bottom edge is within reach", () => {
    expect(
      conversationScrollLoadEdge({
        ...box,
        scrollTop: 1440,
        hasMoreBefore: true,
        hasMoreAfter: true,
      }),
    ).toBe("later");
  });

  it("prefers earlier when a short page shows both edges", () => {
    expect(
      conversationScrollLoadEdge({
        scrollTop: 0,
        clientHeight: 400,
        scrollHeight: 300,
        hasMoreBefore: true,
        hasMoreAfter: true,
      }),
    ).toBe("earlier");
  });

  it("stays put in the middle, without a next page, or before layout", () => {
    expect(
      conversationScrollLoadEdge({ ...box, scrollTop: 800, hasMoreBefore: true, hasMoreAfter: true }),
    ).toBeNull();
    expect(
      conversationScrollLoadEdge({ ...box, scrollTop: 0, hasMoreBefore: false, hasMoreAfter: false }),
    ).toBeNull();
    expect(
      conversationScrollLoadEdge({
        ...box,
        clientHeight: 0,
        hasMoreBefore: true,
        hasMoreAfter: true,
      }),
    ).toBeNull();
  });
});

describe("isNearConversationBottom", () => {
  it("treats a viewport within the threshold as being at the bottom", () => {
    expect(
      isNearConversationBottom({ scrollTop: 559, clientHeight: 400, scrollHeight: 1000 }),
    ).toBe(false);
    expect(
      isNearConversationBottom({ scrollTop: 560, clientHeight: 400, scrollHeight: 1000 }),
    ).toBe(true);
  });

  it("supports a custom threshold and short content", () => {
    expect(
      isNearConversationBottom({ scrollTop: 481, clientHeight: 500, scrollHeight: 1000 }, 20),
    ).toBe(true);
    expect(isNearConversationBottom({ scrollTop: 0, clientHeight: 500, scrollHeight: 300 })).toBe(
      true,
    );
  });
});

describe("conversationJumpScrollTop", () => {
  it("maps top and bottom edges to scroll offsets", () => {
    expect(conversationJumpScrollTop("top", 960)).toBe(0);
    expect(conversationJumpScrollTop("bottom", 960)).toBe(960);
    expect(conversationJumpScrollTop("bottom", -20)).toBe(0);
  });
});

describe("conversationJumpBehavior", () => {
  it("uses instant scrolling when reduced motion is requested", () => {
    expect(conversationJumpBehavior(true)).toBe("auto");
    expect(conversationJumpBehavior(false)).toBe("smooth");
  });
});

describe("conversationTimelineScrollTarget", () => {
  it("restores an away-from-bottom position when the event tab mounts again", () => {
    expect(
      conversationTimelineScrollTarget({
        wasAtBottom: false,
        savedScrollTop: 240,
        scrollHeight: 900,
      }),
    ).toBe(240);
  });

  it("uses the latest bottom when follow mode is active", () => {
    expect(
      conversationTimelineScrollTarget({
        wasAtBottom: true,
        savedScrollTop: 240,
        scrollHeight: 960,
      }),
    ).toBe(960);
  });
});

describe("nextConversationRevisionPollState", () => {
  it("records a failed revision once and still reloads a later revision", () => {
    expect(
      nextConversationRevisionPollState({
        revision: "broken-r2",
        changed: true,
        fileAvailable: true,
      }),
    ).toEqual({ knownRevision: "broken-r2", shouldReload: true });

    expect(
      nextConversationRevisionPollState({
        revision: "broken-r2",
        changed: false,
        fileAvailable: true,
      }),
    ).toEqual({ knownRevision: "broken-r2", shouldReload: false });

    expect(
      nextConversationRevisionPollState({
        revision: "repaired-r3",
        changed: true,
        fileAvailable: true,
      }),
    ).toEqual({ knownRevision: "repaired-r3", shouldReload: true });
  });
});

describe("nextConversationFollowState", () => {
  it("follows new events when the reader was already at the bottom", () => {
    expect(
      nextConversationFollowState({
        previousCount: 4,
        nextCount: 7,
        wasAtBottom: true,
        unseenCount: 2,
      }),
    ).toEqual({ shouldScroll: true, unseenCount: 0 });
  });

  it("keeps the scroll position and accumulates new events away from the bottom", () => {
    expect(
      nextConversationFollowState({
        previousCount: 4,
        nextCount: 7,
        wasAtBottom: false,
        unseenCount: 2,
      }),
    ).toEqual({ shouldScroll: false, unseenCount: 5 });
  });

  it("does not create false additions when the event count shrinks or resets", () => {
    expect(
      nextConversationFollowState({
        previousCount: 7,
        nextCount: 4,
        wasAtBottom: false,
        unseenCount: 2,
      }),
    ).toEqual({ shouldScroll: false, unseenCount: 2 });
    expect(
      nextConversationFollowState({
        previousCount: 4,
        nextCount: 0,
        wasAtBottom: false,
        unseenCount: 2,
      }),
    ).toEqual({ shouldScroll: false, unseenCount: 0 });
  });

  it("clamps invalid counters without reporting additions", () => {
    expect(
      nextConversationFollowState({
        previousCount: -2,
        nextCount: -1,
        wasAtBottom: false,
        unseenCount: -3,
      }),
    ).toEqual({ shouldScroll: false, unseenCount: 0 });
  });
});
