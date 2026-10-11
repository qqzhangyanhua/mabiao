import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

export interface TrendPoint {
  label: string;
  value: number;
}

/** 纯 CSS 柱状图，天数再多也只是柱子变细；每根柱子带 title 当悬停提示。 */
export function TrendChart({
  title,
  points,
  format,
  color = "var(--chart-1)",
}: {
  title: string;
  points: TrendPoint[];
  format: (value: number) => string;
  color?: string;
}) {
  const max = Math.max(0, ...points.map((p) => p.value));
  const sparse = points.length <= 3;
  return (
    <section className={cn(panelClass, "p-4")}>
      <h3 className="mb-3 text-sm font-medium">{title}</h3>
      {points.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <>
          <div
            role="img"
            aria-label={title}
            className={cn("flex h-32 items-end", sparse ? "justify-center gap-2" : "gap-px")}
          >
            {points.map((p) => (
              <div
                key={p.label}
                title={`${p.label}：${format(p.value)}`}
                className={cn(
                  "rounded-t-sm opacity-90 hover:opacity-100",
                  sparse ? "w-8" : "min-w-px flex-1",
                )}
                style={{
                  background: color,
                  height: `${max > 0 ? Math.max((p.value / max) * 100, p.value > 0 ? 2 : 0) : 0}%`,
                }}
              />
            ))}
          </div>
          <div className="mt-1 flex justify-between text-xs text-muted-foreground">
            <span>{points[0]?.label}</span>
            <span>峰值 {format(max)}</span>
            <span>{points[points.length - 1]?.label}</span>
          </div>
        </>
      )}
    </section>
  );
}
