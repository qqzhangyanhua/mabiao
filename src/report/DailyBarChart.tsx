import type { PosterDayBar } from "./posterTypes";

const BAR_WIDTH = 48;
const GAP = 40;
const CHART_HEIGHT = 140;

export function DailyBarChart({ days }: { days: PosterDayBar[] }) {
  const max = Math.max(...days.map((day) => day.tokens), 1);
  const width = days.length * (BAR_WIDTH + GAP) - GAP;
  return (
    <svg
      className="rp-bars"
      viewBox={`0 0 ${width} ${CHART_HEIGHT + 28}`}
      role="img"
      aria-label="按天消耗"
    >
      {days.map((day, index) => {
        const height = (day.tokens / max) * CHART_HEIGHT;
        const x = index * (BAR_WIDTH + GAP);
        return (
          <g key={day.label}>
            <rect
              x={x}
              y={CHART_HEIGHT - height}
              width={BAR_WIDTH}
              height={height}
              rx={8}
              fill="var(--rp-accent)"
            />
            <text
              x={x + BAR_WIDTH / 2}
              y={CHART_HEIGHT + 22}
              textAnchor="middle"
              fill="var(--rp-muted)"
              fontSize="14"
              fontWeight="650"
            >
              {day.label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
