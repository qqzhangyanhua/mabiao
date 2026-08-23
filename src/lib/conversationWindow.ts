/**
 * 会话时间线的已加载窗口。
 *
 * 打开时锚定最新一页，之后只向两端追加；已加载的页先留着。revision 变化时整窗丢弃，
 * 重新拉最新一页——游标是 sequence，源文件中段被改写后旧序号不再可靠。
 */

import type { ConversationEventAnchor } from "../types";

export const CONVERSATION_EVENT_PAGE_SIZE = 200;

export type { ConversationEventAnchor };

export type ConversationEventPageSlice<T extends { sequence: number }> = {
  events: T[];
  has_more_before: boolean;
  has_more_after: boolean;
};

export type ConversationEventWindow<T extends { sequence: number }> = {
  events: T[];
  hasMoreBefore: boolean;
  hasMoreAfter: boolean;
};

export type ConversationEventPageMode = "replace" | "prepend" | "append";

export function emptyConversationEventWindow<
  T extends { sequence: number },
>(): ConversationEventWindow<T> {
  return { events: [], hasMoreBefore: false, hasMoreAfter: false };
}

export function latestPageAnchor(): ConversationEventAnchor {
  return { type: "last" };
}

export function nextEarlierAnchor<T extends { sequence: number }>(
  window: ConversationEventWindow<T>,
): ConversationEventAnchor | null {
  if (!window.hasMoreBefore || window.events.length === 0) {
    return null;
  }
  return { type: "before", sequence: window.events[0].sequence };
}

export function nextLaterAnchor<T extends { sequence: number }>(
  window: ConversationEventWindow<T>,
): ConversationEventAnchor | null {
  if (!window.hasMoreAfter || window.events.length === 0) {
    return null;
  }
  return { type: "after", sequence: window.events[window.events.length - 1].sequence };
}

export function shouldResetConversationEventWindow(
  currentRevision: string | null,
  nextRevision: string,
): boolean {
  return currentRevision !== null && currentRevision !== nextRevision;
}

export function applyConversationEventPage<T extends { sequence: number }>(
  current: ConversationEventWindow<T>,
  page: ConversationEventPageSlice<T>,
  mode: ConversationEventPageMode,
): ConversationEventWindow<T> {
  if (mode === "replace") {
    return {
      events: page.events,
      hasMoreBefore: page.has_more_before,
      hasMoreAfter: page.has_more_after,
    };
  }

  const seen = new Set(current.events.map((event) => event.sequence));
  const incoming = page.events.filter((event) => !seen.has(event.sequence));
  if (mode === "prepend") {
    return {
      events: [...incoming, ...current.events],
      hasMoreBefore: page.has_more_before,
      hasMoreAfter: current.hasMoreAfter,
    };
  }
  return {
    events: [...current.events, ...incoming],
    hasMoreBefore: current.hasMoreBefore,
    hasMoreAfter: page.has_more_after,
  };
}
