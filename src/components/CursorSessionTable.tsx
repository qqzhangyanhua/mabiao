import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  formatClock,
  formatDuration,
  formatTokens,
  projectLabel,
  relativeTime,
} from "../lib/format";
import type {
  CursorSessionListRow,
  CursorSessionPage,
  CursorSessionSortKey,
  SortDir,
} from "../types";
import type { CursorSessionTableSelect } from "./type";
import { EmptyState } from "./EmptyState";
import { ExportButton } from "./ExportButton";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { SessionIdCell, SortArrow } from "./SessionTableParts";
import { Spinner } from "./Spinner";
import { SearchField } from "./ui/Field";
import { Select } from "./ui/Select";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";

const PAGE_SIZE = 20;
const EXPORT_ROW_LIMIT = 20000;
const ALL_PROJECTS = "__all__";

const TABLE_COLUMNS: { key: CursorSessionSortKey; label: string }[] = [
  { key: "session", label: "会话 ID" },
  { key: "project", label: "项目" },
  { key: "model", label: "模型" },
  { key: "turns", label: "轮次" },
  { key: "errors", label: "失败" },
  { key: "tools", label: "工具" },
  { key: "files", label: "文件" },
  { key: "time", label: "最近活跃" },
];

const EXPORT_HEADERS = [
  "会话ID",
  "项目",
  "模型",
  "来源",
  "轮次",
  "成功",
  "失败",
  "中止",
  "工具调用",
  "改动文件",
  "开始时间",
  "最近活跃",
  "原始文件",
];

function modelsLabel(models: string[]): string {
  if (models.length === 0) {
    return "—";
  }
  return models.join(", ");
}

function sourcesLabel(sources: string[]): string {
  if (sources.length === 0) {
    return "—";
  }
  return sources.join(", ");
}

function sessionRowToExportCells(row: CursorSessionListRow): (string | number)[] {
  return [
    row.session_id,
    row.project,
    row.models.join(", "),
    row.sources.join(", "),
    row.turn_count,
    row.success_count,
    row.error_count,
    row.aborted_count,
    row.tool_call_count,
    row.files_touched,
    formatClock(row.first_seen_at),
    formatClock(row.last_seen_at),
    row.source_file,
  ];
}

