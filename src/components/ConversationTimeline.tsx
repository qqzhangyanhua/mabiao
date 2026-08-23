import { useLayoutEffect, useRef, type RefObject, type UIEvent } from "react";
import { useConversationEventPages } from "../lib/useConversationEventPages";
import type { ConversationAgentLink } from "../types";
import { ConversationAgentBranch } from "./ConversationAgentBranch";
import { ConversationEventItem } from "./ConversationEventItem";
import { EmptyState } from "./EmptyState";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

export type ConversationTimelineProps = {
  source: string;
  sessionId: string;
  revision: string;
  eventCount: number;
  agentLinks: ConversationAgentLink[];
  expandedRelationshipIds: string[];
  depth?: number;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  timelineRef?: RefObject<HTMLDivElement | null>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
  onCaptureScrollAnchor?: () => void;
  onRestoreScrollAnchor?: () => void;
};

export function ConversationTimeline({
  source,
  sessionId,
  revision,
  eventCount,
  agentLinks,
  expandedRelationshipIds,
  depth = 0,
  onToggleChild,
  onOpenChild,
  timelineRef,
  onScroll,
  onCaptureScrollAnchor,
  onRestoreScrollAnchor,
}: ConversationTimelineProps) {
  const { eventWindow, loading, loadingEarlier, error, loadEarlier, applyEventContent } =
    useConversationEventPages({ source, sessionId, revision });
  const events = eventWindow.events;
  const firstSequence = events[0]?.sequence;
  const nodeRef = useRef<HTMLDivElement | null>(null);
  const revealAnchorRef = useRef<number | null>(null);

  const setTimelineNode = (node: HTMLDivElement | null) => {
    nodeRef.current = node;
    if (timelineRef) {
      timelineRef.current = node;
    }
  };

  useLayoutEffect(() => {
    const node = nodeRef.current;
    const anchor = revealAnchorRef.current;
    revealAnchorRef.current = null;
    if (anchor !== null && node) {
      node.scrollTop = node.scrollHeight - anchor;
    }
    onRestoreScrollAnchor?.();
  }, [firstSequence, onRestoreScrollAnchor]);

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
              <div className="conversation-timeline-earlier">
                <span className="muted">上方还有更早事件</span>
                <Button
                  size="sm"
                  disabled={loadingEarlier}
                  onClick={() => {
                    const node = nodeRef.current;
                    revealAnchorRef.current = node
                      ? node.scrollHeight - node.scrollTop
                      : null;
                    onCaptureScrollAnchor?.();
                    void loadEarlier();
                  }}
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
            {events.map((event) => (
              <div className="conversation-event-group" key={event.event_id}>
                <ConversationEventItem
                  event={event}
                  source={source}
                  sessionId={sessionId}
                  onEventContentLoaded={applyEventContent}
                />
                {renderAgentLinks(linksForEvent(event.event_id))}
              </div>
            ))}
            {renderAgentLinks(trailingLinks)}
          </>
        )}
      </div>
    </div>
  );
}
