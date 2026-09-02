import { useCallback, useRef, useState, type UIEvent } from "react";
import type { ConversationTimelineHandle } from "../components/ConversationTimeline";
import type { ConversationDetailTab } from "./conversationNavigation";
import {
  conversationJumpBehavior,
  conversationJumpScrollTop,
  conversationTimelineScrollTarget,
  isNearConversationBottom,
  isNearConversationTop,
  nextConversationFollowState,
  type ConversationJumpEdge,
} from "./conversationFollow";

export function useConversationTimelineFollow() {
  const [unseenCount, setUnseenCount] = useState(0);
  const [atTop, setAtTop] = useState(true);
  const [atBottom, setAtBottom] = useState(true);
  const timelineRef = useRef<HTMLDivElement>(null);
  const timelineApiRef = useRef<ConversationTimelineHandle | null>(null);
  const windowEdgesRef = useRef({ hasMoreBefore: false, hasMoreAfter: false });
  const wasAtBottomRef = useRef(true);
  const pendingScrollRef = useRef(false);
  const savedTimelineScrollTopRef = useRef(0);
  const unseenCountRef = useRef(0);
  const jumpingRef = useRef(false);
  const jumpTokenRef = useRef(0);
  const jumpTimerRef = useRef(0);

  const cancelJumps = useCallback(() => {
    jumpTokenRef.current += 1;
    jumpingRef.current = false;
    window.clearTimeout(jumpTimerRef.current);
  }, []);

  const captureTimelineAnchor = useCallback(() => {
    wasAtBottomRef.current = false;
    pendingScrollRef.current = false;
  }, []);

  const syncTimelineEdge = useCallback((timeline: HTMLElement) => {
    const nextAtTop = isNearConversationTop(timeline) && !windowEdgesRef.current.hasMoreBefore;
    const nextAtBottom =
      isNearConversationBottom(timeline) && !windowEdgesRef.current.hasMoreAfter;
    setAtTop(nextAtTop);
    setAtBottom(nextAtBottom);
    if (!jumpingRef.current) {
      wasAtBottomRef.current = nextAtBottom;
      savedTimelineScrollTopRef.current = timeline.scrollTop;
    }
    if (nextAtBottom) {
      unseenCountRef.current = 0;
      setUnseenCount(0);
    }
  }, []);

  const handleWindowChange = useCallback(
    (edges: { hasMoreBefore: boolean; hasMoreAfter: boolean }) => {
      windowEdgesRef.current = edges;
      if (timelineRef.current) {
        syncTimelineEdge(timelineRef.current);
      }
    },
    [syncTimelineEdge],
  );

  const pinTimelineLayout = useCallback(() => {
    const timeline = timelineRef.current;
    if (!timeline) {
      setAtTop(true);
      setAtBottom(true);
      return;
    }

    const pinToFollowedEdge = () => {
      timeline.scrollTop = conversationTimelineScrollTarget({
        wasAtBottom: pendingScrollRef.current || wasAtBottomRef.current,
        savedScrollTop: savedTimelineScrollTopRef.current,
        scrollHeight: timeline.scrollHeight,
      });
      pendingScrollRef.current = false;
      syncTimelineEdge(timeline);
    };

    pinToFollowedEdge();
    const stack = timeline.firstElementChild;
    if (!(stack instanceof HTMLElement)) {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (jumpingRef.current) {
        return;
      }
      if (wasAtBottomRef.current) {
        timeline.scrollTop = timeline.scrollHeight;
      }
      syncTimelineEdge(timeline);
    });
    observer.observe(stack);
    return () => observer.disconnect();
  }, [syncTimelineEdge]);

  const handleTimelineScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      syncTimelineEdge(event.currentTarget);
    },
    [syncTimelineEdge],
  );

  const jumpTimeline = useCallback(
    async (edge: ConversationJumpEdge) => {
      const token = ++jumpTokenRef.current;
      window.clearTimeout(jumpTimerRef.current);

      const hadUnseen = unseenCountRef.current > 0;
      if (edge === "top") {
        pendingScrollRef.current = false;
        wasAtBottomRef.current = false;
        setAtBottom(false);
      } else {
        wasAtBottomRef.current = true;
        setAtBottom(true);
        unseenCountRef.current = 0;
        setUnseenCount(0);
      }

      const needsReload =
        edge === "top"
          ? windowEdgesRef.current.hasMoreBefore
          : windowEdgesRef.current.hasMoreAfter || hadUnseen;
      if (needsReload) {
        jumpingRef.current = false;
        if (edge === "top") {
          await timelineApiRef.current?.jumpToStart();
        } else {
          await timelineApiRef.current?.jumpToEnd();
        }
        if (token !== jumpTokenRef.current) {
          return;
        }
        if (timelineRef.current) {
          syncTimelineEdge(timelineRef.current);
        } else {
          setAtTop(edge === "top");
          setAtBottom(edge === "bottom");
        }
        return;
      }

      const timeline = timelineRef.current;
      if (!timeline) {
        jumpingRef.current = false;
        pendingScrollRef.current = edge === "bottom";
        setAtTop(edge === "top");
        setAtBottom(edge === "bottom");
        return;
      }

      const top = conversationJumpScrollTop(edge, timeline.scrollHeight);
      const maxTop = Math.max(0, timeline.scrollHeight - timeline.clientHeight);
      const targetTop = edge === "top" ? 0 : maxTop;
      const behavior = conversationJumpBehavior(
        window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      );

      if (behavior === "auto" || Math.abs(timeline.scrollTop - targetTop) <= 40) {
        jumpingRef.current = false;
        timeline.scrollTop = top;
        syncTimelineEdge(timeline);
        return;
      }

      jumpingRef.current = true;
      timeline.scrollTo({ top, behavior: "smooth" });

      const settle = () => {
        if (token !== jumpTokenRef.current) {
          return;
        }
        jumpingRef.current = false;
        if (edge === "bottom") {
          timeline.scrollTop = timeline.scrollHeight;
        }
        syncTimelineEdge(timeline);
      };

      const onScrollEnd = () => {
        timeline.removeEventListener("scrollend", onScrollEnd);
        settle();
      };
      timeline.addEventListener("scrollend", onScrollEnd);
      jumpTimerRef.current = window.setTimeout(() => {
        timeline.removeEventListener("scrollend", onScrollEnd);
        settle();
      }, 1200);
    },
    [syncTimelineEdge],
  );

  const rememberEventsScroll = useCallback(
    (detailTab: ConversationDetailTab, nextTab: ConversationDetailTab) => {
    if (detailTab !== "events" || nextTab === "events") {
      return;
    }
    const timeline = timelineRef.current;
    if (timeline) {
      savedTimelineScrollTopRef.current = timeline.scrollTop;
      wasAtBottomRef.current =
        isNearConversationBottom(timeline) && !windowEdgesRef.current.hasMoreAfter;
    }
  }, []);

  const applyFollowedReplace = useCallback((previousCount: number, nextCount: number) => {
    const follow = nextConversationFollowState({
      previousCount,
      nextCount,
      wasAtBottom: wasAtBottomRef.current,
      unseenCount: unseenCountRef.current,
    });
    pendingScrollRef.current = follow.shouldScroll;
    unseenCountRef.current = follow.unseenCount;
    setUnseenCount(follow.unseenCount);
  }, []);

  const pinToLatest = useCallback(() => {
    pendingScrollRef.current = true;
    wasAtBottomRef.current = true;
    unseenCountRef.current = 0;
    setUnseenCount(0);
  }, []);

  const prepareOpen = useCallback(
    (bodyHit: boolean) => {
      savedTimelineScrollTopRef.current = 0;
      wasAtBottomRef.current = !bodyHit;
      pendingScrollRef.current = !bodyHit;
      if (bodyHit) {
        setAtBottom(false);
        setAtTop(false);
      }
      cancelJumps();
      unseenCountRef.current = 0;
      setUnseenCount(0);
      windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
    },
    [cancelJumps],
  );

  const prepareEnterChild = useCallback(() => {
    savedTimelineScrollTopRef.current = 0;
    wasAtBottomRef.current = true;
    pendingScrollRef.current = true;
    cancelJumps();
    unseenCountRef.current = 0;
    setUnseenCount(0);
    windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
  }, [cancelJumps]);

  const prepareBack = useCallback(
    (scrollTop: number) => {
      savedTimelineScrollTopRef.current = scrollTop;
      wasAtBottomRef.current = false;
      pendingScrollRef.current = false;
      cancelJumps();
      unseenCountRef.current = 0;
      setUnseenCount(0);
    },
    [cancelJumps],
  );

  const prepareClose = useCallback(() => {
    windowEdgesRef.current = { hasMoreBefore: false, hasMoreAfter: false };
    savedTimelineScrollTopRef.current = 0;
    unseenCountRef.current = 0;
    setUnseenCount(0);
    pendingScrollRef.current = false;
    cancelJumps();
  }, [cancelJumps]);

  return {
    timelineRef,
    timelineApiRef,
    atTop,
    atBottom,
    unseenCount,
    captureTimelineAnchor,
    handleWindowChange,
    handleTimelineScroll,
    jumpTimeline,
    rememberEventsScroll,
    pinTimelineLayout,
    applyFollowedReplace,
    pinToLatest,
    prepareOpen,
    prepareEnterChild,
    prepareBack,
    prepareClose,
    cancelJumps,
    parentScrollTop: () => timelineRef.current?.scrollTop ?? 0,
  };
}

export type ConversationTimelineFollow = ReturnType<typeof useConversationTimelineFollow>;
