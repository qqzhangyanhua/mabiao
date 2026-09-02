import { useMemo } from "react";
import { chartPalette, donutOption } from "../lib/chartTheme";
import { sourceLabel, formatCompact, projectLabel, relativeTime } from "../lib/format";
import type { ResolvedTheme } from "../hooks/useTheme";
import type { NamedAmount, OverviewDto, SessionRow } from "../types";
import { DonutChart } from "./DonutChart";
import { EmptyState } from "./EmptyState";
import { LegendRow } from "./Kpi";
import { SourceIcon } from "./SourceIcon";
import { Button } from "./ui/Button";
import { ModelLabel } from "./VendorIcon";

export function OverviewDetail({
  data,
  projects,
  sessions,
  theme,
  onOpenConversations,
  onProjectClick,
  onSessionClick,
}: {
  data: OverviewDto;
  projects: NamedAmount[];
  sessions: SessionRow[];
  theme: ResolvedTheme;
  onOpenConversations: () => void;
  onProjectClick?: (project: string) => void;
  onSessionClick?: (session: { id: string; source: string }) => void;
}) {
  const palette = chartPalette(theme);
  const tokenItems = useMemo(
    () => [
      { name: "输入 Token", value: data.input_tokens, color: palette.input },
      { name: "输出 Token", value: data.output_tokens, color: palette.output },
      { name: "缓存读", value: data.cache_read_tokens, color: palette.cacheRead },
      { name: "缓存写", value: data.cache_creation_tokens, color: palette.cacheCreation },
      { name: "推理 Token", value: data.reasoning_tokens, color: palette.reasoning },
    ],
    [
      data.input_tokens,
      data.output_tokens,
      data.cache_read_tokens,
      data.cache_creation_tokens,
      data.reasoning_tokens,
      palette.input,
      palette.output,
      palette.cacheRead,
      palette.cacheCreation,
      palette.reasoning,
    ],
  );
  const tokenOption = useMemo(() => donutOption(tokenItems, theme), [tokenItems, theme]);

  const recent = useMemo(
    () => [...sessions].sort((a, b) => b.ended_at.localeCompare(a.ended_at)).slice(0, 8),
    [sessions],
  );
  const topProjects = projects.slice(0, 5);
  const maxProject = topProjects[0]?.total_tokens ?? 1;
  const tokenTotal = formatCompact(data.total_tokens);
  const sliceTotal = tokenItems.reduce((sum, item) => sum + item.value, 0);
  const sliceShare = (value: number) =>
    sliceTotal === 0 ? 0 : (value / sliceTotal) * 100;

  return (
    <section className="dash-bottom">
      <article className="panel">
        <div className="panel-head">
          <h2>Token 使用统计</h2>
        </div>
        <div className="donut-wrap">
          <DonutChart option={tokenOption} centerValue={tokenTotal} />
          <div className="legend-col is-dense">
            {tokenItems.map((item) => (
              <LegendRow
                key={item.name}
                color={item.color}
                label={item.name}
                value={formatCompact(item.value)}
                extra={`${sliceShare(item.value).toFixed(1)}%`}
              />
            ))}
          </div>
        </div>
      </article>
      <article className="panel">
        <div className="panel-head">
          <h2>Top 5 项目</h2>
          <span className="muted">按 Token 使用量</span>
        </div>
        <ol className="rank-list">
          {topProjects.map((row, index) => (
            <li key={row.name}>
              <span className="rank">{index + 1}</span>
              <button
                type="button"
                className="rank-name rank-link"
                title={`筛选项目 ${projectLabel(row.name)}`}
                onClick={() => onProjectClick?.(row.name)}
              >
                {projectLabel(row.name)}
              </button>
              <span className="rank-bar">
                <i style={{ width: `${(row.total_tokens / maxProject) * 100}%` }} />
              </span>
              <span className="rank-val">{formatCompact(row.total_tokens)}</span>
            </li>
          ))}
          {topProjects.length === 0 ? (
            <li className="empty">
              <EmptyState compact icon="project" title="暂无项目数据" />
            </li>
          ) : null}
        </ol>
      </article>
      <article className="panel">
        <div className="panel-head">
          <h2>最近会话</h2>
          <Button variant="text" onClick={onOpenConversations}>
            查看全部
          </Button>
        </div>
        <ul className="session-list">
          {recent.map((row) => (
            <li key={`${row.source}-${row.session_id}`}>
              <button
                type="button"
                className="sess-open"
                onClick={() => onSessionClick?.({ id: row.session_id, source: row.source })}
              >
                <SourceIcon source={row.source} size={16} />
                <div className="sess-main">
                  <div className="sess-title">{projectLabel(row.project)}</div>
                  <div className="sess-sub">
                    {row.model ? (
                      <ModelLabel name={row.model} size={14} />
                    ) : (
                      sourceLabel(row.source)
                    )}
                  </div>
                </div>
                <span className="sess-time">{relativeTime(row.ended_at)}</span>
                <span className="sess-tokens">{formatCompact(row.total_tokens)}</span>
              </button>
            </li>
          ))}
          {recent.length === 0 ? (
            <li className="empty">
              <EmptyState compact icon="sessions" title="暂无会话" />
            </li>
          ) : null}
        </ul>
      </article>
    </section>
  );
}
