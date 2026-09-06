import type { WorkNotesPosterRenderProps } from "./posterStyleRegistry";
import "./folioRuledPoster.css";

/** `folio-ruled`：栏线纸、左红边、条目随内容撑高。 */
export function FolioRuledPoster({
  data,
  posterRef,
  posterId = "work-notes-poster",
}: WorkNotesPosterRenderProps) {
  return (
    <article ref={posterRef} id={posterId} className="fr-poster">
      <p className="fr-kicker">{data.kicker}</p>
      <p className="fr-range">{data.rangeLabel}</p>
      {data.headline ? <h1 className="fr-headline">{data.headline}</h1> : null}
      <ul className="fr-metrics">
        {data.metrics.map((metric) => (
          <li key={metric.id}>
            <span className="fr-metric-value">{metric.value}</span>
            <span className="fr-metric-label">{metric.label}</span>
          </li>
        ))}
      </ul>
      <ol className="fr-entries">
        {data.entries.map((entry, index) => (
          <li key={`${entry.title}-${index}`} className="fr-entry">
            <div>
              <h2 className="fr-entry-title">{entry.title}</h2>
              <p className="fr-entry-detail">{entry.detail}</p>
              {entry.project ? <p className="fr-entry-project">{entry.project}</p> : null}
            </div>
          </li>
        ))}
      </ol>
      {data.closing ? <p className="fr-closing">{data.closing}</p> : null}
      {data.skippedLabel ? <p className="fr-skipped">{data.skippedLabel}</p> : null}
    </article>
  );
}
