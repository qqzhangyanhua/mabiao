import { DailyBarChart } from "./DailyBarChart";
import type { PosterViewModel } from "./posterTypes";
import { ShareBar } from "./ShareBar";
import "./poster.css";

export function ReportPoster({ data }: { data: PosterViewModel }) {
  return (
    <article id="report-poster" className="report-poster">
      <p className="rp-kicker">{data.kicker}</p>
      <p className="rp-range">{data.rangeLabel}</p>
      <p className="rp-total">{data.totalTokensLabel}</p>
      <p className="rp-unit">{data.totalUnit}</p>
      {data.totalCostLabel ? <p className="rp-cost">{data.totalCostLabel}</p> : null}
      {data.comments.length > 0 ? (
        <div className="rp-comments">
          {data.comments.map((comment) => (
            <p key={comment}>{comment}</p>
          ))}
        </div>
      ) : null}
      {data.days.length > 0 ? (
        <section className="rp-panel">
          <h2 className="rp-panel-title">按天节奏</h2>
          <DailyBarChart days={data.days} />
        </section>
      ) : null}
      {data.sources.length > 0 ? (
        <section className="rp-panel">
          <h2 className="rp-panel-title">来源占比</h2>
          <ShareBar sources={data.sources} />
        </section>
      ) : null}
      {data.stats.length > 0 ? (
        <ul className="rp-stats">
          {data.stats.map((stat) => (
            <li key={stat.label}>
              <span className="rp-stat-label">{stat.label}</span>
              <span className="rp-stat-value">{stat.value}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}
