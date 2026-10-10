import { useCallback, useState } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { RangeBar, useViewState } from "../components/RangeBar";
import { SessionTable } from "../components/SessionTable";
import { SummaryView } from "../components/SummaryView";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { sessionRows, summaryRows } from "../lib/export";

const PAGE_SIZE = 50;

/** 单个成员：趋势、常用来源与模型、会话列表。成员看自己，管理员看任何人。 */
export function MemberPage({ accountId }: { accountId: number }) {
  const { api, profile } = useAuth();
  const view = useViewState();
  const { bounds, offset } = view;
  const [page, setPage] = useState(0);

  const loadName = useCallback(
    async () =>
      profile.role === "admin"
        ? ((await api.accounts()).find((a) => a.id === accountId)?.account ?? `账号 ${accountId}`)
        : profile.account,
    [api, profile, accountId],
  );
  const name = useAsync(loadName);

  const loadSummary = useCallback(
    () =>
      bounds
        ? api.summary({ ...bounds, accountId }, offset)
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, offset, accountId],
  );
  const summary = useAsync(loadSummary);

  const loadSessions = useCallback(
    () =>
      bounds
        ? api.sessions({ ...bounds, accountId }, PAGE_SIZE, page * PAGE_SIZE)
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, accountId, page],
  );
  const sessions = useAsync(loadSessions);

  const title = name.data ?? "成员";
  const pageCount = sessions.data ? Math.max(1, Math.ceil(sessions.data.total / PAGE_SIZE)) : 1;

  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-lg font-semibold">
          {title}
          {accountId === profile.id && (
            <span className="ml-2 text-sm font-normal text-slate-500">（我）</span>
          )}
        </h1>
        <div className="flex gap-2">
          <button
            type="button"
            disabled={!summary.data}
            onClick={() =>
              summary.data && downloadCsv(`${title}-汇总.csv`, summaryRows(summary.data))
            }
            className="rounded border border-slate-300 bg-white px-3 py-1 text-sm hover:bg-slate-100 disabled:opacity-50"
          >
            导出汇总 CSV
          </button>
          <button
            type="button"
            disabled={!sessions.data}
            onClick={() =>
              sessions.data && downloadCsv(`${title}-会话.csv`, sessionRows(sessions.data.sessions))
            }
            className="rounded border border-slate-300 bg-white px-3 py-1 text-sm hover:bg-slate-100 disabled:opacity-50"
          >
            导出本页会话 CSV
          </button>
        </div>
      </div>
      <RangeBar
        view={{
          ...view,
          // 换区间后回到第一页，免得停在一个已经不存在的页码上。
          setPreset: (p) => {
            setPage(0);
            view.setPreset(p);
          },
          setCustom: (r) => {
            setPage(0);
            view.setCustom(r);
          },
        }}
      />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !summary.data ? (
        <Loading />
      ) : (
        <SummaryView
          summary={summary.data}
          mode={view.mode}
          showAccounts={false}
          linkAccounts={false}
        />
      )}

      {bounds && (
        <section className="rounded border border-slate-200 bg-white p-4">
          <h2 className="mb-3 text-sm font-medium text-slate-700">
            会话{sessions.data ? `（共 ${sessions.data.total} 场）` : ""}
          </h2>
          {sessions.error ? (
            <ErrorNote error={sessions.error} onRetry={sessions.reload} />
          ) : sessions.loading || !sessions.data ? (
            <Loading />
          ) : (
            <>
              <SessionTable sessions={sessions.data.sessions} />
              {pageCount > 1 && (
                <div className="mt-3 flex items-center justify-end gap-3 text-sm">
                  <button
                    type="button"
                    disabled={page === 0}
                    onClick={() => setPage(page - 1)}
                    className="rounded border border-slate-300 px-2 py-1 disabled:opacity-50"
                  >
                    上一页
                  </button>
                  <span>
                    {page + 1} / {pageCount}
                  </span>
                  <button
                    type="button"
                    disabled={page + 1 >= pageCount}
                    onClick={() => setPage(page + 1)}
                    className="rounded border border-slate-300 px-2 py-1 disabled:opacity-50"
                  >
                    下一页
                  </button>
                </div>
              )}
            </>
          )}
        </section>
      )}
    </>
  );
}
