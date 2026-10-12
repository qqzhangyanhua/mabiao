import { TrendingUp } from "lucide-react";
import { niceCeil } from "../lib/chart";
import { cn } from "../lib/cn";
import { glassClass } from "./ui/card";
import { IconChip } from "./ui/icon-chip";

export interface TrendPoint {
  label: string;
  value: number;
}

const TICKS = [1, 0.75, 0.5, 0.25, 0];

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
  const ceiling = niceCeil(max);
  const sparse = points.length <= 3;
  const dense = points.length > 60;
  return (
    <section className={cn(glassClass, "p-5")}>
      <div className="mb-4 flex items-center gap-2.5">
        <IconChip tone="violet" size="sm">
          <TrendingUp size={15} />
        </IconChip>
        <h3 className="text-[15px] font-semibold">{title}</h3>
      </div>
      {points.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">这段时间没有数据</p>
      ) : (
        <>
          <div className="flex gap-2">
            <div
              aria-hidden="true"
              className="flex h-48 w-11 shrink-0 flex-col justify-between text-right text-[10px] leading-none text-muted-foreground tabular-nums"
            >
              {TICKS.map((tick) => (
                <span key={tick}>{ceiling > 0 || tick === 0 ? format(ceiling * tick) : ""}</span>
              ))}
            </div>
            <div className="relative h-48 min-w-0 flex-1">
              <div aria-hidden="true" className="absolute inset-0 flex flex-col justify-between">
                {TICKS.map((tick) => (
                  <div
                    key={tick}
                    className={cn(
                      "border-t",
                      tick === 0 ? "border-border" : "border-dashed border-border/70",
                    )}
                  />
                ))}
              </div>
              <div
                role="img"
                aria-label={`${title}，峰值 ${format(max)}`}
                className={cn(
                  "relative flex h-full items-end",
                  sparse ? "justify-center gap-3" : dense ? "gap-px" : "gap-[3px] md:gap-1",
                )}
              >
                {points.map((p) => (
                  <div
                    key={p.label}
                    title={`${p.label}：${format(p.value)}`}
                    className={cn(
                      dense ? "rounded-t-[2px]" : "rounded-t-md",
                      sparse ? "w-8" : "min-w-px flex-1",
                    )}
                    style={{
                      background:
                        p.value > 0
                          ? `linear-gradient(180deg, ${color}, color-mix(in srgb, ${color} 30%, var(--bar-base)))`
                          : "var(--border)",
                      height: `${
                        ceiling > 0 ? Math.max((p.value / ceiling) * 100, p.value > 0 ? 3 : 6) : 6
                      }%`,
                      opacity: p.value > 0 ? 0.95 : 0.6,
                    }}
                  />
                ))}
              </div>
            </div>
          </div>
          <div className="mt-2 flex justify-between pl-13 text-xs text-muted-foreground">
            <span>{points[0]?.label}</span>
            <span className="font-medium text-foreground">峰值 {format(max)}</span>
            <span>{points[points.length - 1]?.label}</span>
          </div>
        </>
      )}
    </section>
  );
}
