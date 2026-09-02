import { memo } from "react";
import { formatClock, formatCompact } from "../lib/format";
import type { CursorAccountUsageDto } from "../types";
import { EmptyState } from "./EmptyState";
import { Button } from "./ui/Button";

const emptyUsage: CursorAccountUsageDto = {
  as_of: null,
  event_count: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  total_tokens: 0,
  daily: [],
  by_model: [],
  headless_tokens: 0,
  interactive_tokens: 0,
  headless_share: null,
};

export const CursorOverviewPanel = memo(function CursorOverviewPanel({
  data,
  onOpenCursor,
  onModelClick,
}: {
  data: CursorAccountUsageDto | null;
  onOpenCursor: () => void;
  onModelClick?: (model: string) => void;
}) {
  const usage = data ?? emptyUsage;
  const neverFetched = usage.as_of == null && usage.event_count === 0 && usage.total_tokens === 0;
  const models = usage.by_model.slice(0, 5);
  const maxModel = models[0]?.total_tokens ?? 1;

  return (
    <article className="panel cursor-overview-panel">
      <div className="panel-head">
        <div>
          <h2>Cursor 账号用量</h2>
          <span className="muted">
            云端账号，不并入上方本机总量
            {usage.as_of ? ` · 刷新于 ${formatClock(usage.as_of)}` : ""}
          </span>
        </div>
        <Button variant="text" onClick={onOpenCursor}>
          查看详情
        </Button>
      </div>
      {neverFetched ? (
        <EmptyState
          compact
          icon="cursor"
          title="尚未拉取 Cursor 账号用量"
          hint="到 Cursor 页点刷新，或在设置里打开独立自动刷新。离线时只读上次缓存，不会并入本机总量。"
        />
      ) : usage.event_count === 0 ? (
        <EmptyState
          compact
          icon="cursor"
          title="当前筛选下没有 Cursor 账号用量"
          hint="换一段时间或模型再看；完整事件在代码量页。"
        />
      ) : (
        <>
          <div className="cursor-overview-metrics">
            <Metric label="总量" value={formatCompact(usage.total_tokens)} />
            <Metric label="输入" value={formatCompact(usage.input_tokens)} />
            <Metric label="输出" value={formatCompact(usage.output_tokens)} />
            <Metric
              label="缓存读 / 写"
              value={`${formatCompact(usage.cache_read_tokens)} / ${formatCompact(usage.cache_creation_tokens)}`}
            />
          </div>
          {models.length > 0 ? (
            <ol className="rank-list cursor-overview-models">
              {models.map((row, index) => (
                <li key={row.name}>
                  <span className="rank">{index + 1}</span>
                  {onModelClick ? (
                    <button
                      type="button"
                      className="rank-name rank-link"
                      title={`筛选模型 ${row.name}`}
                      onClick={() => onModelClick(row.name)}
                    >
                      {row.name}
                    </button>
                  ) : (
                    <span className="rank-name">{row.name}</span>
                  )}
                  <span className="rank-bar">
                    <i style={{ width: `${(row.total_tokens / maxModel) * 100}%` }} />
                  </span>
                  <span className="rank-val">{formatCompact(row.total_tokens)}</span>
                </li>
              ))}
            </ol>
          ) : null}
        </>
      )}
    </article>
  );
});

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="cursor-overview-metric">
      <span className="muted">{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
