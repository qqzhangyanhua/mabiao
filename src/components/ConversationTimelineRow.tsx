import { useLayoutEffect, useRef, type ReactNode } from "react";
import type { TimelineRow } from "../lib/conversationTimelineVirtual";
import { firstUseItemKey, firstUseMarkerText } from "../lib/conversationContext";
import type {
  ConversationAgentLink,
  ConversationEvent,
  ConversationEventContentDto,
} from "../types";
import { ConversationAgentBranch } from "./ConversationAgentBranch";
import { ConversationChatTurn } from "./ConversationChatTurn";
import { ConversationEventItem } from "./ConversationEventItem";
import { ConversationUnadaptedGroup } from "./ConversationUnadaptedGroup";
import { Spinner } from "./Spinner";
import { Button } from "./ui/Button";

export function TimelineVirtualRow({
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

export function TimelineRowView({
  row,
  source,
  sessionId,
  highlightEventId,
  highlightQuery,
  highlightSnippet,
  agentLinks,
  expandedRelationshipIds,
  depth,
  loadingEarlier,
  loadingLater,
  pageError,
  onToggleChild,
  onOpenChild,
  onEventContentLoaded,
  onRevealAdjacent,
  onRevealContextItem,
}: {
  row: TimelineRow;
  source: string;
  sessionId: string;
  highlightEventId: string | null;
  highlightQuery: string | null;
  highlightSnippet: string | null;
  agentLinks: ConversationAgentLink[];
  expandedRelationshipIds: string[];
  depth: number;
  loadingEarlier: boolean;
  loadingLater: boolean;
  pageError: boolean;
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
  onRevealAdjacent: (direction: "earlier" | "later") => void;
  onRevealContextItem?: (itemKey: string) => void;
}) {
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
    const highlighted = event.event_id === highlightEventId;
    return (
      <div className="conversation-event-group" data-event-id={event.event_id} key={event.event_id}>
        <ConversationEventItem
          event={event}
          source={source}
          sessionId={sessionId}
          highlighted={highlighted}
          highlightQuery={highlighted ? highlightQuery : null}
          highlightSnippet={highlighted ? highlightSnippet : null}
          onEventContentLoaded={onEventContentLoaded}
        />
        {renderAgentLinks(linksForEvent(event.event_id))}
      </div>
    );
  }

  if (row.type === "chat") {
    return (
      <ConversationChatTurn
        turn={row.turn}
        source={source}
        sessionId={sessionId}
        highlightEventId={highlightEventId}
        highlightQuery={highlightQuery}
        highlightSnippet={highlightSnippet}
        onEventContentLoaded={onEventContentLoaded}
        linksFor={linksForEvent}
        renderLinks={renderAgentLinks}
      />
    );
  }
  if (row.type === "gate") {
    const earlier = row.edge === "before";
    const busy = earlier ? loadingEarlier : loadingLater;
    return (
      <div className="conversation-timeline-page-gate" role="status">
        {busy ? <Spinner size={12} /> : null}
        <span className="muted">
          {busy
            ? earlier
              ? "正在加载更早的事件"
              : "正在加载更新的事件"
            : earlier
              ? "上滑继续看更早的事件"
              : "下滑继续看更新的事件"}
        </span>
        {pageError ? (
          <Button
            size="sm"
            disabled={loadingEarlier || loadingLater}
            onClick={() => onRevealAdjacent(earlier ? "earlier" : "later")}
          >
            重试
          </Button>
        ) : null}
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
    return (
      <ConversationUnadaptedGroup
        events={row.events}
        renderEvent={renderTimelineEvent}
        defaultOpen={row.events.some((event) => event.event_id === highlightEventId)}
      />
    );
  }
  if (row.type === "trailing") {
    return <>{renderAgentLinks(row.links)}</>;
  }
  if (row.type === "first_use") {
    return (
      <button
        type="button"
        className="conversation-first-use-marker"
        onClick={() => onRevealContextItem?.(firstUseItemKey(row.marker))}
      >
        {firstUseMarkerText(row.marker)}
      </button>
    );
  }
  return renderTimelineEvent(row.event);
}