import { DailyBarChart } from "./DailyBarChart";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import { ShareBar } from "./ShareBar";
import "./purpleGlassPoster.css";

/** 装饰盒 720×200，与 viewBox 对齐，避免把宽场景塞进窄盒子。 */
function PurpleGlassDecor() {
  return (
    <svg
      className="pg-deco"
      viewBox="0 0 720 200"
      width="720"
      height="200"
      preserveAspectRatio="xMidYMin meet"
      aria-hidden="true"
    >
      <circle className="pg-deco-orb pg-deco-orb-a" cx="72" cy="56" r="48" />
      <circle className="pg-deco-orb pg-deco-orb-b" cx="648" cy="40" r="64" />
      <circle className="pg-deco-orb pg-deco-orb-c" cx="580" cy="128" r="22" />
    </svg>
  );
}

/** `purple-glass`：蓝紫渐变、半透明玻璃面板、柔和高光。只消费共享 PosterViewModel。 */
export function PurpleGlassPoster({
  data,
  posterRef,
  posterId = "report-poster",
}: ReportPosterRenderProps) {
  const splitSecondary = data.sources.length > 0 && data.stats.length > 0;
  return (
    <article ref={posterRef} id={posterId} className="pg-poster">
      <PurpleGlassDecor />
      <header className="pg-masthead">
        <p className="pg-kicker">{data.kicker}</p>
        <p className="pg-range">{data.rangeLabel}</p>
      </header>
      <section className="pg-hero">
        <p className="pg-total">{data.totalTokensLabel}</p>
        <div className="pg-hero-meta">
          <p className="pg-unit">{data.totalUnit}</p>
          {data.totalCostLabel ? <p className="pg-cost">{data.totalCostLabel}</p> : null}
        </div>
      </section>
      {data.comments.length > 0 ? (
        <div className="pg-comments">
          {data.comments.map((comment) => (
            <p key={comment}>{comment}</p>
          ))}
        </div>
      ) : null}
      {data.days.length > 0 ? (
        <section className="pg-panel">
          <h2 className="pg-panel-title">按天节奏</h2>
          <DailyBarChart days={data.days} />
        </section>
      ) : null}
      {data.sources.length > 0 || data.stats.length > 0 ? (
        <div className={splitSecondary ? "pg-split" : undefined}>
          {data.sources.length > 0 ? (
            <section className="pg-panel">
              <h2 className="pg-panel-title">来源占比</h2>
              <ShareBar sources={data.sources} />
            </section>
          ) : null}
          {data.stats.length > 0 ? (
            <ul className="pg-stats">
              {data.stats.map((stat) => (
                <li key={stat.label}>
                  <span className="pg-stat-label">{stat.label}</span>
                  <span className="pg-stat-value">{stat.value}</span>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}
