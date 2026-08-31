import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  type RefObject,
  type UIEvent,
} from "react";
import { useConversationEventPages } from "../lib/useConversationEventPages";
import {
  groupTimelineEvents,
  unadaptedGroupLabel,
} from "../lib/conversationEventDisplay";
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

type VisibleEventAnchor = { eventId: string; offset: number };

function captureVisibleEventAnchor(node: HTMLElement): VisibleEventAnchor | null {
  const groups = node.querySelectorAll<HTMLElement>("[data-event-id]");
  for (const group of groups) {
    const eventId = group.dataset.eventId;
    if (!eventId || group.offsetHeight === 0) {
      continue;
    }
    if (group.offsetTop + group.offsetHeight > node.scrollTop) {
      return { eventId, offset: group.offsetTop - node.scrollTop };
    }
  }
  return null;
}

function restoreVisibleEventAnchor(node: HTMLElement, anchor: VisibleEventAnchor) {
  const target = node.querySelector(`[data-event-id="${CSS.escape(anchor.eventId)}"]`);
  if (!(target instanceof HTMLElement)) {
    return;
  }
  node.scrollTop = target.offsetTop - anchor.offset;
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
  const visibleAnchorRef = useRef<VisibleEventAnchor | null>(null);
  const pendingJumpRef = useRef<"top" | "bottom" | null>(null);
  const fallbackApiRef = useRef<ConversationTimelineHandle | null>(null);

  const setTimelineNode = (node: HTMLDivElement | null) => {
    nodeRef.current = node;
    if (timelineRef) {
      timelineRef.current = node;
    }
  };

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
        restoreVisibleEventAnchor(node, visible);
      }
    }
  }, [firstSequence, lastSequence, events.length]);

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

  const eventIds = new Set(events.map((event) => event.event_id));
  const linksForEvent = (eventId: string) =>
    agentLinks.filter((link) => link.launch_event_id === eventId);
  const trailingLinks = agentLinks.filter(
    (link) => link.launch_event_id === null || !eventIds.has(link.launch_event_id),
  );

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
    visibleAnchorRef.current = node ? captureVisibleEventAnchor(node) : null;
    if (direction === "earlier") {
      onCaptureScrollAnchor?.();
      void loadEarlier();
    } else {
      void loadLater();
    }
  }

  return (
    <div
      className="conversation-timeline"
      aria-label="完整事件列表"
      ref={setTimelineNode}
      onScroll={onScroll}
    >
      <div className="conversation-timeline-stack">
        {loading && events.length === 0 ? (
          <div className="conversation-agent-loading">
            <Spinner size={16} />
          </div>
        ) : error && events.length === 0 ? (
          <EmptyState icon="alertTriangle" tone="warn" title="无法读取事件页" hint={error} />
        ) : eventCount === 0 && events.length === 0 && agentLinks.length === 0 ? (
          <EmptyState icon="chat" title="这条会话暂无事件" hint="当前会话没有可展示的语义事件。" />
        ) : (
          <>
            {eventWindow.hasMoreBefore ? (
              <div className="conversation-timeline-page-gate">
                <span className="muted">上方还有更早事件</span>
                <Button
                  size="sm"
                  disabled={loadingEarlier || loadingLater}
                  onClick={() => revealAdjacent("earlier")}
                >
                  {loadingEarlier ? <Spinner size={12} /> : null}
                  加载更早
                </Button>
              </div>
            ) : null}
            {error ? (
              <span className="conversation-inline-error" role="alert">
                {error}
              </span>
            ) : null}
            {groupTimelineEvents(events).map((group) => {
              if (group.type === "unadapted") {
                const firstId = group.events[0]?.event_id;
                return (
                  <details className="conversation-unadapted-group" key={firstId}>
                    <summary>{unadaptedGroupLabel(group.events)}</summary>
                    {group.events.map((event) => renderTimelineEvent(event))}
                  </details>
                );
              }
              return renderTimelineEvent(group.event);
            })}
            {eventWindow.hasMoreAfter ? (
              <div className="conversation-timeline-page-gate">
                <span className="muted">下方还有更新事件</span>
                <Button
                  size="sm"
                  disabled={loadingEarlier || loadingLater}
                  onClick={() => revealAdjacent("later")}
                >
                  {loadingLater ? <Spinner size={12} /> : null}
                  加载更新
                </Button>
              </div>
            ) : null}
            {renderAgentLinks(trailingLinks)}
          </>
        )}
      </div>
    </div>
  );
}
