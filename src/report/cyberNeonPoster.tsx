import { DailyBarChart } from "./DailyBarChart";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import { ShareBar } from "./ShareBar";
import "./cyberNeonPoster.css";

/** 装饰盒 720×180，与 viewBox 对齐，避免把宽场景塞进窄盒子。 */
function CyberNeonDecor() {
  return (
    <svg
      className="cn-deco"
      viewBox="0 0 720 180"
      width="720"
      height="180"
      preserveAspectRatio="xMidYMin meet"
      aria-hidden="true"
    >
      <polyline className="cn-deco-stroke cn-deco-cyan" points="16,58 16,16 58,16" />
      <polyline className="cn-deco-stroke cn-deco-magenta" points="704,58 704,16 662,16" />
      <line className="cn-deco-stroke cn-deco-grid" x1="88" y1="0" x2="132" y2="44" />
      <line className="cn-deco-stroke cn-deco-grid" x1="148" y1="0" x2="192" y2="44" />
      <line className="cn-deco-stroke cn-deco-grid" x1="208" y1="0" x2="252" y2="44" />
      <polygon className="cn-deco-fill cn-deco-cyan" points="596,22 624,8 624,36" />
      <polygon className="cn-deco-fill cn-deco-magenta" points="632,34 660,20 660,48" />
      <rect className="cn-deco-fill cn-deco-cyan" x="72" y="22" width="7" height="7" />
      <rect className="cn-deco-fill cn-deco-magenta" x="641" y="56" width="7" height="7" />
    </svg>
  );
}

/** `cyber-neon`：黑底、青紫霓虹、棱角网格。只消费共享 PosterViewModel。 */
export function CyberNeonPoster({
  data,
  posterRef,
  posterId = "report-poster",
}: ReportPosterRenderProps) {
  const splitSecondary = data.sources.length > 0 && data.stats.length > 0;
  return (
    <article ref={posterRef} id={posterId} className="cn-poster">
      <CyberNeonDecor />
      <header className="cn-masthead">
        <p className="cn-kicker">{data.kicker}</p>
        <span className="cn-rail" aria-hidden="true" />
        <p className="cn-range">{data.rangeLabel}</p>
      </header>
      <section className="cn-hero">
        <p className="cn-total">{data.totalTokensLabel}</p>
        <div className="cn-hero-meta">
          <p className="cn-unit">{data.totalUnit}</p>
          {data.totalCostLabel ? <p className="cn-cost">{data.totalCostLabel}</p> : null}
        </div>
      </section>
      {data.comments.length > 0 ? (
        <div className="cn-comments">
          {data.comments.map((comment) => (
            <p key={comment}>{comment}</p>
          ))}
        </div>
      ) : null}
      {data.days.length > 0 ? (
        <section className="cn-panel">
          <h2 className="cn-panel-title">按天节奏</h2>
          <DailyBarChart days={data.days} />
        </section>
      ) : null}
      {data.sources.length > 0 || data.stats.length > 0 ? (
        <div className={splitSecondary ? "cn-split" : undefined}>
          {data.sources.length > 0 ? (
            <section className="cn-panel">
              <h2 className="cn-panel-title">来源占比</h2>
              <ShareBar sources={data.sources} />
            </section>
          ) : null}
          {data.stats.length > 0 ? (
            <ul className="cn-stats">
              {data.stats.map((stat) => (
                <li key={stat.label}>
                  <span className="cn-stat-label">{stat.label}</span>
                  <span className="cn-stat-value">{stat.value}</span>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}
