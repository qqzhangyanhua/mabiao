import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { CATALOG_PAGE_SIZE } from "../lib/conversationCatalogItems";
import {
  conversationQueryFromFilter,
  conversationRangeLabel,
  conversationRangeTitle,
  conversationSourceLabel,
} from "../lib/conversationDisplay";
import { canExpandProjectSessions, rawProjectName } from "../lib/filterChips";
import { formatCost, formatTokens } from "../lib/format";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";
import type { ConversationPage, ConversationSessionRow, Filter } from "../types";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { SourceLabel } from "./SourceIcon";
import { ModelLabel } from "./VendorIcon";

const emptyPage: ConversationPage = { rows: [], total: 0 };

export function BreakdownProjectSessions({
  filter,
  project,
  revision,
  onOpenConversation,
  onError,
}: {
  filter: Filter;
  project: string;
  revision: string;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const [page, setPage] = useState(1);
  const [data, setData] = useState<ConversationPage>(emptyPage);
  const [loading, setLoading] = useState(false);
  const generationRef = useRef(0);
  const expandable = canExpandProjectSessions(project);
  const query = useMemo(
    () =>
      expandable
        ? conversationQueryFromFilter(filter, {
            projects: [rawProjectName(project)],
            page_size: CATALOG_PAGE_SIZE,
          })
        : null,
    [expandable, filter, project],
  );
  const filterKey = JSON.stringify(query);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 换筛选时回到第一页
    setPage(1);
  }, [filterKey, revision]);

  useEffect(() => {
    if (!query) {
      generationRef.current += 1;
      // eslint-disable-next-line react-hooks/set-state-in-effect -- Cursor 行不请求会话
      setData(emptyPage);
      setLoading(false);
      return;
    }
    const generation = ++generationRef.current;
    setLoading(true);
    invoke<ConversationPage>("get_conversation_sessions_page", {
      query: { ...query, page },
    })
      .then((next) => {
        if (generation === generationRef.current) {
          setData(next);
        }
      })
      .catch((error: unknown) => {
        if (generation === generationRef.current) {
          onError?.(error);
        }
      })
      .finally(() => {
        if (generation === generationRef.current) {
          setLoading(false);
        }
      });
    return () => {
      generationRef.current += 1;
    };
  }, [filterKey, page, query, revision, onError]);

  const pageCount = Math.max(1, Math.ceil(data.total / CATALOG_PAGE_SIZE));

  if (!expandable) {
    return (
      <EmptyState
        compact
        icon="cursor"
        title="该行没有本机会话"
        hint="账号用量事件对不上本机 cwd，请到 Cursor 账号用量查看。"
      />
    );
  }

  return (
    <div className="breakdown-session-panel">
      <div className="breakdown-session-meta">
        <span>共 {formatTokens(data.total)} 个会话</span>
        <span>与对话记录同一目录，按最近活动排序</span>
      </div>
      <LoadingOverlay active={loading && data.rows.length > 0} className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>会话</th>
              <th>来源</th>
              <th>模型</th>
              <th>Token</th>
              <th>费用</th>
              <th>时间</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((row) => (
              <ProjectSessionRow
                key={`${row.source}-${row.session_id}`}
                row={row}
                onOpenConversation={onOpenConversation}
              />
            ))}
            {data.rows.length === 0 ? (
              <tr>
                <td colSpan={6} className="analytics-empty">
                  <EmptyState
                    compact
                    icon="sessions"
                    title={loading ? "正在读取项目会话…" : "该项目在当前筛选下没有对话记录"}
                  />
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </LoadingOverlay>
      <Pagination
        page={page}
        pageCount={pageCount}
        totalCount={data.total}
        onPageChange={setPage}
      />
    </div>
  );
}

function ProjectSessionRow({
  row,
  onOpenConversation,
}: {
  row: ConversationSessionRow;
  onOpenConversation?: (session: { id: string; source: string }) => void;
}) {
  const canOpen = Boolean(row.session_id && onOpenConversation);
  const open = (event: MouseEvent<HTMLTableRowElement> | KeyboardEvent<HTMLTableRowElement>) => {
    event.stopPropagation();
    if (canOpen) {
      onOpenConversation?.({ id: row.session_id, source: row.source });
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLTableRowElement>) => {
    if (!canOpen) {
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      open(event);
    }
  };
  const costCell =
    row.total_tokens === 0 && row.cost == null ? "—" : formatCost(row.cost, row.unpriced);

  return (
    <tr
      className={canOpen ? "clickable" : undefined}
      tabIndex={canOpen ? 0 : undefined}
      title={canOpen ? SESSION_ENTRY_COPY.openConversationRowTitle : undefined}
      aria-label={canOpen ? `${SESSION_ENTRY_COPY.openConversationRow}：${row.title}` : undefined}
      onClick={canOpen ? open : undefined}
      onKeyDown={canOpen ? onKeyDown : undefined}
    >
      <td title={row.title}>
        <div className="conversation-title-cell">
          <strong>{row.title || row.session_id}</strong>
          <span className="mono">{row.session_id}</span>
        </div>
      </td>
      <td>
        <SourceLabel source={row.source} fallback={conversationSourceLabel(row.source)} size={14} />
      </td>
      <td>
        {row.model ? (
          <ModelLabel name={row.model} size={14} />
        ) : (
          <span className="muted">未标注</span>
        )}
      </td>
      <td>
        <strong>{formatTokens(row.total_tokens)}</strong>
      </td>
      <td>
        {costCell}
        {row.unpriced && (row.total_tokens > 0 || row.cost != null) ? (
          <span className="muted"> *</span>
        ) : null}
      </td>
      <td title={conversationRangeTitle(row) || undefined}>{conversationRangeLabel(row)}</td>
    </tr>
  );
}