export function CursorSessionTable({
  revision,
  projectNames,
  selectedProject,
  onSelectProject,
  onSelectSession,
  onError,
}: {
  revision: number;
  projectNames: string[];
  selectedProject: string | null;
  onSelectProject: (project: string | null) => void;
  onSelectSession: CursorSessionTableSelect;
  onError?: (error: unknown) => void;
}) {
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [sortKey, setSortKey] = useState<CursorSessionSortKey>("time");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [page, setPage] = useState(1);
  const [pageData, setPageData] = useState<CursorSessionPage>({ rows: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const requestGeneration = useRef(0);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setSearch(searchInput.trim());
    }, 300);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 筛选或排序变化时回到第一页
    setPage(1);
  }, [search, selectedProject, sortKey, sortDir]);

  useEffect(() => {
    const generation = ++requestGeneration.current;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 标准的“发起请求前先置 loading”写法
    setLoading(true);
    invoke<CursorSessionPage>("get_cursor_sessions_page", {
      query: {
        search: search || null,
        project: selectedProject,
        sortBy: sortKey,
        sortDir,
        page,
        pageSize: PAGE_SIZE,
      },
    })
      .then((result) => {
        if (generation === requestGeneration.current) {
          setPageData(result);
        }
      })
      .catch((error) => {
        if (generation === requestGeneration.current) {
          onError?.(error);
        }
      })
      .finally(() => {
        if (generation === requestGeneration.current) {
          setLoading(false);
        }
      });
  }, [revision, search, selectedProject, sortKey, sortDir, page, onError]);

  const { rows, total } = pageData;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

  const projectOptions = useMemo(
    () => [
      { value: ALL_PROJECTS, label: "全部项目" },
      ...[...projectNames]
        .sort((left, right) => left.localeCompare(right, "zh-CN"))
        .map((name) => ({ value: name, label: projectLabel(name) })),
    ],
    [projectNames],
  );

  function toggleSort(key: CursorSessionSortKey) {
    if (key === sortKey) {
      setSortDir((dir) => (dir === "asc" ? "desc" : "asc"));
      return;
    }
    setSortKey(key);
    setSortDir(key === "session" || key === "project" || key === "model" ? "asc" : "desc");
  }

  async function fetchAllMatchingRows(): Promise<(string | number)[][]> {
    const result = await invoke<CursorSessionPage>("get_cursor_sessions_page", {
      query: {
        search: search || null,
        project: selectedProject,
        sortBy: sortKey,
        sortDir,
        page: 1,
        pageSize: Math.min(Math.max(total, 1), EXPORT_ROW_LIMIT),
      },
    });
    return result.rows.map(sessionRowToExportCells);
  }

  return (
    <section className="panel partition">
      <div className="panel-head">
        <div>
          <h2>会话明细</h2>
          <p className="panel-note">{SESSION_ENTRY_COPY.cursorSessionsTableNote}</p>
        </div>
        <SearchField
          value={searchInput}
          onChange={setSearchInput}
          placeholder="搜索会话 ID、项目、模型或路径"
          ariaLabel="搜索 Cursor 会话"
        />
        <span className="muted">
          共 {total} 个会话
          {loading ? (
            <span className="inline-loading">
              <Spinner size={12} />
              加载中…
            </span>
          ) : null}
        </span>
        <ExportButton
          filename="Cursor会话明细"
          headers={EXPORT_HEADERS}
          getRows={fetchAllMatchingRows}
        />
      </div>
      <LoadingOverlay
        active={loading && rows.length > 0}
        className="table-scroll cursor-session-table-scroll"
      >
        <table className="cursor-session-table">
          <thead>
            <tr>
              {TABLE_COLUMNS.map((column) => (
                <th
                  key={column.key}
                  aria-sort={
                    sortKey === column.key
                      ? sortDir === "asc"
                        ? "ascending"
                        : "descending"
                      : "none"
                  }
                >
                  {column.key === "project" ? (
                    <div className="th-with-filter">
                      <Select
                        variant="plain"
                        ariaLabel="项目"
                        align="left"
                        value={selectedProject ?? ALL_PROJECTS}
                        options={projectOptions}
                        onChange={(project) =>
                          onSelectProject(project === ALL_PROJECTS ? null : project)
                        }
                      />
                      <button
                        type="button"
                        className="sort-th"
                        onClick={() => toggleSort("project")}
                      >
                        <SortArrow active={sortKey === "project"} dir={sortDir} />
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className="sort-th"
                      onClick={() => toggleSort(column.key)}
                    >
                      {column.label}
                      <SortArrow active={sortKey === column.key} dir={sortDir} />
                    </button>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr
                key={`${row.source_file}-${row.session_id}`}
                className="clickable"
                tabIndex={0}
                title={SESSION_ENTRY_COPY.openConversationRowTitle}
                aria-label={`${SESSION_ENTRY_COPY.openConversationRow}：${row.session_id}`}
                onClick={() => onSelectSession(row)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onSelectSession(row);
                  }
                }}
              >
                <td>
                  <div className="cell-stack">
                    <SessionIdCell sessionId={row.session_id} />
                    <span className="muted">{SESSION_ENTRY_COPY.openConversationRow}</span>
                  </div>
                </td>
                <td title={row.project}>
                  <div className="cell-stack">
                    <span>{projectLabel(row.project)}</span>
                    <span className="muted">{row.project}</span>
                  </div>
                </td>
                <td title={modelsLabel(row.models)}>
                  <div className="cell-stack">
                    <span>{modelsLabel(row.models)}</span>
                    <span className="muted">{sourcesLabel(row.sources)}</span>
                  </div>
                </td>
                <td
                  title={`成功 ${row.success_count} · 失败 ${row.error_count} · 中止 ${row.aborted_count} · 提问 ${row.user_prompt_count} · 子代理 ${row.subagent_count}`}
                >
                  {formatTokens(row.turn_count)}
                  {row.error_count > 0 ? (
                    <span className="muted"> / {formatTokens(row.error_count)} 失败</span>
                  ) : null}
                  {row.subagent_count > 0 ? (
                    <span className="muted"> / {formatTokens(row.subagent_count)} 子代理</span>
                  ) : null}
                </td>
                <td>{formatTokens(row.error_count)}</td>
                <td>{formatTokens(row.tool_call_count)}</td>
                <td>{formatTokens(row.files_touched)}</td>
                <td
                  title={
                    row.last_seen_at
                      ? `${formatClock(row.first_seen_at)} → ${formatClock(row.last_seen_at)}${
                          formatDuration(row.first_seen_at, row.last_seen_at)
                            ? ` · ${formatDuration(row.first_seen_at, row.last_seen_at)}`
                            : ""
                        }`
                      : undefined
                  }
                >
                  {row.last_seen_at ? relativeTime(row.last_seen_at) : "—"}
                </td>
              </tr>
            ))}
            {rows.length === 0 ? (
              <tr>
                <td colSpan={8} className="analytics-empty">
                  {loading ? (
                    <EmptyState icon="sessions" title="正在加载会话…" />
                  ) : (
                    <EmptyState
                      icon="sessions"
                      title="当前筛选条件下暂无会话"
                      hint="试试搜索会话 ID，或更换项目筛选"
                    />
                  )}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </LoadingOverlay>
      <Pagination page={page} pageCount={pageCount} totalCount={total} onPageChange={setPage} />
    </section>
  );
}
