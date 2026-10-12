import { useState } from "react";
import type { SessionEvent, SessionUsage } from "../api/types";
import {
  EVENT_KIND_LABEL,
  groupSessionTurns,
  sessionStepCount,
  type SessionChatTurn,
} from "../lib/events";
import { COST_MODE_LABEL, costOf, type CostMode } from "../lib/cost";
import { formatCost, formatDateTime, formatTokens } from "../lib/format";
import { cn } from "../lib/cn";
import { sourceLabel } from "../lib/source";
import { EventTimeline } from "./EventTimeline";
import { SourceIcon } from "./SourceIcon";
import { panelClass } from "./ui/card";
import { UserAvatar } from "./UserAvatar";

const ACTOR_LABEL = { user: "用户", assistant: "助手", tool: "工具" } as const;

function headingOf(event: SessionEvent): string {
  return [event.actor ? ACTOR_LABEL[event.actor] : null, EVENT_KIND_LABEL[event.kind], event.name]
    .filter(Boolean)
    .join(" · ");
}

function eventDomId(eventId: string): string {
  return `session-event-${eventId}`;
}

const BODY_TEXT =
  "min-w-0 max-w-full whitespace-pre-wrap font-sans text-sm [overflow-wrap:anywhere]";

function TurnBlock({
  turn,
  source,
  sessionId,
  selectedId,
}: {
  turn: SessionChatTurn;
  source: string;
  sessionId: string;
  selectedId: string | undefined;
}) {
  const steps = sessionStepCount(turn);
  const [stepsOpen, setStepsOpen] = useState(true);
  const user = turn.role === "user";
  const selected = turn.parts.some((part) =>
    part.type === "tools"
      ? part.steps.some(
          (step) => step.call.event_id === selectedId || step.result?.event_id === selectedId,
        )
      : part.event.event_id === selectedId,
  );
  return (
    <li className={cn("flex min-w-0 max-w-full scroll-mt-4 items-start gap-2.5", user && "flex-row-reverse")}>
      <div className="shrink-0">
        {user ? <UserAvatar seed={sessionId} /> : <SourceIcon source={source} size={32} />}
      </div>
      <div
        className={cn(
          "grid min-w-0 gap-1.5",
          user ? "w-fit max-w-[78%]" : "w-full max-w-[92%] flex-1",
          selected && "rounded-xl ring-1 ring-primary/50",
        )}
      >
        <div className={cn("flex items-baseline gap-2", user && "justify-end")}>
          <h3 className="text-sm font-medium">{user ? "用户" : sourceLabel(source)}</h3>
          <p className="text-xs text-muted-foreground">
            {turn.occurredAt ? formatDateTime(turn.occurredAt) : "没有时间"}
          </p>
        </div>
        {steps > 0 && (
          <button
            type="button"
            className={cn("w-fit text-xs text-muted-foreground", user && "justify-self-end")}
            aria-expanded={stepsOpen}
            onClick={() => setStepsOpen((open) => !open)}
          >
            {stepsOpen ? `收起 ${steps} 个步骤` : `展开 ${steps} 个步骤`}
          </button>
        )}
        <div className={cn("grid min-w-0 gap-2", user && "justify-items-end")}>
          {turn.parts.map((part) => {
            if (part.type === "tools") {
              if (!stepsOpen) return null;
              const names = part.steps
                .map((step) => step.call.name)
                .filter((name): name is string => Boolean(name))
                .slice(0, 3)
                .join("、");
              return (
                <details key={part.steps[0]?.call.event_id} className="w-full min-w-0 max-w-full text-sm">
                  <summary className="cursor-pointer">
                    完成了 {part.steps.length} 步操作
                    {names ? ` · ${names}` : ""}
                  </summary>
                  <div className="mt-2 space-y-2 border-l border-border pl-3">
                    {part.steps.map((step) => (
                      <div key={step.call.event_id} id={eventDomId(step.call.event_id)}>
                        {step.call.name && (
                          <p className="text-xs text-muted-foreground">{step.call.name}</p>
                        )}
                        {step.call.text && <pre className={BODY_TEXT}>{step.call.text}</pre>}
                        {step.result?.text && (
                          <pre className={cn(BODY_TEXT, "text-muted-foreground")}>{step.result.text}</pre>
                        )}
                      </div>
                    ))}
                  </div>
                </details>
              );
            }
            const prose = part.type === "prose";
            return (
              <div
                key={part.event.event_id}
                id={eventDomId(part.event.event_id)}
                className={
                  prose
                    ? cn(
                        "w-fit min-w-0 max-w-full rounded-2xl px-3 py-2",
                        user ? "rounded-br-md" : "rounded-bl-md bg-muted",
                      )
                    : undefined
                }
                style={
                  prose && user
                    ? { background: "color-mix(in srgb, #22d3ee 16%, var(--card))" }
                    : undefined
                }
              >
                {part.type === "notice" && (
                  <p className="text-xs text-muted-foreground">{EVENT_KIND_LABEL[part.event.kind]}</p>
                )}
                {part.event.text ? (
                  <pre className={BODY_TEXT}>{part.event.text}</pre>
                ) : (
                  <p className="text-sm text-muted-foreground">（没有正文）</p>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </li>
  );
}

export function EventList({
  events,
  usage,
  mode,
  source,
  sessionId,
  startedAt,
  endedAt,
}: {
  events: SessionEvent[];
  usage: SessionUsage;
  mode: CostMode;
  source: string;
  sessionId: string;
  startedAt?: string | null;
  endedAt?: string | null;
}) {
  const [pickedId, setPickedId] = useState<string | undefined>(events[0]?.event_id);
  const selected = events.find((event) => event.event_id === pickedId) ?? events[0];
  const selectedId = selected?.event_id;
  const totals = usage.totals;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;

  function select(eventId: string) {
    setPickedId(eventId);
    const node = document.getElementById(eventDomId(eventId));
    if (node && typeof node.scrollIntoView === "function") {
      node.scrollIntoView({ block: "nearest" });
    }
  }

  return (
    <div className="min-w-0 space-y-3">
      <section className={cn(panelClass, "p-4 text-sm")}>
        <div className="text-xs text-muted-foreground">本场消耗（{COST_MODE_LABEL[mode]}）</div>
        <p className="mt-1 tabular-nums">
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
      </section>

      <EventTimeline
        events={events}
        selectedId={selectedId}
        onSelect={select}
        startedAt={startedAt}
        endedAt={endedAt}
        headingOf={headingOf}
      />

      <section className={cn(panelClass, "min-w-0 p-4")}>
        {events.length === 0 ? (
          <p className="py-4 text-sm text-muted-foreground">没有事件</p>
        ) : (
          <ol className="min-w-0 space-y-5">
            {groupSessionTurns(events).map((turn) => (
              <TurnBlock
                key={turn.id}
                turn={turn}
                source={source}
                sessionId={sessionId}
                selectedId={selectedId}
              />
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}
