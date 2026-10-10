import { useCallback } from "react";
import { useAuth } from "../auth";
import { BreakdownTable } from "../components/BreakdownTable";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { RangeBar, useViewState } from "../components/RangeBar";
import { useAsync } from "../hooks";
import { routeHash } from "../lib/route";

/** 项目清单：管理员看全体在各项目上的花费，成员只看自己的。点进去看谁花了多少。 */
export function ProjectsPage() {
  const { api, profile } = useAuth();
  const view = useViewState();
  const { bounds, offset } = view;
  const load = useCallback(
    () =>
      bounds
        ? api.summary(bounds, offset)
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, offset],
  );
  const summary = useAsync(load);

  return (
    <>
      <h1 className="text-lg font-semibold">项目</h1>
      <RangeBar view={view} />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : summary.error ? (
        <ErrorNote error={summary.error} onRetry={summary.reload} />
      ) : summary.loading || !summary.data ? (
        <Loading />
      ) : (
        <>
          <BreakdownTable
            title={profile.role === "admin" ? "全员按项目" : "我的项目"}
            rows={summary.data.by_project}
            mode={view.mode}
            linkOf={(row) =>
              row.id === null ? undefined : routeHash({ page: "project", projectId: row.id })
            }
          />
          {profile.role === "admin" && (
            <p className="text-xs text-slate-500">
              自动归并认错了？进入项目页可以手动改名或合并。
            </p>
          )}
        </>
      )}
    </>
  );
}
