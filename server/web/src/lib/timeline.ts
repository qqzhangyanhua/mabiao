import type { EventKind, SessionEvent } from "../api/types";

export interface TimelineTick {
  event_id: string;
  left: number;
  kind: EventKind;
}

function parseTime(value: string | null | undefined): number | null {
  if (!value) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : ms;
}

/** 时间轴两端：会话起止优先，缺了再用事件时间；都没有则按序号均分。 */
export function timelineSpan(
  events: Pick<SessionEvent, "occurred_at">[],
  startedAt?: string | null,
  endedAt?: string | null,
): { start: number; end: number } | null {
  const times = [
    parseTime(startedAt),
    parseTime(endedAt),
    ...events.map((event) => parseTime(event.occurred_at)),
  ].filter((ms): ms is number => ms !== null);
  if (times.length === 0) return null;
  const start = Math.min(...times);
  const end = Math.max(...times);
  return end > start ? { start, end } : { start, end: start + 1 };
}

export function timelineTicks(
  events: Pick<SessionEvent, "event_id" | "occurred_at" | "kind">[],
  span: { start: number; end: number } | null,
): TimelineTick[] {
  const duration = span ? span.end - span.start : 0;
  return events.map((event, index) => {
    const at = parseTime(event.occurred_at);
    const left =
      span && at !== null && duration > 0
        ? ((at - span.start) / duration) * 100
        : events.length <= 1
          ? 0
          : (index / (events.length - 1)) * 100;
    return {
      event_id: event.event_id,
      kind: event.kind,
      left: Math.min(100, Math.max(0, left)),
    };
  });
}

export function nearestTickId(ticks: TimelineTick[], percent: number): string | undefined {
  if (ticks.length === 0) return undefined;
  return ticks.reduce((best, tick) =>
    Math.abs(tick.left - percent) < Math.abs(best.left - percent) ? tick : best,
  ).event_id;
}

export function percentFromClientX(clientX: number, width: number, left: number): number {
  if (width <= 0) return 0;
  return Math.min(100, Math.max(0, ((clientX - left) / width) * 100));
}

export const EVENT_KIND_TONE: Record<EventKind, string> = {
  message: "var(--chart-1)",
  plan: "var(--chart-3)",
  tool_call: "var(--chart-2)",
  tool_result: "var(--chart-4)",
  model_change: "var(--chart-5)",
  error: "var(--destructive)",
  system_status: "var(--muted-foreground)",
  unadapted: "var(--muted-foreground)",
};
