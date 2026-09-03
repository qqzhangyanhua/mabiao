import { useLayoutEffect, useRef, type ReactNode } from "react";
import type { TimelineRow } from "../lib/conversationTimelineVirtual";
import type {
  ConversationAgentLink,
  ConversationEvent,
  ConversationEventContentDto,
} from "../types";
import { ConversationAgentBranch } from "./ConversationAgentBranch";
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
  onToggleChild,
  onOpenChild,
  onEventContentLoaded,
  onRevealAdjacent,
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
  onToggleChild: (link: ConversationAgentLink) => void;
  onOpenChild: (link: ConversationAgentLink) => void;
  onEventContentLoaded: (content: ConversationEventContentDto) => void;
  onRevealAdjacent: (direction: "earlier" | "later") => void;
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

  if (row.type === "gate") {
    const earlier = row.edge === "before";
    return (
      <div className="conversation-timeline-page-gate">
        <span className="muted">{earlier ? "上方还有更早事件" : "下方还有更新事件"}</span>
        <Button
          size="sm"
          disabled={loadingEarlier || loadingLater}
          onClick={() => onRevealAdjacent(earlier ? "earlier" : "later")}
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
  return renderTimelineEvent(row.event);
}