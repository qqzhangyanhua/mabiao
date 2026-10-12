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
import { SourceIcon } from "./SourceIcon";
import { panelClass } from "./ui/card";

const AXIS_HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];
/** 夜间（0–6 点、21–24 点）铺淡灰底，一眼分出工作时段。 */
const NIGHT_BANDS: [number, number][] = [
  [0, 6],
  [21, 24],
];
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

  const sources = [...new Set(days.flatMap((day) => day.segments.map((seg) => seg.source)))];

  return (
    <div className="flex flex-col gap-4">
      <ul
        aria-label="来源图例"
        className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs text-muted-foreground"
      >
        {sources.map((source) => (
          <li key={source} className="flex items-center gap-1.5">
            <span
              className="size-2.5 rounded-sm"
              style={{ background: sliceColor(source, 0) }}
              aria-hidden="true"
            />
            {sourceLabel(source)}
          </li>
        ))}
      </ul>
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

  const [, month = "", dom = ""] = day.day.split("-");
  const empty = day.segment_count === 0;

  return (
    <section className={cn(panelClass, "overflow-hidden")}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div className="flex items-center gap-3">
          <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-accent leading-none">
            <div className="text-center">
              <div className="text-[10px] text-muted-foreground">{Number(month)} 月</div>
              <div className="mt-0.5 text-lg font-semibold tabular-nums">{Number(dom)}</div>
            </div>
          </div>
          <h2 className="text-sm font-semibold">
            {day.day}
            <span className="ml-2 rounded-full bg-muted px-2 py-0.5 text-xs font-normal text-muted-foreground">
              周{weekdayLabel(day.day)}
            </span>
          </h2>
        </div>
        <p
          className={cn(
            "flex items-center gap-3 text-xs tabular-nums",
            empty ? "text-muted-foreground/70" : "text-muted-foreground",
          )}
        >
          <span>{day.segment_count} 个片段</span>
          <span>{formatTokens(day.total_tokens)} token</span>
          <span className="font-semibold text-foreground">{formatCost(cost)}</span>
        </p>
      </div>
      <div className="px-4 pb-4 pt-2">
        <div className="relative mb-1 h-4 text-[10px] text-muted-foreground">
          {AXIS_HOURS.map((hour) => (
            <span
              key={hour}
              className={cn(
                "absolute tabular-nums",
                hour === 0 ? "" : hour === 24 ? "-translate-x-full" : "-translate-x-1/2",
              )}
              style={{ left: `${(hour / 24) * 100}%` }}
            >
              {String(hour).padStart(2, "0")}:00
            </span>
          ))}
        </div>
        <div
          className="relative overflow-hidden rounded-xl bg-muted/40"
          style={{ height: lanes * LANE_HEIGHT }}
        >
          {NIGHT_BANDS.map(([from, to]) => (
            <div
              key={from}
              aria-hidden="true"
              className="absolute top-0 bottom-0 bg-foreground/[0.035]"
              style={{ left: `${(from / 24) * 100}%`, width: `${((to - from) / 24) * 100}%` }}
            />
          ))}
          {AXIS_HOURS.slice(1, -1).map((hour) => (
            <div
              key={hour}
              className="absolute top-0 bottom-0 border-l border-dashed border-border"
              style={{ left: `${(hour / 24) * 100}%` }}
            />
          ))}
          {layout.map((item) => {
            const left = (item.startMinutes / 1440) * 100;
            const width = Math.max(0, ((item.endMinutes - item.startMinutes) / 1440) * 100);
            const label = item.segment.title || "（无标题）";
            const tone = sliceColor(item.segment.source, 0);
            return (
              <a
                key={`${item.segment.id}:${item.segment.start}`}
                href={sessionHref({ page: "session", sessionId: item.segment.id })}
                className="absolute flex items-center overflow-hidden rounded-lg px-2 text-xs font-medium text-white no-underline shadow-[0_2px_6px_-2px_rgb(15_23_42/45%)] ring-1 ring-white/25 ring-inset hover:z-10 hover:brightness-110 focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                style={{
                  left: `${left}%`,
                  width: `${width}%`,
                  minWidth: 8,
                  top: item.lane * LANE_HEIGHT + 4,
                  height: LANE_HEIGHT - 8,
                  background: `linear-gradient(180deg, color-mix(in srgb, ${tone} var(--seg-top-keep), var(--seg-top-with)), color-mix(in srgb, ${tone} var(--seg-bottom-keep), black))`,
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
      className="pointer-events-none fixed z-50 rounded-2xl border border-border bg-popover px-3.5 py-3 text-xs shadow-[0_16px_40px_-12px_var(--shadow-soft)]"
      style={{
        width: TOOLTIP_WIDTH,
        left: flipX ? pos.x - TOOLTIP_MARGIN - TOOLTIP_WIDTH : pos.x + TOOLTIP_MARGIN,
        top: flipY ? pos.y - TOOLTIP_MARGIN - TOOLTIP_HEIGHT : pos.y + TOOLTIP_MARGIN,
      }}
    >
      <div className="mb-2 flex items-start gap-2 font-semibold text-foreground">
        <span
          aria-hidden="true"
          className="mt-1 size-2.5 shrink-0 rounded-sm"
          style={{ background: sliceColor(segment.source, 0) }}
        />
        <span className="line-clamp-2">{segment.title || "（无标题）"}</span>
      </div>
      <dl className="grid grid-cols-[4.5rem_1fr] gap-x-2 gap-y-1 text-muted-foreground">
        <dt>成员</dt>
        <dd className="truncate text-foreground">{segment.account}</dd>
        <dt>项目</dt>
        <dd className="truncate text-foreground">{projectOf(segment)}</dd>
        <dt>来源</dt>
        <dd className="flex items-center gap-1.5 text-foreground">
          <SourceIcon source={segment.source} size={14} />
          {sourceLabel(segment.source)}
        </dd>
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
