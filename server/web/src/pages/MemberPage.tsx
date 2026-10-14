import { ChartColumn } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useAuth } from "../auth";
import { BackLink } from "../components/BackLink";
import { BehaviorPanel } from "../components/BehaviorPanel";
import { DeviceList } from "../components/DeviceList";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import {
  generatedParam,
  type GeneratedFilter,
} from "../components/GeneratedFilter";
import { SessionListSection } from "../components/SessionListSection";
import { SessionTable } from "../components/SessionTable";
import { SummaryView } from "../components/SummaryView";
import { TablePager } from "../components/TablePager";
import { UsageSection } from "../components/UsageSection";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { sessionRows, summaryRows } from "../lib/export";
import { useScopeInsights } from "../useInsights";
import { useView } from "../view";

const PAGE_SIZE = 50;

export function MemberPage({ accountId }: { accountId: number }) {
  const { api, profile } = useAuth();
  const view = useView();
  const { bounds, offset } = view;
  const [page, setPage] = useState(0);
  const [generated, setGenerated] = useState<GeneratedFilter>("all");
  const scope = useMemo(
    () => (bounds ? { ...bounds, accountId } : null),
    [bounds, accountId],
  );
  const insights = useScopeInsights(scope);
  const loadDevices = useCallback(() => api.devices(accountId), [api, accountId]);
  const devices = useAsync(loadDevices);

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

  const title = name.data ?? "成员";
  const pageCount = sessions.data ? Math.max(1, Math.ceil(sessions.data.total / PAGE_SIZE)) : 1;

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
      {profile.role === "admin" && accountId !== profile.id && (
        <BackLink current={{ page: "member", accountId }} fallback={{ page: "members" }} />
      )}
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
          cacheHit={insights.cacheHit}
          onUnpriced={insights.showUnpriced}
          modelSplits={insights.modelSplits}
          sourceSplits={insights.sourceSplits}
          projectSplits={insights.projectSplits}
        />
      )}

      <DeviceList devices={devices} />
      {insights.scope && (
        <>
          <BehaviorPanel report={insights.behavior} />
          <UsageSection
            scope={insights.scope}
            showAccount={false}
            composition={insights.composition.data}
            costSource={insights.costSource}
            model={insights.model}
            onCostSource={insights.setCostSource}
            onModel={insights.setModel}
            isAdmin={profile.role === "admin"}
          />
        </>
      )}

      {bounds && (
        <SessionListSection
          title={<>会话{sessions.data ? `（共 ${sessions.data.total} 场）` : ""}</>}
          generated={generated}
          onGeneratedChange={(value) => {
            setPage(0);
            setGenerated(value);
          }}
          footer={
            sessions.data ? (
              <TablePager page={page} pageCount={pageCount} onPageChange={setPage} />
            ) : null
          }
        >
          {sessions.error ? (
            <ErrorNote error={sessions.error} onRetry={sessions.reload} />
          ) : sessions.loading || !sessions.data ? (
            <Loading />
          ) : (
            <SessionTable sessions={sessions.data.sessions} />
          )}
        </SessionListSection>
      )}
    </>
  );
}
