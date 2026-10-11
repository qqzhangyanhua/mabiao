import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

export interface TrendPoint {
  label: string;
  value: number;
}

/** 纯 CSS 柱状图。无数据的日子用极淡柱，避免「左边空了一大块」。 */
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
            aria-label={`${title}，峰值 ${format(max)}`}
            className={cn("flex h-40 items-end", sparse ? "justify-center gap-2" : "gap-px")}
          >
            {points.map((p) => (
              <div
                key={p.label}
                title={`${p.label}：${format(p.value)}`}
                className={cn("rounded-t-sm", sparse ? "w-8" : "min-w-px flex-1")}
                style={{
                  background: p.value > 0 ? color : "color-mix(in srgb, var(--border) 80%, transparent)",
                  height: `${
                    max > 0 ? Math.max((p.value / max) * 100, p.value > 0 ? 3 : 2) : 2
                  }%`,
                  opacity: p.value > 0 ? 0.92 : 0.35,
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
