import type { PosterDayBar } from "./posterTypes";

const CHART_HEIGHT = 140;

export function DailyBarChart({ days }: { days: PosterDayBar[] }) {
  const dense = days.length > 10;
  const barWidth = dense ? 12 : 48;
  const gap = dense ? 6 : 40;
  const fontSize = dense ? 9 : 14;
  const max = Math.max(...days.map((day) => day.tokens), 1);
  const width = days.length * (barWidth + gap) - gap;
  return (
    <svg
      className="rp-bars"
      viewBox={`0 0 ${width} ${CHART_HEIGHT + 28}`}
      role="img"
      aria-label="按天消耗"
    >
      {days.map((day, index) => {
        const height = (day.tokens / max) * CHART_HEIGHT;
        const x = index * (barWidth + gap);
        return (
          <g key={`${day.label}-${index}`}>
            <rect
              x={x}
              y={CHART_HEIGHT - height}
              width={barWidth}
              height={height}
              rx={dense ? 3 : 8}
              fill="var(--rp-accent)"
            />
            <text
              x={x + barWidth / 2}
              y={CHART_HEIGHT + 22}
              textAnchor="middle"
              fill="var(--rp-muted)"
              fontSize={fontSize}
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
