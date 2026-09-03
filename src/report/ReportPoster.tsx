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
      <p className="rp-cost">{data.totalCostLabel}</p>
      <div className="rp-comments">
        <p>{data.nightShareComment}</p>
        <p>{data.peakHoursComment}</p>
      </div>
      <section className="rp-panel">
        <h2 className="rp-panel-title">按天节奏</h2>
        <DailyBarChart days={data.days} />
      </section>
      <section className="rp-panel">
        <h2 className="rp-panel-title">来源占比</h2>
        <ShareBar sources={data.sources} />
      </section>
      <ul className="rp-stats">
        <li>
          <span className="rp-stat-label">{data.busiestDayLabel}</span>
          <span className="rp-stat-value">{data.busiestDayValue}</span>
        </li>
        <li>
          <span className="rp-stat-label">{data.topSessionLabel}</span>
          <span className="rp-stat-value">{data.topSessionValue}</span>
        </li>
        <li>
          <span className="rp-stat-label">{data.modelsLabel}</span>
          <span className="rp-stat-value">{data.modelsValue}</span>
        </li>
      </ul>
    </article>
  );
}
