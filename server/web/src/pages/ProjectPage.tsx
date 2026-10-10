import { useCallback, useState } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import {
  generatedParam,
  GeneratedFilterSelect,
  type GeneratedFilter,
} from "../components/GeneratedFilter";
import { ProjectAdmin } from "../components/ProjectAdmin";
import { RangeBar, useViewState } from "../components/RangeBar";
import { SessionTable } from "../components/SessionTable";
import { SummaryView } from "../components/SummaryView";
import { useAsync } from "../hooks";

const SESSION_LIMIT = 50;

/** 单个项目：谁在上面花了多少 token / 费用。成员只看到自己的那部分，管理员看全体并可改名、合并。 */
export function ProjectPage({ projectId }: { projectId: number }) {
  const { api, profile } = useAuth();
  const view = useViewState();
  const { bounds, offset } = view;
  const [generated, setGenerated] = useState<GeneratedFilter>("all");

  const loadProject = useCallback(
    () =>
      bounds
        ? api.project(projectId, bounds, offset)
        : Promise.reject(new Error("日期范围不正确")),
    [api, projectId, bounds, offset],
  );
  const project = useAsync(loadProject);

  const loadSessions = useCallback(
    () =>
      bounds
        ? api.sessions({ ...bounds, projectId }, SESSION_LIMIT, 0, generatedParam(generated))
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, projectId, generated],
  );
  const sessions = useAsync(loadSessions);

  const isAdmin = profile.role === "admin";
  const detail = project.data;

  return (
    <>
      <div>
        <h1 className="text-lg font-semibold">{detail?.project.name ?? "项目"}</h1>
        {detail && (
          <p className="text-xs text-slate-500">
            {detail.project.git_remote ?? "没有 git remote（按目录名归并）"}
            {!isAdmin && " · 只统计你自己的用量"}
          </p>
        )}
      </div>
      <RangeBar view={view} />
      {!bounds ? (
        <Empty>请选择完整的起止日期（结束不早于开始）</Empty>
      ) : project.error ? (
        <ErrorNote error={project.error} onRetry={project.reload} />
      ) : project.loading || !detail ? (
        <Loading />
      ) : (
        <SummaryView
          summary={detail.summary}
          mode={view.mode}
          showAccounts
          linkAccounts={isAdmin}
          showProjects={false}
        />
      )}
      {isAdmin && detail && (
        <ProjectAdmin
          key={detail.project.name}
          project={detail.project}
          onRenamed={project.reload}
        />
      )}
      {bounds && (
        <section className="rounded border border-slate-200 bg-white p-4">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-sm font-medium text-slate-700">
              会话
              {sessions.data
                ? `（共 ${sessions.data.total} 场，显示最近 ${sessions.data.sessions.length} 场）`
                : ""}
            </h2>
            <GeneratedFilterSelect value={generated} onChange={setGenerated} />
          </div>
          {sessions.error ? (
            <ErrorNote error={sessions.error} onRetry={sessions.reload} />
          ) : sessions.loading || !sessions.data ? (
            <Loading />
          ) : (
            <SessionTable sessions={sessions.data.sessions} />
          )}
        </section>
      )}
    </>
  );
}
