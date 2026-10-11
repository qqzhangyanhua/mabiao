import { useCallback, useState } from "react";
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
import { COST_MODE_LABEL, costOf, type CostMode } from "../lib/cost";
import { formatCost, formatDateTime, formatTokens } from "../lib/format";
import { routeHash } from "../lib/route";

const COST_OPTIONS = (Object.keys(COST_MODE_LABEL) as CostMode[]).map((id) => ({
  id,
  label: COST_MODE_LABEL[id],
}));

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="break-all">{children}</dd>
    </div>
  );
}

/** 单场会话：正文、工具调用、上下文清单、本场消耗；成员删自己的，管理员删任意（服务端判定）。 */
export function SessionPage({ sessionId }: { sessionId: number }) {
  const { api } = useAuth();
  const [mode, setMode] = useState<CostMode>("unified");
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<Error | undefined>();
  const load = useCallback(() => api.sessionDetail(sessionId), [api, sessionId]);
  const detail = useAsync(load);

  if (detail.error) return <ErrorNote error={detail.error} onRetry={detail.reload} />;
  if (detail.loading || !detail.data) return <Loading />;
  const { session, usage, context_manifest: manifest } = detail.data;
  const totals = usage.totals;
  const cost = mode === "unified" ? totals.unified_cost_total : totals.cost_snapshot_total;

  async function remove() {
    if (!window.confirm("删除这场会话？只删会话正文，不影响消耗记录与项目。")) return;
    setDeleting(true);
    setDeleteError(undefined);
    try {
      await api.deleteSession(sessionId);
      window.location.hash = routeHash({ page: "member", accountId: session.account_id });
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
        <Button type="button" variant="destructive" size="sm" disabled={deleting} onClick={() => void remove()}>
          删除会话
        </Button>
      </div>
      {deleteError && <ErrorNote error={deleteError} />}

      <section className={cn(panelClass, "p-4")}>
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
          <Meta label="成员">
            <a
              href={routeHash({ page: "member", accountId: session.account_id })}
              className="text-primary hover:underline"
            >
              {session.account}
            </a>
          </Meta>
          <Meta label="设备">{session.device_name}</Meta>
          <Meta label="来源">{session.source}</Meta>
          <Meta label="模型">{session.model}</Meta>
          <Meta label="项目">
            {session.project_id !== null ? (
              <a
                href={routeHash({ page: "project", projectId: session.project_id })}
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

      <section className={cn(panelClass, "p-4")}>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-sm font-medium">本场消耗</h2>
          <Segmented label="费用口径" value={mode} onChange={setMode} options={COST_OPTIONS} />
        </div>
        {totals.record_count === 0 ? (
          <p className="text-sm text-muted-foreground">这场会话没有对上的消耗记录</p>
        ) : (
          <>
            <p className="text-sm">
              {totals.record_count} 条记录 · {formatTokens(totals.total_tokens)} token ·{" "}
              <span className="font-medium">{formatCost(cost)}</span>
              {mode === "unified" && totals.unified_unpriced_count > 0 && (
                <span className="ml-2 text-xs text-warning">
                  另有 {totals.unified_unpriced_count} 条没有价目，未计入
                </span>
              )}
            </p>
            <table className="mt-2 w-full text-sm">
              <tbody>
                {usage.by_model.map((row) => (
                  <tr key={row.key} className="border-t border-border/70">
                    <td className="py-1 pr-2">{row.label}</td>
                    <td className="py-1 text-right tabular-nums">
                      {formatTokens(row.total_tokens)}
                    </td>
                    <td className="py-1 text-right tabular-nums">{formatCost(costOf(row, mode))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </section>

      <section className={cn(panelClass, "p-4")}>
        <h2 className="mb-3 text-sm font-medium">上下文清单</h2>
        <ContextManifestView manifest={manifest} />
      </section>

      <section className={cn(panelClass, "p-4")}>
        <h2 className="mb-3 text-sm font-medium">正文与工具调用</h2>
        <EventList events={detail.data.events} />
      </section>
    </>
  );
}
