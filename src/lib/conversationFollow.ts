export type ConversationScrollMetrics = {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
};

export type ConversationRequestGate<T> = {
  acquire: () => boolean;
  queueLatest: (intent: T) => void;
  clearPending: () => void;
  release: () => T | null;
};

export function createConversationRequestGate<T = never>(): ConversationRequestGate<T> {
  let busy = false;
  let pending: T | null = null;
  return {
    acquire() {
      if (busy) {
        return false;
      }
      busy = true;
      return true;
    },
    queueLatest(intent) {
      pending = intent;
    },
    clearPending() {
      pending = null;
    },
    release() {
      if (pending !== null) {
        const intent = pending;
        pending = null;
        return intent;
      }
      busy = false;
      return null;
    },
  };
}

export function isConversationResponseCurrent({
  mounted,
  generation,
  currentGeneration,
}: {
  mounted: boolean;
  generation: number;
  currentGeneration: number;
}): boolean {
  return mounted && generation === currentGeneration;
}

export function isNearConversationTop(
  { scrollTop }: ConversationScrollMetrics,
  threshold = 40,
): boolean {
  return Math.max(0, scrollTop) <= Math.max(0, threshold);
}

export function isNearConversationBottom(
  { scrollTop, clientHeight, scrollHeight }: ConversationScrollMetrics,
  threshold = 40,
): boolean {
  return scrollHeight - scrollTop - clientHeight <= Math.max(0, threshold);
}

/** 滚到这一距离内就接着取下一页，不用点「加载更早 / 加载更新」。 */
export const CONVERSATION_SCROLL_LOAD_THRESHOLD = 160;

export type ConversationScrollLoadEdge = "earlier" | "later";

export function conversationScrollLoadEdge(
  metrics: ConversationScrollMetrics & {
    hasMoreBefore: boolean;
    hasMoreAfter: boolean;
  },
  threshold = CONVERSATION_SCROLL_LOAD_THRESHOLD,
): ConversationScrollLoadEdge | null {
  if (metrics.clientHeight <= 0) {
    return null;
  }
  if (metrics.hasMoreBefore && isNearConversationTop(metrics, threshold)) {
    return "earlier";
  }
  if (metrics.hasMoreAfter && isNearConversationBottom(metrics, threshold)) {
    return "later";
  }
  return null;
}

export type ConversationJumpEdge = "top" | "bottom";

export function conversationJumpScrollTop(
  edge: ConversationJumpEdge,
  scrollHeight: number,
): number {
  return edge === "top" ? 0 : Math.max(0, scrollHeight);
}

export function conversationJumpBehavior(reducedMotion: boolean): ScrollBehavior {
  return reducedMotion ? "auto" : "smooth";
}

export function conversationTimelineScrollTarget({
  wasAtBottom,
  savedScrollTop,
  scrollHeight,
}: {
  wasAtBottom: boolean;
  savedScrollTop: number;
  scrollHeight: number;
}): number {
  return wasAtBottom ? Math.max(0, scrollHeight) : Math.max(0, savedScrollTop);
}

export function nextConversationRevisionPollState({
  revision,
  changed,
  fileAvailable,
}: {
  revision: string;
  changed: boolean;
  fileAvailable: boolean;
}): { knownRevision: string; shouldReload: boolean } {
  return {
    knownRevision: revision,
    shouldReload: changed && fileAvailable,
  };
}

export type ConversationFollowInput = {
  previousCount: number;
  nextCount: number;
  wasAtBottom: boolean;
  unseenCount: number;
};

export type ConversationFollowState = {
  shouldScroll: boolean;
  unseenCount: number;
};

export function nextConversationFollowState({
  previousCount,
  nextCount,
  wasAtBottom,
  unseenCount,
}: ConversationFollowInput): ConversationFollowState {
  const normalizedPrevious = Math.max(0, previousCount);
  const normalizedNext = Math.max(0, nextCount);
  const normalizedUnseen = Math.min(Math.max(0, unseenCount), normalizedNext);
  const addedCount = Math.max(0, normalizedNext - normalizedPrevious);

  if (addedCount === 0) {
    return { shouldScroll: false, unseenCount: normalizedUnseen };
  }
  if (wasAtBottom) {
    return { shouldScroll: true, unseenCount: 0 };
  }
  return {
    shouldScroll: false,
    unseenCount: Math.min(normalizedNext, normalizedUnseen + addedCount),
  };
}
