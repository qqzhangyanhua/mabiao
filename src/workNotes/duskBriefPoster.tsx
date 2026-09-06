import type { WorkNotesPosterRenderProps } from "./posterStyleRegistry";
import "./duskBriefPoster.css";

/** `dusk-brief`：暮色简报、金线分栏、条目随内容撑高。 */
export function DuskBriefPoster({
  data,
  posterRef,
  posterId = "work-notes-poster",
}: WorkNotesPosterRenderProps) {
  return (
    <article ref={posterRef} id={posterId} className="db-poster">
      <header className="db-mast">
        <p className="db-kicker">{data.kicker}</p>
        <p className="db-range">{data.rangeLabel}</p>
      </header>
      <ul className="db-metrics">
        {data.metrics.map((metric) => (
          <li key={metric.id}>
            <span className="db-metric-value">{metric.value}</span>
            <span className="db-metric-label">{metric.label}</span>
          </li>
        ))}
      </ul>
      {data.headline ? <h1 className="db-headline">{data.headline}</h1> : null}
      <ol className="db-entries">
        {data.entries.map((entry, index) => (
          <li key={`${entry.title}-${index}`} className="db-entry">
            <div>
              <h2 className="db-entry-title">{entry.title}</h2>
              <p className="db-entry-detail">{entry.detail}</p>
              {entry.project ? <p className="db-entry-project">{entry.project}</p> : null}
            </div>
          </li>
        ))}
      </ol>
      {data.closing ? <p className="db-closing">{data.closing}</p> : null}
      {data.skippedLabel ? <p className="db-skipped">{data.skippedLabel}</p> : null}
    </article>
  );
}
