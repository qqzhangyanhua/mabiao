import { Coins, MessagesSquare, ShieldCheck, Wallet } from "lucide-react";
import type { ReactNode } from "react";
import { formatCost, formatPercent, formatTokens } from "../lib/format";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { cn } from "../lib/cn";
import { Sparkline } from "./Sparkline";
import { Badge } from "./ui/badge";
import { glassClass } from "./ui/card";
import { IconChip, type ChipTone } from "./ui/icon-chip";

export function MetricCard({
  value,
  label,
  hint,
  badge,
  icon,
  tone = "violet",
  trend,
  trendColor = "var(--chart-1)",
}: {
  value: string;
  label: string;
  hint?: ReactNode;
  badge?: ReactNode;
  icon?: ReactNode;
  tone?: ChipTone;
  trend?: number[];
  trendColor?: string;
}) {
  return (
    <div className={cn(glassClass, "flex h-full min-w-0 flex-col justify-center gap-1.5 px-3.5 py-3")}>
      <div className="flex min-w-0 items-center gap-2.5">
        {icon ? (
          <IconChip tone={tone} size="sm">
            {icon}
          </IconChip>
        ) : null}
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs text-muted-foreground" title={label}>
            {label}
          </div>
          <div
            title={value}
            className="truncate text-xl leading-tight font-semibold tracking-tight tabular-nums"
          >
            {value}
          </div>
        </div>
      </div>
      {trend ? <Sparkline values={trend} color={trendColor} height={20} /> : null}
      {badge ? <div>{badge}</div> : null}
      {hint ? (
        <p
          className="line-clamp-1 text-xs text-muted-foreground"
          title={typeof hint === "string" ? hint : undefined}
        >
          {hint}
        </p>
      ) : null}
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
  costTrend,
  tokenTrend,
}: {
  cost: number;
  tokens: number;
  sessions: number | undefined;
  cacheHit: number | null;
  unpriced: number;
  mode: CostMode;
  recordCount: number;
  costTrend?: number[];
  tokenTrend?: number[];
}) {
  return (
    <section className="grid grid-cols-2 gap-3" aria-label="合计">
      <MetricCard
        value={formatCost(cost)}
        label={`费用（${COST_MODE_LABEL[mode]}）`}
        icon={<Wallet size={16} />}
        tone="amber"
        trend={costTrend}
        trendColor="var(--chart-3)"
        badge={
          unpriced > 0 ? (
            <Badge variant="warning" title={`另有 ${unpriced} 条记录的模型没有价目，未计入`}>
              {unpriced} 条未计入
            </Badge>
          ) : undefined
        }
      />
      <MetricCard
        value={formatTokens(tokens)}
        label="Token"
        hint={`${recordCount} 条消耗记录`}
        icon={<Coins size={16} />}
        tone="blue"
        trend={tokenTrend}
        trendColor="var(--chart-4)"
      />
      <MetricCard
        value={sessions === undefined ? "…" : String(sessions)}
        label="会话"
        hint="与当前区间重叠的已推送会话"
        icon={<MessagesSquare size={16} />}
        tone="violet"
      />
      {cacheHit !== null && (
        <MetricCard
          value={formatPercent(cacheHit)}
          label="缓存命中率"
          hint="缓存读 ÷（输入 + 缓存读）；来自当前区间已取回的消耗记录"
          icon={<ShieldCheck size={16} />}
          tone="emerald"
        />
      )}
    </section>
  );
}
