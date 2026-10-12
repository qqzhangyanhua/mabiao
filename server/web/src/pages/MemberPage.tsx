import { ChartColumn } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import {
  generatedParam,
  GeneratedFilterSelect,
  type GeneratedFilter,
} from "../components/GeneratedFilter";
import { SessionTable } from "../components/SessionTable";
import { SummaryView } from "../components/SummaryView";
import { Button } from "../components/ui/button";
import { panelClass } from "../components/ui/card";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { downloadCsv } from "../lib/csv";
import { sessionRows, summaryRows } from "../lib/export";
import { cacheHitRate } from "../lib/tokens";
import { splitsBy, totalSplit } from "../lib/usageAgg";
import { useView } from "../view";

const PAGE_SIZE = 50;

export function MemberPage({ accountId }: { accountId: number }) {
  const { api, profile } = useAuth();
  const view = useView();
  const { bounds, offset } = view;
  const [page, setPage] = useState(0);
  const [generated, setGenerated] = useState<GeneratedFilter>("all");

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
        ? api.sessions(
            { ...bounds, accountId },
            PAGE_SIZE,
            page * PAGE_SIZE,
            generatedParam(generated),
          )
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, accountId, page, generated],
  );
  const sessions = useAsync(loadSessions);
  const loadUsage = useCallback(
    () =>
      bounds
        ? api.usage({ ...bounds, accountId }, 1000, 0)
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, accountId],
  );
  const usage = useAsync(loadUsage);

  const title = name.data ?? "成员";
  const pageCount = sessions.data ? Math.max(1, Math.ceil(sessions.data.total / PAGE_SIZE)) : 1;
  const records = usage.data?.records ?? [];

  usePageExports(
    useMemo(
      () => [
        {
          label: "导出汇总 CSV",
          disabled: !summary.data,
          onClick: () =>
            summary.data && downloadCsv(`${title}-汇总.csv`, summaryRows(summary.data)),
        },
        {
          label: "导出本页会话 CSV",
          disabled: !sessions.data,
          onClick: () =>
            sessions.data && downloadCsv(`${title}-会话.csv`, sessionRows(sessions.data.sessions)),
        },
      ],
      [summary.data, sessions.data, title],
    ),
  );

  return (
    <>
      <PageHeader
        icon={<ChartColumn size={22} strokeWidth={1.8} />}
        title={
          <>
            {title}
            {accountId === profile.id && (
              <span className="ml-2 text-sm font-normal text-muted-foreground">（我）</span>
            )}
          </>
        }
        subtitle="该成员已推送的消耗记录与会话"
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
          range={{ fromDay: view.fromDay, toDay: view.toDay }}
          showAccounts={false}
          linkAccounts={false}
          sessions={sessions.data?.total}
          cacheHit={records.length > 0 ? cacheHitRate(totalSplit(records)) : null}
          modelSplits={splitsBy(records, (row) => row.model)}
          projectSplits={splitsBy(records, (row) =>
            row.project_id !== null ? String(row.project_id) : "",
          )}
        />
      )}

      {bounds && (
        <section className={cn(panelClass, "p-4")}>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-sm font-medium">
              会话{sessions.data ? `（共 ${sessions.data.total} 场）` : ""}
            </h2>
            <GeneratedFilterSelect
              value={generated}
              onChange={(value) => {
                setPage(0);
                setGenerated(value);
              }}
            />
          </div>
          {sessions.error ? (
            <ErrorNote error={sessions.error} onRetry={sessions.reload} />
          ) : sessions.loading || !sessions.data ? (
            <Loading />
          ) : (
            <>
              <SessionTable sessions={sessions.data.sessions} />
              {pageCount > 1 && (
                <div className="mt-3 flex items-center justify-end gap-3 text-sm">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={page === 0}
                    onClick={() => setPage(page - 1)}
                  >
                    上一页
                  </Button>
                  <span>
                    {page + 1} / {pageCount}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={page + 1 >= pageCount}
                    onClick={() => setPage(page + 1)}
                  >
                    下一页
                  </Button>
                </div>
              )}
            </>
          )}
        </section>
      )}
    </>
  );
}
