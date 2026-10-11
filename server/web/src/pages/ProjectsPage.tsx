import { useCallback, useMemo } from "react";
import { useAuth } from "../auth";
import { DimensionCards } from "../components/DimensionCards";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { summaryRows } from "../lib/export";
import { splitsBy } from "../lib/usageAgg";
import { useHref, useView } from "../view";

export function ProjectsPage() {
  const { api, profile } = useAuth();
  const view = useView();
  const href = useHref();
  const { bounds, offset } = view;
  const load = useCallback(
    () => (bounds ? api.summary(bounds, offset) : Promise.reject(new Error("日期范围不正确"))),
    [api, bounds, offset],
  );
  const summary = useAsync(load);
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
          onClick: () => data && downloadCsv("项目.csv", summaryRows(data)),
        },
      ],
      [data],
    ),
  );

  return (
    <>
      <PageHeader title="项目" subtitle="按项目汇总已推送用量" />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !data ? (
        <Loading />
      ) : (
        <>
          <DimensionCards
            title={profile.role === "admin" ? "全员按项目" : "我的项目"}
            rows={data.by_project}
            mode={view.mode}
            splits={splitsBy(usage.data?.records ?? [], (row) =>
              row.project_id !== null ? String(row.project_id) : "",
            )}
            hrefOf={(row) => (row.id === null ? undefined : href({ page: "project", projectId: row.id }))}
          />
          {profile.role === "admin" && (
            <p className="text-xs text-muted-foreground">
              自动归并认错了？进入项目页可以手动改名或合并。
            </p>
          )}
        </>
      )}
    </>
  );
}
