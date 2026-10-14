import { useCallback, useState } from "react";
import type { ScopeParams } from "../api/client";
import type { UsageComposition } from "../api/types";
import { useAuth } from "../auth";
import { useAsync } from "../hooks";
import {
  UNIFIED_COST_SOURCE_LABEL,
  unifiedCostSourceLabel,
  type UnifiedCostSource,
} from "../lib/cost";
import { formatCost, formatDateTime, formatTokens } from "../lib/format";
import { sourceLabel } from "../lib/source";
import { useHref } from "../view";
import { ErrorNote, Loading } from "./Feedback";
import { SourceBadge } from "./SourceBadge";
import { TablePager } from "./TablePager";
import { panelClass } from "./ui/card";
import { Segmented } from "./ui/segmented";
import { numCellClass, tableClass, thClass, theadRowClass, trClass } from "./ui/table";
import { cn } from "../lib/cn";

const PAGE_SIZE = 50;
const SOURCE_OPTIONS = [
  { id: "" as const, label: "全部" },
  ...(Object.keys(UNIFIED_COST_SOURCE_LABEL) as UnifiedCostSource[]).map((id) => ({
    id,
    label: UNIFIED_COST_SOURCE_LABEL[id],
  })),
];

export function UsageSection({
  scope,
  showAccount,
  composition,
  costSource,
  model,
  onCostSource,
  onModel,
  isAdmin,
}: {
  scope: ScopeParams;
  showAccount: boolean;
  composition: UsageComposition | undefined;
  costSource: UnifiedCostSource | "";
  model: string;
  onCostSource: (value: UnifiedCostSource | "") => void;
  onModel: (value: string) => void;
  isAdmin: boolean;
}) {
  const { api } = useAuth();
  const href = useHref();
  const [source, setSource] = useState("");
  const [page, setPage] = useState(0);
  const filterKey = [
    scope.accountId ?? "",
    scope.projectId ?? "",
    scope.from ?? "",
    scope.to ?? "",
    costSource,
    model,
    source,
  ].join("\0");
  const [pageKey, setPageKey] = useState(filterKey);
  if (pageKey !== filterKey) {
    setPageKey(filterKey);
    setPage(0);
  }

  const load = useCallback(
    () =>
      api.usage(scope, PAGE_SIZE, page * PAGE_SIZE, {
        source: source || undefined,
        model: model || undefined,
        costSource: costSource || undefined,
      }),
    [api, scope, page, source, model, costSource],
  );
  const usage = useAsync(load);
  const pageCount = usage.data ? Math.max(1, Math.ceil(usage.data.totals.record_count / PAGE_SIZE)) : 1;
  const unpriced = composition?.unpriced_models ?? [];

  return (
    <section className={cn(panelClass, "p-4")}>
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-medium">消耗明细</h2>
        <Segmented
          label="定价来源"
          size="compact"
          value={costSource}
          onChange={onCostSource}
          options={SOURCE_OPTIONS}
        />
      </div>
      <div className="mb-3 flex flex-wrap items-center gap-2 text-sm">
        <label className="flex items-center gap-2">
          <span className="text-muted-foreground">来源</span>
          <select
            aria-label="来源"
            className="h-8 rounded-lg border border-input bg-card px-2"
            value={source}
            onChange={(event) => setSource(event.target.value)}
          >
            <option value="">全部</option>
            {(composition?.by_source ?? []).map((row) => (
              <option key={row.key} value={row.key}>
                {sourceLabel(row.key)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex items-center gap-2">
          <span className="text-muted-foreground">模型</span>
          <select
            aria-label="模型"
            className="h-8 max-w-56 rounded-lg border border-input bg-card px-2"
            value={model}
            onChange={(event) => onModel(event.target.value)}
          >
            <option value="">全部</option>
            {(composition?.by_model ?? []).map((row) => (
              <option key={row.key} value={row.key}>
                {row.key}
              </option>
            ))}
            {model && !(composition?.by_model ?? []).some((row) => row.key === model) && (
              <option value={model}>{model}</option>
            )}
          </select>
        </label>
      </div>
      {unpriced.length > 0 && (
        <p className="mb-3 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <span>未定价模型</span>
          {unpriced.map((row) => (
            <button
              key={row.model}
              type="button"
              className="rounded-md bg-warning/15 px-2 py-0.5 text-warning"
              onClick={() => {
                onCostSource("none");
                onModel(row.model);
              }}
            >
              {row.model} · {row.count}
            </button>
          ))}
          {isAdmin && (
            <a href={href({ page: "pricing" })} className="text-primary hover:underline">
              去价目
            </a>
          )}
        </p>
      )}
      {usage.error ? (
        <ErrorNote error={usage.error} onRetry={usage.reload} />
      ) : usage.loading || !usage.data ? (
        <Loading />
      ) : usage.data.records.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">没有符合筛选的消耗记录</p>
      ) : (
        <>
          <div className="overflow-x-auto">
            <table className={tableClass}>
              <caption className="sr-only">消耗明细</caption>
              <thead>
                <tr className={theadRowClass}>
                  {["时间", showAccount ? "成员" : null, "来源", "模型", "提供商", "项目", "token", "统一价", "定价来源", "客户端快照"]
                    .filter((label): label is string => label !== null)
                    .map((label) => (
                      <th key={label} scope="col" className={`px-3 py-2 ${thClass}`}>
                        {label}
                      </th>
                    ))}
                </tr>
              </thead>
              <tbody>
                {usage.data.records.map((row) => (
                  <tr key={row.id} className={trClass}>
                    <td className="whitespace-nowrap px-3 py-2 tabular-nums">
                      {row.session_pk !== null ? (
                        <a href={href({ page: "session", sessionId: row.session_pk })} className="text-primary hover:underline">
                          {formatDateTime(row.occurred_at)}
                        </a>
                      ) : (
                        formatDateTime(row.occurred_at)
                      )}
                    </td>
                    {showAccount && <td className="px-3 py-2">{row.account}</td>}
                    <td className="px-3 py-2">
                      <SourceBadge source={row.source} />
                    </td>
                    <td className="max-w-40 truncate px-3 py-2" title={row.model}>
                      {row.model}
                    </td>
                    <td className="px-3 py-2">{row.provider || "—"}</td>
                    <td className="max-w-40 truncate px-3 py-2" title={row.project}>
                      {row.project || "—"}
                    </td>
                    <td className={`px-3 py-2 ${numCellClass}`}>{formatTokens(row.tokens.total)}</td>
                    <td className={`px-3 py-2 ${numCellClass}`}>
                      {row.unified_cost === null ? "—" : formatCost(row.unified_cost)}
                    </td>
                    <td className="px-3 py-2">{unifiedCostSourceLabel(row.unified_cost_source)}</td>
                    <td className={`px-3 py-2 ${numCellClass}`}>
                      {row.cost_snapshot === null ? "—" : formatCost(row.cost_snapshot)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <TablePager page={page} pageCount={pageCount} onPageChange={setPage} />
        </>
      )}
    </section>
  );
}
