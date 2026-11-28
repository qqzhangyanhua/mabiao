import { Users } from "lucide-react";
import { useMemo } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import { SummaryView } from "../components/SummaryView";
import { BehaviorPanel } from "../components/BehaviorPanel";
import { UsageSection } from "../components/UsageSection";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { summaryRows } from "../lib/export";
import { useScopeBehavior, useScopeComposition } from "../useInsights";
import { useView } from "../view";

export function OverviewPage() {
  const { api } = useAuth();
  const view = useView();
  const { bounds, offset } = view;
  const load = useMemo(
    () => (bounds ? () => api.summary(bounds, offset) : null),
    [api, bounds, offset],
  );
  const summary = useAsync(load);
  const loadSessions = useMemo(
    () => (bounds ? () => api.sessions(bounds, 1, 0) : null),
    [api, bounds],
  );
  const sessions = useAsync(loadSessions);
  const insights = useScopeComposition(bounds);
  const behavior = useScopeBehavior(bounds);

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

  return (
    <>
      <PageHeader
        icon={<Users size={22} strokeWidth={1.8} />}
        title="团队总览"
        subtitle="已推送消耗记录的费用、Token 与拆分"
      />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !data ? (
        <Loading />
      ) : (
        <>
          <SummaryView
            summary={data}
            mode={view.mode}
            range={{ fromDay: view.fromDay, toDay: view.toDay }}
            showAccounts
            linkAccounts
            sessions={sessions.data?.total}
            cacheHit={insights.cacheHit}
            onUnpriced={insights.showUnpriced}
            modelSplits={insights.modelSplits}
            sourceSplits={insights.sourceSplits}
            projectSplits={insights.projectSplits}
          />
          {insights.scope && (
            <>
              <BehaviorPanel report={behavior} />
              <UsageSection
                scope={insights.scope}
                showAccount
                composition={insights.composition.data}
                costSource={insights.costSource}
                model={insights.model}
                onCostSource={insights.setCostSource}
                onModel={insights.setModel}
                isAdmin
              />
            </>
          )}
        </>
      )}
    </>
  );
}
