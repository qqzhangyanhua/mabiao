import { memo, useMemo, type CSSProperties } from "react";
import { Icon } from "../icons";
import { applicationStackedTrendOption, modelPalette } from "../lib/chartTheme";
import type { ResolvedTheme } from "../hooks/useTheme";
import { formatCompact, formatTokens, projectLabel } from "../lib/format";
import { applicationEfficiencyTable, applicationProjectMatrixTable } from "../lib/exportRows";
import type { ApplicationAnalyticsDto, Grain } from "../types";
import { EmptyState } from "./EmptyState";
import { ExportButton } from "./ExportButton";
import { ExportableChart } from "./ExportableChart";
import { SourceLabel } from "./SourceIcon";
import { GrainSwitch } from "./ui/GrainSwitch";

function formatPercent(value: number | null): string {
  return value == null ? "—" : `${(value * 100).toFixed(1)}%`;
}

function formatAverage(value: number | null): string {
  return value == null ? "—" : formatCompact(Math.round(value));
}

const emptyAnalytics: ApplicationAnalyticsDto = {
  summary: {
    total_tokens: 0,
    session_count: 0,
    cache_hit_rate: null,
    average_session_tokens: null,
    reasoning_share: null,
  },
  by_application: [],
  trend: [],
  projects: [],
};

export const ApplicationAnalytics = memo(function ApplicationAnalytics({
  analytics,
  grain,
  setGrain,
  theme,
}: {
  analytics: ApplicationAnalyticsDto | null;
  grain: Grain;
  setGrain: (grain: Grain) => void;
  theme: ResolvedTheme;
}) {
  const data = analytics ?? emptyAnalytics;
  const option = useMemo(
    () => applicationStackedTrendOption(data.trend, data.by_application, theme),
    [data.trend, data.by_application, theme],
  );
  const maxProjectCell = Math.max(
    0,
    ...data.projects.flatMap((row) =>
      data.by_application.map((application) => row.values[application.source] ?? 0),
    ),
  );
  const efficiencyExport = applicationEfficiencyTable(data);
  const matrixExport = applicationProjectMatrixTable(data);

  return (
    <div className="stack application-analytics">
      <section className="efficiency-cards">
        <article className="efficiency-card tone-purple">
          <span className="efficiency-label">
            <Icon name="tokens" size={14} /> 缓存命中率
          </span>
          <strong>{formatPercent(data.summary.cache_hit_rate)}</strong>
          <small>缓存读 ÷（输入 + 缓存读），近似口径</small>
        </article>
        <article className="efficiency-card tone-cyan">
          <span className="efficiency-label">
            <Icon name="sessions" size={14} /> 平均会话 Token
          </span>
          <strong>{formatAverage(data.summary.average_session_tokens)}</strong>
          <small>{formatTokens(data.summary.session_count)} 个去重会话</small>
        </article>
        <article className="efficiency-card tone-orange">
          <span className="efficiency-label">
            <Icon name="trend" size={14} /> 推理占比
          </span>
          <strong>{formatPercent(data.summary.reasoning_share)}</strong>
          <small>推理 Token ÷ 总 Token</small>
        </article>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>来源趋势堆叠图</h2>
            <p className="panel-note">
              查看各来源在总 Token 中的时间分布。Cursor 为账号用量，不计入上方本机效率卡片。
            </p>
          </div>
          <GrainSwitch value={grain} onChange={setGrain} />
        </div>
        {data.trend.length > 0 ? (
          <>
            <div className="application-trend-legend" role="list">
              {data.by_application.map((application, index) => (
                <span key={application.source} className="application-trend-legend-item" role="listitem">
                  <span
                    className="application-trend-swatch"
                    style={{ background: modelPalette[index % modelPalette.length] }}
                    aria-hidden
                  />
                  <SourceLabel
                    source={application.source}
                    fallback={application.application}
                    size={16}
                  />
                </span>
              ))}
            </div>
            <ExportableChart option={option} style={{ height: 360 }} filename="来源趋势图" />
          </>
        ) : (
          <div className="analytics-empty">
            <EmptyState icon="trend" title="当前筛选条件下暂无趋势数据" />
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>来源效率明细</h2>
            <p className="panel-note">
              按来源比较缓存复用、单会话规模与推理开销。Cursor 会话数按账号事件计。
            </p>
          </div>
          <ExportButton
            filename="来源效率"
            headers={efficiencyExport.headers}
            rows={efficiencyExport.rows}
          />
        </div>
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>来源</th>
                <th>总 Token</th>
                <th>会话数</th>
                <th>平均会话 Token</th>
                <th>缓存命中率</th>
                <th>推理占比</th>
              </tr>
            </thead>
            <tbody>
              {data.by_application.map((row) => (
                <tr key={row.source}>
                  <td>
                    <strong>
                      <SourceLabel source={row.source} fallback={row.application} />
                    </strong>
                  </td>
                  <td>{formatTokens(row.metrics.total_tokens)}</td>
                  <td>{formatTokens(row.metrics.session_count)}</td>
                  <td>{formatAverage(row.metrics.average_session_tokens)}</td>
                  <td>{formatPercent(row.metrics.cache_hit_rate)}</td>
                  <td>{formatPercent(row.metrics.reasoning_share)}</td>
                </tr>
              ))}
              {data.by_application.length === 0 ? (
                <tr>
                  <td colSpan={6} className="analytics-empty">
                    <EmptyState icon="source" title="暂无来源数据" />
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>来源 × 项目交叉统计</h2>
            <p className="panel-note">
              行按项目总 Token 排序，颜色越深表示该来源在项目中的消耗越高。
            </p>
          </div>
          <ExportButton
            filename="来源项目交叉"
            headers={matrixExport.headers}
            rows={matrixExport.rows}
          />
        </div>
        <div className="table-scroll cross-table-wrap">
          <table className="cross-table">
            <thead>
              <tr>
                <th className="sticky-col">项目</th>
                {data.by_application.map((application) => (
                  <th key={application.source}>
                    <SourceLabel
                      source={application.source}
                      fallback={application.application}
                      size={14}
                    />
                  </th>
                ))}
                <th>总计</th>
              </tr>
            </thead>
            <tbody>
              {data.projects.map((row) => (
                <tr key={row.project}>
                  <td className="sticky-col" title={row.project}>
                    {projectLabel(row.project)}
                  </td>
                  {data.by_application.map((application) => {
                    const value = row.values[application.source] ?? 0;
                    const intensity = maxProjectCell > 0 ? value / maxProjectCell : 0;
                    return (
                      <td
                        key={application.source}
                        className={value > 0 ? "matrix-cell active" : "matrix-cell"}
                        style={{ "--matrix-alpha": 0.08 + intensity * 0.44 } as CSSProperties}
                        title={`${row.project} · ${application.application}: ${formatTokens(value)} Token`}
                      >
                        {value > 0 ? formatCompact(value) : "—"}
                      </td>
                    );
                  })}
                  <td>
                    <strong>{formatCompact(row.total_tokens)}</strong>
                  </td>
                </tr>
              ))}
              {data.projects.length === 0 ? (
                <tr>
                  <td colSpan={data.by_application.length + 2} className="analytics-empty">
                    <EmptyState icon="project" title="暂无项目交叉数据" />
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
});
