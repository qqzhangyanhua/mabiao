import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { humanStatus, relativeTime } from "../lib/format";
import { workNotesEngineLabel } from "../lib/workNotesPreference";
import type { WorkNotesDto, WorkNotesHistoryPage, WorkNotesHistoryRow } from "../types";
import { EmptyState } from "./EmptyState";
import { Pagination } from "./Pagination";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";
import { WorkNotesShare } from "./WorkNotesShare";

const PAGE_SIZE = 10;

type Scope = "all" | "current";

/**
 * 每次生成的纪要按「区间+引擎+模型+补充指令+会话集合」各存一条，互不覆盖。
 * 这里只做只读的列表 + 展开 + 删除，不提供“恢复到当前生成条件”的跳转——
 * 点开就是看历史内容本身，不是把选择器改回当时那一套。
 */
export function WorkNotesHistory({ currentEngineId }: { currentEngineId: string | null }) {
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<Scope>("all");
  const [page, setPage] = useState(1);
  const [data, setData] = useState<WorkNotesHistoryPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadRev, setReloadRev] = useState(0);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [expandedDto, setExpandedDto] = useState<WorkNotesDto | null>(null);
  const [expandedLoading, setExpandedLoading] = useState(false);
  const [expandedError, setExpandedError] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<number | null>(null);
  const [deletingId, setDeletingId] = useState<number | null>(null);

  const engineFilter = scope === "current" ? currentEngineId : null;

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 切页/切范围时先置 loading，避免沿用上一页的列表
    setLoading(true);
    setError(null);
    void invoke<WorkNotesHistoryPage>("list_work_notes_history", {
      query: { engine: engineFilter, page, page_size: PAGE_SIZE },
    })
      .then((result) => {
        if (!cancelled) {
          setData(result);
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setData(null);
          setError(humanStatus(caught));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, engineFilter, page, reloadRev]);

  function changeScope(next: Scope) {
    setScope(next);
    setPage(1);
    setExpandedId(null);
    setExpandedDto(null);
    setExpandedError(null);
    setPendingDeleteId(null);
  }

  function toggleExpand(row: WorkNotesHistoryRow) {
    setPendingDeleteId(null);
    if (expandedId === row.id) {
      setExpandedId(null);
      setExpandedDto(null);
      setExpandedError(null);
      return;
    }
    setExpandedId(row.id);
    setExpandedDto(null);
    setExpandedError(null);
    setExpandedLoading(true);
    void invoke<WorkNotesDto>("get_work_notes_history_entry", { id: row.id })
      .then((dto) => {
        setExpandedDto(dto);
      })
      .catch((caught: unknown) => {
        setExpandedError(humanStatus(caught));
      })
      .finally(() => {
        setExpandedLoading(false);
      });
  }

  function remove(id: number) {
    setDeletingId(id);
    void invoke("delete_work_notes_history_entry", { id })
      .then(() => {
        setPendingDeleteId(null);
        if (expandedId === id) {
          setExpandedId(null);
          setExpandedDto(null);
        }
        setReloadRev((value) => value + 1);
      })
      .catch((caught: unknown) => {
        setError(humanStatus(caught));
      })
      .finally(() => {
        setDeletingId(null);
      });
  }

  const rows = data?.rows ?? [];
  const total = data?.total ?? 0;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

  return (
    <div className="work-notes-history">
      <div className="work-notes-history-head">
        <Button onClick={() => setOpen((next) => !next)} aria-expanded={open}>
          {open ? "收起历史纪要" : "历史纪要"}
        </Button>
        {open ? (
          <Segmented
            value={scope}
            options={[
              { value: "all" as const, label: "全部引擎" },
              { value: "current" as const, label: "当前引擎" },
            ]}
            ariaLabel="历史纪要范围"
            disabled={currentEngineId == null}
            onChange={changeScope}
          />
        ) : null}
      </div>
      {open ? (
        <div className="work-notes-history-body">
          {error ? (
            <EmptyState icon="alertTriangle" tone="warn" title="加载历史纪要失败" hint={error} />
          ) : null}
          {loading ? <p className="work-notes-scale">正在加载历史纪要…</p> : null}
          {!loading && !error && rows.length === 0 ? (
            <EmptyState icon="notes" title="还没有历史纪要" compact />
          ) : null}
          {!loading && !error && rows.length > 0 ? (
            <ul className="work-notes-history-list">
              {rows.map((row) => (
                <li key={row.id} className="work-notes-history-item">
                  <div className="work-notes-history-row-wrap">
                    <button
                      type="button"
                      className="work-notes-history-row"
                      onClick={() => toggleExpand(row)}
                      aria-expanded={expandedId === row.id}
                    >
                      <span className="work-notes-history-range">
                        {row.start_date} 至 {row.end_date}
                      </span>
                      <span className="work-notes-history-headline">
                        {row.headline || "（未生成要点）"}
                      </span>
                      <span className="work-notes-history-meta">
                        {workNotesEngineLabel(row.engine)}
                        {row.model ? ` · ${row.model}` : ""} · {row.session_count} 个会话 ·{" "}
                        {relativeTime(row.created_at)}
                      </span>
                    </button>
                    <div className="work-notes-history-actions">
                      {pendingDeleteId === row.id ? (
                        <>
                          <Button
                            variant="danger"
                            disabled={deletingId === row.id}
                            onClick={() => remove(row.id)}
                          >
                            确认删除
                          </Button>
                          <Button
                            disabled={deletingId === row.id}
                            onClick={() => setPendingDeleteId(null)}
                          >
                            取消
                          </Button>
                        </>
                      ) : (
                        <Button variant="danger" onClick={() => setPendingDeleteId(row.id)}>
                          删除
                        </Button>
                      )}
                    </div>
                  </div>
                  {expandedId === row.id ? (
                    <div className="work-notes-history-detail">
                      {expandedLoading ? <p className="work-notes-scale">正在加载…</p> : null}
                      {expandedError ? (
                        <EmptyState
                          icon="alertTriangle"
                          tone="warn"
                          title="加载失败"
                          hint={expandedError}
                          compact
                        />
                      ) : null}
                      {expandedDto ? <WorkNotesShare dto={expandedDto} /> : null}
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          ) : null}
          <Pagination
            page={page}
            pageCount={pageCount}
            totalCount={total}
            onPageChange={setPage}
          />
        </div>
      ) : null}
    </div>
  );
}
