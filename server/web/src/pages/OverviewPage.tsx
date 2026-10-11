import { useCallback, useMemo } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { SummaryView } from "../components/SummaryView";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { summaryRows } from "../lib/export";
import { splitsBy, totalSplit } from "../lib/usageAgg";
import { cacheHitRate } from "../lib/tokens";
import { useView } from "../view";

export function OverviewPage() {
  const { api } = useAuth();
  const view = useView();
  const { bounds, offset } = view;
  const load = useCallback(
    () => (bounds ? api.summary(bounds, offset) : Promise.reject(new Error("日期范围不正确"))),
    [api, bounds, offset],
  );
  const summary = useAsync(load);
  const loadSessions = useCallback(
    () => (bounds ? api.sessions(bounds, 1, 0) : Promise.reject(new Error("日期范围不正确"))),
    [api, bounds],
  );
  const sessions = useAsync(loadSessions);
  const loadUsage = useCallback(
    () => (bounds ? api.usage(bounds, 1000, 0) : Promise.reject(new Error("日期范围不正确"))),
    [api, bounds],
  );
  const usage = useAsync(loadUsage);

  const data = summary.data;
  usePageExports(
    useMemo(
      () => [
        {
          label: "导出 CSV",
          disabled: !data,
          onClick: () => data && downloadCsv("团队总览.csv", summaryRows(data)),
        },
      ],
      [data],
    ),
  );

  const records = usage.data?.records ?? [];
  const split = totalSplit(records);

  return (
    <>
      <h1 className="text-lg font-semibold">团队总览</h1>
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !data ? (
        <Loading />
      ) : (
        <SummaryView
          summary={data}
          mode={view.mode}
          range={{ fromDay: view.fromDay, toDay: view.toDay }}
          showAccounts
          linkAccounts
          sessions={sessions.data?.total}
          cacheHit={records.length > 0 ? cacheHitRate(split) : null}
          modelSplits={splitsBy(records, (row) => row.model)}
          projectSplits={splitsBy(records, (row) =>
            row.project_id !== null ? String(row.project_id) : "",
          )}
        />
      )}
    </>
  );
}
