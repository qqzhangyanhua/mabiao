import { Bot } from "lucide-react";
import { useCallback, useMemo } from "react";
import { useAuth } from "../auth";
import { DimensionCards } from "../components/DimensionCards";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import { SourceIcon } from "../components/SourceIcon";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { downloadCsv } from "../lib/csv";
import { summaryRows } from "../lib/export";
import { sourceLabel } from "../lib/source";
import { splitsBy } from "../lib/usageAgg";
import { useView } from "../view";

export function SourcesPage() {
  const { api, profile } = useAuth();
  const view = useView();
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
          onClick: () => data && downloadCsv("Agent工具.csv", summaryRows(data)),
        },
      ],
      [data],
    ),
  );

  return (
    <>
      <PageHeader
        icon={<Bot size={22} strokeWidth={1.8} />}
        title="Agent 工具"
        subtitle={
          profile.role === "admin"
            ? "按来源汇总全员已推送用量"
            : "按来源汇总你已推送的用量"
        }
      />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !data ? (
        <Loading />
      ) : (
        <DimensionCards
          title="按 Agent 工具"
          kind="source"
          rows={data.by_source}
          mode={view.mode}
          limit={null}
          splits={splitsBy(usage.data?.records ?? [], (row) => row.source)}
          labelOf={(row) => sourceLabel(row.label)}
          markOf={(row) => (
            <span className="grid size-7 shrink-0 place-items-center">
              <SourceIcon source={row.key} size={22} />
            </span>
          )}
        />
      )}
    </>
  );
}
