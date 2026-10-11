import { CircleDollarSign, Coins, Hash, Layers, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { SummaryResponse } from "../api/types";
import { cn } from "../lib/cn";
import { costOf, COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { eachDay, type DayRange } from "../lib/range";
import { routeHash } from "../lib/route";
import { BreakdownTable } from "./BreakdownTable";
import { TrendChart } from "./TrendChart";
import { panelClass } from "./ui/card";

function Kpi({
  icon: Icon,
  tone,
  label,
  value,
  note,
  hint,
}: {
  icon: LucideIcon;
  tone: string;
  label: string;
  value: string;
  note?: ReactNode;
  hint?: string;
}) {
  return (
    <div className={cn(panelClass, "p-4")}>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span
          className="grid size-6 place-items-center rounded-md"
          style={{ color: tone, background: `color-mix(in srgb, ${tone} 18%, transparent)` }}
        >
          <Icon size={14} />
        </span>
        {label}
      </div>
      <div className="mt-2 text-2xl font-semibold tracking-tight tabular-nums">{value}</div>
      {note && <div className="mt-1 text-xs text-warning">{note}</div>}
      {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

function dailyPoints(
  rows: { key: string; value: number }[],
  range: DayRange | undefined,
): { label: string; value: number }[] {
  const days = range ? eachDay(range.fromDay, range.toDay) : [];
  if (days.length === 0) return rows.map((row) => ({ label: row.key, value: row.value }));
  const values = new Map(rows.map((row) => [row.key, row.value]));
  return days.map((label) => ({ label, value: values.get(label) ?? 0 }));
}

interface Props {
  summary: SummaryResponse;
  mode: CostMode;
  /** 选中的日历区间。有的话按天趋势会补上没有数据的日期，避免一根柱子撑满。 */
  range?: DayRange;
  /** 团队总览要按成员拆；单个成员页没必要。 */
  showAccounts: boolean;
  /** 管理员可以从成员榜点进成员页。 */
  linkAccounts: boolean;
  /** 单个项目页里「按项目」只有一行，没必要。默认显示。 */
  showProjects?: boolean;
}

/** 团队总览与成员页共用：合计卡片、按天趋势，以及按成员 / 来源 / 模型 / 项目的拆分。 */
export function SummaryView({
  summary,
  mode,
  range,
  showAccounts,
  linkAccounts,
  showProjects = true,
}: Props) {
  const { totals } = summary;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;
  const unpriced = mode === "unified" ? totals.unified_unpriced_count : 0;
  const dimensionHint = [
    `${summary.by_model.length} 个模型`,
    showProjects ? `${summary.by_project.length} 个项目` : null,
    showAccounts ? `${summary.by_account.length} 个成员` : null,
  ]
    .filter((item) => item !== null)
    .join(" · ");

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <Kpi icon={Hash} tone="var(--chart-1)" label="记录数" value={String(totals.record_count)} />
        <Kpi
          icon={Coins}
          tone="var(--chart-2)"
          label="Token"
          value={formatTokens(totals.total_tokens)}
        />
        <Kpi
          icon={CircleDollarSign}
          tone="var(--chart-3)"
          label={`费用（${COST_MODE_LABEL[mode]}）`}
          value={formatCost(cost)}
          note={unpriced > 0 ? `另有 ${unpriced} 条记录的模型没有价目，未计入` : undefined}
        />
        <Kpi
          icon={Layers}
          tone="var(--chart-4)"
          label="来源"
          value={String(summary.by_source.length)}
          hint={dimensionHint}
        />
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <TrendChart
          title="每天 Token"
          points={dailyPoints(
            summary.by_day.map((d) => ({ key: d.key, value: d.total_tokens })),
            range,
          )}
          format={formatTokens}
          color="var(--chart-1)"
        />
        <TrendChart
          title={`每天费用（${COST_MODE_LABEL[mode]}）`}
          points={dailyPoints(
            summary.by_day.map((d) => ({ key: d.key, value: costOf(d, mode) })),
            range,
          )}
          format={formatCost}
          color="var(--chart-2)"
        />
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {showAccounts && (
          <BreakdownTable
            title="按成员"
            rows={summary.by_account}
            mode={mode}
            linkOf={
              linkAccounts
                ? (row) =>
                    row.id === null ? undefined : routeHash({ page: "member", accountId: row.id })
                : undefined
            }
          />
        )}
        <BreakdownTable title="按来源" rows={summary.by_source} mode={mode} />
        <BreakdownTable title="按模型" rows={summary.by_model} mode={mode} />
        {showProjects && (
          <BreakdownTable
            title="按项目"
            rows={summary.by_project}
            mode={mode}
            linkOf={(row) =>
              row.id === null ? undefined : routeHash({ page: "project", projectId: row.id })
            }
          />
        )}
      </div>
    </div>
  );
}
