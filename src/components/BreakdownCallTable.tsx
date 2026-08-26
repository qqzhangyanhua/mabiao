import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { toDateValue } from "../lib/calendar";
import {
  filterWithCallRange,
  formatClock,
  formatCost,
  formatTokens,
  projectLabel,
  type CallRangePreset,
} from "../lib/format";
import type { Filter, UsageCallPage, UsageCallRow } from "../types";
import { EmptyState } from "./EmptyState";
import { LoadingOverlay } from "./LoadingOverlay";
import { Pagination } from "./Pagination";
import { SourceLabel } from "./SourceIcon";
import { DatePicker } from "./ui/DatePicker";
import { Segmented } from "./ui/Segmented";
import { ModelLabel } from "./VendorIcon";

const PAGE_SIZE = 20;

const CALL_RANGE_OPTIONS = [
  { value: "today", label: "当天" },
  { value: "3", label: "近 3 天" },
  { value: "7", label: "近 7 天" },
  { value: "custom", label: "区间" },
] as const;

function seedCustomRange(): { from: string; to: string } {
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - 6);
  return { from: toDateValue(start), to: toDateValue(today) };
}

export function BreakdownCallTable({
  filter,
  revision,
  onOpenConversation,
  onError,
}: {
  filter: Filter;
  revision: string;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const [page, setPage] = useState(1);
  const [rangePreset, setRangePreset] = useState<CallRangePreset>("7");
  const [customFrom, setCustomFrom] = useState("");
  const [customTo, setCustomTo] = useState("");
  const [data, setData] = useState<UsageCallPage>({ rows: [], total: 0 });
  const [loading, setLoading] = useState(true);
  const generationRef = useRef(0);
  const callFilter = useMemo(
    () => filterWithCallRange(filter, rangePreset, customFrom, customTo),
    [filter, rangePreset, customFrom, customTo],
  );
  const filterKey = JSON.stringify(callFilter);

  function selectRange(next: CallRangePreset) {
    if (next === "custom" && (!customFrom || !customTo)) {
      const seeded = seedCustomRange();
      setCustomFrom(seeded.from);
      setCustomTo(seeded.to);
    }
    setRangePreset(next);
  }

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 筛选或缓存刷新时回到第一页
    setPage(1);
  }, [filterKey, revision]);

  useEffect(() => {
    const generation = ++generationRef.current;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 标准的“发起请求前先置 loading”写法
    setLoading(true);
    invoke<UsageCallPage>("get_usage_calls_page", {
      filter: callFilter,
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
  }, [filterKey, page, revision, onError, callFilter]);

  const pageCount = Math.max(1, Math.ceil(data.total / PAGE_SIZE));

  return (
    <div className="panel">
      <div className="panel-head">
        <div>
          <h2>明细调用</h2>
          <p className="panel-note">
            日期只作用于明细调用，不影响上方聚合。有会话 ID 的行可点开对应会话。
          </p>
        </div>
        <span className="muted">共 {formatTokens(data.total)} 条</span>
      </div>
      <div className="usage-call-range">
        <Segmented
          value={rangePreset}
          options={CALL_RANGE_OPTIONS}
          ariaLabel="明细调用日期"
          onChange={selectRange}
        />
        {rangePreset === "custom" ? (
          <div className="custom-range">
            <DatePicker
              ariaLabel="开始日期"
              value={customFrom}
              max={customTo || undefined}
              onChange={setCustomFrom}
            />
            <span>至</span>
            <DatePicker
              ariaLabel="结束日期"
              value={customTo}
              min={customFrom || undefined}
              onChange={setCustomTo}
            />
          </div>
        ) : null}
      </div>
      <Pagination page={page} pageCount={pageCount} totalCount={data.total} onPageChange={setPage} />
      <LoadingOverlay
        active={loading && data.rows.length > 0}
        className="table-scroll conversation-usage-scroll"
      >
        <table className="conversation-usage-table usage-call-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>应用</th>
              <th>模型</th>
              <th>项目</th>
              <th>输入</th>
              <th>输出</th>
              <th>总量</th>
              <th>费用</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((row, index) => (
              <UsageCallRowView
                key={`${row.occurred_at}-${row.source}-${row.session_id}-${index}`}
                row={row}
                onOpenConversation={onOpenConversation}
              />
            ))}
            {data.rows.length === 0 ? (
              <tr>
                <td colSpan={8} className="analytics-empty">
                  {loading ? (
                    <EmptyState icon="provider" title="正在读取明细调用…" />
                  ) : (
                    <EmptyState icon="provider" title="当前筛选条件下暂无明细调用" />
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

function UsageCallRowView({
  row,
  onOpenConversation,
}: {
  row: UsageCallRow;
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
      onClick={canOpen ? open : undefined}
      onKeyDown={canOpen ? onKeyDown : undefined}
      title={canOpen ? "打开对应会话" : undefined}
    >
      <td>{formatClock(row.occurred_at)}</td>
      <td>
        <SourceLabel source={row.source} size={14} />
      </td>
      <td>
        <ModelLabel name={row.model} provider={row.provider} size={14} />
      </td>
      <td title={row.project}>{projectLabel(row.project) || "—"}</td>
      <td>{formatTokens(row.input_tokens)}</td>
      <td>{formatTokens(row.output_tokens)}</td>
      <td>
        <strong>{formatTokens(row.total_tokens)}</strong>
      </td>
      <td>
        {formatCost(row.cost, row.unpriced)}
        {row.unpriced ? " · 单价未配置" : ""}
      </td>
    </tr>
  );
}
