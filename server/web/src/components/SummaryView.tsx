import { useState } from "react";
import type { Breakdown, SummaryResponse } from "../api/types";
import { costOf, type CostMode } from "../lib/cost";
import { formatCost, formatTokens } from "../lib/format";
import { eachDay, type DayRange } from "../lib/range";
import { sourceLabel } from "../lib/source";
import type { TokenSplit } from "../lib/tokens";
import { useHref } from "../view";
import { BreakdownTable } from "./BreakdownTable";
import { SourceIcon } from "./SourceIcon";
import { DimensionCards } from "./DimensionCards";
import { KpiColumn } from "./KpiColumn";
import { ModelRanking } from "./ModelRanking";
import { TrendChart } from "./TrendChart";
import { Segmented } from "./ui/segmented";

function dailyPoints(
  rows: { key: string; value: number }[],
  range: DayRange | undefined,
): { label: string; value: number }[] {
  const days = range ? eachDay(range.fromDay, range.toDay) : [];
  if (days.length === 0) return rows.map((row) => ({ label: row.key, value: row.value }));
  const values = new Map(rows.map((row) => [row.key, row.value]));
  return days.map((label) => ({ label, value: values.get(label) ?? 0 }));
}

type Dimension = "day" | "account" | "source" | "model" | "project";

const DIMENSIONS: { id: Dimension; label: string }[] = [
  { id: "day", label: "按天" },
  { id: "account", label: "按成员" },
  { id: "source", label: "按 Agent 工具" },
  { id: "model", label: "按模型" },
  { id: "project", label: "按项目" },
];

interface Props {
  summary: SummaryResponse;
  mode: CostMode;
  range?: DayRange;
  showAccounts: boolean;
  linkAccounts: boolean;
  showProjects?: boolean;
  sessions?: number;
  cacheHit?: number | null;
  modelSplits?: Map<string, TokenSplit>;
  projectSplits?: Map<string, TokenSplit>;
  sourceSplits?: Map<string, TokenSplit>;
}

export function SummaryView({
  summary,
  mode,
  range,
  showAccounts,
  linkAccounts,
  showProjects = true,
  sessions,
  cacheHit = null,
  modelSplits,
  projectSplits,
  sourceSplits,
}: Props) {
  const href = useHref();
  const { totals } = summary;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;
  const unpriced = mode === "unified" ? totals.unified_unpriced_count : 0;
  const [metric, setMetric] = useState<"tokens" | "cost">("tokens");
  const [dimension, setDimension] = useState<Dimension>(showAccounts ? "account" : "source");
  const dims = DIMENSIONS.filter((item) => {
    if (item.id === "account") return showAccounts;
    if (item.id === "project") return showProjects;
    return true;
  });

  const tableRows: Record<Dimension, Breakdown[]> = {
    day: summary.by_day,
    account: summary.by_account,
    source: summary.by_source,
    model: summary.by_model,
    project: summary.by_project,
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <TrendChart
            className="h-full"
            title={metric === "tokens" ? "每天 Token" : "每天费用"}
            action={
              <Segmented
                label="趋势指标"
                value={metric}
                onChange={setMetric}
                options={[
                  { id: "tokens", label: "Token" },
                  { id: "cost", label: "费用" },
                ]}
                size="compact"
              />
            }
            points={dailyPoints(
              summary.by_day.map((d) => ({
                key: d.key,
                value: metric === "tokens" ? d.total_tokens : costOf(d, mode),
              })),
              range,
            )}
            format={metric === "tokens" ? formatTokens : formatCost}
            color={metric === "tokens" ? "var(--chart-1)" : "var(--chart-2)"}
          />
        </div>
        <KpiColumn
          cost={cost}
          tokens={totals.total_tokens}
          sessions={sessions}
          cacheHit={cacheHit}
          unpriced={unpriced}
          mode={mode}
          recordCount={totals.record_count}
          costTrend={dailyPoints(
            summary.by_day.map((d) => ({ key: d.key, value: costOf(d, mode) })),
            range,
          ).map((p) => p.value)}
          tokenTrend={dailyPoints(
            summary.by_day.map((d) => ({ key: d.key, value: d.total_tokens })),
            range,
          ).map((p) => p.value)}
        />
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <ModelRanking title="按模型" rows={summary.by_model} mode={mode} splits={modelSplits} />
        <DimensionCards
          title="按 Agent 工具"
          kind="source"
          rows={summary.by_source}
          mode={mode}
          splits={sourceSplits}
          labelOf={(row) => sourceLabel(row.label)}
          markOf={(row) => (
            <span className="grid size-7 shrink-0 place-items-center">
              <SourceIcon source={row.key} size={22} />
            </span>
          )}
          moreHref={href({ page: "sources" })}
        />
        {showProjects && (
          <DimensionCards
            title="按项目"
            rows={summary.by_project}
            mode={mode}
            splits={projectSplits}
            columns={3}
            className="lg:col-span-2"
            moreHref={href({ page: "projects" })}
            hrefOf={(row) =>
              row.id === null ? undefined : href({ page: "project", projectId: row.id })
            }
          />
        )}
        {!showProjects && showAccounts && (
          <DimensionCards
            title="按成员"
            kind="member"
            rows={summary.by_account}
            mode={mode}
            columns={3}
            className="lg:col-span-2"
            hrefOf={
              linkAccounts
                ? (row) =>
                    row.id === null ? undefined : href({ page: "member", accountId: row.id })
                : undefined
            }
          />
        )}
      </div>

      <div>
        <div className="mb-3">
          <Segmented label="拆分维度" value={dimension} onChange={setDimension} options={dims} />
        </div>
        <BreakdownTable
          title={dims.find((item) => item.id === dimension)?.label ?? "拆分"}
          rows={tableRows[dimension]}
          mode={mode}
          labelOf={dimension === "source" ? (row) => sourceLabel(row.label) : undefined}
          markOf={
            dimension === "source" ? (row) => <SourceIcon source={row.key} size={16} /> : undefined
          }
          linkOf={
            dimension === "account" && linkAccounts
              ? (row) => (row.id === null ? undefined : href({ page: "member", accountId: row.id }))
              : dimension === "project"
                ? (row) =>
                    row.id === null ? undefined : href({ page: "project", projectId: row.id })
                : undefined
          }
        />
      </div>
    </div>
  );
}
