import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { formatClock, formatTokens } from "../lib/format";
import type { ConversationUsagePage, ConversationUsageRecord } from "../types";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { ModelLabel } from "./VendorIcon";

const PAGE_SIZE = 20;

export function ConversationUsageTable({
  source,
  sessionId,
  refreshKey,
  onTotalChange,
  onError,
}: {
  source: string;
  sessionId: string;
  refreshKey: string;
  onTotalChange?: (total: number | null) => void;
  onError?: (error: unknown) => void;
}) {
  const [page, setPage] = useState(1);
  const [data, setData] = useState<ConversationUsagePage>({ rows: [], total: 0 });
  const [loading, setLoading] = useState(true);
  const generationRef = useRef(0);

  useEffect(() => {
    const generation = ++generationRef.current;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 标准的“发起请求前先置 loading”写法
    setLoading(true);
    invoke<ConversationUsagePage>("get_conversation_usage_records", {
      source,
      sessionId,
      page,
      pageSize: PAGE_SIZE,
    })
      .then((next) => {
        if (generation !== generationRef.current) {
          return;
        }
        setData(next);
        onTotalChange?.(next.total);
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
  }, [source, sessionId, page, refreshKey, onError, onTotalChange]);

  const pageCount = Math.max(1, Math.ceil(data.total / PAGE_SIZE));

  return (
    <div className="conversation-usage-panel">
      <Pagination page={page} pageCount={pageCount} totalCount={data.total} onPageChange={setPage} />
      <LoadingOverlay
        active={loading && data.rows.length > 0}
        className="table-scroll conversation-usage-scroll"
      >
        <table className="conversation-usage-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>模型</th>
              <th>接口</th>
              <th>输入</th>
              <th>输出</th>
              <th>缓存读</th>
              <th>缓存写</th>
              <th>推理</th>
              <th>总量</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((record, index) => (
              <UsageRecordRow
                key={`${record.occurred_at}-${record.source_file}-${index}`}
                record={record}
              />
            ))}
            {data.rows.length === 0 ? (
              <tr>
                <td colSpan={9} className="analytics-empty">
                  {loading ? (
                    <EmptyState icon="chat" title="正在读取用量明细…" />
                  ) : (
                    <EmptyState icon="chat" title="这条会话暂无用量明细" />
                  )}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </LoadingOverlay>
    </div>
  );
}

function UsageRecordRow({ record }: { record: ConversationUsageRecord }) {
  return (
    <tr>
      <td>{formatClock(record.occurred_at)}</td>
      <td>
        <ModelLabel name={record.model} provider={record.provider} />
      </td>
      <td>{record.provider || "未标注"}</td>
      <td>{formatTokens(record.input_tokens)}</td>
      <td>{formatTokens(record.output_tokens)}</td>
      <td>{formatTokens(record.cache_read_tokens)}</td>
      <td>{formatTokens(record.cache_creation_tokens)}</td>
      <td>{formatTokens(record.reasoning_tokens)}</td>
      <td>
        <strong>{formatTokens(record.total_tokens)}</strong>
      </td>
    </tr>
  );
}
