import { useState } from "react";
import type { SessionEvent } from "../api/types";
import {
  EVENT_FILTER_LABEL,
  EVENT_KIND_LABEL,
  filterEvents,
  type EventFilter,
} from "../lib/events";
import { formatDateTime } from "../lib/format";
import { Segmented } from "./ui/segmented";

const ACTOR_LABEL = { user: "用户", assistant: "助手", tool: "工具" } as const;

const FILTERS = (Object.keys(EVENT_FILTER_LABEL) as EventFilter[]).map((id) => ({
  id,
  label: EVENT_FILTER_LABEL[id],
}));

function EventRow({ event }: { event: SessionEvent }) {
  const heading = [
    event.actor ? ACTOR_LABEL[event.actor] : null,
    EVENT_KIND_LABEL[event.kind],
    event.name,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <li className="border-b border-border/70 py-2 last:border-0">
      <div className="flex items-baseline gap-2 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{heading}</span>
        {event.occurred_at && <span>{formatDateTime(event.occurred_at)}</span>}
      </div>
      {event.text ? (
        <pre className="mt-1 max-h-80 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
          {event.text}
        </pre>
      ) : (
        <p className="mt-1 text-xs text-muted-foreground">（没有正文）</p>
      )}
    </li>
  );
}

/** 会话正文与工具调用，可按类别过滤。 */
export function EventList({ events }: { events: SessionEvent[] }) {
  const [filter, setFilter] = useState<EventFilter>("all");
  const shown = filterEvents(events, filter);
  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <Segmented label="事件类别" value={filter} onChange={setFilter} options={FILTERS} />
        <span className="text-xs text-muted-foreground">
          {shown.length} / {events.length} 条
        </span>
      </div>
      {shown.length === 0 ? (
        <p className="py-4 text-sm text-muted-foreground">没有这一类事件</p>
      ) : (
        <ul>
          {shown.map((event) => (
            <EventRow key={event.event_id} event={event} />
          ))}
        </ul>
      )}
    </div>
  );
}
