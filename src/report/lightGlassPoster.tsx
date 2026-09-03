import { DailyBarChart } from "./DailyBarChart";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import { ShareBar } from "./ShareBar";
import "./lightGlassPoster.css";

function LightGlassDecor() {
  return (
    <svg className="lg-deco" viewBox="0 0 720 140" aria-hidden="true">
      <circle className="lg-deco-sun" cx="656" cy="40" r="20" />
      <rect className="lg-deco-stem" x="654" y="56" width="4" height="26" rx="2" />
      <ellipse
        className="lg-deco-leaf"
        cx="638"
        cy="74"
        rx="16"
        ry="7"
        transform="rotate(-38 638 74)"
      />
      <ellipse
        className="lg-deco-leaf"
        cx="674"
        cy="76"
        rx="16"
        ry="7"
        transform="rotate(34 674 76)"
      />
    </svg>
  );
}

/** `light-glass`：浅色磨砂、玻璃卡片、插画装饰。只消费共享 PosterViewModel。 */
export function LightGlassPoster({
  data,
  posterRef,
  posterId = "report-poster",
}: ReportPosterRenderProps) {
  return (
    <article ref={posterRef} id={posterId} className="lg-poster">
      <LightGlassDecor />
      <p className="lg-kicker">{data.kicker}</p>
      <p className="lg-range">{data.rangeLabel}</p>
      <section className="lg-hero">
        <p className="lg-total">{data.totalTokensLabel}</p>
        <p className="lg-unit">{data.totalUnit}</p>
        {data.totalCostLabel ? <p className="lg-cost">{data.totalCostLabel}</p> : null}
      </section>
      {data.comments.length > 0 ? (
        <div className="lg-comments">
          {data.comments.map((comment) => (
            <p key={comment}>{comment}</p>
          ))}
        </div>
      ) : null}
      {data.days.length > 0 ? (
        <section className="lg-card">
          <h2 className="lg-card-title">按天节奏</h2>
          <DailyBarChart days={data.days} />
        </section>
      ) : null}
      {data.sources.length > 0 ? (
        <section className="lg-card">
          <h2 className="lg-card-title">来源占比</h2>
          <ShareBar sources={data.sources} />
        </section>
      ) : null}
      {data.stats.length > 0 ? (
        <ul className="lg-stats">
          {data.stats.map((stat) => (
            <li key={stat.label}>
              <span className="lg-stat-label">{stat.label}</span>
              <span className="lg-stat-value">{stat.value}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}
