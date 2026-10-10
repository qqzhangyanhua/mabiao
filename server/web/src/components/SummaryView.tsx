import type { ReactNode } from "react";
import type { SummaryResponse } from "../api/types";
import { costOf, COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { routeHash } from "../lib/route";
import { BreakdownTable } from "./BreakdownTable";
import { TrendChart } from "./TrendChart";

function Card({ label, value, note }: { label: string; value: string; note?: ReactNode }) {
  return (
    <div className="rounded border border-slate-200 bg-white p-4">
      <div className="text-xs text-slate-500">{label}</div>
      <div className="mt-1 text-2xl font-semibold tabular-nums">{value}</div>
      {note && <div className="mt-1 text-xs text-amber-700">{note}</div>}
    </div>
  );
}

interface Props {
  summary: SummaryResponse;
  mode: CostMode;
  /** 团队总览要按成员拆；单个成员页没必要。 */
  showAccounts: boolean;
  /** 管理员可以从成员榜点进成员页。 */
  linkAccounts: boolean;
}

/** 团队总览与成员页共用：合计卡片、按天趋势，以及按成员 / 来源 / 模型 / 项目的拆分。 */
export function SummaryView({ summary, mode, showAccounts, linkAccounts }: Props) {
  const { totals } = summary;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;
  const unpriced = mode === "unified" ? totals.unified_unpriced_count : 0;
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <Card label="记录数" value={String(totals.record_count)} />
        <Card label="Token" value={formatTokens(totals.total_tokens)} />
        <Card
          label={`费用（${COST_MODE_LABEL[mode]}）`}
          value={formatCost(cost)}
          note={unpriced > 0 ? `另有 ${unpriced} 条记录的模型没有价目，未计入` : undefined}
        />
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <TrendChart
          title="每天 Token"
          points={summary.by_day.map((d) => ({ label: d.key, value: d.total_tokens }))}
          format={formatTokens}
        />
        <TrendChart
          title={`每天费用（${COST_MODE_LABEL[mode]}）`}
          points={summary.by_day.map((d) => ({ label: d.key, value: costOf(d, mode) }))}
          format={formatCost}
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
        <BreakdownTable title="按项目" rows={summary.by_project} mode={mode} />
      </div>
    </div>
  );
}
