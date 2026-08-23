/**
 * 会话时间线的已加载窗口。
 *
 * 打开时锚定最新一页，之后向两端按页取数。窗口有上限：向一端越过上限时丢掉远离视口
 * 的另一端。revision 在跟随最新时整窗换成最新一页；离开底部读历史时保留当前窗口。
 */

import type { ConversationEventAnchor } from "../types";

export const CONVERSATION_EVENT_PAGE_SIZE = 200;
export const CONVERSATION_EVENT_WINDOW_MAX_PAGES = 5;
export const CONVERSATION_EVENT_WINDOW_LIMIT =
  CONVERSATION_EVENT_PAGE_SIZE * CONVERSATION_EVENT_WINDOW_MAX_PAGES;

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

export type ConversationEventTrimKeep = "start" | "end";

export function emptyConversationEventWindow<
  T extends { sequence: number },
>(): ConversationEventWindow<T> {
  return { events: [], hasMoreBefore: false, hasMoreAfter: false };
}

export function latestPageAnchor(): ConversationEventAnchor {
  return { type: "last" };
}

export function firstPageAnchor(): ConversationEventAnchor {
  return { type: "first" };
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

export function trimConversationEventWindow<T extends { sequence: number }>(
  current: ConversationEventWindow<T>,
  {
    keep,
    limit = CONVERSATION_EVENT_WINDOW_LIMIT,
  }: {
    keep: ConversationEventTrimKeep;
    limit?: number;
  },
): ConversationEventWindow<T> {
  const cap = Math.max(0, limit);
  if (current.events.length <= cap) {
    return current;
  }

  if (keep === "start") {
    return {
      events: current.events.slice(0, cap),
      hasMoreBefore: current.hasMoreBefore,
      hasMoreAfter: true,
    };
  }

  return {
    events: current.events.slice(current.events.length - cap),
    hasMoreBefore: true,
    hasMoreAfter: current.hasMoreAfter,
  };
}

export function advanceConversationEventWindow<T extends { sequence: number }>(
  current: ConversationEventWindow<T>,
  page: ConversationEventPageSlice<T>,
  mode: ConversationEventPageMode,
  limit = CONVERSATION_EVENT_WINDOW_LIMIT,
): ConversationEventWindow<T> {
  const merged = applyConversationEventPage(current, page, mode);
  if (mode === "replace") {
    return merged;
  }
  return trimConversationEventWindow(merged, {
    keep: mode === "prepend" ? "start" : "end",
    limit,
  });
}
