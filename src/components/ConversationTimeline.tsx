import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
  type UIEvent,
} from "react";
import { useConversationEventPages } from "../lib/useConversationEventPages";
import {
  buildTimelineRows,
  pruneTimelineMeasurements,
  TIMELINE_ROW_ESTIMATE,
  timelineAnchorAtOffset,
  timelineHighlightIndex,
  timelineOffsetAt,
  timelineScrollCorrection,
  timelineScrollTopForAnchor,
  timelineViewKind,
  timelineVisibleRange,
  type TimelineHeightAnchor,
} from "../lib/conversationTimelineVirtual";
import type { ConversationAgentLink } from "../types";
import { TimelineRowView, TimelineVirtualRow } from "./ConversationTimelineRow";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";

export type ConversationTimelineHandle = {
  jumpToStart: () => Promise<void>;
  jumpToEnd: () => Promise<void>;
};

export type ConversationTimelineProps = {
  source: string;
  sessionId: string;
  revision: string;
  eventCount: number;
  agentLinks: ConversationAgentLink[];
  expandedRelationshipIds: string[];
  depth?: number;
  followLatest?: boolean;
  initialSequence?: number | null;
  highlightEventId?: string | null;
  highlightQuery?: string | null;
  highlightSnippet?: string | null;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  timelineRef?: RefObject<HTMLDivElement | null>;
  timelineApiRef?: RefObject<ConversationTimelineHandle | null>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
  onWindowChange?: (edges: { hasMoreBefore: boolean; hasMoreAfter: boolean }) => void;
  onCaptureScrollAnchor?: () => void;
};

