import { ChevronRight } from "lucide-react";
import { useCallback, useState } from "react";
import { useAuth } from "../auth";
import { ContextManifestView } from "../components/ContextManifestView";
import { ErrorNote, Loading } from "../components/Feedback";
import { EventList } from "../components/EventList";
import { PageHeader } from "../components/PageHeader";
import { MetaPill, SourceBadge } from "../components/SourceBadge";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { panelClass } from "../components/ui/card";
import { Segmented } from "../components/ui/segmented";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { COST_MODE_LABEL, type CostMode } from "../lib/cost";
import { formatDateTime } from "../lib/format";
import { useHref } from "../view";

const COST_OPTIONS = (Object.keys(COST_MODE_LABEL) as CostMode[]).map((id) => ({
  id,
  label: COST_MODE_LABEL[id],
}));

export function SessionPage({ sessionId }: { sessionId: number }) {
  const { api } = useAuth();
  const href = useHref();
  const [mode, setMode] = useState<CostMode>("unified");
  const [manifestOpen, setManifestOpen] = useState(false);
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

  const projectLabel = session.project_name ?? session.project ?? "—";

  return (
    <>
      <PageHeader
        title={
          <>
            {session.title || "（无标题）"}
            {session.generated_by_work_notes && (
              <Badge variant="secondary" className="ml-2 align-middle font-normal">
                码表生成
              </Badge>
            )}
          </>
        }
        subtitle={
          <span className="tabular-nums">
            {formatDateTime(session.started_at)} – {formatDateTime(session.ended_at)} ·{" "}
            {session.event_count} 轮 · 打码 {detail.data.redaction_count} 处
          </span>
        }
        extra={
          <div className="flex items-center gap-2">
            <Segmented
              label="费用口径"
              value={mode}
              onChange={setMode}
              options={COST_OPTIONS}
              size="compact"
            />
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
        }
      />
      {deleteError && <ErrorNote error={deleteError} />}

      <section className={cn(panelClass, "p-4")}>
        <div className="flex flex-wrap items-center gap-2">
          <MetaPill href={href({ page: "member", accountId: session.account_id })}>
            {session.account}
          </MetaPill>
          <SourceBadge source={session.source} />
          <MetaPill>{session.model}</MetaPill>
          {session.project_id !== null ? (
            <MetaPill href={href({ page: "project", projectId: session.project_id })}>
              {projectLabel}
            </MetaPill>
          ) : (
            <MetaPill>{projectLabel}</MetaPill>
          )}
          <MetaPill>{session.device_name}</MetaPill>
        </div>
      </section>

      <EventList
        events={detail.data.events}
        usage={usage}
        mode={mode}
        source={session.source}
        sessionId={String(sessionId)}
        startedAt={session.started_at}
        endedAt={session.ended_at}
      />

      <section className={cn(panelClass, "p-4")}>
        <button
          type="button"
          className="flex w-full items-center gap-1.5 text-left text-sm font-medium"
          aria-expanded={manifestOpen}
          onClick={() => setManifestOpen((open) => !open)}
        >
          <ChevronRight
            className={cn("size-4 shrink-0 text-muted-foreground", manifestOpen && "rotate-90")}
            aria-hidden
          />
          上下文清单
        </button>
        {manifestOpen ? (
          <div className="mt-3">
            <ContextManifestView manifest={manifest} />
          </div>
        ) : null}
      </section>
    </>
  );
}
