import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { formatBucket } from "../lib/chartTheme";
import {
  filterForBucket,
  formatCost,
  formatTokens,
  projectLabel,
  relativeTime,
} from "../lib/format";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";
import type { Filter, Grain, SessionPage, SessionRow } from "../types";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { SessionIdCell } from "./SessionTableParts";
import { SourceLabel } from "./SourceIcon";
import { grainDetailTitle } from "./ui/GrainSwitch";
import { ModelLabel } from "./VendorIcon";

const PAGE_SIZE = 20;
const TABLE_COLUMNS = 7;
const emptyPage: SessionPage = { rows: [], total: 0, total_tokens: 0, last_ended: null };

function bucketHeading(grain: Grain, bucket: string): string {
  return grain === "day" ? bucket : formatBucket(bucket);
}

function idleNote(grain: Grain): string {
  return grain === "day"
    ? "点上方按日明细的一行，列出当日所有会话。"
    : `点上方${grainDetailTitle[grain]}的一行，列出该时段所有会话。`;
}

function idleEmptyTitle(grain: Grain): string {
  return grain === "day"
    ? "点上方按日明细的一行，查看当日会话"
    : `点上方${grainDetailTitle[grain]}的一行，查看该时段会话`;
}

export function TrendBucketSessions({
  filter,
  grain,
  bucket,
  revision,
  onOpenConversation,
  onError,
}: {
  filter: Filter;
  grain: Grain;
  bucket: string | null;
  revision: string;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const [page, setPage] = useState(1);
  const [data, setData] = useState<SessionPage>(emptyPage);
  const [loading, setLoading] = useState(false);
  const generationRef = useRef(0);
  const queryFilter = useMemo(
    () => (bucket ? filterForBucket(filter, grain, bucket) : null),
    [bucket, filter, grain],
  );
  const filterKey = JSON.stringify(queryFilter);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 换时段或筛选时回到第一页
    setPage(1);
  }, [filterKey, revision, bucket]);

  useEffect(() => {
    if (!queryFilter) {
      generationRef.current += 1;
      // eslint-disable-next-line react-hooks/set-state-in-effect -- 非法 bucket 时清掉上一时段列表
      setData(emptyPage);
      setLoading(false);
      return;
    }
    const generation = ++generationRef.current;
    setLoading(true);
    invoke<SessionPage>("get_sessions_page", {
      filter: queryFilter,
      page,
      pageSize: PAGE_SIZE,
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
  }, [bucket, filterKey, onError, page, queryFilter, revision]);

  const pageCount = Math.max(1, Math.ceil(data.total / PAGE_SIZE));
  const selected = Boolean(bucket && queryFilter);
  const heading = selected && bucket ? `${bucketHeading(grain, bucket)} 的会话` : "会话明细";
  const note = selected
    ? "本机消耗记录按会话汇总，Cursor 账号用量不在此列。点行打开对话记录。"
    : idleNote(grain);
  const emptyTitle = loading
    ? "正在读取会话…"
    : selected
      ? "该时段没有本机会话"
      : idleEmptyTitle(grain);
  const emptyHint = selected ? undefined : "按会话汇总本机消耗；Cursor 账号用量不在此列。";

  return (
    <section className="panel">
      <div className="panel-head">
        <div>
          <h2>{heading}</h2>
          <p className="panel-note">{note}</p>
        </div>
        {selected ? (
          <span className="muted">
            共 {formatTokens(data.total)} 个 · {formatTokens(data.total_tokens)} Token
          </span>
        ) : null}
      </div>
      <LoadingOverlay active={loading && data.rows.length > 0} className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>会话</th>
              <th>来源</th>
              <th>项目</th>
              <th>模型</th>
              <th>Token</th>
              <th>费用</th>
              <th>最近活动</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((row) => (
              <TrendSessionRow
                key={`${row.source}-${row.session_id}`}
                row={row}
                onOpenConversation={onOpenConversation}
              />
            ))}
            {data.rows.length === 0 ? (
              <tr>
                <td colSpan={TABLE_COLUMNS} className="analytics-empty">
                  <EmptyState compact icon="sessions" title={emptyTitle} hint={emptyHint} />
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
    </section>
  );
}

function TrendSessionRow({
  row,
  onOpenConversation,
}: {
  row: SessionRow;
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
  const costCell =
    row.total_tokens === 0 && row.cost == null ? "—" : formatCost(row.cost, row.unpriced);

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
        <SessionIdCell sessionId={row.session_id} />
      </td>
      <td>
        <SourceLabel source={row.source} size={14} />
      </td>
      <td title={row.project}>{projectLabel(row.project)}</td>
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
      <td title={row.ended_at || row.started_at}>{relativeTime(row.ended_at || row.started_at)}</td>
    </tr>
  );
}
