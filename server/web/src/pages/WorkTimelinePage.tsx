import { Clock, Coins, Folder, Layers, UserRound, Wallet } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { FilterSelect } from "../components/FilterSelect";
import { PageHeader } from "../components/PageHeader";
import { SourceIcon } from "../components/SourceIcon";
import { WorkTimelineBoard } from "../components/WorkTimelineBoard";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { timelineRows } from "../lib/export";
import { formatCost, formatTokens } from "../lib/format";
import { sourceLabel } from "../lib/source";
import { useView } from "../view";

export function WorkTimelinePage() {
  const { api, profile } = useAuth();
  const view = useView();
  const { bounds, offset, mode } = view;
  const [accountId, setAccountId] = useState<number | undefined>(
    profile.role === "admin" ? undefined : profile.id,
  );
  const [projectId, setProjectId] = useState<number | undefined>();
  const [source, setSource] = useState<string>("");

  const loadTimeline = useCallback(
    () =>
      bounds
        ? api.workTimeline(
            {
              ...bounds,
              accountId: profile.role === "admin" ? accountId : profile.id,
              projectId,
              source: source || undefined,
            },
            offset,
          )
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, offset, accountId, projectId, source, profile],
  );
  const timeline = useAsync(loadTimeline);

  const loadFilters = useCallback(
    () =>
      bounds
        ? Promise.all([
            api.summary(
              { ...bounds, accountId: profile.role === "admin" ? accountId : profile.id },
              offset,
            ),
            profile.role === "admin" ? api.accounts() : Promise.resolve([]),
          ])
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, offset, accountId, profile],
  );
  const filters = useAsync(loadFilters);
  const summary = filters.data?.[0];
  const accounts = filters.data?.[1] ?? [];

  const data = timeline.data;
  usePageExports(
    useMemo(
      () => [
        {
          label: "导出 CSV",
          disabled: !data || data.segment_count === 0,
          onClick: () => data && downloadCsv("工作时间线.csv", timelineRows(data)),
        },
      ],
      [data],
    ),
  );

  const cost = data
    ? mode === "unified"
      ? data.unified_cost
      : data.cost_snapshot
    : 0;

  return (
    <>
      <PageHeader
        icon={<Clock size={22} strokeWidth={1.8} />}
        title="工作时间线"
        subtitle="按本地日历日铺已推送会话的起止区间。Token 与费用只统计该场落在当天的消耗记录，不进本机 token KPI，也不画账号用量或代码量。"
      />
      <div className="glass flex w-fit max-w-full flex-wrap items-center gap-2 rounded-2xl p-1.5">
        {profile.role === "admin" && (
          <FilterSelect
            icon={<UserRound size={15} />}
            label="成员"
            value={accountId ?? ""}
            onChange={(event) =>
              setAccountId(event.target.value ? Number(event.target.value) : undefined)
            }
          >
            <option value="">全部成员</option>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.account}
              </option>
            ))}
          </FilterSelect>
        )}
        <FilterSelect
          icon={<Folder size={15} />}
          label="项目"
          value={projectId ?? ""}
          onChange={(event) =>
            setProjectId(event.target.value ? Number(event.target.value) : undefined)
          }
        >
          <option value="">全部项目</option>
          {(summary?.by_project ?? [])
            .filter((row) => row.id !== null)
            .map((row) => (
              <option key={row.id} value={row.id ?? ""}>
                {row.label}
              </option>
            ))}
        </FilterSelect>
        <FilterSelect
          icon={source ? <SourceIcon source={source} size={15} /> : <Layers size={15} />}
          label="来源"
          value={source}
          onChange={(event) => setSource(event.target.value)}
        >
          <option value="">全部来源</option>
          {(summary?.by_source ?? []).map((row) => (
            <option key={row.key} value={row.key}>
              {sourceLabel(row.key)}
            </option>
          ))}
        </FilterSelect>
      </div>
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : timeline.error ? (
        <ErrorNote error={timeline.error} onRetry={timeline.reload} />
      ) : timeline.loading || !data ? (
        <Loading />
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2 text-sm tabular-nums">
            <span className="glass inline-flex items-center gap-2 rounded-full px-3.5 py-1.5">
              <Clock size={14} className="text-primary" aria-hidden="true" />
              <b className="font-semibold">{data.segment_count}</b>
              <span className="text-muted-foreground">个片段</span>
            </span>
            <span className="glass inline-flex items-center gap-2 rounded-full px-3.5 py-1.5">
              <Coins size={14} className="text-chart-4" aria-hidden="true" />
              <b className="font-semibold">{formatTokens(data.total_tokens)}</b>
              <span className="text-muted-foreground">token</span>
            </span>
            <span className="glass inline-flex items-center gap-2 rounded-full px-3.5 py-1.5">
              <Wallet size={14} className="text-chart-3" aria-hidden="true" />
              <b className="font-semibold">{formatCost(cost)}</b>
            </span>
            {data.truncated ? (
              <span className="text-xs text-warning">会话过多，只展示最早的 2000 场</span>
            ) : null}
          </div>
          <WorkTimelineBoard days={data.days} mode={mode} />
        </>
      )}
    </>
  );
}
