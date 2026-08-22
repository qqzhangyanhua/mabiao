import { describe, expect, it } from "vitest";
import {
  CONVERSATION_WINDOW_INITIAL,
  conversationWindowSlice,
  initialConversationHiddenCount,
  revealEarlierConversationEvents,
} from "./conversationWindow";

describe("initialConversationHiddenCount", () => {
  it("短会话全部渲染", () => {
    expect(initialConversationHiddenCount(0)).toBe(0);
    expect(initialConversationHiddenCount(CONVERSATION_WINDOW_INITIAL)).toBe(0);
  });

  it("长会话只留末尾一窗", () => {
    expect(initialConversationHiddenCount(1000, 60)).toBe(940);
  });
});

describe("revealEarlierConversationEvents", () => {
  it("按步长展开，不会越过开头", () => {
    expect(revealEarlierConversationEvents(200, 60)).toBe(140);
    expect(revealEarlierConversationEvents(20, 60)).toBe(0);
    expect(revealEarlierConversationEvents(0, 60)).toBe(0);
  });
});

describe("conversationWindowSlice", () => {
  const events = [1, 2, 3, 4, 5];

  it("没有隐藏时返回原数组本身，避免无谓的重新渲染", () => {
    expect(conversationWindowSlice(events, 0)).toBe(events);
  });

  it("窗口锚在末尾", () => {
    expect(conversationWindowSlice(events, 3)).toEqual([4, 5]);
  });

  it("隐藏数超出长度时不会越界", () => {
    expect(conversationWindowSlice(events, 99)).toEqual([]);
    expect(conversationWindowSlice(events, -1)).toBe(events);
  });
});
