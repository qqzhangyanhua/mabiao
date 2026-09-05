import { memo, useCallback, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { areaTrendOption, formatBucket } from "../lib/chartTheme";
import { bucketToDateRange } from "../lib/calendar";
import { chartClickDataIndex } from "../lib/chartClick";
import { trendSeriesTable } from "../lib/exportRows";
import { formatCompact, formatDelta, formatTokens, formatUsd } from "../lib/format";
import { cacheTokens, summarizeTrend, trendTableRowsNewestFirst } from "../lib/trendStats";
import type { ResolvedTheme } from "../hooks/useTheme";
import { Icon } from "../icons";
import type { Filter, Grain, SeriesPoint } from "../types";
import { EmptyState } from "./EmptyState";
import { ExportableChart } from "./ExportableChart";
import { ExportButton } from "./ExportButton";
import { KpiCard } from "./Kpi";
import { Pagination } from "./Pagination";
import { RangeBackButton } from "./RangeBackButton";
import { TrendBucketSessions } from "./TrendBucketSessions";
import { GrainSwitch, grainDetailTitle, grainSparsePrev, grainUnit } from "./ui/GrainSwitch";

const PAGE_SIZE = 20;
const TABLE_COLUMNS = 9;

function TokenCell({ value }: { value: number }) {
  return <td title={formatCompact(value)}>{formatTokens(value)}</td>;
}

export const Trend = memo(function Trend({
  grain,
  setGrain,
  points,
  theme,
  filter,
  revision,
  onRangeSelect,
  onRangeBack,
  onOpenConversation,
  onError,
}: {
  grain: Grain;
  setGrain: (grain: Grain) => void;
  points: SeriesPoint[];
  theme: ResolvedTheme;
  filter: Filter;
  revision: string;
  onRangeSelect?: (from: string, to: string) => void;
  onRangeBack?: () => void;
  onOpenConversation?: (session: { id: string; source: string }) => void;
  onError?: (error: unknown) => void;
}) {
  const rangeStart = points[0]?.bucket ?? "";
  const rangeEnd = points[points.length - 1]?.bucket ?? "";
  const pagingKey = `${grain}:${rangeStart}:${rangeEnd}`;
  const [page, setPage] = useState(1);
  const [pageKey, setPageKey] = useState(pagingKey);
  const [selectedBucket, setSelectedBucket] = useState<string | null>(null);
  const sessionsPanelRef = useRef<HTMLDivElement>(null);
  if (pageKey !== pagingKey) {
    setPageKey(pagingKey);
    setPage(1);
    setSelectedBucket(null);
  }
  const newestBucket = points[points.length - 1]?.bucket ?? null;
  const activeBucket =
    selectedBucket != null && points.some((point) => point.bucket === selectedBucket)
      ? selectedBucket
      : newestBucket;

  const option = useMemo(() => areaTrendOption(points, theme), [points, theme]);
  const stats = useMemo(() => summarizeTrend(points), [points]);
  const tableRows = useMemo(() => trendTableRowsNewestFirst(points), [points]);
  const exportTable = useMemo(() => trendSeriesTable(points), [points]);

  const pageCount = Math.max(1, Math.ceil(tableRows.length / PAGE_SIZE));
  if (pageKey === pagingKey && page > pageCount) {
    setPage(pageCount);
  }
  const currentPage = Math.min(page, pageCount);
  const pagedRows = useMemo(() => {
    const start = (currentPage - 1) * PAGE_SIZE;
    return tableRows.slice(start, start + PAGE_SIZE);
  }, [currentPage, tableRows]);

  const selectBucket = useCallback(
    (bucket: string) => {
      const range = bucketToDateRange(grain, bucket);
      if (!range) {
        return;
      }
      onRangeSelect?.(range.from, range.to);
    },
    [grain, onRangeSelect],
  );

  const selectTrendPoint = useCallback(
    (params: unknown) => {
      const index = chartClickDataIndex(params);
      const point = index == null ? undefined : points[index];
      if (point) {
        selectBucket(point.bucket);
      }
    },
    [points, selectBucket],
  );

  function selectSessionBucket(bucket: string) {
    setSelectedBucket(bucket);
    window.requestAnimationFrame(() => {
      sessionsPanelRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  }

  function onRowKeyDown(event: KeyboardEvent<HTMLTableRowElement>, bucket: string) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      selectSessionBucket(bucket);
    }
  }

  return (
    <div className="stack">
      <section className="kpi-row">
        <KpiCard
          icon="tokens"
          tone="purple"
          label="区间总 Token"
          value={formatCompact(stats.totalTokens)}
          spark={stats.sparkTokens}
        />
        <KpiCard
          icon="cost"
          tone="orange"
          label="区间总费用"
          value={formatUsd(stats.hasCost ? stats.totalCost : null, !stats.hasCost)}
          spark={stats.sparkCost}
        />
        <KpiCard
          icon="daily"
          tone="blue"
          label={`平均每${grainUnit[grain]} Token`}
          value={formatCompact(Math.round(stats.bucketAvg))}
          spark={stats.sparkTokens}
        />
        <KpiCard
          icon="trend"
          tone="cyan"
          label="峰值时段"
          value={stats.peak ? formatCompact(stats.peak.total_tokens) : "—"}
          delta={stats.peak ? { text: formatBucket(stats.peak.bucket), tone: "flat" } : null}
        />
      </section>

      <div className="panel">
        <div className="panel-head">
          <div>
            <h2>时间趋势</h2>
            <p className="panel-note">
              按{grainUnit[grain]}查看输入 / 输出 Token。Cursor 为账号用量，叠加在本机消耗之上
              {onRangeSelect ? "。点击数据点可下钻到该时段" : ""}
              {onRangeBack ? "，返回上一级可回到之前的范围" : ""}
            </p>
          </div>
          <div className="panel-head-actions">
            {onRangeBack ? <RangeBackButton onClick={onRangeBack} /> : null}
            <GrainSwitch value={grain} onChange={setGrain} />
          </div>
        </div>
        {points.length > 0 ? (
          <ExportableChart
            option={option}
            style={{ height: 320 }}
            filename="时间趋势图"
            onEvents={onRangeSelect ? { click: selectTrendPoint } : undefined}
          />
        ) : (
          <div className="analytics-empty chart-empty">
            <EmptyState
              icon="trend"
              title="当前筛选条件下暂无趋势数据"
              hint="调整时间范围或来源后再试"
            />
          </div>
        )}
      </div>

      <div className="panel">
        <div className="panel-head">
          <div>
            <h2>{grainDetailTitle[grain]}</h2>
            <p className="panel-note">
              总量含缓存和推理。无用量时段不出现，环比相对{grainSparsePrev[grain]}。共{" "}
              {points.length} 段，最新在上
              {onRangeSelect ? "。图表可下钻到该时段" : ""}。
            </p>
          </div>
          <ExportButton
            filename={grainDetailTitle[grain]}
            headers={exportTable.headers}
            rows={exportTable.rows}
          />
        </div>
        {points.length > 0 ? (
          <p className="session-below-bridge" role="note">
            <Icon name="chevron" size={14} className="flip" />
            点下面一行，该时段全部会话在页面下方显示
          </p>
        ) : null}
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>时间</th>
                <th>占总量</th>
                <th>总量</th>
                <th>输入</th>
                <th>输出</th>
                <th>缓存</th>
                <th>推理</th>
                <th>费用</th>
                <th>环比</th>
              </tr>
            </thead>
            <tbody>
              {pagedRows.map((row) => {
                const { point } = row;
                const delta = formatDelta(row.periodDelta, grainSparsePrev[grain]);
                return (
                  <tr
                    key={point.bucket}
                    className={activeBucket === point.bucket ? "clickable selected" : "clickable"}
                    onClick={() => selectSessionBucket(point.bucket)}
                    onKeyDown={(event) => onRowKeyDown(event, point.bucket)}
                    tabIndex={0}
                    aria-selected={activeBucket === point.bucket}
                    aria-expanded={activeBucket === point.bucket}
                    title={
                      activeBucket === point.bucket
                        ? "会话明细已在下方打开"
                        : "点此在下方查看该时段会话"
                    }
                  >
                    <td>
                      <span className="breakdown-expand-toggle">
                        <Icon name="chevron" size={12} className="breakdown-expand-caret" />
                        {formatBucket(point.bucket)}
                      </span>
                    </td>
                    <td>
                      <span className="cell-bar">
                        <i style={{ width: `${row.shareOfTotal}%` }} />
                      </span>
                      <span className="cell-bar-label">{row.shareOfTotal.toFixed(1)}%</span>
                    </td>
                    <TokenCell value={point.total_tokens} />
                    <TokenCell value={point.input_tokens} />
                    <TokenCell value={point.output_tokens} />
                    <TokenCell value={cacheTokens(point)} />
                    <TokenCell value={point.reasoning_tokens} />
                    <td>{formatUsd(point.cost, point.cost == null)}</td>
                    <td>
                      {delta ? <span className={`delta ${delta.tone}`}>{delta.text}</span> : "—"}
                    </td>
                  </tr>
                );
              })}
              {points.length === 0 ? (
                <tr>
                  <td colSpan={TABLE_COLUMNS} className="analytics-empty">
                    <EmptyState icon="trend" title="暂无趋势数据" />
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
        <Pagination
          page={currentPage}
          pageCount={pageCount}
          totalCount={points.length}
          onPageChange={setPage}
        />
      </div>

      {points.length > 0 ? (
        <div ref={sessionsPanelRef} className="session-below-anchor">
          <TrendBucketSessions
            filter={filter}
            grain={grain}
            bucket={activeBucket}
            revision={revision}
            onOpenConversation={onOpenConversation}
            onError={onError}
          />
        </div>
      ) : null}
    </div>
  );
});
