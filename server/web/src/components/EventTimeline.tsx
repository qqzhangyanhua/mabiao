import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import type { SessionEvent } from "../api/types";
import { formatDateTime } from "../lib/format";
import {
  EVENT_KIND_TONE,
  nearestTickId,
  percentFromClientX,
  timelineSpan,
  timelineTicks,
} from "../lib/timeline";
import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

export function EventTimeline({
  events,
  selectedId,
  onSelect,
  startedAt,
  endedAt,
  headingOf,
}: {
  events: SessionEvent[];
  selectedId: string | undefined;
  onSelect: (eventId: string) => void;
  startedAt?: string | null;
  endedAt?: string | null;
  headingOf: (event: SessionEvent) => string;
}) {
  const rail = useRef<HTMLDivElement>(null);
  const span = timelineSpan(events, startedAt, endedAt);
  const ticks = timelineTicks(events, span);
  const selected = events.find((event) => event.event_id === selectedId);
  const selectedTick = ticks.find((tick) => tick.event_id === selectedId);
  const index = Math.max(0, events.findIndex((event) => event.event_id === selectedId));

  function pickAt(clientX: number) {
    const box = rail.current?.getBoundingClientRect();
    if (!box) return;
    const id = nearestTickId(ticks, percentFromClientX(clientX, box.width, box.left));
    if (id) onSelect(id);
  }

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    pickAt(event.clientX);
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    if (event.buttons === 0) return;
    pickAt(event.clientX);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "ArrowRight" && index < events.length - 1) {
      event.preventDefault();
      const next = events[index + 1];
      if (next) onSelect(next.event_id);
    }
    if (event.key === "ArrowLeft" && index > 0) {
      event.preventDefault();
      const prev = events[index - 1];
      if (prev) onSelect(prev.event_id);
    }
  }

  const from = startedAt ?? events.find((event) => event.occurred_at)?.occurred_at ?? null;
  const to =
    endedAt ??
    [...events].reverse().find((event) => event.occurred_at)?.occurred_at ??
    null;

  return (
    <div className={cn(panelClass, "p-3")}>
      <div className="mb-2 flex items-baseline justify-between gap-3 text-xs text-muted-foreground">
        <span className="tabular-nums">{from ? formatDateTime(from) : "没有时间"}</span>
        <span className="min-w-0 truncate text-center">
          {selected ? headingOf(selected) : "事件时间线"}
          {events.length > 0 ? ` · ${events.length} 步` : ""}
        </span>
        <span className="tabular-nums">{to ? formatDateTime(to) : ""}</span>
      </div>
      {events.length === 0 ? (
        <p className="py-3 text-sm text-muted-foreground">没有这一类事件</p>
      ) : (
        <div
          ref={rail}
          role="slider"
          aria-label="事件时间线"
          aria-valuemin={1}
          aria-valuemax={events.length}
          aria-valuenow={index + 1}
          aria-valuetext={selected ? headingOf(selected) : undefined}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          className="relative h-10 cursor-ew-resize touch-none rounded-md bg-muted/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <div className="pointer-events-none absolute inset-x-3 top-1/2 h-px -translate-y-1/2 bg-border" />
          {selectedTick && (
            <div
              aria-hidden
              className="pointer-events-none absolute top-1 bottom-1 w-px bg-primary"
              style={{ left: `clamp(6px, ${selectedTick.left}%, calc(100% - 6px))` }}
            />
          )}
          {ticks.map((tick) => {
            const event = events.find((item) => item.event_id === tick.event_id);
            const active = tick.event_id === selectedId;
            return (
              <div
                key={tick.event_id}
                aria-hidden
                title={event ? headingOf(event) : tick.kind}
                className={cn(
                  "pointer-events-none absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-sm",
                  active ? "z-10 h-6 w-1.5" : "h-4 w-1",
                )}
                style={{
                  left: `clamp(6px, ${tick.left}%, calc(100% - 6px))`,
                  background: EVENT_KIND_TONE[tick.kind],
                }}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
