import { Folder } from "lucide-react";
import { useCallback, useState } from "react";
import { useAuth } from "../auth";
import { BackLink } from "../components/BackLink";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { PageHeader } from "../components/PageHeader";
import {
  generatedParam,
  type GeneratedFilter,
} from "../components/GeneratedFilter";
import { ProjectAdmin } from "../components/ProjectAdmin";
import { SessionListSection } from "../components/SessionListSection";
import { SessionTable } from "../components/SessionTable";
import { SummaryView } from "../components/SummaryView";
import { TablePager } from "../components/TablePager";
import { useAsync } from "../hooks";
import { cacheHitRate } from "../lib/tokens";
import { splitsBy, totalSplit } from "../lib/usageAgg";
import { useView } from "../view";

const PAGE_SIZE = 50;

export function ProjectPage({ projectId }: { projectId: number }) {
  const { api, profile } = useAuth();
  const view = useView();
  const { bounds, offset } = view;
  const [generated, setGenerated] = useState<GeneratedFilter>("all");
  const [page, setPage] = useState(0);

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
        ? api.sessions(
            { ...bounds, projectId },
            PAGE_SIZE,
            page * PAGE_SIZE,
            generatedParam(generated),
          )
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, projectId, generated, page],
  );
  const sessions = useAsync(loadSessions);
  const loadUsage = useCallback(
    () =>
      bounds
        ? api.usage({ ...bounds, projectId }, 1000, 0)
        : Promise.reject(new Error("日期范围不正确")),
    [api, bounds, projectId],
  );
  const usage = useAsync(loadUsage);

  const isAdmin = profile.role === "admin";
  const detail = project.data;
  const records = usage.data?.records ?? [];
  const pageCount = sessions.data ? Math.max(1, Math.ceil(sessions.data.total / PAGE_SIZE)) : 1;

  return (
    <>
      <BackLink current={{ page: "project", projectId }} fallback={{ page: "projects" }} />
      <PageHeader
        icon={<Folder size={22} strokeWidth={1.8} />}
        title={detail?.project.name ?? "项目"}
        subtitle={
          detail
            ? `${detail.project.git_remote ?? "没有 git remote（按目录名归并）"}${
                !isAdmin ? " · 只统计你自己的用量" : ""
              }`
            : undefined
        }
      />
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
          range={{ fromDay: view.fromDay, toDay: view.toDay }}
          showAccounts
          linkAccounts={isAdmin}
          showProjects={false}
          sessions={sessions.data?.total}
          cacheHit={records.length > 0 ? cacheHitRate(totalSplit(records)) : null}
          modelSplits={splitsBy(records, (row) => row.model)}
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
