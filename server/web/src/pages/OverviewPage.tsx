import { useCallback } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { RangeBar, useViewState } from "../components/RangeBar";
import { SummaryView } from "../components/SummaryView";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { summaryRows } from "../lib/export";

/** 团队总览（管理员）。费用切口径只改展示，不重新请求。 */
export function OverviewPage() {
  const { api } = useAuth();
  const view = useViewState();
  const { bounds, offset } = view;
  const load = useCallback(
    () => (bounds ? api.summary(bounds, offset) : Promise.reject(new Error("日期范围不正确"))),
    [api, bounds, offset],
  );
  const { data, error, loading, reload } = useAsync(load);

  return (
    <>
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">团队总览</h1>
        <button
          type="button"
          disabled={!data}
          onClick={() => data && downloadCsv("团队总览.csv", summaryRows(data))}
          className="rounded border border-slate-300 bg-white px-3 py-1 text-sm hover:bg-slate-100 disabled:opacity-50"
        >
          导出 CSV
        </button>
      </div>
      <RangeBar view={view} />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : error ? (
        <ErrorNote error={error} onRetry={reload} />
      ) : loading || !data ? (
        <Loading />
      ) : (
        <SummaryView summary={data} mode={view.mode} showAccounts linkAccounts />
      )}
    </>
  );
}
