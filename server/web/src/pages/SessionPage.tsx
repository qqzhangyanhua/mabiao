import { useCallback, useState, type ReactNode } from "react";
import { useAuth } from "../auth";
import { ContextManifestView } from "../components/ContextManifestView";
import { ErrorNote, Loading } from "../components/Feedback";
import { EventList } from "../components/EventList";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { panelClass } from "../components/ui/card";
import { Segmented } from "../components/ui/segmented";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { formatDateTime } from "../lib/format";
import { sourceLabel } from "../lib/source";
import { useHref } from "../view";

const COST_OPTIONS = (Object.keys(COST_MODE_LABEL) as CostMode[]).map((id) => ({
  id,
  label: COST_MODE_LABEL[id],
}));

function Meta({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="break-all">{children}</dd>
    </div>
  );
}

export function SessionPage({ sessionId }: { sessionId: number }) {
  const { api } = useAuth();
  const href = useHref();
  const [mode, setMode] = useState<CostMode>("unified");
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<Error | undefined>();
  const load = useCallback(() => api.sessionDetail(sessionId), [api, sessionId]);
  const detail = useAsync(load);

  if (detail.error) return <ErrorNote error={detail.error} onRetry={detail.reload} />;
  if (detail.loading || !detail.data) return <Loading />;
  const { session, usage, context_manifest: manifest } = detail.data;

  async function remove() {
    if (!window.confirm("删除这场会话？只删会话正文，不影响消耗记录与项目。")) return;
    setDeleting(true);
    setDeleteError(undefined);
    try {
      await api.deleteSession(sessionId);
      window.location.hash = href({ page: "member", accountId: session.account_id });
    } catch (error) {
      setDeleteError(error instanceof Error ? error : new Error(String(error)));
      setDeleting(false);
    }
  }

  return (
    <>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <h1 className="text-lg font-semibold">
          {session.title || "（无标题）"}
          {session.generated_by_work_notes && (
            <Badge variant="secondary" className="ml-2 align-middle font-normal">
              码表生成
            </Badge>
          )}
        </h1>
        <div className="flex items-center gap-2">
          <Segmented label="费用口径" value={mode} onChange={setMode} options={COST_OPTIONS} />
          <Button
            type="button"
            variant="destructive"
            size="sm"
            disabled={deleting}
            onClick={() => void remove()}
          >
            删除会话
          </Button>
        </div>
      </div>
      {deleteError && <ErrorNote error={deleteError} />}

      <section className={cn(panelClass, "p-4")}>
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
          <Meta label="成员">
            <a
              href={href({ page: "member", accountId: session.account_id })}
              className="text-primary hover:underline"
            >
              {session.account}
            </a>
          </Meta>
          <Meta label="设备">{session.device_name}</Meta>
          <Meta label="来源">{sourceLabel(session.source)}</Meta>
          <Meta label="模型">{session.model}</Meta>
          <Meta label="项目">
            {session.project_id !== null ? (
              <a
                href={href({ page: "project", projectId: session.project_id })}
                className="text-primary hover:underline"
              >
                {session.project_name ?? session.project}
              </a>
            ) : (
              session.project || "—"
            )}
          </Meta>
          <Meta label="开始">{formatDateTime(session.started_at)}</Meta>
          <Meta label="结束">{formatDateTime(session.ended_at)}</Meta>
          <Meta label="打码处数">{detail.data.redaction_count}</Meta>
        </dl>
      </section>

      <EventList
        events={detail.data.events}
        usage={usage}
        mode={mode}
        startedAt={session.started_at}
        endedAt={session.ended_at}
      />

      <section className={cn(panelClass, "p-4")}>
        <h2 className="mb-3 text-sm font-medium">上下文清单</h2>
        <ContextManifestView manifest={manifest} />
      </section>
    </>
  );
}