export function ConversationTimeline({
  source,
  sessionId,
  revision,
  eventCount,
  agentLinks,
  expandedRelationshipIds,
  depth = 0,
  followLatest = false,
  initialSequence = null,
  highlightEventId = null,
  highlightQuery = null,
  highlightSnippet = null,
  onToggleChild,
  onOpenChild,
  timelineRef,
  timelineApiRef,
  onScroll,
  onWindowChange,
  onCaptureScrollAnchor,
}: ConversationTimelineProps) {
  const {
    eventWindow,
    loading,
    loadingEarlier,
    loadingLater,
    error,
    loadEarlier,
    loadLater,
    jumpToFirst,
    jumpToLast,
    applyEventContent,
  } = useConversationEventPages({
    source,
    sessionId,
    revision,
    followLatest,
    initialSequence,
  });
  const scrolledToHighlight = useRef(false);
  const events = eventWindow.events;
  const firstSequence = events[0]?.sequence;
  const lastSequence = events[events.length - 1]?.sequence;
  const nodeRef = useRef<HTMLDivElement | null>(null);
  const visibleAnchorRef = useRef<TimelineHeightAnchor | null>(null);
  const pendingJumpRef = useRef<"top" | "bottom" | null>(null);
  const fallbackApiRef = useRef<ConversationTimelineHandle | null>(null);
  const measuredLiveRef = useRef(new Map<string, number>());
  const measureQueued = useRef(false);
  const [viewport, setViewport] = useState({ scrollTop: 0, height: 0 });
  const [measured, setMeasured] = useState(() => new Map<string, number>());

  const setTimelineNode = (node: HTMLDivElement | null) => {
    nodeRef.current = node;
    if (timelineRef) {
      timelineRef.current = node;
    }
  };

  const rows = useMemo(
    () =>
      buildTimelineRows({
        events,
        hasMoreBefore: eventWindow.hasMoreBefore,
        hasMoreAfter: eventWindow.hasMoreAfter,
        error,
        agentLinks,
      }),
    [agentLinks, error, eventWindow.hasMoreAfter, eventWindow.hasMoreBefore, events],
  );

  const keys = useMemo(() => rows.map((row) => row.key), [rows]);

  const syncViewport = useCallback(() => {
    const node = nodeRef.current;
    if (!node) {
      return;
    }
    const next = { scrollTop: node.scrollTop, height: node.clientHeight };
    setViewport((current) =>
      current.scrollTop === next.scrollTop && current.height === next.height ? current : next,
    );
  }, []);

  const range = timelineVisibleRange({
    scrollTop: viewport.scrollTop,
    viewportHeight: viewport.height,
    keys,
    measured,
    preferEnd: followLatest && !eventWindow.hasMoreAfter,
  });

  const measureRow = useCallback(
    (key: string, height: number) => {
      if (height <= 0) {
        return;
      }
      const live = measuredLiveRef.current;
      const previousMeasured = live.get(key);
      if (previousMeasured === height) {
        return;
      }
      const previous = previousMeasured ?? TIMELINE_ROW_ESTIMATE;
      const index = keys.indexOf(key);
      const node = nodeRef.current;
      if (node && index >= 0) {
        const itemOffset = timelineOffsetAt(keys, index, live);
        const correction = timelineScrollCorrection({
          itemOffset,
          previousHeight: previous,
          nextHeight: height,
          scrollTop: node.scrollTop,
        });
        if (correction !== 0) {
          node.scrollTop += correction;
        }
      }
      live.set(key, height);
      if (measureQueued.current) {
        return;
      }
      measureQueued.current = true;
      queueMicrotask(() => {
        measureQueued.current = false;
        if (!nodeRef.current) {
          return;
        }
        setMeasured(new Map(measuredLiveRef.current));
        syncViewport();
      });
    },
    [keys, syncViewport],
  );

  useLayoutEffect(() => {
    const live = measuredLiveRef.current;
    const size = live.size;
    pruneTimelineMeasurements(live, keys);
    if (live.size !== size) {
      setMeasured(new Map(live));
    }
  }, [keys]);

  useLayoutEffect(() => {
    const node = nodeRef.current;
    const jump = pendingJumpRef.current;
    const visible = visibleAnchorRef.current;
    pendingJumpRef.current = null;
    visibleAnchorRef.current = null;
    if (node) {
      if (jump === "top") {
        node.scrollTop = 0;
      } else if (jump === "bottom") {
        node.scrollTop = node.scrollHeight;
      } else if (visible) {
        const top = timelineScrollTopForAnchor(visible, keys, measuredLiveRef.current);
        if (top !== null) {
          node.scrollTop = top;
        }
      } else if (followLatest && !eventWindow.hasMoreAfter) {
        node.scrollTop = node.scrollHeight;
      }
    }
    syncViewport();
  }, [
    eventWindow.hasMoreAfter,
    events.length,
    firstSequence,
    followLatest,
    keys,
    lastSequence,
    syncViewport,
  ]);

  useLayoutEffect(() => {
    if (!highlightEventId || loading || scrolledToHighlight.current) {
      return;
    }
    const node = nodeRef.current;
    if (!node) {
      return;
    }
    const index = timelineHighlightIndex(keys, highlightEventId);
    if (index < 0) {
      return;
    }
    const top = timelineOffsetAt(keys, index, measuredLiveRef.current);
    node.scrollTop = Math.max(0, top - 8);
    scrolledToHighlight.current = true;
    syncViewport();
  }, [highlightEventId, keys, loading, syncViewport]);

  useLayoutEffect(() => {
    const node = nodeRef.current;
    if (!node) {
      return;
    }
    const observer = new ResizeObserver(() => syncViewport());
    observer.observe(node);
    syncViewport();
    return () => observer.disconnect();
  }, [syncViewport]);

  useEffect(() => {
    onWindowChange?.({
      hasMoreBefore: eventWindow.hasMoreBefore,
      hasMoreAfter: eventWindow.hasMoreAfter,
    });
  }, [eventWindow.hasMoreAfter, eventWindow.hasMoreBefore, onWindowChange]);

  useImperativeHandle(timelineApiRef ?? fallbackApiRef, () => ({
    async jumpToStart() {
      const node = nodeRef.current;
      if (!eventWindow.hasMoreBefore) {
        if (node) {
          node.scrollTop = 0;
        }
        return;
      }
      pendingJumpRef.current = "top";
      await jumpToFirst();
    },
    async jumpToEnd() {
      pendingJumpRef.current = "bottom";
      await jumpToLast();
    },
  }));

  function revealAdjacent(direction: "earlier" | "later") {
    const node = nodeRef.current;
    const eligible = new Set(
      rows.filter((row) => row.type === "event" || row.type === "unadapted").map((row) => row.key),
    );
    visibleAnchorRef.current = node
      ? timelineAnchorAtOffset(
          keys,
          node.scrollTop,
          measuredLiveRef.current,
          TIMELINE_ROW_ESTIMATE,
          eligible,
        )
      : null;
    if (direction === "earlier") {
      onCaptureScrollAnchor?.();
      void loadEarlier();
    } else {
      void loadLater();
    }
  }

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    syncViewport();
    onScroll?.(event);
  }

  const viewKind = timelineViewKind({
    loading,
    error,
    eventCount,
    eventsLength: events.length,
    agentLinkCount: agentLinks.length,
  });

  return (
    <div
      className="conversation-timeline"
      aria-label="完整事件列表"
      ref={setTimelineNode}
      onScroll={handleScroll}
    >
      <div className="conversation-timeline-stack">
        {viewKind === "loading" ? (
          <div className="conversation-agent-loading">
            <Spinner size={16} />
          </div>
        ) : viewKind === "error" ? (
          <EmptyState icon="alertTriangle" tone="warn" title="无法读取事件页" hint={error ?? undefined} />
        ) : viewKind === "empty" ? (
          <EmptyState icon="chat" title="这条会话暂无事件" hint="当前会话没有可展示的语义事件。" />
        ) : (
          <>
            {range.paddingTop > 0 ? (
              <div
                className="conversation-timeline-spacer"
                style={{ height: range.paddingTop }}
                aria-hidden
              />
            ) : null}
            {rows.slice(range.start, range.end).map((row) => (
              <TimelineVirtualRow key={row.key} rowKey={row.key} onMeasure={measureRow}>
                <TimelineRowView
                  row={row}
                  source={source}
                  sessionId={sessionId}
                  highlightEventId={highlightEventId}
                  highlightQuery={highlightQuery}
                  highlightSnippet={highlightSnippet}
                  agentLinks={agentLinks}
                  expandedRelationshipIds={expandedRelationshipIds}
                  depth={depth}
                  loadingEarlier={loadingEarlier}
                  loadingLater={loadingLater}
                  onToggleChild={onToggleChild}
                  onOpenChild={onOpenChild}
                  onEventContentLoaded={applyEventContent}
                  onRevealAdjacent={revealAdjacent}
                />
              </TimelineVirtualRow>
            ))}
            {range.paddingBottom > 0 ? (
              <div
                className="conversation-timeline-spacer"
                style={{ height: range.paddingBottom }}
                aria-hidden
              />
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
