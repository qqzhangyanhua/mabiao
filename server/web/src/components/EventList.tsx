import { useState } from "react";
import type { SessionEvent } from "../api/types";
import {
  EVENT_FILTER_LABEL,
  EVENT_KIND_LABEL,
  filterEvents,
  type EventFilter,
} from "../lib/events";
import { formatDateTime } from "../lib/format";

const ACTOR_LABEL = { user: "用户", assistant: "助手", tool: "工具" } as const;

function EventRow({ event }: { event: SessionEvent }) {
  const heading = [
    event.actor ? ACTOR_LABEL[event.actor] : null,
    EVENT_KIND_LABEL[event.kind],
    event.name,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <li className="border-b border-slate-100 py-2 last:border-0">
      <div className="flex items-baseline gap-2 text-xs text-slate-500">
        <span className="font-medium text-slate-700">{heading}</span>
        {event.occurred_at && <span>{formatDateTime(event.occurred_at)}</span>}
      </div>
      {event.text ? (
        <pre className="mt-1 max-h-80 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
          {event.text}
        </pre>
      ) : (
        <p className="mt-1 text-xs text-slate-400">（没有正文）</p>
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
      <div className="mb-2 flex gap-1" role="group" aria-label="事件类别">
        {(Object.keys(EVENT_FILTER_LABEL) as EventFilter[]).map((f) => (
          <button
            key={f}
            type="button"
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
            className={`rounded px-3 py-1 text-sm ${filter === f ? "bg-slate-800 text-white" : "bg-slate-100 hover:bg-slate-200"}`}
          >
            {EVENT_FILTER_LABEL[f]}
          </button>
        ))}
        <span className="ml-2 self-center text-xs text-slate-500">
          {shown.length} / {events.length} 条
        </span>
      </div>
      {shown.length === 0 ? (
        <p className="py-4 text-sm text-slate-500">没有这一类事件</p>
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
