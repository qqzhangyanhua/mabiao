import { useState } from "react";
import type { SessionEvent, SessionUsage } from "../api/types";
import {
  EVENT_FILTER_LABEL,
  EVENT_KIND_LABEL,
  filterEvents,
  type EventFilter,
} from "../lib/events";
import { COST_MODE_LABEL, costOf, type CostMode } from "../lib/cost";
import { formatCost, formatDateTime, formatTokens } from "../lib/format";
import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";
import { Segmented } from "./ui/segmented";

const ACTOR_LABEL = { user: "用户", assistant: "助手", tool: "工具" } as const;

const FILTERS = (Object.keys(EVENT_FILTER_LABEL) as EventFilter[]).map((id) => ({
  id,
  label: EVENT_FILTER_LABEL[id],
}));

function headingOf(event: SessionEvent): string {
  return [event.actor ? ACTOR_LABEL[event.actor] : null, EVENT_KIND_LABEL[event.kind], event.name]
    .filter(Boolean)
    .join(" · ");
}

function sideOf(event: SessionEvent): { input?: string; output?: string } {
  if (event.kind === "tool_result" || (event.actor === "assistant" && event.kind === "message")) {
    return { output: event.text ?? undefined };
  }
  return { input: event.text ?? undefined };
}

export function EventList({
  events,
  usage,
  mode,
}: {
  events: SessionEvent[];
  usage: SessionUsage;
  mode: CostMode;
}) {
  const [filter, setFilter] = useState<EventFilter>("all");
  const shown = filterEvents(events, filter);
  const [pickedId, setPickedId] = useState<string | undefined>(shown[0]?.event_id);
  const selected = shown.find((event) => event.event_id === pickedId) ?? shown[0];
  const selectedId = selected?.event_id;
  const io = selected ? sideOf(selected) : {};
  const totals = usage.totals;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;

  return (
    <div className="space-y-3">
      <div
        className={cn(panelClass, "flex gap-1 overflow-x-auto p-2")}
        role="list"
        aria-label="事件时间线"
      >
        {shown.map((event, index) => (
          <div key={event.event_id} role="listitem">
            <button
              type="button"
              title={headingOf(event)}
              onClick={() => setPickedId(event.event_id)}
              aria-pressed={event.event_id === selectedId}
              className={cn(
                "h-8 min-w-8 shrink-0 rounded-md px-2 text-xs tabular-nums focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                event.event_id === selectedId
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted text-muted-foreground hover:text-foreground",
              )}
            >
              {index + 1}
            </button>
          </div>
        ))}
        {shown.length === 0 && (
          <span className="px-2 py-1 text-sm text-muted-foreground">没有这一类事件</span>
        )}
      </div>

      <div className="grid grid-cols-1 gap-3 md:grid-cols-[18rem_1fr]">
        <section className={cn(panelClass, "p-3")}>
          <div className="mb-2">
            <Segmented label="事件类别" value={filter} onChange={setFilter} options={FILTERS} />
            <p className="mt-2 text-xs text-muted-foreground">
              {shown.length} / {events.length} 条
            </p>
          </div>
          {shown.length === 0 ? (
            <p className="py-4 text-sm text-muted-foreground">没有这一类事件</p>
          ) : (
            <ol className="max-h-[28rem] space-y-1 overflow-auto">
              {shown.map((event) => (
                <li key={event.event_id}>
                  <button
                    type="button"
                    onClick={() => setPickedId(event.event_id)}
                    aria-current={event.event_id === selectedId ? "step" : undefined}
                    className={cn(
                      "w-full rounded-md px-2 py-2 text-left text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      event.event_id === selectedId ? "bg-accent" : "hover:bg-muted",
                    )}
                  >
                    <div className="truncate font-medium">{headingOf(event)}</div>
                    {event.text && (
                      <div className="truncate text-xs text-muted-foreground">{event.text}</div>
                    )}
                    {event.occurred_at && (
                      <div className="text-xs text-muted-foreground">
                        {formatDateTime(event.occurred_at)}
                      </div>
                    )}
                  </button>
                </li>
              ))}
            </ol>
          )}
        </section>

        <section className={cn(panelClass, "p-4")}>
          {selected ? (
            <>
              <h3 className="text-sm font-medium">{headingOf(selected)}</h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {selected.occurred_at ? formatDateTime(selected.occurred_at) : "没有时间"}
              </p>
              {io.input && (
                <div className="mt-3">
                  <h4 className="text-xs text-muted-foreground">输入</h4>
                  <pre className="mt-1 max-h-72 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
                    {io.input}
                  </pre>
                </div>
              )}
              {io.output && (
                <div className="mt-3">
                  <h4 className="text-xs text-muted-foreground">输出</h4>
                  <pre className="mt-1 max-h-72 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
                    {io.output}
                  </pre>
                </div>
              )}
              {!io.input && !io.output && (
                <p className="mt-3 text-sm text-muted-foreground">（没有正文）</p>
              )}
              <div className="mt-4 border-t border-border pt-3 text-sm">
                <div className="text-xs text-muted-foreground">本场消耗（{COST_MODE_LABEL[mode]}）</div>
                <p className="mt-1">
                  {totals.record_count} 条记录 · {formatTokens(totals.total_tokens)} token ·{" "}
                  <span className="font-medium">{formatCost(cost)}</span>
                </p>
                {usage.by_model.length > 0 && (
                  <ul className="mt-2 space-y-1 text-xs text-muted-foreground">
                    {usage.by_model.map((row) => (
                      <li key={row.key} className="flex justify-between gap-3">
                        <span>{row.label}</span>
                        <span className="tabular-nums">
                          {formatTokens(row.total_tokens)} · {formatCost(costOf(row, mode))}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </>
          ) : (
            <p className="py-6 text-sm text-muted-foreground">选择左侧一步查看正文</p>
          )}
        </section>
      </div>
    </div>
  );
}
