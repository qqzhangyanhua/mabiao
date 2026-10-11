import type { ReactNode } from "react";
import { formatCost, formatPercent, formatTokens } from "../lib/format";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import { Badge } from "./ui/badge";
import { panelClass } from "./ui/card";

export function MetricCard({
  value,
  label,
  hint,
  badge,
}: {
  value: string;
  label: string;
  hint?: ReactNode;
  badge?: ReactNode;
}) {
  return (
    <div className={cn(panelClass, "p-4")}>
      <div className="text-2xl font-semibold tracking-tight tabular-nums">{value}</div>
      <div className="mt-1 text-sm text-foreground">{label}</div>
      {badge ? <div className="mt-2">{badge}</div> : null}
      {hint ? <p className="mt-1 text-xs text-muted-foreground/80">{hint}</p> : null}
    </div>
  );
}

export function KpiColumn({
  cost,
  tokens,
  sessions,
  cacheHit,
  unpriced,
  mode,
  recordCount,
}: {
  cost: number;
  tokens: number;
  sessions: number | undefined;
  cacheHit: number | null;
  unpriced: number;
  mode: CostMode;
  recordCount: number;
}) {
  return (
    <section className="grid grid-cols-2 gap-3" aria-label="合计">
      <MetricCard
        value={formatCost(cost)}
        label={`费用（${COST_MODE_LABEL[mode]}）`}
        badge={
          unpriced > 0 ? (
            <Badge variant="warning" title={`另有 ${unpriced} 条记录的模型没有价目，未计入`}>
              {unpriced} 条未计入
            </Badge>
          ) : undefined
        }
      />
      <MetricCard value={formatTokens(tokens)} label="Token" hint={`${recordCount} 条消耗记录`} />
      <MetricCard
        value={sessions === undefined ? "…" : String(sessions)}
        label="会话"
        hint="与当前区间重叠的已推送会话"
      />
      {cacheHit !== null && (
        <MetricCard
          value={formatPercent(cacheHit)}
          label="缓存命中率"
          hint="缓存读 ÷（输入 + 缓存读）；来自当前区间已取回的消耗记录"
        />
      )}
    </section>
  );
}
