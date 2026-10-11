import { useState, type MouseEvent } from "react";
import type { WorkDay, WorkSegment } from "../api/types";
import { sliceColor } from "../lib/chart";
import { formatCost, formatTokens } from "../lib/format";
import { sourceLabel } from "../lib/source";
import { useHref } from "../view";
import {
  dayStartIso,
  formatClock,
  formatDuration,
  laneCount,
  layoutSegments,
  weekdayLabel,
  type LaneSegment,
} from "../lib/workTimeline";
import { costOf, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import { Empty } from "./Feedback";
import { panelClass } from "./ui/card";

const AXIS_HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];
const LANE_HEIGHT = 34;
const TOOLTIP_WIDTH = 260;
const TOOLTIP_HEIGHT = 220;
const TOOLTIP_MARGIN = 14;

function projectOf(segment: WorkSegment): string {
  return segment.project_name ?? segment.project ?? "未归属项目";
}

export function WorkTimelineBoard({ days, mode }: { days: WorkDay[]; mode: CostMode }) {
  const href = useHref();
  const [hovered, setHovered] = useState<WorkSegment | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ x: number; y: number } | null>(null);

  if (days.length === 0) {
    return <Empty>这段时间没有与区间重叠的会话</Empty>;
  }

  function enter(event: MouseEvent, item: LaneSegment) {
    setHovered(item.segment);
    setTooltipPos({ x: event.clientX, y: event.clientY });
  }

  function move(event: MouseEvent) {
    if (hovered) setTooltipPos({ x: event.clientX, y: event.clientY });
  }

  function leave() {
    setHovered(null);
    setTooltipPos(null);
  }

  return (
    <div className="flex flex-col gap-4">
      {days.map((day) => (
        <DayRow
          key={day.day}
          day={day}
          mode={mode}
          sessionHref={href}
          onEnter={enter}
          onMove={move}
          onLeave={leave}
        />
      ))}
      {hovered && tooltipPos ? <SegmentTooltip segment={hovered} pos={tooltipPos} mode={mode} /> : null}
    </div>
  );
}

function DayRow({
  day,
  mode,
  sessionHref,
  onEnter,
  onMove,
  onLeave,
}: {
  day: WorkDay;
  mode: CostMode;
  sessionHref: (route: { page: "session"; sessionId: number }) => string;
  onEnter: (event: MouseEvent, item: LaneSegment) => void;
  onMove: (event: MouseEvent) => void;
  onLeave: () => void;
}) {
  const origin = dayStartIso(day.day);
  const layout = layoutSegments(day.segments, origin);
  const lanes = Math.max(1, laneCount(layout));
  const cost = costOf({ unified_cost: day.unified_cost, cost_snapshot: day.cost_snapshot }, mode);

  return (
    <section className={cn(panelClass, "overflow-hidden")}>
      <div className="flex flex-wrap items-baseline justify-between gap-2 border-b border-border px-4 py-2">
        <h2 className="text-sm font-medium">
          {day.day}
          <span className="ml-2 text-xs font-normal text-muted-foreground">周{weekdayLabel(day.day)}</span>
        </h2>
        <p className="text-xs text-muted-foreground tabular-nums">
          {day.segment_count} 个片段 · {formatTokens(day.total_tokens)} token · {formatCost(cost)}
        </p>
      </div>
      <div className="px-4 pb-3 pt-2">
        <div className="relative mb-1 h-4 text-[10px] text-muted-foreground">
          {AXIS_HOURS.map((hour) => (
            <span
              key={hour}
              className="absolute -translate-x-1/2 tabular-nums"
              style={{ left: `${(hour / 24) * 100}%` }}
            >
              {String(hour).padStart(2, "0")}:00
            </span>
          ))}
        </div>
        <div className="relative" style={{ height: lanes * LANE_HEIGHT }}>
          {AXIS_HOURS.slice(1, -1).map((hour) => (
            <div
              key={hour}
              className="absolute top-0 bottom-0 w-px bg-border/70"
              style={{ left: `${(hour / 24) * 100}%` }}
            />
          ))}
          {layout.map((item) => {
            const left = (item.startMinutes / 1440) * 100;
            const width = Math.max(0, ((item.endMinutes - item.startMinutes) / 1440) * 100);
            const label = item.segment.title || "（无标题）";
            return (
              <a
                key={`${item.segment.id}:${item.segment.start}`}
                href={sessionHref({ page: "session", sessionId: item.segment.id })}
                className="absolute flex items-center overflow-hidden rounded-md px-1.5 text-xs text-white no-underline hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                style={{
                  left: `${left}%`,
                  width: `${width}%`,
                  top: item.lane * LANE_HEIGHT + 4,
                  height: LANE_HEIGHT - 8,
                  background: sliceColor(item.segment.source, 0),
                }}
                onMouseEnter={(event) => onEnter(event, item)}
                onMouseMove={onMove}
                onMouseLeave={onLeave}
                aria-label={`${label}，点击打开会话`}
              >
                <span className="truncate">{label}</span>
              </a>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function SegmentTooltip({
  segment,
  pos,
  mode,
}: {
  segment: WorkSegment;
  pos: { x: number; y: number };
  mode: CostMode;
}) {
  const flipX = pos.x + TOOLTIP_MARGIN + TOOLTIP_WIDTH > window.innerWidth;
  const flipY = pos.y + TOOLTIP_MARGIN + TOOLTIP_HEIGHT > window.innerHeight;
  const cost = costOf(
    { unified_cost: segment.unified_cost, cost_snapshot: segment.cost_snapshot },
    mode,
  );
  return (
    <div
      role="tooltip"
      className="pointer-events-none fixed z-50 rounded-lg border border-border bg-card px-3 py-2 text-xs shadow-[0_8px_24px_var(--shadow-soft)]"
      style={{
        width: TOOLTIP_WIDTH,
        left: flipX ? pos.x - TOOLTIP_MARGIN - TOOLTIP_WIDTH : pos.x + TOOLTIP_MARGIN,
        top: flipY ? pos.y - TOOLTIP_MARGIN - TOOLTIP_HEIGHT : pos.y + TOOLTIP_MARGIN,
      }}
    >
      <div className="mb-1 font-medium text-foreground">{segment.title || "（无标题）"}</div>
      <dl className="grid grid-cols-[4.5rem_1fr] gap-x-2 gap-y-1 text-muted-foreground">
        <dt>成员</dt>
        <dd className="truncate text-foreground">{segment.account}</dd>
        <dt>项目</dt>
        <dd className="truncate text-foreground">{projectOf(segment)}</dd>
        <dt>来源</dt>
        <dd className="text-foreground">{sourceLabel(segment.source)}</dd>
        <dt>开始</dt>
        <dd className="text-foreground">{formatClock(segment.start)}</dd>
        <dt>结束</dt>
        <dd className="text-foreground">{formatClock(segment.end)}</dd>
        <dt>时长</dt>
        <dd className="text-foreground">{formatDuration(segment.start, segment.end)}</dd>
        <dt>Token</dt>
        <dd className="text-foreground">{formatTokens(segment.total_tokens)}</dd>
        <dt>费用</dt>
        <dd className="text-foreground">{formatCost(cost)}</dd>
      </dl>
    </div>
  );
}
