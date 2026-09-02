import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  formatCacheHitRate,
  formatCompact,
  formatTokens,
  projectLabel,
  relativeTime,
} from "../lib/format";
import {
  LOW_CACHE_HIT_LIMIT,
  lowCacheHitEmptyHint,
  lowCacheHitEmptyTitle,
} from "../lib/lowCacheHit";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";
import type { Filter, LowCacheHitSessionRow, LowCacheHitSessionsDto } from "../types";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { SessionIdCell } from "./SessionTableParts";
import { ModelLabel } from "./VendorIcon";

const emptyDto: LowCacheHitSessionsDto = {
  source: "",
  computable: false,
  rows: [],
};

export function LowCacheHitSessions({
  filter,
  source,
  application,
  revision,
  onOpenConversation,
  onError,
}: {
  filter: Filter;
  source: string | null;
  application: string | null;
  revision: string;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const [data, setData] = useState<LowCacheHitSessionsDto>(emptyDto);
  const [loading, setLoading] = useState(false);
  const generationRef = useRef(0);
  const filterKey = JSON.stringify(filter);

  useEffect(() => {
    if (!source) {
      generationRef.current += 1;
      // eslint-disable-next-line react-hooks/set-state-in-effect -- 取消选择时清掉上一来源的列表
      setData(emptyDto);
      setLoading(false);
      return;
    }
    const generation = ++generationRef.current;
    setLoading(true);
    invoke<LowCacheHitSessionsDto>("get_low_cache_hit_sessions", {
      filter,
      source,
      limit: LOW_CACHE_HIT_LIMIT,
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
  }, [filterKey, source, revision, onError, filter]);

  const rows = source && data.source === source ? data.rows : [];
  const computable = Boolean(source) && data.source === source && data.computable;
  const emptyTitle = lowCacheHitEmptyTitle(source, computable);
  const emptyHint = lowCacheHitEmptyHint(source, computable);
  const label = application ?? source;

  return (
    <section className="panel">
      <div className="panel-head">
        <div>
          <h2>低缓存命中会话</h2>
          <p className="panel-note">
            {source
              ? `${label} 当前筛选范围内命中率最低的 ${LOW_CACHE_HIT_LIMIT} 条。点行打开对话记录。只在同一来源内比较。`
              : "点上来源效率明细的一行，列出该来源命中率最低的会话。"}
          </p>
        </div>
        {computable ? <span className="muted">共 {formatTokens(rows.length)} 条</span> : null}
      </div>
      <LoadingOverlay active={loading && rows.length > 0} className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>会话</th>
              <th>项目</th>
              <th>模型</th>
              <th>缓存命中率</th>
              <th>总 Token</th>
              <th>最近活动</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <LowCacheHitRow
                key={`${row.source}-${row.session_id}`}
                row={row}
                onOpenConversation={onOpenConversation}
              />
            ))}
            {rows.length === 0 ? (
              <tr>
                <td colSpan={6} className="analytics-empty">
                  {loading && source ? (
                    <EmptyState icon="sessions" title="正在读取低命中会话…" />
                  ) : (
                    <EmptyState icon="sessions" title={emptyTitle} hint={emptyHint} />
                  )}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </LoadingOverlay>
    </section>
  );
}

function LowCacheHitRow({
  row,
  onOpenConversation,
}: {
  row: LowCacheHitSessionRow;
  onOpenConversation?: (session: { id: string; source: string }) => void;
}) {
  const canOpen = Boolean(row.session_id && onOpenConversation);
  const open = () => {
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
      open();
    }
  };

  return (
    <tr
      className={canOpen ? "clickable" : undefined}
      tabIndex={canOpen ? 0 : undefined}
      title={canOpen ? SESSION_ENTRY_COPY.openConversationRowTitle : undefined}
      aria-label={
        canOpen ? `${SESSION_ENTRY_COPY.openConversationRow}：${row.session_id}` : undefined
      }
      onClick={canOpen ? open : undefined}
      onKeyDown={canOpen ? onKeyDown : undefined}
    >
      <td>
        <div className="cell-stack">
          <SessionIdCell sessionId={row.session_id} />
          {canOpen ? <span className="muted">{SESSION_ENTRY_COPY.openConversationRow}</span> : null}
        </div>
      </td>
      <td title={row.project}>{projectLabel(row.project)}</td>
      <td>
        {row.model ? <ModelLabel name={row.model} size={14} /> : <span className="muted">—</span>}
      </td>
      <td>{formatCacheHitRate(row.cache_hit_rate)}</td>
      <td>{formatCompact(row.total_tokens)}</td>
      <td>{relativeTime(row.ended_at)}</td>
    </tr>
  );
}
