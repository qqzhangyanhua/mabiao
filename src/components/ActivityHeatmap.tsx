import {
  memo,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
} from "react";
import {
  heatmapGrid,
  heatmapMonthLabels,
  placeHeatmapTooltip,
  quantileCuts,
  tokenHeatmapLevel,
} from "../lib/calendar";
import { formatCompact, formatUsd } from "../lib/format";
import type { SeriesPoint } from "../types";

const WEEKDAY_MARKS = ["一", "", "三", "", "五", "", ""] as const;
const LEVELS = [0, 1, 2, 3, 4] as const;

type HoverTip = {
  date: string;
  tokens: number;
  cost: number | null;
  cell: { left: number; top: number; right: number; bottom: number };
};

export const ActivityHeatmap = memo(function ActivityHeatmap({
  points,
  range,
  onDayClick,
}: {
  points: SeriesPoint[];
  range: { from: string; to: string };
  onDayClick?: (day: string) => void;
}) {
  const weeks = useMemo(() => heatmapGrid(range.from, range.to), [range.from, range.to]);
  const months = useMemo(() => heatmapMonthLabels(weeks), [weeks]);
  const byDay = useMemo(() => new Map(points.map((point) => [point.bucket, point])), [points]);
  const cuts = useMemo(
    () => quantileCuts(points.map((point) => point.total_tokens).filter((value) => value > 0)),
    [points],
  );
  const [hover, setHover] = useState<HoverTip | null>(null);
  const [tipPos, setTipPos] = useState<{ left: number; top: number } | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const monthByWeek = useMemo(() => {
    const labels = new Map<number, string>();
    for (const month of months) {
      labels.set(month.weekIndex, month.label);
    }
    return labels;
  }, [months]);

  useLayoutEffect(() => {
    if (!hover) {
      setTipPos(null);
      return;
    }
    const el = tipRef.current;
    if (!el) {
      return;
    }
    const rect = el.getBoundingClientRect();
    setTipPos(
      placeHeatmapTooltip(
        hover.cell,
        { width: rect.width, height: rect.height },
        {
          width: window.innerWidth,
          height: window.innerHeight,
        },
      ),
    );
  }, [hover]);

  function showTip(event: MouseEvent<HTMLElement>, date: string) {
    const cellRect = event.currentTarget.getBoundingClientRect();
    const point = byDay.get(date);
    setHover({
      date,
      tokens: point?.total_tokens ?? 0,
      cost: point?.cost ?? null,
      cell: {
        left: cellRect.left,
        top: cellRect.top,
        right: cellRect.right,
        bottom: cellRect.bottom,
      },
    });
  }

  return (
    <div className="heatmap" style={{ "--heat-weeks": weeks.length } as CSSProperties}>
      <div className="heatmap-body">
        <div className="heatmap-months" aria-hidden="true">
          {weeks.map((week, weekIndex) => (
            <span key={week.days[0]?.date ?? weekIndex}>{monthByWeek.get(weekIndex) ?? ""}</span>
          ))}
        </div>
        <div className="heatmap-weekdays" aria-hidden="true">
          {WEEKDAY_MARKS.map((label, index) => (
            <span key={`${label}-${index}`}>{label}</span>
          ))}
        </div>
        <div className="heatmap-grid" onMouseLeave={() => setHover(null)}>
          {WEEKDAY_MARKS.map((_, row) =>
            weeks.map((week) => {
              const cell = week.days[row];
              if (!cell) {
                return null;
              }
              const tokens = byDay.get(cell.date)?.total_tokens ?? 0;
              const level = cell.future ? 0 : tokenHeatmapLevel(tokens, cuts);
              const className = `heat-cell heat-${level}${cell.future ? " is-future" : ""}`;
              if (cell.future) {
                return <span key={cell.date} className={className} aria-hidden />;
              }
              return (
                <button
                  key={cell.date}
                  type="button"
                  className={className}
                  aria-label={`${cell.date} · ${formatCompact(tokens)} Token · 打开工作时间线`}
                  onMouseEnter={(event) => showTip(event, cell.date)}
                  onClick={() => onDayClick?.(cell.date)}
                />
              );
            }),
          )}
        </div>
      </div>
      <div className="heatmap-legend" aria-hidden="true">
        <span>少</span>
        {LEVELS.map((level) => (
          <i key={level} className={`heat-cell heat-${level}`} />
        ))}
        <span>多</span>
      </div>
      {hover ? (
        <div
          ref={tipRef}
          className="heatmap-tip"
          style={
            tipPos
              ? { left: tipPos.left, top: tipPos.top }
              : { left: 0, top: 0, visibility: "hidden" }
          }
        >
          <div>
            {hover.date} · {formatCompact(hover.tokens)} Token
          </div>
          {hover.cost != null ? <div>{formatUsd(hover.cost, false)}</div> : null}
        </div>
      ) : null}
    </div>
  );
});
