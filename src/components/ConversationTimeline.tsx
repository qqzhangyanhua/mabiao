import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
  type UIEvent,
} from "react";
import { useConversationEventPages } from "../lib/useConversationEventPages";
import { groupTimelineEvents, unadaptedGroupLabel } from "../lib/conversationEventDisplay";
import {
  pruneTimelineMeasurements,
  TIMELINE_ROW_ESTIMATE,
  timelineAnchorAtOffset,
  timelineOffsetAt,
  timelineScrollCorrection,
  timelineScrollTopForAnchor,
  timelineVisibleRange,
  type TimelineHeightAnchor,
} from "../lib/conversationTimelineVirtual";
import type { ConversationAgentLink, ConversationEvent } from "../types";
import { ConversationAgentBranch } from "./ConversationAgentBranch";
import { ConversationEventItem } from "./ConversationEventItem";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

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
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  timelineRef?: RefObject<HTMLDivElement | null>;
  timelineApiRef?: RefObject<ConversationTimelineHandle | null>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
  onWindowChange?: (edges: { hasMoreBefore: boolean; hasMoreAfter: boolean }) => void;
  onCaptureScrollAnchor?: () => void;
};

type TimelineRow =
  | { key: string; type: "gate"; edge: "before" | "after" }
  | { key: string; type: "error"; message: string }
  | { key: string; type: "event"; event: ConversationEvent }
  | { key: string; type: "unadapted"; events: ConversationEvent[] }
  | { key: string; type: "trailing"; links: ConversationAgentLink[] };

function TimelineVirtualRow({
  rowKey,
  onMeasure,
  children,
}: {
  rowKey: string;
  onMeasure: (key: string, height: number) => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) {
      return;
    }
    const report = () => onMeasure(rowKey, node.offsetHeight);
    report();
    const observer = new ResizeObserver(report);
    observer.observe(node);
    return () => observer.disconnect();
  }, [onMeasure, rowKey]);
  return (
    <div className="conversation-timeline-row" ref={ref}>
      {children}
    </div>
  );
}

function UnadaptedEventGroup({
  events,
  renderEvent,
}: {
  events: ConversationEvent[];
  renderEvent: (event: ConversationEvent) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <details
      className="conversation-unadapted-group"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>{unadaptedGroupLabel(events)}</summary>
      {open ? events.map((event) => renderEvent(event)) : null}
    </details>
  );
}

export function ConversationTimeline({
  source,
  sessionId,
  revision,
  eventCount,
  agentLinks,
  expandedRelationshipIds,
  depth = 0,
  followLatest = false,
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
  } = useConversationEventPages({ source, sessionId, revision, followLatest });
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

  const rows = useMemo(() => {
    const eventIds = new Set(events.map((event) => event.event_id));
    const next: TimelineRow[] = [];
    if (eventWindow.hasMoreBefore) {
      next.push({ key: "gate:before", type: "gate", edge: "before" });
    }
    if (error) {
      next.push({ key: "error", type: "error", message: error });
    }
    for (const group of groupTimelineEvents(events)) {
      if (group.type === "unadapted") {
        next.push({ key: "unadapted", type: "unadapted", events: group.events });
      } else {
        next.push({ key: `event:${group.event.event_id}`, type: "event", event: group.event });
      }
    }
    if (eventWindow.hasMoreAfter) {
      next.push({ key: "gate:after", type: "gate", edge: "after" });
    }
    const trailing = agentLinks.filter(
      (link) => link.launch_event_id === null || !eventIds.has(link.launch_event_id),
    );
    if (trailing.length > 0) {
      next.push({ key: "trailing", type: "trailing", links: trailing });
    }
    return next;
  }, [agentLinks, error, eventWindow.hasMoreAfter, eventWindow.hasMoreBefore, events]);

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

  const linksForEvent = (eventId: string) =>
    agentLinks.filter((link) => link.launch_event_id === eventId);

  function renderAgentLinks(links: ConversationAgentLink[]) {
    return links.map((link) => (
      <ConversationAgentBranch
        key={link.relationship_id}
        link={link}
        expanded={expandedRelationshipIds.includes(link.relationship_id)}
        expandedRelationshipIds={expandedRelationshipIds}
        depth={depth}
        onToggleChild={onToggleChild}
        onOpenChild={onOpenChild}
      />
    ));
  }

  function renderTimelineEvent(event: ConversationEvent) {
    return (
      <div className="conversation-event-group" data-event-id={event.event_id} key={event.event_id}>
        <ConversationEventItem
          event={event}
          source={source}
          sessionId={sessionId}
          onEventContentLoaded={applyEventContent}
        />
        {renderAgentLinks(linksForEvent(event.event_id))}
      </div>
    );
  }

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

  function renderRow(row: TimelineRow) {
    if (row.type === "gate") {
      const earlier = row.edge === "before";
      return (
        <div className="conversation-timeline-page-gate">
          <span className="muted">{earlier ? "上方还有更早事件" : "下方还有更新事件"}</span>
          <Button
            size="sm"
            disabled={loadingEarlier || loadingLater}
            onClick={() => revealAdjacent(earlier ? "earlier" : "later")}
          >
            {(earlier ? loadingEarlier : loadingLater) ? <Spinner size={12} /> : null}
            {earlier ? "加载更早" : "加载更新"}
          </Button>
        </div>
      );
    }
    if (row.type === "error") {
      return (
        <span className="conversation-inline-error" role="alert">
          {row.message}
        </span>
      );
    }
    if (row.type === "unadapted") {
      return <UnadaptedEventGroup events={row.events} renderEvent={renderTimelineEvent} />;
    }
    if (row.type === "trailing") {
      return <>{renderAgentLinks(row.links)}</>;
    }
    return renderTimelineEvent(row.event);
  }

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    syncViewport();
    onScroll?.(event);
  }

  const showEmpty =
    !loading && !error && eventCount === 0 && events.length === 0 && agentLinks.length === 0;

  return (
    <div
      className="conversation-timeline"
      aria-label="完整事件列表"
      ref={setTimelineNode}
      onScroll={handleScroll}
    >
      <div className="conversation-timeline-stack">
        {loading && events.length === 0 ? (
          <div className="conversation-agent-loading">
            <Spinner size={16} />
          </div>
        ) : error && events.length === 0 ? (
          <EmptyState icon="alertTriangle" tone="warn" title="无法读取事件页" hint={error} />
        ) : showEmpty ? (
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
                {renderRow(row)}
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
