import type { ReactNode } from "react";
import { formatCost, formatPercent, formatTokens } from "../lib/format";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import { panelClass } from "./ui/card";

function Row({
  label,
  value,
  note,
  hint,
}: {
  label: string;
  value: string;
  note?: ReactNode;
  hint?: string;
}) {
  return (
    <div className="border-b border-border/70 py-3 last:border-0">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 text-2xl font-semibold tracking-tight tabular-nums">{value}</div>
      {note && <div className="mt-1 text-xs text-warning">{note}</div>}
      {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
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
    <section className={cn(panelClass, "p-4")} aria-label="合计">
      <Row
        label={`费用（${COST_MODE_LABEL[mode]}）`}
        value={formatCost(cost)}
        note={unpriced > 0 ? `另有 ${unpriced} 条记录的模型没有价目，未计入` : undefined}
      />
      <Row label="Token" value={formatTokens(tokens)} hint={`${recordCount} 条消耗记录`} />
      <Row
        label="会话"
        value={sessions === undefined ? "…" : String(sessions)}
        hint="与当前区间重叠的已推送会话"
      />
      {cacheHit !== null && (
        <Row
          label="缓存命中率"
          value={formatPercent(cacheHit)}
          hint="缓存读 ÷（输入 + 缓存读）；来自当前区间已取回的消耗记录"
        />
      )}
    </section>
  );
}
